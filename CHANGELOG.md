# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),  
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.3.0]

### Added

- **Networking Core (`redixel-core::net`):**
  - Introduced the backend-agnostic `NetworkManager` trait exposed via `ctx.network()`, covering `poll()` (drains `Connected`/`Disconnected`/`Message` events with zero-allocation borrowed payloads), `send`/`broadcast` over two delivery guarantees (`NetworkChannel::ReliableOrdered`, `UnreliableSequenced`), `rtt()`, `is_connected()`, and `server_tickrate()` for automatic client tickrate adoption.
  - Added `ClientId`/`SERVER_ID`, `NoOpNetwork` (zero-cost default when no transport is configured), and `SequenceBuffer<T>` — a fixed-capacity ring buffer for client-side prediction/reconciliation.
  - `Game::on_fixed_update` (default no-op) plus `GameContext::fixed_delta()`/`fixed_tick()`, run on a deterministic accumulator so simulation and network ticks stay decoupled from render framerate.
- **`redixel-net` crate — WebTransport transport:**
  - New crate implementing `NetworkManager` over **WebTransport (QUIC/HTTP-3)** via `wtransport`, unifying reliable and unreliable delivery on one encrypted connection: framed reliable streams for `ReliableOrdered`, and datagrams with RFC 1982 sequence framing (newest-wins, stale-drop) for `UnreliableSequenced`.
  - Unreliable messages larger than the connection's datagram limit are **fragmented across datagrams** and reassembled newest-wins on receipt (an incomplete older sequence is abandoned the moment a newer one starts). A message that cannot be sent is dropped, never promoted onto the reliable stream — so the channel's ordering guarantee holds for payloads of any size, and a large snapshot can never head-of-line block genuine reliable traffic.
  - `NetConfig`/`NetMode` (`Server`/`Client`/`Offline`) with `CertSource::SelfSigned` (LAN/self-host) and `CertSource::Pem` (production domain certs). The handshake exchanges a hello frame carrying the client's `protocol_id` — a mismatch closes the connection before the game ever observes the peer — and a welcome frame assigning each client its `ClientId` plus the server's authoritative tickrate. The server admits at most `max_clients` peers, turning further sessions away with an HTTP 429; a pending handshake occupies a slot, so it is bounded by a 10 s timeout that reclaims the slot and closes the connection.
  - Every length a peer controls is bounded: reliable frames cap at 4 MiB, and datagram reassembly caps both the fragment count (1024) and the reassembled message (1 MiB), so a peer cannot size the receiver's allocations.
  - The background tokio runtime is sized for the role — 2 worker threads on a server, 1 on a client — instead of tokio's default of one per core: the QUIC work is I/O-bound, and a client's extra workers would only contend with the render thread.
  - `LoopbackNetwork` — an in-process server/client pair for tests and single-process hosting, exercising the same channel/sequencing semantics as the real transport.
- **Headless / Dedicated Server mode:** `RuntimeConfig::headless()` and the new `HeadlessRuntime` run the engine with no `winit`/`wgpu`, driving only `on_start` and the fixed-update loop — the same `Game` implementation runs unmodified as an authoritative Linux VPS server. `RuntimeConfig::with_net()` enables networking on any configuration.
- **Multiplayer example:** A new authoritative-server multiplayer twin-stick shooter demo (headless server + windowed client) demonstrating the full stack end-to-end — weapons/powerups replication, and Android/iOS mobile client support. It models the channel split the engine prescribes: superseded state (world snapshots, player input) rides `UnreliableSequenced`, while discrete one-shot events (hit/death/pickup cosmetics, batched as `EffectBatch`) ride `ReliableOrdered`, since a dropped effect would never play again. The client additionally drops any snapshot not strictly newer than the last applied tick.
- **`redixel::prelude`** now re-exports `CertSource` and `DEFAULT_PROTOCOL_ID` alongside `NetConfig`/`NetMode` — they are the types of `NetConfig`'s `cert` and `protocol_id` fields, so without them a crate depending only on `redixel` could not configure a production TLS certificate.
- **`net` Cargo feature:** `redixel-net` is an optional, feature-gated dependency (`redixel`/`redixel-runtime` crates) so games that don't need networking pay zero cost for it.
- `EngineSettings::load_config_json()` helper and a new `engine.tickrate` key in `config/config.json`.
- `TimeManager::interpolation_alpha()` (render-time interpolation support) and `set_max_substeps()` (spiral-of-death clamp, now configurable).
- **CI:** Added a `macOS` entry to the `Desktop` job matrix (build + test + clippy on `macos-latest`) and a new dedicated `iOS` job (clippy + `--lib` build of every example for `aarch64-apple-ios`).
- **CI: extended example coverage to every job.** `format`, `desktop`, and `wasm` invoke `--workspace`/check the root package, which no longer includes `examples/*` after the workspace restructuring above — only `android`/`ios` were already per-package (`cargo apk build`/`--manifest-path`). Added a reusable composite action (`.github/actions/cargo-each`) that runs a cargo subcommand against the root workspace and every `examples/*/Cargo.toml`, wired into all five jobs. Networked examples with heavier native dependencies are no longer excluded from the Android/iOS build steps — only the WASM job still excludes examples that don't support that target.
- `staticlib` added to every example's `crate-type` (alongside `cdylib`/`rlib`) — the crate now also builds as a static library, the standard way to embed a Rust library in an Xcode project.
- **iOS entry point:** `redixel::run_ios`/`run_ios_with`, using the portable `EventLoop::run_app` (winit has no `run_app_on_demand` on iOS — that's desktop-only) so no `unsafe` is needed at that layer. Each example exposes a `#[unsafe(no_mangle)] extern "C" fn ios_main()` — the same idea as Android's JNI-loaded `android_main`, just via a different OS-level mechanism. Unlike desktop/WASM, this is _not_ reached through the crate's own `fn main()`: `UIApplicationMain` must be called before anything else touches UIKit, so winit needs to own the actual process entry — the exported symbol is meant to be called from an Xcode project with no competing `main.swift`/`AppDelegate`.

### Changed

- **Workspace restructuring:** Examples are no longer members of the root Cargo workspace — each is now its own standalone, nested workspace with an independent `Cargo.lock`. This decouples the engine's build/test graph from per-example dependencies, at the cost of needing `--manifest-path examples/<name>/Cargo.toml` (rather than `-p <name>`) to target a specific example from the repo root.
- **`redixel-runtime` internals:** Extracted the shared fixed-step loop (timing → network update → `on_fixed_update` → network flush) into a new internal `SimulationCore`, used identically by the windowed `Runtime` and the new `HeadlessRuntime`.
- Updated `README.md`: all example commands now use `--manifest-path` per the workspace restructuring above; added a **Multiplayer** section documenting the authoritative-server/client workflow and required firewall port; added a Wayland/XWayland performance note; added a **Running on iOS** section.
- `deploy-frontend.yml`: switched WASM example discovery from `cargo metadata` (which no longer sees examples now that they've left the root workspace) to a direct `grep` over `examples/*/Cargo.toml`.
- **CI/CD Pipeline:** Enforced a global `CARGO_TARGET_DIR` environment variable across GitHub Actions. This allows the `cargo-each` script to share compiled artifacts (like `wgpu` and `tokio`) across the newly isolated example workspaces, eliminating redundant from-scratch compilations and drastically reducing CI runtimes.

## [0.2.0]

### Added

- **Unified InputSource System**:
  - Introduced the `InputSource` enum in `redixel-core` to seamlessly merge `KeyCode` and `MouseButton` bindings.
  - Added native mouse querying methods to `InputQuery` (`mouse_just_pressed`, `mouse_held`, `mouse_just_released`).
  - Added support for real-time cursor position tracking (`mouse_position`) and mouse wheel delta accumulation (`scroll_delta`).
- **Automated Web Deployment Pipeline:**
  - Implemented CI/CD workflow to compile examples into WASM and sync artifacts to the frontend repository.
- **Runtime:** Introduced the `RuntimeConfig` struct to explicitly inject Window, Renderer, and target FPS settings into the engine.
- **Time:** Introduced `display_fps()` to `TimeManager`, which calculates a smoothed rolling average of the framerate using a zero-allocation, fixed-size ring buffer.
- **Android Support:** - Integrated `android_logger` for native Logcat integration.
  - Implemented platform-specific entry point via `#[unsafe(no_mangle)] android_main`.
  - Added JNI-based lifecycle management (suspend/resume) ensuring graphics resource safety.
  - Configured `Cargo.toml` targets for `aarch64-linux-android` support.
- **Cross-Platform Architecture:**
  - Standardized entry points for PC, Web, and Android to ensure platform-agnostic `Game` trait implementation.
  - Optimized memory management for mobile constraints using `Box::into_raw` and reborrowing strategies.

### Changed

- **Event Loop Cascading**: Streamlined the main event loop using the _Chain of Responsibility_ pattern, safely delegating OS events between the `Context` and `WindowManager`.
- **InputManager Refactoring & Double Buffering**:
  - `InputBind::bind` now accepts a unified `InputSource` instead of a raw `KeyCode`, altering the public API.
  - Implemented a robust **Double Buffering** system with event queues (`pending_keys`, `pending_mouse`) to prevent dropped OS events.
  - Added a **Deferral Strategy** within the `tick()` lifecycle to completely eliminate "Phantom Clicks" (rapid press/release events within the same frame are now safely buffered and executed across frames).
- **Runtime Architecture Overhaul:** Refactored the core runtime module for better testability and maintainability:
  - Decoupled `Runtime` from the global `EngineSettings` singleton by introducing explicit dependency injection via the new `RuntimeConfig` struct.
  - Extracted the massive core game loop and rendering pipeline into a dedicated, clean `run_frame()` method.
  - Made event delegation explicit in `on_window_event` by replacing implicit match guards with clear `if` statements and early returns.
- **Core:** The main composition root (`redixel::run`) now handles reading the global state and assembling the `RuntimeConfig` prior to engine startup.
- **Tests:** Refactored `runtime.rs` unit tests to use mocked configurations (`mock_config()`), allowing them to run in parallel without shared global state.
- **Time:** The `every_seconds()` callback now yields the smoothed `display_fps()` instead of the raw, instantaneous FPS. This prevents UI counters and window titles from jittering rapidly due to OS context switching, while keeping the internal game physics strictly tied to the raw `delta_time()`.

## [0.1.0]

### Added

- `CONTRIBUTING.md` guide enforcing strict coding styles, environment setup, and CI workflow.
- Initial project structure for the **Redixel Engine**.
- Engine bootstrap (`main.rs`, `lib.rs`) with `redixel::init()` entry point.
- **Runtime system** implementing `winit::ApplicationHandler`, orchestrating:
  - Event processing
  - Surface creation
  - Redraw requests
- **Platform layer**:
  - `WindowManager` for window creation, lifecycle handling, and redraw requests.
  - `InputManager` for basic input event dispatch (keyboard, mouse wheel, pointer movement).
- **Graphics layer**:
  - Implemented **`RendererDevice`**, handling:
    - WGPU instance creation (`Instance`)
    - Surface creation from a `winit` window
    - Adapter selection with `HighPerformance` preference
    - Device & queue creation via `request_device`
    - Automatic surface format and present-mode selection
    - Surface configuration (`SurfaceConfiguration`) including SRGB format detection
  - Implemented **`Renderer`**, providing:
    - Clear-color rendering pipeline (basic render pass)
    - Swapchain acquisition (`get_current_texture`)
    - Command encoder creation & submission
    - Resize handling that updates surface configuration
    - Presentation of rendered frames
- **Web Assembly (WASM) Support**:
  - Enabled `wasm32-unknown-unknown` target support.
  - Integrated `wasm-bindgen` for JavaScript interoperability.
  - Added `console_error_panic_hook` for mapping Rust panics to the browser console.
  - Enabled `wgpu`'s `webgl` feature flag for broad browser compatibility.
  - Implemented DOM manipulation logic to attach the `winit` window to the HTML Canvas.
- **Engine module layout** (`engine`, `runtime`, `platform/input`, `platform/window`, `graphics/renderer`, `graphics/renderer_device`).
- CI pipeline (`.github/workflows/ci.yml`) including toolchain bootstrap, fmt, and clippy checks.
- Repository configuration files (`rust-toolchain.toml`, `rustfmt.toml`).
- **Error Handling System**:
  - Implemented a centralized `RedixelError` enum using `thiserror` to capture and contextually wrap errors from `winit`, `wgpu`, and `web-sys`.
  - Added robust error propagation across the runtime, enabling graceful shutdown on failure.
  - Integrated `log` crate with `env_logger` (Desktop) and `console_log` (WASM) for structured logging and debugging.
- **TimeManager and Limiting**:
  - Implemented `TimeManager` for precise frame timing, delta-time calculation, and performance monitoring.
  - Added a high-precision **hybrid sleep/spin-lock** mechanism to enforce target framerates with minimal CPU overhead.
- **Configuration System**:
  - Implemented **`EngineSettings`** as a thread-safe global singleton (`OnceLock`, `RwLock`) enabling concurrent access from any thread.
  - Integrated `serde` and `serde_json` for robust parsing of external `config.json` files with automatic error recovery and logging.
  - Added a generic `get_path<T>` utility for querying nested settings using dot-notation strings (e.g., `"renderer.present_mode"`).
  - Implemented logic to map integer configuration values directly to `wgpu` enums (Backend, PresentMode).
  - Added `CONFIG.md` documentation comprehensively detailing the `app`, `window`, and `renderer` schemas and their default behaviors.
- **Unit Tests for Core Logic:** Implemented comprehensive unit tests across key engine components:
  - **`Runtime`**: Verifies core state management, fatal error capture, and reliable asynchronous communication channel (MPSC bridge) operation.
  - **`TimeManager`**: Validates FPS calculation accuracy, frame limiting precision, correct target duration conversion, and reliable interval callback triggering.
  - **`InputManager`**: Confirms accurate event filtering to distinguish between valid player inputs (Keyboard, Pointer, Scroll) and system events.
  - **`WindowManager`**: Ensures precise FPS title formatting and correct event filtering logic for window-specific events (e.g., Focus, Scaling).
- **Continuous Integration (CI) Enhancements:**
  - Integrated essential Linux graphics dependencies (`xvfb` and `mesa-vulkan-drivers`) to enable integration testing of graphics-dependent code via CPU-emulated Vulkan.
- **Math Library (`redixel-math`)**:
  - Implemented core linear algebra structures: `Vec2`, `Mat4`, and `Color`.
  - Added logic for orthographic projections, matrix multiplication (column-major for GPU), vector normalization, and lerping.
- **Type-Safe Input System**:
  - Upgraded `InputManager` with a generic, zero-overhead `InputAction` trait.
  - Implemented strict state machine tracking (`JustPressed`, `Held`, `JustReleased`).
  - Added OS-level key repeat filtering and decoupled read/write access via `InputQuery` and `InputBind` traits.
- **Game API & Context (`redixel-core`)**:
  - Introduced the main `Game` trait (`on_start`, `on_update`, `on_render`).
  - Implemented `GameContext` to expose a safe, unified interface to the user without exposing internal dependencies.
  - Created the `DrawCommand` queue to buffer rendering primitives (`ClearColor`, `Rect`, `Triangle`) with intelligent deduplication logic.
- **Engine Prelude**:
  - Added `redixel::prelude::*` to drastically improve Developer Experience (DX) and streamline imports for game developers.

### Changed

- Updated `LICENSE` copyright to "Redixel Core Team".
- Updated `README.md` with professional formatting and architecture overview.
- Updated `ROADMAP.md` to reflect the current technical status of Phase 1 and next infrastructure steps.
- Refactored core initialization modules (`WindowManager::new`, `Renderer::new`, `Runtime`) to return `Result<T, RedixelError>`, eliminating fragile `unwrap()` and `expect()` calls in critical paths.
- Updated Application Entry Points:
  - **Desktop (`main`)**: Now returns `Result` and prints formatted fatal errors to `stderr` via the logging system.
  - **WASM (`init`)**: Now implements `From<RedixelError>` for `JsValue`, ensuring Rust errors are correctly mapped and displayed as exceptions in the Browser Console.
- **Architectural Overhaul (Cargo Workspace):** Migrated the monolithic structure into a strict Cargo Workspace (`redixel-core`, `redixel-platform`, `redixel-renderer`, `redixel-runtime`).
- **Public API Facade:** Introduced the `redixel` crate to act as a clean, unified public API for end-users, hiding internal complexity.
- **Pure Rust WebAssembly:** Eliminated external `index.html` and JavaScript bindings setup. The engine now dynamically injects the `<canvas>` into the DOM and enforces styling purely via Rust (`web-sys`).
- Updated `winit` Web API integration to safely build `WindowAttributesWeb` without relying on deprecated trait extensions.
- Removed dead code (e.g., `SetLoggerError` from engine error variants) to ensure the framework remains unopinionated about the consumer's logging setup.
- **Internal API Encapsulation**: Refactored internal crates (`redixel-runtime`, `redixel-platform`) to use private modules and surgical `pub use` exports, preventing namespace pollution and protecting internal structures.
