fn main() {
    #[cfg(not(any(target_os = "android", target_arch = "wasm32", target_os = "ios")))]
    if let Err(e) = shooter_mp::native_main() {
        eprintln!("Engine error: {e:?}");
        std::process::exit(1);
    }

    #[cfg(target_arch = "wasm32")]
    if let Err(e) = shooter_mp::wasm_main() {
        panic!("Engine error: {e:?}");
    }
}
