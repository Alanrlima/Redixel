use redixel_core::{Game, RedixelError, game::GameContext};

use crate::{context::Context, time::TimeManager};

/// How the caller should proceed after advancing the fixed-update simulation:
/// every owed step ran (`Continue`), the game requested shutdown mid-step
/// (`Exit`), or a step recorded a fatal error (`Fatal`).
pub(crate) enum StepFlow {
    Continue,
    Exit,
    Fatal(RedixelError),
}

/// Owns the framerate-independent simulation state — timing, the game context,
/// and the game itself — and provides the single fixed-update loop shared by the
/// windowed [`Runtime`](crate::runtime::Runtime) and the
/// [`HeadlessRuntime`](crate::runtime::HeadlessRuntime).
pub(crate) struct SimulationCore<G: Game> {
    pub(crate) time: TimeManager,
    pub(crate) context: Context<G::Action>,
    pub(crate) game: G,
}

impl<G: Game> SimulationCore<G> {
    /// Bundles the timing, context, and game into one simulation owner.
    pub(crate) fn new(time: TimeManager, context: Context<G::Action>, game: G) -> Self {
        Self { time, context, game }
    }

    /// Runs `on_start` once and surfaces any error the game recorded.
    pub(crate) fn start(&mut self) -> Result<(), RedixelError> {
        self.game.on_start(&mut self.context);
        match self.context.take_error() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Feeds elapsed real time into the fixed-step accumulator and runs every
    /// owed step: advance the transport, adopt the server's authoritative
    /// tickrate the moment a connected client learns it (a no-op everywhere
    /// else — see [`NetworkManager::server_tickrate`](redixel_core::net::NetworkManager::server_tickrate)),
    /// stamp the fixed timing, invoke `on_fixed_update`, then flush the
    /// transport. Stops early on a recorded error or an exit request.
    pub(crate) fn run_fixed_updates(&mut self, frame_delta: f64) -> StepFlow {
        self.time.accumulate(frame_delta);
        while self.time.next_fixed_step() {
            self.context.network.update(self.time.fixed_delta());

            if let Some(rate) = self.context.network.server_tickrate() {
                self.time.set_tickrate(rate);
            }

            let fixed_delta: f64 = self.time.fixed_delta();
            self.context.set_fixed(fixed_delta, self.time.fixed_tick());
            self.game.on_fixed_update(&mut self.context);
            self.context.network.flush();

            if let Some(e) = self.context.take_error() {
                return StepFlow::Fatal(e);
            }

            if self.context.should_exit() {
                return StepFlow::Exit;
            }
        }

        StepFlow::Continue
    }
}
