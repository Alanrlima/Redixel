use std::net::SocketAddr;

use redixel::prelude::{HeadlessRuntime, NetConfig, RedixelError, RuntimeConfig};

use crate::{DEFAULT_PORT, client::Client, server::Server};

/// Desktop is the only platform with argv, so it is the only one that can pick
/// between hosting the authoritative server and joining one. `entry_point!`
/// routes the generated `main()` here with logging already installed.
pub fn run() -> Result<(), RedixelError> {
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
