#[cfg(any(target_os = "android", target_os = "ios"))]
use std::net::SocketAddr;

#[cfg(target_os = "android")]
use winit::platform::android::activity::{AndroidApp, WindowManagerFlags};

pub mod client;
pub mod effects;
pub mod proto;
pub mod server;

pub const DEFAULT_PORT: u16 = 5000;

/// Android has no argv, so the server address is fixed here — edit it to your
/// PC's LAN IP (e.g. `192.168.0.1:5000`) before building the client APK.
#[cfg(target_os = "android")]
const ANDROID_SERVER_ADDR: &str = "127.0.0.1:5000";

/// IOS has no argv, so the server address is fixed here — edit it to your
/// PC's LAN IP (e.g. `192.168.0.1:5000`) before building the client APK.
#[cfg(target_os = "ios")]
const IOS_SERVER_ADDR: &str = "127.0.0.1:5000";

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("REDIXEL_ENGINE"),
    );

    let connect: SocketAddr = ANDROID_SERVER_ADDR
        .parse()
        .expect("ANDROID_SERVER_ADDR must be a valid `ip:port`");

    let config = redixel::build_config().with_net(redixel::prelude::NetConfig::client(connect));

    app.set_window_flags(WindowManagerFlags::KEEP_SCREEN_ON, WindowManagerFlags::empty());
    if let Err(e) = redixel::run_android_with(client::Client::new(), app, config) {
        log::error!("Engine error: {e:?}");
    }
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn ios_main() {
    let connect: SocketAddr = IOS_SERVER_ADDR
        .parse()
        .expect("IOS_SERVER_ADDR must be a valid `ip:port`");
    let config = redixel::build_config().with_net(redixel::prelude::NetConfig::client(connect));

    if let Err(e) = redixel::run_ios_with(client::Client::new(), config) {
        log::error!("Engine error: {e:?}");
    }
}
