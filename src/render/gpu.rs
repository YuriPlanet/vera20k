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
#[cfg(any(not(target_os = "horizon"), feature = "horizon-client"))]
use winit::event_loop::OwnedDisplayHandle;
#[cfg(any(not(target_os = "horizon"), feature = "horizon-client"))]
use winit::window::Window;

#[cfg(target_os = "horizon")]
#[path = "gpu_memory.rs"]
mod memory;

/// A platform window and logical drawable size for wgpu surface creation.
///
/// `surface_handle` returns an owned handle. A native backend must keep the
/// underlying window alive through that handle for as long as wgpu owns the
/// resulting `'static` surface.
pub trait NativeWindow: Clone + Send + Sync {
    type Handle: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static;

    fn surface_handle(&self) -> Self::Handle;
    fn inner_size(&self) -> (u32, u32);
}

/// Largest 2D texture edge the renderer will ask for.
///
/// Atlases are packed against the requested limit, so this is the real ceiling on how
/// many pre-rendered sprites fit. 16384 is what current desktop GPUs offer; asking for
/// more would fail on hardware that could otherwise run the game, and the request is
/// clamped to the adapter's own maximum anyway.
pub(crate) const MAX_USEFUL_TEXTURE_DIM: u32 = 16_384;

/// One production frame, backed by either the window or its hidden capture target.
pub struct GpuFrame {
    pub texture: wgpu::Texture,
    surface: Option<wgpu::SurfaceTexture>,
}

impl GpuFrame {
    fn from_surface(surface: wgpu::SurfaceTexture) -> Self {
        Self {
            texture: surface.texture.clone(),
            surface: Some(surface),
        }
    }

    pub fn present(self) {
        if let Some(surface) = self.surface {
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
    // Erased to `DisplayAndWindowHandle` (not just `WindowHandle`) because wgpu 29
    // surfaces require a display handle alongside the window handle.
    window: Arc<dyn wgpu::DisplayAndWindowHandle>,
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
    #[cfg(target_os = "horizon")]
    _memory_monitor: Option<memory::Monitor>,
}

impl GpuContext {
    /// Initialize wgpu with the given window.
    ///
    /// This blocks on async wgpu calls using [`block_on`]. Safe to call from
    /// winit's sync ApplicationHandler::resumed().
    /// `hidden_capture` is the startup owner's deliberate capture choice, not
    /// the current visibility/occlusion of a player's ordinary window.
    #[cfg(any(not(target_os = "horizon"), feature = "horizon-client"))]
    pub fn new(
        window: Arc<Window>,
        _display: OwnedDisplayHandle,
        hidden_capture: bool,
    ) -> Result<Self> {
        block_on(Self::new_async(window, hidden_capture))
    }

    /// Initialize the diagnostic native-window entry without a capture target.
    pub fn new_with_native_window<W: NativeWindow>(window: W) -> Result<Self> {
        block_on(Self::new_async(window, false))
    }

    async fn new_async<W: NativeWindow>(window: W, hidden_capture: bool) -> Result<Self> {
        // wgpu 29: `Instance::new` takes the descriptor by value and
        // `InstanceDescriptor` is built through `new_without_display_handle`.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY.with_env(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let (window_width, window_height) = window.inner_size();
        let window: Arc<dyn wgpu::DisplayAndWindowHandle> = Arc::new(window.surface_handle());

        // Create the surface from our window. The surface is what we draw pixels to.
        log::info!("gpu_surface_create_started");
        let surface: wgpu::Surface<'static> = instance
            .create_surface(window.clone())
            .context("Failed to create wgpu surface from window")?;
        log::info!("gpu_surface_created");

        // Request a GPU adapter (physical device) that can render to our surface.
        // HighPerformance prefers discrete GPUs over integrated ones.
        log::info!("gpu_adapter_request_started");
        let adapter: wgpu::Adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("No suitable GPU adapter found — is a GPU available?")?;
        log::info!("gpu_adapter_request_finished");

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
        log::info!("gpu_device_request_started");
        let (device, queue): (wgpu::Device, wgpu::Queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("RA2 Device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                ..Default::default()
            })
            .await
            .context("Failed to create wgpu device")?;
        log::info!("gpu_device_request_finished");

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
        let swapchain_format: wgpu::TextureFormat = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);
        let (surface_format, view_formats) =
            if cfg!(feature = "horizon-client") && !swapchain_format.is_srgb() {
                let srgb_view = match swapchain_format {
                    wgpu::TextureFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8UnormSrgb,
                    wgpu::TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8UnormSrgb,
                    format => bail!(
                        "Horizon client rendering requires an sRGB view of an 8-bit \
                     color surface, adapter selected {format:?} from {:?}",
                        surface_caps.formats
                    ),
                };
                if !downlevel
                    .flags
                    .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
                {
                    bail!(
                        "Horizon client rendering requires sRGB surface views, adapter \
                     {swapchain_format:?} has downlevel flags {:?}",
                        downlevel.flags
                    );
                }
                (srgb_view, vec![srgb_view])
            } else {
                (swapchain_format, Vec::new())
            };

        // Configure the surface with our chosen format and the window's current size.
        let config: wgpu::SurfaceConfiguration = wgpu::SurfaceConfiguration {
            usage: required_surface_usages,
            format: swapchain_format,
            width: window_width.max(1), // wgpu panics on 0-sized surfaces
            height: window_height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        // wgpu-hal 29.0.4 Metal acquire_texture refuses any macOS NSWindow
        // without NSWindowOcclusionStateVisible, including deliberately hidden
        // captures. Render those frames offscreen with the same format/usages;
        // the normal render, loading, readback and capture-driver paths remain
        // shared. Ordinary occluded windows retain swapchain skip/recovery.
        // https://docs.rs/wgpu-hal/29.0.4/src/wgpu_hal/metal/surface.rs.html#126-151
        let capture_target = hidden_capture.then(|| CaptureTarget::new(&device, &config));

        log::info!(
            "GPU initialized: {}x{}, swapchain_format={:?}, render_format={:?}",
            config.width,
            config.height,
            swapchain_format,
            surface_format,
        );

        #[cfg(target_os = "horizon")]
        let memory_monitor = memory::Monitor::start(&device);
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
            #[cfg(target_os = "horizon")]
            _memory_monitor: memory_monitor,
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
        log::debug!("Surface resized to {}x{}", width, height);
    }

    /// Take the window's next frame to draw into, or `None` when there is none
    /// this time and the next redraw should try again.
    ///
    /// A stale or suboptimal swapchain is reconfigured once and asked again;
    /// a lost surface is recreated first. Timeout or occlusion skips this redraw;
    /// a surface still lost after a recreate attempt remains an error.
    pub fn acquire_frame(&self) -> Result<Option<GpuFrame>> {
        if let Some(target) = &self.capture_target {
            return Ok(Some(target.frame()));
        }
        let mut surface = self.surface.lock().unwrap_or_else(PoisonError::into_inner);
        acquire_with_recovery(|recovery| {
            if let Some(recovery) = recovery {
                log::warn!("Surface texture: {recovery:?}; recovering the surface");
                if recovery == SurfaceRecovery::Recreate {
                    *surface = self
                        .instance
                        .create_surface(self.window.clone())
                        .context("recreate lost wgpu surface")?;
                }
                surface.configure(&self.device, &self.config);
            }
            Ok(classify(
                surface.get_current_texture(),
                GpuFrame::from_surface,
            ))
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

#[cfg(any(not(target_os = "horizon"), feature = "horizon-client"))]
impl NativeWindow for Arc<Window> {
    type Handle = Arc<Window>;

    fn surface_handle(&self) -> Self::Handle {
        Arc::clone(self)
    }

    fn inner_size(&self) -> (u32, u32) {
        let size = self.as_ref().inner_size();
        (size.width, size.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceRecovery {
    Reconfigure,
    Recreate,
}

/// Request at most two frames, recovering before the second acquisition only.
/// Outcome of a single `get_current_texture` attempt, normalized from wgpu 29's
/// `CurrentSurfaceTexture`. `Skip` means "no frame now, retry the redraw"; the
/// recovery variants say which fix to apply before asking again.
enum Acquisition<Frame> {
    Ok(Frame),
    Skip,
    Reconfigure,
    Lost,
}

/// Map a `CurrentSurfaceTexture` to an [`Acquisition`], turning a held texture
/// into `Frame` via `to_frame`. A suboptimal texture is still usable, matching
/// the pinned wgpu 27 behavior where suboptimal acquisitions were plain `Ok`s.
fn classify<Frame>(
    current: wgpu::CurrentSurfaceTexture,
    to_frame: impl FnOnce(wgpu::SurfaceTexture) -> Frame,
) -> Acquisition<Frame> {
    use wgpu::CurrentSurfaceTexture as C;
    match current {
        C::Success(texture) | C::Suboptimal(texture) => Acquisition::Ok(to_frame(texture)),
        C::Timeout | C::Occluded | C::Validation => Acquisition::Skip,
        C::Outdated => Acquisition::Reconfigure,
        C::Lost => Acquisition::Lost,
    }
}

/// Request at most two frames, recovering before the second acquisition only.
/// A second timeout/outdated/validation skips the redraw; a surface still lost
/// after a recreate attempt is surfaced as an error.
fn acquire_with_recovery<Frame>(
    mut acquire: impl FnMut(Option<SurfaceRecovery>) -> Result<Acquisition<Frame>>,
) -> Result<Option<Frame>> {
    let recovery = match acquire(None)? {
        Acquisition::Ok(frame) => return Ok(Some(frame)),
        Acquisition::Skip => return Ok(None),
        Acquisition::Reconfigure => SurfaceRecovery::Reconfigure,
        Acquisition::Lost => SurfaceRecovery::Recreate,
    };
    match acquire(Some(recovery))? {
        Acquisition::Ok(frame) => Ok(Some(frame)),
        Acquisition::Skip | Acquisition::Reconfigure => Ok(None),
        Acquisition::Lost => bail!("wgpu surface lost after recovery attempt"),
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

    use super::{Acquisition, SurfaceRecovery, acquire_with_recovery, block_on};

    /// Run one acquisition over `results`, the acquire answers in call order;
    /// returns its outcome and how often it reconfigured the surface.
    fn acquire_over(
        results: Vec<Acquisition<u32>>,
    ) -> (anyhow::Result<Option<u32>>, Vec<SurfaceRecovery>) {
        let mut results = results.into_iter();
        let mut recoveries = Vec::new();
        let outcome = acquire_with_recovery(|recovery| {
            recoveries.extend(recovery);
            Ok(results.next().expect("acquired more often than scripted"))
        });
        assert!(results.next().is_none(), "scripted answers left unasked");
        (outcome, recoveries)
    }

    #[test]
    fn timeout_skips_without_recovery() {
        let (frame, recoveries) = acquire_over(vec![Acquisition::Skip]);
        assert_eq!(frame.unwrap(), None);
        assert!(recoveries.is_empty());
    }

    #[test]
    fn lost_surface_recreates_and_stale_surface_reconfigures_once() {
        let (frame, recoveries) = acquire_over(vec![Acquisition::Lost, Acquisition::Ok(7)]);
        assert_eq!(frame.unwrap(), Some(7));
        assert_eq!(recoveries, vec![SurfaceRecovery::Recreate]);
        let (frame, recoveries) =
            acquire_over(vec![Acquisition::Reconfigure, Acquisition::Reconfigure]);
        assert_eq!(frame.unwrap(), None);
        assert_eq!(recoveries, vec![SurfaceRecovery::Reconfigure]);
        let (frame, recoveries) = acquire_over(vec![Acquisition::Lost, Acquisition::Lost]);
        assert!(frame.is_err());
        assert_eq!(recoveries, vec![SurfaceRecovery::Recreate]);
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
