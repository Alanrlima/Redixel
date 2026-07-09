use std::{net::SocketAddr, path::PathBuf};

use redixel_core::{NetworkManager, NoOpNetwork};

/// Default netcode protocol id. Override per game to reject clients built
/// against a mismatched version early.
pub const DEFAULT_PROTOCOL_ID: u64 = 0x5245_4449_5845_4C00;

/// What role this peer plays on the network.
#[derive(Debug, Clone)]
pub enum NetMode {
    Server { bind: SocketAddr },
    Client { connect: SocketAddr },
    Offline,
}

/// TLS certificate source for the server. `SelfSigned` (default) works for
/// LAN/self-host, since native clients skip validation; use `Pem` with a real
/// domain certificate for public/online hosting.
#[derive(Debug, Clone, Default)]
pub enum CertSource {
    #[default]
    SelfSigned,
    Pem {
        cert: PathBuf,
        key: PathBuf,
    },
}

/// Declarative networking configuration consumed by [`build`]. `server_name`
/// (client only) selects TLS mode: `Some` validates against a real domain
/// cert (online), `None` connects by IP with validation disabled (LAN).
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub mode: NetMode,
    pub max_clients: usize,
    pub protocol_id: u64,
    pub cert: CertSource,
    pub server_name: Option<String>,
}

impl NetConfig {
    /// A server bound to `bind` with sensible defaults.
    pub fn server(bind: SocketAddr) -> Self {
        Self {
            mode: NetMode::Server { bind },
            max_clients: 64,
            protocol_id: DEFAULT_PROTOCOL_ID,
            cert: CertSource::default(),
            server_name: None,
        }
    }

    /// A client connecting to `connect` with sensible defaults.
    pub fn client(connect: SocketAddr) -> Self {
        Self {
            mode: NetMode::Client { connect },
            max_clients: 1,
            protocol_id: DEFAULT_PROTOCOL_ID,
            cert: CertSource::default(),
            server_name: None,
        }
    }
}

/// Builds the [`NetworkManager`] for `config` on the current platform.
///
/// `tickrate` is the local authoritative rate: a server embeds it in its
/// welcome frame for clients to adopt; a client ignores its own value entirely
/// and adopts whatever the server announces instead (see
/// [`NetworkManager::server_tickrate`](redixel_core::NetworkManager::server_tickrate)).
///
/// Never fails fatally: a transport that cannot start (e.g. a server whose
/// socket fails to bind) logs an error and degrades to a [`NoOpNetwork`] so the
/// game keeps running. Use the backend constructors directly if you need to
/// handle binding errors explicitly.
pub fn build(config: &NetConfig, tickrate: f64) -> Box<dyn NetworkManager> {
    if matches!(config.mode, NetMode::Offline) {
        return Box::new(NoOpNetwork);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::webtransport::build(config, tickrate)
    }

    #[cfg(target_arch = "wasm32")]
    {
        crate::wasm::build(config, tickrate)
    }
}
