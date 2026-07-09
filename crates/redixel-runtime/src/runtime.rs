use std::sync::{
    Arc, mpsc,
    mpsc::{Receiver, Sender},
};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

use wgpu::SurfaceError;

use redixel_core::{Game, RedixelError, game::GameContext, net::NetworkManager};
#[cfg(feature = "net")]
use redixel_net::NetConfig;
use redixel_platform::{WindowManager, window::WindowConfig};
use redixel_renderer::{Renderer, RendererConfig};

use crate::{
    context::{Context, DrawCommand},
    settings::EngineSettings,
    simulation::{SimulationCore, StepFlow},
    time::TimeManager,
};

pub const DEFAULT_TICKRATE: f64 = 60.0;

#[derive(Clone)]
pub struct RuntimeConfig {
    pub window: WindowConfig,
    pub renderer: RendererConfig,
    pub target_fps: f64,
    /// Fixed-update rate in Hz driving `on_fixed_update` (physics/networking).
    pub tickrate: f64,
    /// Networking configuration. `None` installs a zero-cost no-op network.
    #[cfg(feature = "net")]
    pub net: Option<NetConfig>,
}

impl RuntimeConfig {
    /// A configuration for a **headless server**: no window, no GPU, just a
    /// fixed-update tick driving simulation. The window/renderer fields are
    /// placeholders never touched by [`HeadlessRuntime`]. `tickrate` follows
    /// the same `config.json` (`engine.tickrate`) fallback as the windowed
    /// path, defaulting to [`DEFAULT_TICKRATE`] when the file or key is
    /// absent. Enable networking with [`with_net`](Self::with_net).
    pub fn headless() -> Self {
        EngineSettings::load_config_json();

        let tickrate: f64 = EngineSettings::global_read().get_path("engine.tickrate", DEFAULT_TICKRATE);

        Self {
            target_fps: 0.0,
            tickrate,
            #[cfg(feature = "net")]
            net: None,
            window: WindowConfig {
                title: String::from("redixel-headless"),
                width: 0,
                height: 0,
                fullscreen: false,
            },
            renderer: RendererConfig::default(),
        }
    }

    /// Enables networking on this configuration with the given transport role.
    #[cfg(feature = "net")]
    pub fn with_net(mut self, net: NetConfig) -> Self {
        self.net = Some(net);
        self
    }

    /// Builds the network manager described by the config, or a no-op when
    /// networking is disabled (feature off or no [`net`](Self::net) set).
    pub(crate) fn build_network(&self) -> Box<dyn NetworkManager> {
        #[cfg(feature = "net")]
        {
            match &self.net {
                Some(cfg) => redixel_net::build(cfg, self.tickrate),
                None => Box::new(redixel_core::net::NoOpNetwork),
            }
        }
        #[cfg(not(feature = "net"))]
        {
            Box::new(redixel_core::net::NoOpNetwork)
        }
    }
}

/// Runs a game **headless** (server mode): no `winit`, no `wgpu`. Drives only
/// `on_start` then a fixed-rate `on_fixed_update` loop (networking + physics),
/// pacing each tick with a sleep to keep CPU/RAM minimal on a VPS. `on_update`
/// and `on_render` never fire.
///
/// The same `Game` implementation runs here and in the windowed [`Runtime`] —
/// only the rendering callbacks are skipped.
#[cfg(not(target_arch = "wasm32"))]
pub struct HeadlessRuntime<G: Game> {
    sim: SimulationCore<G>,
    tickrate: f64,
    tick_dur: Duration,
}

#[cfg(not(target_arch = "wasm32"))]
impl<G: Game> HeadlessRuntime<G> {
    /// Builds a headless runner from `config`, installing the configured network
    /// transport. A non-positive tickrate falls back to [`DEFAULT_TICKRATE`].
    pub fn new(game: G, config: RuntimeConfig) -> Self {
        let tickrate: f64 = if config.tickrate > 0.0 {
            config.tickrate
        } else {
            DEFAULT_TICKRATE
        };

        let mut time: TimeManager = TimeManager::new();
        time.set_tickrate(tickrate);

        let context: Context<G::Action> = Context::with_network(config.build_network());
        let tick_dur: Duration = Duration::from_secs_f64(1.0 / tickrate);

        Self {
            sim: SimulationCore::new(time, context, game),
            tickrate,
            tick_dur,
        }
    }

    /// Runs `on_start`, then the fixed-update loop until the game calls
    /// `ctx.exit()`. Blocks the calling thread; returns any fatal game error.
    pub fn run(mut self) -> Result<(), RedixelError> {
        use std::thread::sleep;
        use std::time::Instant;

        self.sim.start()?;

        let mut last: Instant = Instant::now();
        log::info!("Headless runtime started at {} Hz.", self.tickrate);

        loop {
            let now: Instant = Instant::now();
            let frame_delta: f64 = now.duration_since(last).as_secs_f64();
            last = now;

            match self.sim.run_fixed_updates(frame_delta) {
                StepFlow::Exit => {
                    log::info!("Headless runtime stopping (game requested shutdown).");
                    return Ok(());
                }
                StepFlow::Fatal(e) => return Err(e),
                StepFlow::Continue => {}
            }

            self.sim.context.reset_frame();

            let work: Duration = Instant::now().duration_since(now);
            if work < self.tick_dur {
                sleep(self.tick_dur - work);
            }
        }
    }
}

/// Thin convenience wrapper around [`HeadlessRuntime::new`] + [`HeadlessRuntime::run`].
#[cfg(not(target_arch = "wasm32"))]
pub fn run_headless<G: Game>(game: G, config: RuntimeConfig) -> Result<(), RedixelError> {
    HeadlessRuntime::new(game, config).run()
}

type BridgePayload = Result<(Renderer, WindowManager), RedixelError>;

struct RunningState<G: Game> {
    renderer: Renderer,
    window: WindowManager,
    sim: SimulationCore<G>,
}

enum AppState<G: Game> {
    Initializing,
    Loading,
    Running(Box<RunningState<G>>),
}

/// Implements [`ApplicationHandler`] and drives the engine from creation to
/// shutdown. Owns the application state and the async initialisation bridge.
pub struct Runtime<G: Game> {
    state: AppState<G>,
    pending_game: Option<G>,
    fatal_error: Option<RedixelError>,
    bridge_tx: Sender<BridgePayload>,
    bridge_rx: Receiver<BridgePayload>,
    config: RuntimeConfig,
    is_suspended: bool,
}

impl<G: Game> Runtime<G> {
    pub fn new(game: G, config: RuntimeConfig) -> Self {
        let (bridge_tx, bridge_rx): (Sender<BridgePayload>, Receiver<BridgePayload>) = mpsc::channel();
        Self {
            state: AppState::Initializing,
            pending_game: Some(game),
            fatal_error: None,
            bridge_tx,
            bridge_rx,
            config,
            is_suspended: false,
        }
    }

    /// Moves the stored fatal error out of the runtime.
    /// Called by `redixel::run()` after `run_app` returns.
    pub fn take_error(&mut self) -> Option<RedixelError> {
        self.fatal_error.take()
    }

    fn abort(&mut self, event_loop: &dyn ActiveEventLoop, error: RedixelError) {
        self.fatal_error = Some(error);
        event_loop.exit();
    }

    fn transition_to_running(&mut self, renderer: Renderer, window: WindowManager) -> Result<(), RedixelError> {
        window.request_redraw();

        let mut time: TimeManager = TimeManager::new();
        time.set_target_fps(self.config.target_fps);
        time.set_tickrate(self.config.tickrate);

        let initial_size: PhysicalSize<u32> = window.surface_size();
        let mut context: Context<G::Action> = Context::with_network(self.config.build_network());
        context.update_state(initial_size.width, initial_size.height);

        let game: G = self.pending_game.take().expect("pending_game already consumed");
        let mut sim: SimulationCore<G> = SimulationCore::new(time, context, game);
        sim.start()?;

        self.state = AppState::Running(Box::new(RunningState { renderer, window, sim }));
        Ok(())
    }

    async fn init_gpu(
        tx: Sender<BridgePayload>,
        window: Arc<dyn Window>,
        window_mgr: WindowManager,
        proxy: EventLoopProxy,
        config: RendererConfig,
    ) {
        let result: Result<(Renderer, WindowManager), RedixelError> =
            Renderer::new(window, config).await.map(|r: Renderer| (r, window_mgr));

        tx.send(result).ok();
        proxy.wake_up();
    }

    fn spawn_gpu_init(&self, event_loop: &dyn ActiveEventLoop, window_mgr: WindowManager) {
        let tx: Sender<BridgePayload> = self.bridge_tx.clone();
        let window: Arc<dyn Window> = window_mgr.window_arc();
        let proxy: EventLoopProxy = event_loop.create_proxy();
        let config: RendererConfig = self.config.renderer.clone();

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(Self::init_gpu(tx, window, window_mgr, proxy, config));

        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || pollster::block_on(Self::init_gpu(tx, window, window_mgr, proxy, config)));
    }

    fn on_can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        match WindowManager::new(event_loop, &self.config.window) {
            Ok(window) => {
                self.spawn_gpu_init(event_loop, window);
                self.state = AppState::Loading;
            }
            Err(e) => self.abort(event_loop, e),
        }
    }

    fn on_proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        let payload: Result<(Renderer, WindowManager), RedixelError> = match self.bridge_rx.try_recv() {
            Ok(p) => p,
            Err(..) => return,
        };

        match payload {
            Ok((renderer, window)) => {
                if let Err(e) = self.transition_to_running(renderer, window) {
                    self.abort(event_loop, e);
                }
            }
            Err(e) => self.abort(event_loop, e),
        }
    }

    fn on_app_suspended(&mut self) {
        if let AppState::Running(state) = &mut self.state {
            state.renderer.suspend();
        }
    }

    fn on_app_resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.is_suspended = false;

        let result: Result<(), RedixelError> = if let AppState::Running(state) = &mut self.state {
            state.renderer.resume(&state.window.window_arc())
        } else {
            Ok(())
        };

        if let Err(e) = result {
            self.abort(event_loop, e);
        }
    }

    fn run_frame(&mut self, event_loop: &dyn ActiveEventLoop) {
        let AppState::Running(state) = &mut self.state else {
            return;
        };

        state.sim.context.tick_input();
        state.sim.time.begin_frame();

        let frame_delta: f64 = state.sim.time.delta_time();
        state.sim.context.update_timing(frame_delta, state.sim.time.fps());

        match state.sim.run_fixed_updates(frame_delta) {
            StepFlow::Exit => {
                event_loop.exit();
                return;
            }
            StepFlow::Fatal(e) => {
                self.fatal_error = Some(e);
                event_loop.exit();
                return;
            }
            StepFlow::Continue => {}
        }

        state.sim.game.on_update(&mut state.sim.context);

        if state.sim.context.should_exit() {
            event_loop.exit();
            return;
        }

        state.sim.game.on_render(&mut state.sim.context);

        for cmd in state.sim.context.drain_commands() {
            match cmd {
                DrawCommand::ClearColor(c) => {
                    state.renderer.set_clear_color(c);
                }
                DrawCommand::Rect { position, size, color } => {
                    state.renderer.draw_rect(position, size, color);
                }
                DrawCommand::Triangle { p1, p2, p3, color } => {
                    state.renderer.draw_triangle(p1, p2, p3, color);
                }
            }
        }

        match state.renderer.render() {
            Ok(()) => {}
            Err(RedixelError::Surface(SurfaceError::Timeout)) => {}
            Err(RedixelError::Surface(SurfaceError::Lost | SurfaceError::Outdated)) => {
                state.renderer.resize(state.window.surface_size());
            }
            Err(e) => {
                self.fatal_error = Some(e);
                event_loop.exit();
                return;
            }
        }

        state.sim.context.reset_frame();
        state.sim.time.end_frame();
        state
            .sim
            .time
            .every_seconds(1.0, |fps: f64| state.window.set_title_fps(fps));

        state.window.request_redraw();
    }

    fn on_window_event(&mut self, event_loop: &dyn ActiveEventLoop, event: WindowEvent) {
        let AppState::Running(state) = &mut self.state else {
            return;
        };

        if self.is_suspended {
            return;
        }

        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                event_loop.exit();
            }

            WindowEvent::SurfaceResized(size) => {
                state.renderer.resize(size);
                state.sim.context.update_state(size.width, size.height);
            }

            WindowEvent::RedrawRequested => {
                self.run_frame(event_loop);
            }

            ref e => {
                if state.sim.context.process_input_event(e) {
                    return;
                }

                state.window.process_window_event(e);
            }
        }
    }
}

impl<G: Game> ApplicationHandler for Runtime<G> {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        match &mut self.state {
            AppState::Initializing => {
                log::info!("OS requested a surface. Initializing graphics bridge...");
                self.on_can_create_surfaces(event_loop);
            }
            AppState::Loading => {
                log::debug!("OS requested a surface, but ignored (waiting for GPU init).");
            }
            AppState::Running(..) => {
                log::info!("App Resumed. Waking up engine and reconstructing GPU surface.");
                self.on_app_resumed(event_loop);
            }
        }
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        log::info!("OS requested suspension. Halting engine updates.");
        self.is_suspended = true;

        match self.state {
            AppState::Initializing | AppState::Loading => {
                log::debug!("Backgrounded before initialization completed.");
            }
            AppState::Running(..) => {
                log::info!("Dropping active GPU surface to comply with OS background limits.");
                self.on_app_suspended();
            }
        }
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        if matches!(self.state, AppState::Loading) {
            log::info!("GPU Initialization completed asynchronously. Transitioning to Running state.");
            self.on_proxy_wake_up(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.on_window_event(event_loop, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use mpsc::TryRecvError;

    use redixel_core::GameContext;
    use redixel_math::{Color, Vec2};

    struct Dummy;
    impl Game for Dummy {
        type Action = ();
        fn on_start(&mut self, _ctx: &mut dyn GameContext<()>) {}
        fn on_update(&mut self, _ctx: &mut dyn GameContext<()>) {}
        fn on_render(&mut self, _ctx: &mut dyn GameContext<()>) {}
    }

    fn mock_config() -> RuntimeConfig {
        RuntimeConfig {
            target_fps: 60.0,
            tickrate: 60.0,
            #[cfg(feature = "net")]
            net: None,
            window: WindowConfig {
                width: 800,
                height: 600,
                fullscreen: false,
                title: String::from("TEST_TITLE"),
            },
            renderer: RendererConfig {
                backends: wgpu::Backends::all(),
                present_mode: wgpu::PresentMode::AutoVsync,
            },
        }
    }

    #[test]
    fn initial_state_is_initializing() {
        let rt: Runtime<Dummy> = Runtime::new(Dummy, mock_config());
        assert!(matches!(rt.state, AppState::Initializing));
        assert!(rt.fatal_error.is_none());
        assert!(rt.pending_game.is_some());
    }

    #[test]
    fn bridge_channel_is_open() {
        let rt: Runtime<Dummy> = Runtime::new(Dummy, mock_config());
        rt.bridge_tx
            .send(Err(RedixelError::Dummy))
            .expect("channel must be open at construction");
        assert!(rt.bridge_rx.try_recv().is_ok());
    }

    #[test]
    fn bridge_delivers_error_correctly() {
        let rt: Runtime<Dummy> = Runtime::new(Dummy, mock_config());
        rt.bridge_tx.send(Err(RedixelError::Dummy)).unwrap();
        let received: Result<BridgePayload, TryRecvError> = rt.bridge_rx.try_recv();
        assert!(matches!(received.unwrap(), Err(RedixelError::Dummy)));
    }

    #[test]
    fn take_error_moves_and_clears() {
        let mut rt: Runtime<Dummy> = Runtime::new(Dummy, mock_config());
        rt.fatal_error = Some(RedixelError::Dummy);
        assert!(matches!(rt.take_error(), Some(RedixelError::Dummy)));
        assert!(rt.fatal_error.is_none());
    }

    #[test]
    fn context_draw_commands_accumulate() {
        let mut ctx: Context<()> = Context::new();
        ctx.draw_rect(Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0), Color::RED);
        ctx.draw_rect(Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0), Color::BLUE);
        assert_eq!(ctx.commands.len(), 2);

        let drained: Vec<DrawCommand> = ctx.drain_commands().collect();
        assert_eq!(drained.len(), 2);
        assert!(ctx.commands.is_empty());
    }

    #[test]
    fn context_clear_color_deduplicates() {
        let mut ctx: Context<()> = Context::new();

        ctx.clear_color(Color::RED);
        ctx.clear_color(Color::BLUE);

        let clears: Vec<&DrawCommand> = ctx
            .commands
            .iter()
            .filter(|c: &&DrawCommand| matches!(c, DrawCommand::ClearColor(..)))
            .collect();

        assert_eq!(clears.len(), 1);
    }

    #[test]
    fn context_exit_flag_roundtrip() {
        let mut ctx: Context<()> = Context::new();
        assert!(!ctx.should_exit());
        ctx.exit();
        assert!(ctx.should_exit());
        ctx.reset_frame();
        assert!(!ctx.should_exit());
    }

    #[test]
    fn context_timing_update() {
        let mut ctx: Context<()> = Context::new();
        ctx.update_timing(0.016, 62.5);
        assert!((ctx.delta_time() - 0.016).abs() < 1e-9);
        assert!((ctx.fps() - 62.5).abs() < 1e-9);
    }
}
