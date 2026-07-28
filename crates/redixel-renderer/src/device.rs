use std::sync::Arc;

use wgpu::{
    Adapter, AdapterInfo, Backend, BackendOptions, Backends, Device, DeviceType, ExperimentalFeatures, Features,
    Instance, InstanceDescriptor, InstanceFlags, MemoryBudgetThresholds, MemoryHints, PowerPreference, PresentMode,
    Queue, RequestAdapterOptions, Surface, SurfaceCapabilities, SurfaceColorSpace, SurfaceConfiguration, TextureFormat,
    TextureUsages, Trace,
    wgt::{DeviceDescriptor, SurfaceConfiguration as WgtSurfaceConfiguration},
};

use winit::{dpi::PhysicalSize, window::Window};

use redixel_core::RedixelError;

use crate::renderer::RendererConfig;

/// Formats an adapter for logging, tolerating the fields a backend leaves blank.
///
/// Browsers deliberately withhold adapter identity to limit fingerprinting, so
/// under WebGPU the name and both driver strings arrive empty. Blank fields are
/// replaced or dropped rather than logged as empty text; the device type and
/// backend are populated on every platform and carry the useful signal when the
/// rest is missing.
///
/// Takes the individual fields rather than an [`AdapterInfo`] so it stays
/// testable without constructing one — `wgpu` adds fields to that struct
/// between releases.
fn describe_adapter(name: &str, driver: &str, driver_info: &str, device_type: DeviceType, backend: Backend) -> String {
    let name: &str = match name.trim() {
        "" => "unidentified adapter",
        name => name,
    };

    let driver: String = match (driver.trim(), driver_info.trim()) {
        ("", "") => String::new(),
        ("", detail) | (detail, "") => format!(" driver: {detail}"),
        (driver, detail) => format!(" driver: {driver} {detail}"),
    };

    format!("{name} [{device_type:?} / {backend:?}]{driver}")
}

/// Owns the WGPU logical device, presentation surface, and submission queue.
///
/// This is a pure graphics-layer type with no knowledge of the windowing
/// system beyond the `Arc<dyn Window>` it receives at construction time.
/// All configurable behaviour is injected via [`RendererConfig`].
#[derive(Debug)]
pub(crate) struct GpuDevice {
    pub(crate) instance: Instance,
    pub(crate) surface: Option<Surface<'static>>,
    pub(crate) device: Device,
    pub(crate) queue: Queue,
    pub(crate) config: SurfaceConfiguration,
}

impl GpuDevice {
    pub(crate) async fn new(window: Arc<dyn Window>, cfg: &RendererConfig) -> Result<Self, RedixelError> {
        let instance: Instance = Self::create_instance(cfg.backends);
        let surface: Surface<'_> = Self::create_surface(&instance, &window)?;
        let adapter: Adapter = Self::request_adapter(&instance, &surface).await?;

        let (device, queue): (Device, Queue) = Self::request_device(&adapter).await?;
        let config: WgtSurfaceConfiguration<Vec<TextureFormat>> =
            Self::build_surface_config(&window, &surface, &adapter, cfg.present_mode);

        surface.configure(&device, &config);
        Self::log_adapter(&adapter);

        Ok(Self {
            device,
            queue,
            config,
            instance,
            surface: Some(surface),
        })
    }

    /// Reconfigures the swap chain to match a new window size.
    /// No-ops for zero-area sizes (minimised window).
    pub(crate) fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }

        self.config.width = new_size.width;
        self.config.height = new_size.height;

        if let Some(surface) = &self.surface {
            surface.configure(&self.device, &self.config);
        }
    }

    /// Drops the surface when the application is suspended.
    /// This is necessary for platforms like Android to release graphics resources.
    pub(crate) fn suspend(&mut self) {
        self.surface = None;
    }

    /// Re-initializes the surface after the application is resumed.
    /// Recreates the surface and configures it with the existing device settings.
    pub(crate) fn resume(&mut self, window: &Arc<dyn Window>) -> Result<(), RedixelError> {
        if self.surface.is_none() {
            let surface: Surface = Self::create_surface(&self.instance, window)?;
            surface.configure(&self.device, &self.config);
            self.surface = Some(surface);
        }

        Ok(())
    }

    fn create_instance(backends: Backends) -> Instance {
        Instance::new(InstanceDescriptor {
            backends,
            display: None,
            flags: InstanceFlags::default(),
            memory_budget_thresholds: MemoryBudgetThresholds::default(),
            backend_options: BackendOptions::default(),
        })
    }

    fn create_surface(instance: &Instance, window: &Arc<dyn Window>) -> Result<Surface<'static>, RedixelError> {
        #[cfg(target_os = "windows")]
        {
            // On Windows, `raw_window_handle()` enforces thread identity and will
            // return `HandleError::Unavailable` when called from outside the event-
            // loop thread. We initialise on a background thread, so we use
            // `window_handle_any_thread` to bypass the guard safely — the `Arc`
            // guarantees the window outlives this call.
            use wgpu::SurfaceTargetUnsafe;
            use wgpu::rwh::HasDisplayHandle;
            use winit::platform::windows::WindowExtWindows;

            unsafe {
                instance
                    .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle: Some(window.display_handle()?.as_raw()),
                        raw_window_handle: window.window_handle_any_thread()?.as_raw(),
                    })
                    .map_err(RedixelError::from)
            }
        }

        #[cfg(not(target_os = "windows"))]
        instance.create_surface(window.clone()).map_err(RedixelError::from)
    }

    /// Reports which GPU actually backs this session.
    ///
    /// Worth logging unconditionally: a software rasteriser (llvmpipe,
    /// lavapipe) is selected silently when no hardware adapter is present, and
    /// any timing measured against it says nothing about real hardware.
    fn log_adapter(adapter: &Adapter) {
        let info: AdapterInfo = adapter.get_info();
        log::info!(
            "GPU adapter: {}",
            describe_adapter(&info.name, &info.driver, &info.driver_info, info.device_type, info.backend)
        );
    }

    async fn request_adapter(instance: &Instance, surface: &Surface<'static>) -> Result<Adapter, RedixelError> {
        instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(RedixelError::from)
    }

    async fn request_device(adapter: &Adapter) -> Result<(Device, Queue), RedixelError> {
        adapter
            .request_device(&DeviceDescriptor {
                label: Some("REDIXEL_DEVICE"),
                required_features: Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: MemoryHints::Performance,
                trace: Trace::Off,
                experimental_features: ExperimentalFeatures::default(),
            })
            .await
            .map_err(RedixelError::from)
    }

    fn build_surface_config(
        window: &Arc<dyn Window>,
        surface: &Surface,
        adapter: &Adapter,
        desired_present_mode: PresentMode,
    ) -> SurfaceConfiguration {
        let size: PhysicalSize<u32> = window.surface_size();
        let caps: SurfaceCapabilities = surface.get_capabilities(adapter);

        let format: TextureFormat = caps
            .formats
            .iter()
            .copied()
            .find(|f: &TextureFormat| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let present_mode: PresentMode = caps
            .present_modes
            .iter()
            .copied()
            .find(|&m: &PresentMode| m == desired_present_mode)
            .unwrap_or(caps.present_modes[0]);

        SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![format],
            desired_maximum_frame_latency: 2,
            color_space: SurfaceColorSpace::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_a_fully_populated_native_adapter() {
        let described: String = describe_adapter(
            "NVIDIA GeForce GTX 1660 Ti",
            "NVIDIA",
            "595.84",
            DeviceType::DiscreteGpu,
            Backend::Vulkan,
        );

        assert_eq!(
            described,
            "NVIDIA GeForce GTX 1660 Ti [DiscreteGpu / Vulkan] driver: NVIDIA 595.84"
        );
    }

    #[test]
    fn names_an_adapter_the_browser_refuses_to_identify() {
        let described: String = describe_adapter("", "", "", DeviceType::Other, Backend::BrowserWebGpu);

        assert_eq!(described, "unidentified adapter [Other / BrowserWebGpu]");
    }

    #[test]
    fn drops_the_driver_clause_when_both_halves_are_blank() {
        let described: String = describe_adapter("llvmpipe", "  ", "", DeviceType::Cpu, Backend::Vulkan);

        assert_eq!(described, "llvmpipe [Cpu / Vulkan]");
    }

    #[test]
    fn keeps_whichever_driver_half_is_present() {
        let only_name: String = describe_adapter("a", "Mesa", "", DeviceType::Cpu, Backend::Gl);
        let only_detail: String = describe_adapter("a", "", "25.0.1", DeviceType::Cpu, Backend::Gl);

        assert_eq!(only_name, "a [Cpu / Gl] driver: Mesa");
        assert_eq!(only_detail, "a [Cpu / Gl] driver: 25.0.1");
    }
}
