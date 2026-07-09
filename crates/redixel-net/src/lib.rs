mod inbound;
mod seq;
#[cfg(target_arch = "wasm32")]
mod wasm;

pub mod config;
pub mod loopback;
#[cfg(not(target_arch = "wasm32"))]
pub mod webtransport;

pub use config::{CertSource, DEFAULT_PROTOCOL_ID, NetConfig, NetMode, build};
pub use loopback::LoopbackNetwork;
#[cfg(not(target_arch = "wasm32"))]
pub use webtransport::{WebTransportClient, WebTransportServer};
