use winit::{event::MouseButton, keyboard::KeyCode};

use redixel_math::{Color, Vec2};

use crate::{InputAction, InputSource, RedixelError, net::NetworkManager};

/// The entry point for user game logic.
///
/// The associated type `Action` is your game's input action enum. The engine
/// is generic over it — `GameContext` exposes a typed input API with zero
/// overhead.
///
/// ```rust,ignore
/// #[derive(Clone, PartialEq, Eq, Hash)]
/// enum MyAction { MoveUp, Shoot }
///
/// struct MyGame;
///
/// impl Game for MyGame {
///     type Action = MyAction;
///
///     fn on_start(&mut self, ctx: &mut dyn GameContext<MyAction>) {
///         // Bind keys and mouse buttons using `.into()`
///         ctx.input_mut().bind(MyAction::MoveUp, KeyCode::KeyW.into());
///         ctx.input_mut().bind(MyAction::Shoot, MouseButton::Left.into());
///     }
///
///     fn on_update(&mut self, ctx: &mut dyn GameContext<MyAction>) {
///         if ctx.input().held(MyAction::MoveUp) { /* ... */ }
///         
///         if ctx.input().just_pressed(MyAction::Shoot) {
///             if let Some(pos) = ctx.input().mouse_position() {
///                 // Shoot towards mouse position
///             }
///         }
///     }
///
///     fn on_render(&mut self, ctx: &mut dyn GameContext<MyAction>) {
///         ctx.draw_rect(Vec2::new(0.0, 0.0), Vec2::new(50.0, 50.0), Color::WHITE);
///     }
/// }
/// ```
pub trait Game: 'static {
    /// The action enum that maps to keybinds/mouse for this game.
    ///
    /// Use `type Action = ()` if you don't need input.
    type Action: InputAction;

    /// Called once after the GPU context is ready. Bind keys and load assets here.
    ///
    /// On a headless server there is no GPU context — this still runs first so
    /// the same game code can set up its world and networking.
    fn on_start(&mut self, ctx: &mut dyn GameContext<Self::Action>);

    /// Called on a fixed cadence (the tickrate), decoupled from the render
    /// framerate, using an accumulator in the `TimeManager`. May run zero, one,
    /// or several times per rendered frame.
    ///
    /// Put deterministic simulation here: physics, and **all** networking
    /// (poll inbound events, step the authoritative world, broadcast state).
    /// `ctx.fixed_delta()` is constant; `ctx.fixed_tick()` is a monotonic tick
    /// counter — tag inputs/snapshots with it for prediction/reconciliation.
    ///
    /// This is the **only** game callback invoked in headless/server mode.
    /// Default is a no-op so non-networked games need not implement it.
    fn on_fixed_update(&mut self, _ctx: &mut dyn GameContext<Self::Action>) {}

    /// Called every rendered frame before rendering. Update visual/interpolated
    /// state here. Not invoked in headless/server mode.
    fn on_update(&mut self, ctx: &mut dyn GameContext<Self::Action>);

    /// Called every rendered frame after `on_update`. Issue draw calls here.
    /// Not invoked in headless/server mode.
    fn on_render(&mut self, ctx: &mut dyn GameContext<Self::Action>);
}

/// The interface through which [`Game`] methods talk to the engine each frame.
///
/// Generic over `A: InputAction` so that `input()` returns a typed view
/// without any dynamic dispatch or downcasting.
pub trait GameContext<A: InputAction> {
    /// Requests a clean engine shutdown after the current frame.
    fn exit(&mut self);

    /// Returns `true` if [`exit`] was called this frame.
    fn should_exit(&self) -> bool;

    /// Seconds elapsed between the two most recent frames (delta time).
    ///
    /// Use in `on_update` for frame-rate-dependent visuals. Inside
    /// `on_fixed_update`, prefer [`fixed_delta`](Self::fixed_delta).
    fn delta_time(&self) -> f64;

    /// The constant timestep of the fixed-update loop, in seconds (e.g. `1/60`).
    ///
    /// This is the dt to integrate with inside `on_fixed_update` for
    /// deterministic, framerate-independent simulation.
    fn fixed_delta(&self) -> f64;

    /// Monotonic count of fixed-update ticks since startup.
    ///
    /// Stamp inputs and snapshots with this for client-side prediction and
    /// server reconciliation (see [`SequenceBuffer`](crate::net::SequenceBuffer)).
    fn fixed_tick(&self) -> u64;

    /// Current FPS measurement.
    fn fps(&self) -> f64;

    /// Access the networking transport.
    ///
    /// Always returns a valid manager — a zero-cost
    /// [`NoOpNetwork`](crate::net::NoOpNetwork) when networking is disabled — so
    /// game code never needs to branch on whether the net is configured.
    fn network(&mut self) -> &mut dyn NetworkManager;

    /// Width of the rendering surface in pixels.
    fn surface_width(&self) -> u32;

    /// Height of the rendering surface in pixels.
    fn surface_height(&self) -> u32;

    /// Returns a read-only view of the current input state.
    ///
    /// Use this to query actions, raw keys, and mouse state.
    fn input(&self) -> &dyn InputQuery<A>;

    /// Returns a mutable handle to bind actions to input sources.
    ///
    /// Call this in `on_start` to register your input bindings.
    fn input_mut(&mut self) -> &mut dyn InputBind<A>;

    /// Sets the background clear colour for this frame.
    fn clear_color(&mut self, color: Color);

    /// Draws a filled triangle.
    ///
    /// - `p1`, `p2`, `p3` — The three vertices of the triangle in world coordinates
    /// - `color`          — fill colour
    fn draw_triangle(&mut self, p1: Vec2, p2: Vec2, p3: Vec2, color: Color);

    /// Draws a filled, axis-aligned rectangle.
    ///
    /// - `position` — top-left corner in world/screen coordinates (y-down)
    /// - `size`     — width × height in pixels
    /// - `color`    — fill colour
    fn draw_rect(&mut self, position: Vec2, size: Vec2, color: Color);

    /// Extracts any pending engine error out of the context.
    fn take_error(&mut self) -> Option<RedixelError>;
}

/// Read-only input queries for the current frame.
pub trait InputQuery<A: InputAction> {
    /// Returns `true` on the exact frame the action's bound input went down.
    fn just_pressed(&self, action: A) -> bool;

    /// Returns `true` every frame the action's bound input is held down.
    fn held(&self, action: A) -> bool;

    /// Returns `true` on the exact frame the action's bound input came up.
    fn just_released(&self, action: A) -> bool;

    /// Returns `true` if the action is down in any capacity.
    fn is_down(&self, action: A) -> bool {
        self.just_pressed(action.clone()) || self.held(action)
    }

    /// Returns `true` if a raw `KeyCode` is currently held, bypassing bindings.
    /// Useful for debug keys or engine-level shortcuts.
    fn key_held(&self, key: KeyCode) -> bool;

    /// Returns `true` if a raw `KeyCode` was just pressed this frame.
    fn key_just_pressed(&self, key: KeyCode) -> bool;

    /// Returns `true` if a raw `KeyCode` was just released this frame.
    fn key_just_released(&self, key: KeyCode) -> bool;

    /// Returns `true` if a raw `MouseButton` is currently held, bypassing bindings.
    fn mouse_held(&self, button: MouseButton) -> bool;

    /// Returns `true` if a raw `MouseButton` was just pressed this frame.
    fn mouse_just_pressed(&self, button: MouseButton) -> bool;

    /// Returns `true` if a raw `MouseButton` was just released this frame.
    fn mouse_just_released(&self, button: MouseButton) -> bool;

    /// Returns the current cursor position in surface pixels.
    /// Returns `None` if the cursor is outside the window.
    fn mouse_position(&self) -> Option<Vec2>;

    /// Returns the accumulated mouse scroll delta for the current frame.
    /// `x` represents horizontal scrolling, `y` represents vertical.
    fn scroll_delta(&self) -> Vec2;
}

/// Mutable binding configuration — call only in `on_start`.
pub trait InputBind<A: InputAction> {
    /// Binds an action to a source (Keyboard Key or Mouse Button).
    /// Multiple sources can share the same action.
    fn bind(&mut self, action: A, source: InputSource);

    /// Removes all bindings for `action`.
    fn unbind(&mut self, action: A);

    /// Removes all bindings entirely.
    fn clear_bindings(&mut self);
}
