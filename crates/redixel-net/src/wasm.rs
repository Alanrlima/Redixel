use redixel_core::{NetworkManager, NoOpNetwork};

use crate::config::NetConfig;

/// Browser WebTransport client — not yet implemented.
///
/// Honours [`build`](crate::config::build)'s contract of never failing fatally:
/// logs an error and degrades to a [`NoOpNetwork`], so a WASM build of a
/// networked game still runs (offline) instead of panicking.
pub fn build(_config: &NetConfig, _tickrate: f64) -> Box<dyn NetworkManager> {
    log::error!("Browser WebTransport is not implemented yet; falling back to the no-op network.");
    Box::new(NoOpNetwork)
}
