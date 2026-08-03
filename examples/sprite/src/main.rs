fn main() {
    #[cfg(not(any(target_os = "android", target_arch = "wasm32", target_os = "ios")))]
    if let Err(e) = sprite::desktop_main() {
        eprintln!("Engine error: {e:?}");
        std::process::exit(0);
    }

    #[cfg(target_arch = "wasm32")]
    if let Err(e) = sprite::wasm_main() {
        panic!("Engine error: {e:?}");
    }
}
