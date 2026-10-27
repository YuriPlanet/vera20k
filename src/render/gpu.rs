//! wgpu device, queue, and surface initialization.
//!
//! GpuContext is created once during app startup and lives for the entire session.
//! It holds all the wgpu state needed for rendering. Other render modules
//! (sprite.rs, terrain.rs, etc.) borrow device/queue from here.
//!
//! ## Why block_on?
//! wgpu's initialization is async (request_adapter, request_device), but winit's
//! ApplicationHandler::resumed() is sync. [`block_on`] bridges this gap.
//!
//! ## Why Arc<Window>?
//! wgpu::Surface requires the window to live as long as the surface ('static lifetime).
//! Wrapping Window in Arc satisfies this because Arc provides 'static ownership.
//!
//! ## Dependency rules
//! - gpu.rs is part of render/ — see render/mod.rs for dependency rules.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Context as TaskContext, Poll, Wake, Waker};

use anyhow::{Context, Result, bail};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
#[path = "web_present.rs"]
mod web_present;

/// Largest 2D texture edge the renderer will ask for.
///
/// Atlases are packed against the requested limit, so this is the real ceiling on how
/// many pre-rendered sprites fit. 16384 is what current desktop GPUs offer; asking for
/// more would fail on hardware that could otherwise run the game, and the request is
/// clamped to the adapter's own maximum anyway.
pub(crate) const MAX_USEFUL_TEXTURE_DIM: u32 = 16_384;

/// True on the browser's WebGL2 fallback device, which has no storage buffers,
/// compute, fragment atomics or reinterpreting texture views. Renderers that
/// use those select render-target equivalents there; every other device keeps
/// its ordinary path. Always false outside the browser build.
pub(crate) fn webgl_fallback(device: &wgpu::Device) -> bool {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        device.limits().max_storage_buffers_per_shader_stage == 0
    }
    // Tests can run the fallback's passes on a native device, where the GPU
    // goldens check them against the storage/compute paths they replace.
    #[cfg(all(
        test,
        not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))
    ))]
    {
        let _ = device;
        std::env::var_os("VERA20K_TEST_WEBGL_FALLBACK").is_some_and(|value| value == "1")
    }
    #[cfg(not(any(
        test,
        all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
    )))]
    {
        let _ = device;
        false
    }
}

/// One production frame, backed by either the window or its hidden capture target.
pub struct GpuFrame {
    pub texture: wgpu::Texture,
    surface: Option<wgpu::SurfaceTexture>,
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    web_present: Option<Arc<web_present::WebPresent>>,
}

impl GpuFrame {
    fn from_surface(surface: wgpu::SurfaceTexture) -> Self {
        Self {
            texture: surface.texture.clone(),
            surface: Some(surface),
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            web_present: None,
        }
    }

    pub fn present(self) {
        if let Some(surface) = self.surface {
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            if let Some(presenter) = self.web_present {
                presenter.present(&surface);
            }
            surface.present();
        }
    }
}

/// Derived render target for an explicitly hidden diagnostic window.
struct CaptureTarget {
    texture: wgpu::Texture,
}

impl CaptureTarget {
    fn new(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        Self {
            texture: device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Hidden Capture Frame"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: config.usage,
                view_formats: &config.view_formats,
            }),
        }
    }

    fn resize(&mut self, device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) {
        *self = Self::new(device, config);
    }

    fn frame(&self) -> GpuFrame {
        GpuFrame {
            texture: self.texture.clone(),
            surface: None,
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            web_present: None,
        }
    }
}

/// Borrowed, serialization-ready identity of the adapter selected for this GPU
/// context. This is observation only: it mirrors the immutable `AdapterInfo`
/// captured during adapter selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GpuAdapterObservation<'a> {
    pub name: &'a str,
    pub vendor: u32,
    pub device: u32,
    pub device_type: wgpu::DeviceType,
    pub driver: &'a str,
    pub driver_info: &'a str,
    pub backend: wgpu::Backend,
}

impl<'a> From<&'a wgpu::AdapterInfo> for GpuAdapterObservation<'a> {
    fn from(info: &'a wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.as_str(),
            vendor: info.vendor,
            device: info.device,
            device_type: info.device_type,
            driver: info.driver.as_str(),
            driver_info: info.driver_info.as_str(),
            backend: info.backend,
        }
    }
}

/// Holds all wgpu state needed for rendering.
///
/// Created once during app initialization when the window becomes available.
/// Other render modules borrow `device` and `queue` from this struct.
pub struct GpuContext {
    /// The GPU surface we render to (tied to the window).
    // Loading progress shares the GPU context with asset uploads. Only this
    // owner locks the surface, briefly, to acquire or replace a lost swapchain.
    surface: Mutex<wgpu::Surface<'static>>,
    instance: wgpu::Instance,
    window: Arc<Window>,
    /// The logical GPU device — used to create buffers, textures, pipelines.
    pub device: wgpu::Device,
    /// The command queue — used to submit render commands to the GPU.
    pub queue: wgpu::Queue,
    /// Current surface configuration (format, size, present mode).
    pub config: wgpu::SurfaceConfiguration,
    /// The texture format the surface uses (needed when creating pipelines).
    pub surface_format: wgpu::TextureFormat,
    /// Exact identity returned by the adapter that backs this context.
    adapter_info: wgpu::AdapterInfo,
    capture_target: Option<CaptureTarget>,
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    web_present: Arc<web_present::WebPresent>,
}

impl GpuContext {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) async fn new_browser(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
    ) -> Result<Self> {
        Self::new_async(window, display, false).await
    }

    /// Initialize wgpu with the given window.
    ///
    /// This blocks on async wgpu calls using [`block_on`]. Safe to call from
    /// winit's sync ApplicationHandler::resumed().
    /// `hidden_capture` is the startup owner's deliberate capture choice, not
    /// the current visibility/occlusion of a player's ordinary window.
    pub fn new(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
        hidden_capture: bool,
    ) -> Result<Self> {
        block_on(Self::new_async(window, display, hidden_capture))
    }

    /// Async initialization — called by new() via block_on().
    async fn new_async(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
        hidden_capture: bool,
    ) -> Result<Self> {
        // The standard override selects a backend for reproducible
        // shroud GPU comparisons while retaining primary backends by default.
        // wgpu 29 requires the display at instance creation for Wayland EGL:
        // https://docs.rs/wgpu-types/29.0.1/wgpu_types/struct.InstanceDescriptor.html
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let instance: wgpu::Instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY.with_env(),
            ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display))
        });
        // Browsers without a usable WebGPU adapter fall back to WebGL2. A
        // surface-less probe with the adapter options used below decides
        // first: a canvas given a WebGPU context can no longer create WebGL2.
        // wgpu's own probe (util::is_browser_webgpu_supported) asks with
        // default options, which can succeed where this request fails.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let instance: wgpu::Instance = {
            let probe = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::BROWSER_WEBGPU,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let webgpu = probe
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .is_ok();
            if !webgpu {
                log::warn!("WebGPU adapter unavailable; using the WebGL2 fallback");
            }
            wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: if webgpu {
                    wgpu::Backends::BROWSER_WEBGPU
                } else {
                    wgpu::Backends::GL
                },
                ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display))
            })
        };

        // Create the surface from our window. The surface is what we draw pixels to.
        let surface: wgpu::Surface<'static> = instance
            .create_surface(window.clone())
            .context("Failed to create wgpu surface from window")?;

        // Request a GPU adapter (physical device) that can render to our surface.
        // HighPerformance prefers discrete GPUs over integrated ones.
        let adapter: wgpu::Adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("No suitable GPU adapter found — is a GPU available?")?;

        let adapter_info = adapter.get_info();
        log::info!(
            "Using GPU adapter: {} ({:?})",
            adapter_info.name,
            adapter_info.backend
        );

        // Atlas sizing is bounded by whatever limits we *request*, not by the hardware.
        let adapter_limits = adapter.limits();
        log::info!(
            "GPU limits: max_texture_dimension_2d={} (requesting {})",
            adapter_limits.max_texture_dimension_2d,
            adapter_limits
                .max_texture_dimension_2d
                .min(MAX_USEFUL_TEXTURE_DIM),
        );

        // Request a logical device and command queue from the adapter.
        //
        // `Limits::default()` caps 2D textures at 8192, which is a portability baseline
        // rather than a hardware one — desktop GPUs commonly do 16384. The unit atlas is
        // sized against whatever we request here, so asking for the default silently
        // halved the space available for pre-rendered facings. Ask for what the adapter
        // actually offers, bounded by what the atlases can use, and fall back
        // automatically on a device that offers less.
        let required_limits = wgpu::Limits {
            max_texture_dimension_2d: adapter_limits
                .max_texture_dimension_2d
                .min(MAX_USEFUL_TEXTURE_DIM),
            ..wgpu::Limits::default()
        };
        // WebGL2 has no storage buffers or compute, so the WebGPU baseline is
        // unattainable; request what the context offers. Renderers detect the
        // fallback through `webgl_fallback`.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let required_limits = if adapter_info.backend == wgpu::Backend::Gl {
            wgpu::Limits {
                max_texture_dimension_2d: required_limits.max_texture_dimension_2d,
                ..adapter_limits.clone()
            }
        } else {
            required_limits
        };
        let (device, queue): (wgpu::Device, wgpu::Queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("RA2 Device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                ..Default::default()
            })
            .await
            .context("Failed to create wgpu device")?;

        // Pick the best texture format for the surface.
        // This is usually Bgra8UnormSrgb on Windows, Bgra8Unorm on some other platforms.
        let surface_caps: wgpu::SurfaceCapabilities = surface.get_capabilities(&adapter);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let required_surface_usages = wgpu::TextureUsages::RENDER_ATTACHMENT;
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let required_surface_usages = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        if !surface_caps.usages.contains(required_surface_usages) {
            bail!(
                "exact stock-shell presentation requires surface usages \
                 {required_surface_usages:?}, adapter exposes {:?}",
                surface_caps.usages
            );
        }
        let downlevel = adapter.get_downlevel_capabilities();
        // The WebGL2 fallback replaces both requirements below with
        // render-target equivalents; see `webgl_fallback`.
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let webgl = false;
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let webgl = adapter_info.backend == wgpu::Backend::Gl;
        if !webgl && !downlevel.flags.contains(wgpu::DownlevelFlags::VIEW_FORMATS) {
            bail!(
                "exact stock-shell presentation requires compatible sRGB/unorm \
                 texture views, adapter downlevel flags are {:?}",
                downlevel.flags
            );
        }
        // TerrainDrawRenderer reduces ordered read-only Bullet destinations
        // with fragment atomics. Reject an insufficient downlevel adapter here
        // rather than panicking later during render-pipeline validation.
        if !webgl
            && !downlevel
                .flags
                .contains(wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE)
        {
            bail!(
                "native projectile drawing requires fragment-writable storage, \
                 adapter downlevel flags are {:?}",
                downlevel.flags
            );
        }
        let surface_format: wgpu::TextureFormat = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        // Configure the surface with our chosen format and the window's current size.
        let window_size: winit::dpi::PhysicalSize<u32> = window.inner_size();
        let config: wgpu::SurfaceConfiguration = wgpu::SurfaceConfiguration {
            usage: required_surface_usages,
            format: surface_format,
            width: window_size.width.max(1), // wgpu panics on 0-sized surfaces
            height: window_size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let web_present = Arc::new(web_present::WebPresent::new(&device, &queue, &config));
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let surface_format = surface_format.add_srgb_suffix();
        // wgpu-hal 29.0.4 Metal acquire_texture refuses any macOS NSWindow
        // without NSWindowOcclusionStateVisible, including deliberately hidden
        // captures. Render those frames offscreen with the same format/usages;
        // the normal render, loading, readback and capture-driver paths remain
        // shared. Ordinary occluded windows retain swapchain skip/recovery.
        // https://docs.rs/wgpu-hal/29.0.4/src/wgpu_hal/metal/surface.rs.html#126-151
        let capture_target = hidden_capture.then(|| CaptureTarget::new(&device, &config));

        log::info!(
            "GPU initialized: {}x{}, format={:?}",
            config.width,
            config.height,
            surface_format
        );

        Ok(Self {
            surface: Mutex::new(surface),
            instance,
            window,
            device,
            queue,
            config,
            surface_format,
            adapter_info,
            capture_target,
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            web_present,
        })
    }

    /// Return a borrowed capture-only view of the selected adapter identity.
    pub(crate) fn capture_adapter_observation(&self) -> GpuAdapterObservation<'_> {
        GpuAdapterObservation::from(&self.adapter_info)
    }

    /// Handle window resize — reconfigure the surface with new dimensions.
    ///
    /// Called from ApplicationHandler::window_event when WindowEvent::Resized fires.
    /// Ignores zero-sized dimensions (happens during window minimize on some platforms).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // Minimized window — skip reconfigure
        }
        self.config.width = width;
        self.config.height = height;
        self.surface
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .configure(&self.device, &self.config);
        if let Some(target) = &mut self.capture_target {
            target.resize(&self.device, &self.config);
        }
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            self.web_present = Arc::new(web_present::WebPresent::new(
                &self.device,
                &self.queue,
                &self.config,
            ));
        }
        log::debug!("Surface resized to {}x{}", width, height);
    }

    /// Take the window's next frame to draw into, or `None` when there is none
    /// this time and the next redraw should try again.
    ///
    /// A stale or suboptimal swapchain is reconfigured once and asked again;
    /// a lost surface is recreated first, as required by wgpu 29. Timeout and
    /// occlusion skip this redraw. Validation failures remain errors.
    /// https://docs.rs/wgpu/29.0.4/wgpu/enum.CurrentSurfaceTexture.html
    pub fn acquire_frame(&self) -> Result<Option<GpuFrame>> {
        if let Some(target) = &self.capture_target {
            return Ok(Some(target.frame()));
        }
        let mut surface = self.surface.lock().unwrap_or_else(PoisonError::into_inner);
        acquire_with_recovery(|recovery| {
            if let Some(recovery) = recovery {
                log::warn!(
                    "Surface texture: {recovery:?}; recovering the surface at {}x{}",
                    self.config.width,
                    self.config.height
                );
                if recovery == SurfaceRecovery::Recreate {
                    *surface = self
                        .instance
                        .create_surface(self.window.clone())
                        .context("recreate lost wgpu surface")?;
                }
                surface.configure(&self.device, &self.config);
            }
            Ok(surface.get_current_texture())
        })
        .map(|frame| {
            frame.map(|surface| {
                let frame = GpuFrame::from_surface(surface);
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                let frame = GpuFrame {
                    texture: self.web_present.texture.clone(),
                    web_present: Some(self.web_present.clone()),
                    ..frame
                };
                frame
            })
        })
    }

    /// Create a depth texture matching the current surface dimensions.
    ///
    /// Must be recreated whenever the window is resized (surface dimensions change).
    /// Uses Depth32Float format matching the batch pipeline's depth_stencil state.
    pub fn create_depth_texture(&self) -> wgpu::TextureView {
        let texture: wgpu::Texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: self.config.width.max(1),
                height: self.config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        texture.create_view(&Default::default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceRecovery {
    Reconfigure,
    Recreate,
}

/// Request at most two frames, recovering before the second acquisition only.
fn acquire_with_recovery(
    mut acquire: impl FnMut(Option<SurfaceRecovery>) -> Result<wgpu::CurrentSurfaceTexture>,
) -> Result<Option<wgpu::SurfaceTexture>> {
    use wgpu::CurrentSurfaceTexture;

    let recovery = match acquire(None)? {
        CurrentSurfaceTexture::Success(frame) => return Ok(Some(frame)),
        CurrentSurfaceTexture::Suboptimal(frame) => {
            // Drop the acquired texture before reconfiguring its surface.
            drop(frame);
            SurfaceRecovery::Reconfigure
        }
        CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return Ok(None),
        CurrentSurfaceTexture::Outdated => SurfaceRecovery::Reconfigure,
        CurrentSurfaceTexture::Lost => SurfaceRecovery::Recreate,
        CurrentSurfaceTexture::Validation => bail!("surface acquisition validation failed"),
    };
    match acquire(Some(recovery))? {
        CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
            Ok(Some(frame))
        }
        CurrentSurfaceTexture::Timeout
        | CurrentSurfaceTexture::Occluded
        | CurrentSurfaceTexture::Outdated => Ok(None),
        CurrentSurfaceTexture::Lost => bail!("surface still lost after recovery"),
        CurrentSurfaceTexture::Validation => bail!("surface acquisition validation failed"),
    }
}

/// Run `future` to completion on the calling thread.
///
/// Wakes are recorded in a flag behind a condvar rather than the thread's park
/// token, which code inside `poll` could consume.
pub fn block_on<F: Future>(future: F) -> F::Output {
    struct Signal {
        woken: Mutex<bool>,
        condvar: Condvar,
    }

    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            *self.woken.lock().unwrap_or_else(PoisonError::into_inner) = true;
            self.condvar.notify_one();
        }
    }

    let signal = Arc::new(Signal {
        woken: Mutex::new(false),
        condvar: Condvar::new(),
    });
    let waker = Waker::from(Arc::clone(&signal));
    let mut context = TaskContext::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        let mut woken = signal.woken.lock().unwrap_or_else(PoisonError::into_inner);
        while !*woken {
            woken = signal
                .condvar
                .wait(woken)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *woken = false;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::Poll;

    use wgpu::CurrentSurfaceTexture::{Lost, Occluded, Outdated, Timeout, Validation};

    use super::{CaptureTarget, SurfaceRecovery, acquire_with_recovery, block_on};

    /// Run one acquisition over `results`, the acquire answers in call order;
    /// returns its outcome and recovery requests. Real frame presentation is
    /// exercised by the normal window and GPU readback tests.
    fn acquire_over(
        results: Vec<wgpu::CurrentSurfaceTexture>,
    ) -> (
        anyhow::Result<Option<wgpu::SurfaceTexture>>,
        Vec<Option<SurfaceRecovery>>,
    ) {
        let mut results = results.into_iter();
        let mut requests = Vec::new();
        let outcome = acquire_with_recovery(|recovery| {
            requests.push(recovery);
            Ok(results.next().expect("acquired more often than scripted"))
        });
        assert!(results.next().is_none(), "scripted answers left unasked");
        (outcome, requests)
    }

    #[test]
    fn timeout_and_occlusion_skip_without_recovery() {
        let (outcome, requests) = acquire_over(vec![Timeout]);
        assert!(outcome.unwrap().is_none());
        assert_eq!(requests, [None]);
        let (outcome, requests) = acquire_over(vec![Occluded]);
        assert!(outcome.unwrap().is_none());
        assert_eq!(requests, [None]);
    }

    #[test]
    fn stale_swapchain_reconfigures_once_and_skips_if_still_stale() {
        let (outcome, requests) = acquire_over(vec![Outdated, Outdated]);
        assert!(outcome.unwrap().is_none());
        assert_eq!(requests, [None, Some(SurfaceRecovery::Reconfigure)]);
    }

    #[test]
    fn lost_surface_recreates_once_and_validation_stays_an_error() {
        let (outcome, requests) = acquire_over(vec![Lost, Lost]);
        assert!(outcome.unwrap_err().to_string().contains("still lost"));
        assert_eq!(requests, [None, Some(SurfaceRecovery::Recreate)]);
        let (outcome, requests) = acquire_over(vec![Validation]);
        assert!(outcome.unwrap_err().to_string().contains("validation"));
        assert_eq!(requests, [None]);
    }

    #[test]
    #[ignore = "requires a wgpu adapter for hidden capture frame readback"]
    fn hidden_capture_frame_renders_reads_back_and_resizes_without_a_swapchain() {
        use crate::render::frame_readback::PendingBgra8Readback;

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: None,
            ..Default::default()
        }))
        .expect("hidden capture regression requires a GPU adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("request hidden capture regression device");
        let mut config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: 2,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let mut target = CaptureTarget::new(&device, &config);
        let render =
            |target: &CaptureTarget, config: &wgpu::SurfaceConfiguration, color: wgpu::Color| {
                let frame = target.frame();
                assert!(frame.surface.is_none());
                assert_eq!(frame.texture.format(), config.format);
                assert_eq!(frame.texture.usage(), config.usage);
                assert_eq!(frame.texture.width(), config.width);
                assert_eq!(frame.texture.height(), config.height);
                let view = frame.texture.create_view(&Default::default());
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Hidden capture production frame regression"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(color),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
                let pending = PendingBgra8Readback::encode(
                    &device,
                    &mut encoder,
                    &frame.texture,
                    config.format,
                    config.width,
                    config.height,
                )
                .expect("read back the actual capture frame");
                let submission = queue.submit([encoder.finish()]);
                frame.present();
                pending
                    .finish(&device, submission, std::time::Duration::from_secs(10))
                    .expect("finish hidden frame readback")
            };
        assert_eq!(
            render(&target, &config, wgpu::Color::RED),
            [0, 0, 255, 255, 0, 0, 255, 255]
        );
        (config.width, config.height) = (1, 2);
        target.resize(&device, &config);
        assert_eq!(
            render(&target, &config, wgpu::Color::BLUE),
            [255, 0, 0, 255, 255, 0, 0, 255]
        );
    }

    #[test]
    fn block_on_sleeps_until_another_thread_wakes_the_future() {
        let ready = Arc::new(AtomicBool::new(false));
        let mut spawned = false;
        let output = block_on(std::future::poll_fn(|context| {
            if ready.load(Ordering::Acquire) {
                return Poll::Ready(7);
            }
            if !spawned {
                spawned = true;
                let (ready, waker) = (Arc::clone(&ready), context.waker().clone());
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    ready.store(true, Ordering::Release);
                    waker.wake();
                });
            }
            Poll::Pending
        }));
        assert_eq!(output, 7);
    }
}
