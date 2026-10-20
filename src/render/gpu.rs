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
use winit::window::Window;

/// Largest 2D texture edge the renderer will ask for.
///
/// Atlases are packed against the requested limit, so this is the real ceiling on how
/// many pre-rendered sprites fit. 16384 is what current desktop GPUs offer; asking for
/// more would fail on hardware that could otherwise run the game, and the request is
/// clamped to the adapter's own maximum anyway.
pub(crate) const MAX_USEFUL_TEXTURE_DIM: u32 = 16_384;

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
    pub surface: wgpu::Surface<'static>,
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
}

impl GpuContext {
    /// Initialize wgpu with the given window.
    ///
    /// This blocks on async wgpu calls using [`block_on`]. Safe to call from
    /// winit's sync ApplicationHandler::resumed().
    pub fn new(window: Arc<Window>) -> Result<Self> {
        block_on(Self::new_async(window))
    }

    /// Async initialization — called by new() via block_on().
    async fn new_async(window: Arc<Window>) -> Result<Self> {
        // wgpu 27's standard override selects a backend for reproducible
        // shroud GPU comparisons while retaining primary backends by default.
        // https://docs.rs/wgpu-types/27.0.1/wgpu_types/struct.Backends.html#method.with_env
        let instance: wgpu::Instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY.with_env(),
            ..Default::default()
        });

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
        if !downlevel.flags.contains(wgpu::DownlevelFlags::VIEW_FORMATS) {
            bail!(
                "exact stock-shell presentation requires compatible sRGB/unorm \
                 texture views, adapter downlevel flags are {:?}",
                downlevel.flags
            );
        }
        // TerrainDrawRenderer reduces ordered read-only Bullet destinations
        // with fragment atomics. Reject an insufficient downlevel adapter here
        // rather than panicking later during render-pipeline validation.
        if !downlevel
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

        log::info!(
            "GPU initialized: {}x{}, format={:?}",
            config.width,
            config.height,
            surface_format
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            surface_format,
            adapter_info,
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
        self.surface.configure(&self.device, &self.config);
        log::debug!("Surface resized to {}x{}", width, height);
    }

    /// Take the window's next frame to draw into, or `None` when there is none
    /// this time and the next redraw should try again.
    ///
    /// A swapchain the driver no longer considers fit for its window shows up
    /// only here: `SurfaceTexture::present` drops that status, and the only
    /// other reconfigure is a resize event, which a change that leaves the
    /// window's size alone never sends. So a stale, lost or failed swapchain is
    /// reconfigured once and asked again, as wgpu 27's own example does
    /// (`SurfaceWrapper::acquire`, `examples/features/src/framework.rs` at
    /// v27.0.1). A frame wgpu marks suboptimal is drawn as it is, as there.
    pub fn acquire_frame(&self) -> Result<Option<wgpu::SurfaceTexture>, wgpu::SurfaceError> {
        acquire_with_recovery(
            || self.surface.get_current_texture(),
            |error| {
                log::warn!(
                    "Surface texture: {error}; reconfiguring the surface at {}x{}",
                    self.config.width,
                    self.config.height
                );
                self.surface.configure(&self.device, &self.config);
            },
        )
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

/// One frame acquisition under [`GpuContext::acquire_frame`]'s rule. A
/// timeout skips the frame. A stale, lost or failed swapchain is reconfigured
/// once and asked again; still stale or timed out, the frame is skipped, and
/// any other failure is the caller's error.
fn acquire_with_recovery<Frame>(
    mut acquire: impl FnMut() -> Result<Frame, wgpu::SurfaceError>,
    reconfigure: impl FnOnce(&wgpu::SurfaceError),
) -> Result<Option<Frame>, wgpu::SurfaceError> {
    use wgpu::SurfaceError;

    let error = match acquire() {
        Ok(frame) => return Ok(Some(frame)),
        Err(error @ SurfaceError::Timeout) => {
            log::debug!("Surface texture: {error}; skipping this frame");
            return Ok(None);
        }
        Err(
            error @ (SurfaceError::Outdated
            | SurfaceError::Lost
            | SurfaceError::OutOfMemory
            | SurfaceError::Other),
        ) => error,
    };
    reconfigure(&error);
    match acquire() {
        Ok(frame) => Ok(Some(frame)),
        Err(error @ (SurfaceError::Timeout | SurfaceError::Outdated)) => {
            log::debug!("Surface texture after reconfiguring: {error}; skipping this frame");
            Ok(None)
        }
        Err(error) => Err(error),
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

    use wgpu::SurfaceError::{Lost, Outdated, Timeout};

    use super::{acquire_with_recovery, block_on};

    /// Run one acquisition over `results`, the acquire answers in call order;
    /// returns its outcome and how often it reconfigured the surface.
    fn acquire_over(
        results: Vec<Result<u32, wgpu::SurfaceError>>,
    ) -> (Result<Option<u32>, wgpu::SurfaceError>, u32) {
        let mut results = results.into_iter();
        let mut reconfigures = 0;
        let outcome = acquire_with_recovery(
            || results.next().expect("acquired more often than scripted"),
            |_| reconfigures += 1,
        );
        assert!(results.next().is_none(), "scripted answers left unasked");
        (outcome, reconfigures)
    }

    #[test]
    fn stale_swapchain_is_reconfigured_and_its_frame_taken() {
        assert_eq!(acquire_over(vec![Err(Outdated), Ok(7)]), (Ok(Some(7)), 1));
    }

    #[test]
    fn lost_swapchain_is_reconfigured_and_its_frame_taken() {
        assert_eq!(acquire_over(vec![Err(Lost), Ok(7)]), (Ok(Some(7)), 1));
    }

    #[test]
    fn timeout_skips_the_frame_without_reconfiguring() {
        assert_eq!(acquire_over(vec![Err(Timeout)]), (Ok(None), 0));
    }

    #[test]
    fn swapchain_still_stale_after_its_reconfigure_skips_the_frame() {
        assert_eq!(
            acquire_over(vec![Err(Outdated), Err(Outdated)]),
            (Ok(None), 1)
        );
    }

    #[test]
    fn surface_still_lost_after_its_reconfigure_is_an_error() {
        assert_eq!(acquire_over(vec![Err(Lost), Err(Lost)]), (Err(Lost), 1));
    }

    #[test]
    fn ready_frame_is_taken_without_reconfiguring() {
        assert_eq!(acquire_over(vec![Ok(7)]), (Ok(Some(7)), 0));
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
