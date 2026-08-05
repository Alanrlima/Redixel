pub mod client;
pub mod effects;
pub mod proto;
pub mod server;

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod native;
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
mod preset;

redixel::entry_point! {
    game: preset::client(),
    config: preset::config(),
    desktop: native::run,
}
