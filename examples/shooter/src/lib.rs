#[cfg(not(any(target_os = "android", target_os = "ios")))]
use redixel::prelude::RedixelError;

pub mod common;
pub mod effects;
pub mod game;

use game::Shooter;

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn desktop_main() -> Result<(), RedixelError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    redixel::run_desktop(Shooter::new())?;
    Ok(())
}

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn ios_main() {
    if let Err(e) = redixel::run_ios(Shooter::new()) {
        log::error!("Engine error: {e:?}");
    }
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() -> Result<(), RedixelError> {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info)?;
    redixel::run_wasm(Shooter::new())?;
    Ok(())
}

#[cfg(target_os = "android")]
use winit::platform::android::activity::{AndroidApp, WindowManagerFlags};

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("REDIXEL_ENGINE"),
    );

    app.set_window_flags(WindowManagerFlags::KEEP_SCREEN_ON, WindowManagerFlags::empty());
    if let Err(e) = redixel::run_android(Shooter::new(), app) {
        log::error!("Engine error: {e:?}");
    }
}
