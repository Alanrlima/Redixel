use redixel_core::NetworkManager;

use crate::config::NetConfig;

/// Browser WebTransport client — not yet implemented, see
/// `redixel::run_wasm_with`. Unreachable today: that function already panics
/// before construction reaches this call.
pub fn build(_config: &NetConfig, _tickrate: f64) -> Box<dyn NetworkManager> {
    todo!("browser WebTransport client not yet implemented")
}
