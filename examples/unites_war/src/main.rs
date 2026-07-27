fn main() {
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    if let Err(e) = unites_war::desktop_main() {
        eprintln!("Engine error: {e:?}");
        std::process::exit(1);
    }

    #[cfg(target_arch = "wasm32")]
    if let Err(e) = unites_war::wasm_main() {
        panic!("Engine error: {e:?}");
    }
}
