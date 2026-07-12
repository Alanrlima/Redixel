pub mod client;
pub mod effects;
pub mod proto;
pub mod server;

pub const DEFAULT_PORT: u16 = 5000;

/// Android, iOS, and wasm have no argv, so the server address is fixed here —
/// edit it to your PC's LAN IP (e.g. `192.168.0.1:5000`) before building any
/// of those three clients.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
const SERVER_ADDR: &str = "127.0.0.1:5000";

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod native {
    use std::net::SocketAddr;

    use redixel::prelude::{HeadlessRuntime, NetConfig, RedixelError, RuntimeConfig};

    use crate::{DEFAULT_PORT, client::Client, server::Server};

    pub fn native_main() -> Result<(), RedixelError> {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        let args: Vec<String> = std::env::args().collect();
        let mode: Option<&String> = args.get(1);

        match mode.map(String::as_str) {
            Some("server") => run_server(args.get(2)),
            Some("client") => run_client(args.get(2)),
            _ => {
                eprintln!("usage:");
                eprintln!("  shooter_mp server [BIND_ADDR]     (default 0.0.0.0:5000)");
                eprintln!("  shooter_mp client [SERVER_ADDR]   (default 127.0.0.1:5000)");
                std::process::exit(2);
            }
        }
    }

    fn run_server(addr_arg: Option<&String>) -> Result<(), RedixelError> {
        let bind: SocketAddr = parse_addr(addr_arg, [0, 0, 0, 0]);
        let config: RuntimeConfig = RuntimeConfig::headless().with_net(NetConfig::server(bind));
        log::info!("Starting authoritative server on {bind} ...");
        HeadlessRuntime::new(Server::new(), config).run()
    }

    fn run_client(addr_arg: Option<&String>) -> Result<(), RedixelError> {
        let connect: SocketAddr = parse_addr(addr_arg, [127, 0, 0, 1]);
        let config: RuntimeConfig = redixel::build_config().with_net(NetConfig::client(connect));
        log::info!("Connecting to {connect} ...");
        redixel::run_desktop_with(Client::new(), config)
    }

    fn parse_addr(arg: Option<&String>, default_ip: [u8; 4]) -> SocketAddr {
        let default: SocketAddr = SocketAddr::from((default_ip, DEFAULT_PORT));

        let raw: &String = match arg {
            Some(s) => s,
            None => return default,
        };

        if let Ok(addr) = raw.parse::<SocketAddr>() {
            return addr;
        }

        if let Ok(addr) = format!("{raw}:{DEFAULT_PORT}").parse::<SocketAddr>() {
            return addr;
        }

        log::warn!("Could not parse address '{raw}', using {default}.");
        default
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::net::SocketAddr;

    use winit::platform::android::activity::{AndroidApp, WindowManagerFlags};

    use redixel::prelude::NetConfig;

    use crate::{SERVER_ADDR, client::Client};

    #[unsafe(no_mangle)]
    pub fn android_main(app: AndroidApp) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("REDIXEL_ENGINE"),
        );

        let connect: SocketAddr = SERVER_ADDR.parse().expect("SERVER_ADDR must be a valid `ip:port`");
        let config = redixel::build_config().with_net(NetConfig::client(connect));

        app.set_window_flags(WindowManagerFlags::KEEP_SCREEN_ON, WindowManagerFlags::empty());
        if let Err(e) = redixel::run_android_with(Client::new(), app, config) {
            log::error!("Engine error: {e:?}");
        }
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use std::net::SocketAddr;

    use redixel::prelude::NetConfig;

    use crate::{SERVER_ADDR, client::Client};

    #[unsafe(no_mangle)]
    pub extern "C" fn ios_main() {
        let connect: SocketAddr = SERVER_ADDR.parse().expect("SERVER_ADDR must be a valid `ip:port`");
        let config = redixel::build_config().with_net(NetConfig::client(connect));

        if let Err(e) = redixel::run_ios_with(Client::new(), config) {
            log::error!("Engine error: {e:?}");
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::net::SocketAddr;

    use wasm_bindgen::prelude::*;

    use redixel::prelude::{NetConfig, RedixelError};

    use crate::{SERVER_ADDR, client::Client};

    /// SHA-256 hash (64 lowercase hex chars, no separators) of the server's
    /// self-signed certificate, logged by the native server on startup —
    /// paste it here to let the browser trust a self-signed/LAN server.
    /// Leave empty when connecting to a CA-trusted `server_name` deployment.
    const SERVER_CERT_HASH_HEX: &str = "GENERATED_CERT_HASH";

    fn parse_cert_hash_hex(hex: &str) -> [u8; 32] {
        let mut hash: [u8; 32] = [0; 32];
        for (i, byte) in hash.iter_mut().enumerate() {
            let start: usize = i * 2;
            *byte = u8::from_str_radix(&hex[start..start + 2], 16).expect("SERVER_CERT_HASH_HEX must be 64 hex chars");
        }
        hash
    }

    #[wasm_bindgen(start)]
    pub fn wasm_main() -> Result<(), RedixelError> {
        console_error_panic_hook::set_once();
        console_log::init_with_level(log::Level::Info)?;

        let connect: SocketAddr = SERVER_ADDR.parse().expect("SERVER_ADDR must be a valid `ip:port`");

        let mut net_config: NetConfig = NetConfig::client(connect);
        if !SERVER_CERT_HASH_HEX.is_empty() {
            net_config = net_config.with_server_cert_hash(parse_cert_hash_hex(SERVER_CERT_HASH_HEX));
        }

        let config = redixel::build_config().with_net(net_config);
        redixel::run_wasm_with(Client::new(), config)
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub use native::native_main;

#[cfg(target_arch = "wasm32")]
pub use wasm::wasm_main;
