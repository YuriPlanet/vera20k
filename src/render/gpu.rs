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

#[path = "web_present.rs"]
mod web_present;

/// Largest 2D texture edge the renderer will ask for.
///
/// Atlases are packed against the requested limit, so this is the real ceiling on how
/// many pre-rendered sprites fit. 16384 is what current desktop GPUs offer; asking for
/// more would fail on hardware that could otherwise run the game, and the request is
/// clamped to the adapter's own maximum anyway.
pub(crate) const MAX_USEFUL_TEXTURE_DIM: u32 = 16_384;

/// The edge asked for instead on a Windows or Linux GPU that draws from the
/// system's memory (`DeviceType::IntegratedGpu`, and the processor itself).
/// That memory is shared with the game and the system, and a driver short of
/// it removes the device: an Intel HD 620 lost its DirectX 12 device seconds
/// into a match whose sprite atlas was one 11721x11968 page, 134 MiB of
/// palette indices. At 8192 a page is 64 MiB at most and the atlases pack into
/// more pages. The cost: on a screen wider than 2048 px the zoom-out stops
/// where the view no longer fits one page (`camera::minimum_view_zoom`), at
/// 0.42 on a 3440 px screen instead of 0.25. Apple's unified memory is the
/// GPU's own, so Metal keeps [`MAX_USEFUL_TEXTURE_DIM`].
pub(crate) const SHARED_MEMORY_TEXTURE_DIM: u32 = 8_192;

/// A shader module built without naga's loop counter. naga wraps every loop
/// in a countdown from `u32::MAX` so a driver never meets an endless loop,
/// at a cost in every iteration; the shaders built here loop only over
/// counts the CPU already caps. A combat light replays at most `PASS_LIGHTS`
/// earlier lights (`combat_light`), a terrain edit at most one window of
/// commands and as many shadow halves (`terrain_batch::WINDOW`), and a scaled
/// copy covers at most the frame pixels one window pixel spans, bounded by
/// the zoom range (`scaled_copy`). The bounds and integer-division checks
/// stay on. Unmeasured on the integrated GPUs it is for; this Mac draws no
/// scene with lights and edits in the background.
///
/// # Safety
///
/// Every loop in `desc` terminates for every input the CPU sends.
pub(crate) fn bounded_loop_module(
    device: &wgpu::Device,
    desc: wgpu::ShaderModuleDescriptor<'_>,
) -> wgpu::ShaderModule {
    let checks = wgpu::ShaderRuntimeChecks {
        force_loop_bounding: false,
        ..wgpu::ShaderRuntimeChecks::checked()
    };
    // SAFETY: the callers' loops are bounded by counts the CPU clamps, as the
    // documentation above lists.
    unsafe { device.create_shader_module_trusted(desc, checks) }
}

/// Whether a 2D texture of `size` fits the device's limit, which is what
/// [`requested_texture_dim`] asked for: 16384 on most desktop cards, 8192 on
/// shared-memory chips, 4096 or less on old and WebGL devices. wgpu fails
/// validation on a larger one, and this game treats that as fatal, so an
/// atlas sized from a map's, mod's or theater's data asks here first and
/// refuses, clamps or leaves pieces out with a word in the log.
pub(crate) fn texture_fits(device: &wgpu::Device, size: [u32; 2]) -> bool {
    let limit = device.limits().max_texture_dimension_2d;
    size[0] <= limit && size[1] <= limit
}

/// A GPU that draws from the system's memory outside Metal: a Windows or
/// Linux integrated chip, or the processor itself. Its texture pages are
/// smaller ([`requested_texture_dim`]), its command buffers shorter
/// (`terrain_submission`), and its first match frames are timed for the log.
pub(crate) fn shared_memory_graphics(
    backend: wgpu::Backend,
    device_type: wgpu::DeviceType,
) -> bool {
    matches!(
        device_type,
        wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::Cpu
    ) && backend != wgpu::Backend::Metal
}

/// The 2D texture edge this device is asked for: what the adapter offers, up
/// to [`MAX_USEFUL_TEXTURE_DIM`], or [`SHARED_MEMORY_TEXTURE_DIM`] on a GPU
/// that draws from the system's memory outside Metal.
pub(crate) fn requested_texture_dim(
    backend: wgpu::Backend,
    device_type: wgpu::DeviceType,
    adapter_max: u32,
) -> u32 {
    let useful = if shared_memory_graphics(backend, device_type) {
        SHARED_MEMORY_TEXTURE_DIM
    } else {
        MAX_USEFUL_TEXTURE_DIM
    };
    adapter_max.min(useful)
}

/// True on an OpenGL device: the browser's WebGL2 fallback or the desktop's
/// OpenGL compatibility mode, both requested without storage buffers or
/// compute (`GpuContext::new_async`), and without reinterpreting texture
/// views. Renderers that use those select render-target equivalents there;
/// every other device, whose limits include storage buffers, keeps its
/// ordinary path.
pub(crate) fn webgl_fallback(device: &wgpu::Device) -> bool {
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
    #[cfg(not(all(
        test,
        not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))
    )))]
    {
        device.limits().max_storage_buffers_per_shader_stage == 0
    }
}

/// One production frame, backed by either the window or its hidden capture target.
pub struct GpuFrame {
    pub texture: wgpu::Texture,
    surface: Option<wgpu::SurfaceTexture>,
    web_present: Option<Arc<web_present::WebPresent>>,
}

impl GpuFrame {
    fn from_surface(surface: wgpu::SurfaceTexture) -> Self {
        Self {
            texture: surface.texture.clone(),
            surface: Some(surface),
            web_present: None,
        }
    }

    /// Hand the acquired surface texture to the queue. wgpu 30 moved
    /// presentation off `SurfaceTexture`, so the caller passes its queue.
    pub fn present(self, queue: &wgpu::Queue) {
        if let Some(surface) = self.surface {
            if let Some(presenter) = self.web_present {
                presenter.present(&surface);
            }
            queue.present(surface);
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
                // Frames are read back and copied like a window frame's; an
                // OpenGL surface's own usage is a render target only.
                usage: config.usage | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
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
    /// The thread frames reach the driver on. First, so it finishes the frame
    /// on its way and ends before the surface and device below drop.
    handoff: super::frame_handoff::FrameHandoff,
    /// The GPU surface we render to (tied to the window).
    // Loading progress shares the GPU context with asset uploads. Only this
    // owner locks the surface, briefly, to acquire or replace a lost swapchain.
    // Android suspension drops it while retaining the device and game resources.
    surface: Mutex<Option<wgpu::Surface<'static>>>,
    instance: wgpu::Instance,
    window: Arc<Window>,
    /// The logical GPU device — used to create buffers, textures, pipelines.
    pub device: wgpu::Device,
    /// The command queue — used to submit render commands to the GPU.
    pub queue: super::frame_handoff::HandoffQueue,
    /// Current surface configuration (format, size, present mode).
    pub config: wgpu::SurfaceConfiguration,
    /// The texture format the surface uses (needed when creating pipelines).
    pub surface_format: wgpu::TextureFormat,
    /// Exact identity returned by the adapter that backs this context.
    adapter_info: wgpu::AdapterInfo,
    capture_target: Option<CaptureTarget>,
    /// The GPU time of each frame, while the Performance Stats panel shows.
    pub(crate) timer: super::gpu_timer::GpuTimer,
    /// VERA's whole-viewport colour-vision filter. Every present path shares
    /// it, including the synchronous loading repaints outside the frame loop.
    pub(crate) color_vision: super::color_vision::ColorVisionFilter,
    /// Where frames are composed when the surface can only be drawn to: the
    /// browser canvas and the OpenGL compatibility mode. `None` elsewhere.
    web_present: Option<Arc<web_present::WebPresent>>,
}

/// The context of every [`GpuContext::new`] failure: this computer has no
/// graphics adapter, driver or surface the renderer can use. Startup tells the
/// player about the graphics driver when it finds this in an error.
#[derive(Debug)]
pub struct GraphicsUnavailable;

impl std::fmt::Display for GraphicsUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("No usable graphics adapter or driver")
    }
}

impl GpuContext {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) async fn new_browser(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
    ) -> Result<Self> {
        // The browser picks its one API itself; there is no start record to name the adapter in.
        Self::new_async(window, display, false, wgpu::Backends::empty(), |_| {}).await
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
        backends: wgpu::Backends,
        on_adapter: impl FnOnce(&wgpu::AdapterInfo),
    ) -> Result<Self> {
        block_on(Self::new_async(
            window,
            display,
            hidden_capture,
            backends,
            on_adapter,
        ))
        .context(GraphicsUnavailable)
    }

    /// Async initialization — called by new() via block_on(). `backends` is
    /// what this start asks wgpu for, and `on_adapter` hears which adapter
    /// was chosen before the driver is asked for a device, the call a driver
    /// can hang in (`persistence::startup_backend::starting`).
    async fn new_async(
        window: Arc<Window>,
        display: OwnedDisplayHandle,
        hidden_capture: bool,
        backends: wgpu::Backends,
        on_adapter: impl FnOnce(&wgpu::AdapterInfo),
    ) -> Result<Self> {
        // The instance over the APIs this start asks for, the surface it
        // draws to and the adapter that can; `choose_adapter` says how the
        // APIs and DirectX 12's shader compilers are chosen.
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let (instance, surface, adapter, fxc_after_dxc) =
            choose_adapter(&window, display, backends).await?;
        // Browsers without a usable WebGPU adapter fall back to WebGL2. A
        // surface-less probe with the adapter options used below decides
        // first: a canvas given a WebGPU context can no longer create WebGL2.
        // wgpu's own probe (util::is_browser_webgpu_supported) asks with
        // default options, which can succeed where this request fails.
        // The browser's APIs are its own.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let _ = backends;
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
                    // wgpu 30 can round reported limits into WebGPU buckets;
                    // off keeps the probe asking for exact limits as in 29.x.
                    apply_limit_buckets: false,
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
                flags: instance_flags(),
                ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display))
            })
        };

        // The surface is what we draw pixels to, and the adapter (physical
        // device) one that can render to it.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let surface: wgpu::Surface<'static> = instance
            .create_surface(window.clone())
            .context("Failed to create wgpu surface from window")?;
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let adapter: wgpu::Adapter = instance
            .request_adapter(&adapter_options(&surface))
            .await
            .context("No suitable GPU adapter found — is a GPU available?")?;

        let adapter_info = adapter.get_info();
        log::info!(
            "Using GPU adapter: {} ({:?}, driver {} {})",
            adapter_info.name,
            adapter_info.backend,
            adapter_info.driver,
            adapter_info.driver_info
        );
        // wgpu compiles DirectX 12 shaders with Microsoft's DXC when
        // dxcompiler.dll sits beside the game, where the Windows installer puts
        // it (`.github/release/windows/fetch-dxc.ps1`), and otherwise with
        // Windows' 2015 FXC (`Dx12Compiler::Auto`), which `choose_adapter`
        // also falls back to for a card DXC hides; a player's log says which.
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        if adapter_info.backend == wgpu::Backend::Dx12 {
            log::info!(
                "DirectX 12 shaders compile with {}",
                if fxc_after_dxc {
                    "FXC: the driver has no Shader Model 6 for DXC"
                } else if dxc_beside_game() {
                    "DXC (dxcompiler.dll beside the game)"
                } else {
                    "FXC: no dxcompiler.dll beside the game"
                }
            );
        }
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        {
            *DEVICE_IDENTITY
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(DeviceIdentity {
                backend: adapter_info.backend,
                vendor: adapter_info.vendor,
                device: adapter_info.device,
                name: adapter_info.name.clone(),
            });
        }
        on_adapter(&adapter_info);

        // Atlas sizing is bounded by whatever limits we *request*, not by the hardware.
        let adapter_limits = adapter.limits();
        let texture_dim = requested_texture_dim(
            adapter_info.backend,
            adapter_info.device_type,
            adapter_limits.max_texture_dimension_2d,
        );
        log::info!(
            "GPU limits: max_texture_dimension_2d={} (requesting {texture_dim}{})",
            adapter_limits.max_texture_dimension_2d,
            if texture_dim
                < adapter_limits
                    .max_texture_dimension_2d
                    .min(MAX_USEFUL_TEXTURE_DIM)
            {
                ", the shared-memory graphics cap"
            } else {
                ""
            },
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
            max_texture_dimension_2d: texture_dim,
            ..wgpu::Limits::default()
        };
        // OpenGL runs the WebGL2 fallback renderers: the browser's WebGL2
        // context, and on the desktop the compatibility mode `choose_adapter`
        // falls back to (`gl_compatibility`). WebGL2 has no storage buffers or compute, so the WebGPU
        // baseline is unattainable; request what the context offers, without
        // storage or compute. Desktop GL may offer both, but asking for none
        // gives the fallback the device it was written for, and renderers
        // detect it through `webgl_fallback`.
        let gl_fallback = adapter_info.backend == wgpu::Backend::Gl;
        let required_limits = if gl_fallback {
            let webgl2 = wgpu::Limits::downlevel_webgl2_defaults();
            wgpu::Limits {
                max_texture_dimension_2d: required_limits.max_texture_dimension_2d,
                max_storage_buffers_per_shader_stage: webgl2.max_storage_buffers_per_shader_stage,
                max_storage_textures_per_shader_stage: webgl2.max_storage_textures_per_shader_stage,
                max_dynamic_storage_buffers_per_pipeline_layout: webgl2
                    .max_dynamic_storage_buffers_per_pipeline_layout,
                max_storage_buffer_binding_size: webgl2.max_storage_buffer_binding_size,
                max_compute_workgroup_storage_size: webgl2.max_compute_workgroup_storage_size,
                max_compute_invocations_per_workgroup: webgl2.max_compute_invocations_per_workgroup,
                max_compute_workgroup_size_x: webgl2.max_compute_workgroup_size_x,
                max_compute_workgroup_size_y: webgl2.max_compute_workgroup_size_y,
                max_compute_workgroup_size_z: webgl2.max_compute_workgroup_size_z,
                max_compute_workgroups_per_dimension: webgl2.max_compute_workgroups_per_dimension,
                ..adapter_limits.clone()
            }
        } else {
            required_limits
        };
        // The GPU timer marks with a compute pass that writes a storage
        // buffer, which the device above has none of; like WebGL2, the
        // compatibility mode goes untimed.
        let required_features = if gl_fallback {
            wgpu::Features::empty()
        } else {
            super::gpu_timer::GpuTimer::features(&adapter)
        };
        // The two calls below are the driver's, and an old one can hang in
        // either: an Intel HD 620's 2017 Vulkan driver never returned from
        // them, and its log ended at the limits line above. Each is named, so
        // a log that stops here says which.
        log::info!("Requesting the graphics device");
        let (device, queue): (wgpu::Device, wgpu::Queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("RA2 Device"),
                required_features,
                required_limits,
                ..Default::default()
            })
            .await
            .context("Failed to create wgpu device")?;
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        watch_device_loss(&device);

        // Pick the best texture format for the surface.
        // This is usually Bgra8UnormSrgb on Windows, Bgra8Unorm on some other platforms.
        let surface_caps: wgpu::SurfaceCapabilities = surface.get_capabilities(&adapter);
        // The browser canvas and an OpenGL surface are drawn to only
        // (`web_present`); frames are composed in a texture of their own.
        let present_offscreen = cfg!(all(
            feature = "wasm",
            target_arch = "wasm32",
            target_os = "unknown"
        )) || gl_fallback;
        let required_surface_usages = if present_offscreen {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
        };
        if !surface_caps.usages.contains(required_surface_usages) {
            bail!(
                "exact stock-shell presentation requires surface usages \
                 {required_surface_usages:?}, adapter exposes {:?}",
                surface_caps.usages
            );
        }
        let downlevel = adapter.get_downlevel_capabilities();
        // The WebGL2 fallback replaces the requirements below with
        // render-target equivalents; see `webgl_fallback`.
        let webgl = gl_fallback;
        if !webgl && !downlevel.flags.contains(wgpu::DownlevelFlags::VIEW_FORMATS) {
            bail!(
                "exact stock-shell presentation requires compatible sRGB/unorm \
                 texture views, adapter downlevel flags are {:?}",
                downlevel.flags
            );
        }
        // The packed sprite resolve reads its compute results through a
        // writable storage binding (terrain_packed.rs). Reject an insufficient
        // downlevel adapter here rather than panicking later during bind group
        // layout validation.
        if !webgl
            && !downlevel
                .flags
                .contains(wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE)
        {
            bail!(
                "native sprite drawing requires fragment-writable storage, \
                 adapter downlevel flags are {:?}",
                downlevel.flags
            );
        }
        // The packed sprite stencil and its destination snapshot share a pass
        // with per-attachment write masks (terrain_packed.rs).
        if !webgl
            && !downlevel
                .flags
                .contains(wgpu::DownlevelFlags::INDEPENDENT_BLEND)
        {
            bail!(
                "native sprite drawing requires per-attachment write masks, \
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
        // Desktop OpenGL has no reinterpreting views, so `web_present` can
        // only copy the sRGB frame to an sRGB window surface.
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        if gl_fallback && !surface_format.is_srgb() {
            bail!(
                "OpenGL presentation requires an sRGB window surface, adapter offers {:?}",
                surface_caps.formats
            );
        }

        // Configure the surface with our chosen format and the window's current size.
        let window_size: winit::dpi::PhysicalSize<u32> = window.inner_size();
        let (present_mode, desired_maximum_frame_latency) = present_settings(true);
        let config: wgpu::SurfaceConfiguration = wgpu::SurfaceConfiguration {
            usage: required_surface_usages,
            format: surface_format,
            width: window_size.width.max(1), // wgpu panics on 0-sized surfaces
            height: window_size.height.max(1),
            // `App::sync_present_mode` applies the Options page's VSync box
            // once the profile is loaded.
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency,
            // wgpu 30 selects an HDR colour space per surface format; Auto keeps
            // the pre-30 sRGB behaviour for every desktop and WebGL2 backend.
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        log::info!(
            "Configuring the surface: {}x{} {:?}, {:?}",
            config.width,
            config.height,
            config.format,
            config.present_mode
        );
        surface.configure(&device, &config);
        let web_present = present_offscreen
            .then(|| Arc::new(web_present::WebPresent::new(&device, &queue, &config)));
        let surface_format = if present_offscreen {
            surface_format.add_srgb_suffix()
        } else {
            surface_format
        };
        // wgpu-hal 30.0.1 Metal acquire_texture returns SurfaceError::Occluded
        // for any macOS NSWindow without NSWindowOcclusionStateVisible, including
        // deliberately hidden captures. Render those frames offscreen with the
        // same format/usages; the normal render, loading, readback and
        // capture-driver paths remain shared. Ordinary occluded windows retain
        // swapchain skip/recovery.
        // https://docs.rs/wgpu-hal/30.0.1/src/wgpu_hal/metal/surface.rs.html#347-371
        let capture_target = hidden_capture.then(|| CaptureTarget::new(&device, &config));
        // The timer builds a compute pipeline, and an old driver's shader
        // compiler can fail on one: an Intel Iris Pro's Metal compiler did
        // on wgpu's indirect-call check ([`instance_flags`]). wgpu reports
        // that as an internal error, which would end the game; it goes untimed.
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let timer = super::gpu_timer::GpuTimer::new(&device, &queue);
        let (validation, internal) = (validation.pop(), internal.pop());
        let timer = match validation.await.or(internal.await) {
            None => timer,
            Some(error) => {
                log::warn!("GPU timer unavailable, frames go untimed: {error}");
                super::gpu_timer::GpuTimer::default()
            }
        };

        log::info!(
            "GPU initialized: {}x{}, format={:?}",
            config.width,
            config.height,
            surface_format
        );

        let handoff = super::frame_handoff::FrameHandoff::new();
        let queue = super::frame_handoff::HandoffQueue::new(queue, handoff.waiter());
        Ok(Self {
            handoff,
            surface: Mutex::new(Some(surface)),
            instance,
            window,
            device,
            queue,
            config,
            surface_format,
            adapter_info,
            capture_target,
            timer,
            color_vision: super::color_vision::ColorVisionFilter::new(),
            web_present,
        })
    }

    /// Publish this frame's colour-vision correction; `None` means `Off`.
    /// The frame loop calls this once per frame, so the synchronous loading
    /// repaints (which present outside the frame loop) use the same value.
    pub(crate) fn set_color_vision_matrix(&self, matrix: Option<super::color_vision::Mat3>) {
        self.color_vision.set_matrix(matrix);
    }

    /// Apply the published correction as the last draw before `present`.
    pub(crate) fn encode_color_vision(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
    ) {
        self.color_vision
            .encode(&self.device, &self.queue, encoder, target);
    }

    /// Finish and submit `encoder`, then present `frame`, on the hand-off
    /// thread (`frame_handoff`). Bounded submissions the frame already made
    /// stay ahead of this one, as they went through the queue on this thread.
    pub(crate) fn hand_off_frame(
        &self,
        encoder: wgpu::CommandEncoder,
        frame: GpuFrame,
    ) -> super::frame_handoff::FrameSubmission {
        let queue = (*self.queue).clone();
        let index = Arc::new(std::sync::OnceLock::new());
        let submitted = Arc::clone(&index);
        self.handoff.send(move || {
            let _ = submitted.set(queue.submit(std::iter::once(encoder.finish())));
            frame.present(&queue);
        });
        super::frame_handoff::FrameSubmission::new(index, self.handoff.waiter())
    }

    /// The time this thread spent blocked on a frame on its way to the driver
    /// since the last call.
    pub(crate) fn take_handoff_wait(&self) -> crate::util::time::Duration {
        self.handoff.waiter().take_waited()
    }

    /// Return a borrowed capture-only view of the selected adapter identity.
    pub(crate) fn capture_adapter_observation(&self) -> GpuAdapterObservation<'_> {
        GpuAdapterObservation::from(&self.adapter_info)
    }

    /// The adapter's name when it is the system's software renderer, which
    /// draws with the processor: Windows' Microsoft Basic Render Driver or
    /// Mesa's llvmpipe. wgpu keeps it as the last choice when no graphics card
    /// and driver qualify (`request_adapter` sorts `DeviceType::Cpu` last
    /// instead of leaving it out); Metal never offers one.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    pub(crate) fn software_renderer(&self) -> Option<&str> {
        (self.adapter_info.device_type == wgpu::DeviceType::Cpu)
            .then_some(self.adapter_info.name.as_str())
    }

    /// The graphics API the device draws through.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    pub(crate) fn graphics_api(&self) -> wgpu::Backend {
        self.adapter_info.backend
    }

    /// What the driver said once it has lost the graphics device: it crashed,
    /// Windows reset it after it stopped responding (TDR), or the card ran out
    /// of memory. Nothing reaches the screen after that, while the game would
    /// run on behind a black window. `None` while the device works.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    pub(crate) fn device_lost(&self) -> Option<&str> {
        lost_device()
    }

    /// Whether this device draws from the system's memory
    /// ([`shared_memory_graphics`]).
    pub(crate) fn shared_memory_graphics(&self) -> bool {
        shared_memory_graphics(self.adapter_info.backend, self.adapter_info.device_type)
    }

    /// Why DirectX 12 lost the device, in the driver's own terms: a GPU that
    /// stopped responding (Windows ends that after two seconds), a reset, a
    /// driver internal error or a rejected call. wgpu reports every one as
    /// "Device is lost", so the reason comes from `GetDeviceRemovedReason` on
    /// the raw device, which DirectX defines for exactly this state. `None` on
    /// other APIs and platforms.
    #[cfg(windows)]
    pub(crate) fn device_removed_reason(&self) -> Option<String> {
        use windows::Win32::Graphics::Dxgi;
        if self.adapter_info.backend != wgpu::Backend::Dx12 {
            return None;
        }
        // SAFETY: the hal device is only read, through a method DirectX 12
        // defines for a lost device, and the guard is dropped before the
        // device could be.
        let hal = unsafe { self.device.as_hal::<wgpu::hal::api::Dx12>() }?;
        // SAFETY: as above: a read of the live hal device.
        let result = unsafe { hal.raw_device().GetDeviceRemovedReason() };
        let error = match result {
            Ok(()) => return Some("DirectX reports no removal".to_owned()),
            Err(error) => error,
        };
        let code = error.code();
        let meaning = if code == Dxgi::DXGI_ERROR_DEVICE_HUNG {
            "DXGI_ERROR_DEVICE_HUNG: the GPU stopped responding to its commands, which Windows ends after two seconds"
        } else if code == Dxgi::DXGI_ERROR_DEVICE_RESET {
            "DXGI_ERROR_DEVICE_RESET: the driver reset the GPU"
        } else if code == Dxgi::DXGI_ERROR_DEVICE_REMOVED {
            "DXGI_ERROR_DEVICE_REMOVED: the adapter was removed or its driver updated"
        } else if code == Dxgi::DXGI_ERROR_DRIVER_INTERNAL_ERROR {
            "DXGI_ERROR_DRIVER_INTERNAL_ERROR: the driver failed inside itself"
        } else if code == Dxgi::DXGI_ERROR_INVALID_CALL {
            "DXGI_ERROR_INVALID_CALL: the driver was given a call it rejects"
        } else {
            "an unlisted reason"
        };
        Some(format!("DirectX removal reason {code}: {meaning}"))
    }

    #[cfg(not(windows))]
    pub(crate) fn device_removed_reason(&self) -> Option<String> {
        None
    }

    /// Whether the window currently owns a render surface.
    #[cfg(target_os = "android")]
    pub(crate) fn has_surface(&self) -> bool {
        self.surface
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Release Android's native surface before the suspended callback returns.
    /// Device resources survive; no acquisition can recreate the surface until
    /// the next resumed callback explicitly restores it.
    /// https://docs.rs/winit/0.30.13/winit/application/trait.ApplicationHandler.html#method.suspended
    #[cfg(target_os = "android")]
    pub(crate) fn suspend_surface(&mut self) {
        self.handoff.waiter().wait();
        drop(
            self.surface
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }

    /// Rebuild the swapchain against the window's current native surface.
    ///
    /// Android destroys the activity's surface when the screen turns off or the
    /// app goes to the background, and winit's `Window` reports the replacement
    /// native handle once it is resumed. A wgpu surface keeps the handle it was
    /// created with, so it can never present again — reconfiguring it is not
    /// enough, it has to be replaced.
    /// Retain the configured dimensions here: the app's existing resize owner
    /// updates the depth and offscreen targets together with the surface.
    #[cfg(target_os = "android")]
    pub(crate) fn restore_surface(&mut self) -> anyhow::Result<()> {
        self.handoff.waiter().wait();
        let slot = self
            .surface
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Ok(());
        }
        let surface = self
            .instance
            .create_surface(self.window.clone())
            .context("recreate the wgpu surface after a resume")?;
        surface.configure(&self.device, &self.config);
        *slot = Some(surface);
        log::info!(
            "Surface restored at {}x{}",
            self.config.width,
            self.config.height
        );
        Ok(())
    }

    /// Handle window resize — reconfigure the surface with new dimensions.
    ///
    /// Called from ApplicationHandler::window_event when WindowEvent::Resized fires.
    /// Ignores zero-sized dimensions (happens during window minimize on some platforms).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // Minimized window — skip reconfigure
        }
        // Configuring a surface needs its last frame presented.
        self.handoff.waiter().wait();
        self.config.width = width;
        self.config.height = height;
        if let Some(surface) = self
            .surface
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
        {
            surface.configure(&self.device, &self.config);
        }
        if let Some(target) = &mut self.capture_target {
            target.resize(&self.device, &self.config);
        }
        if self.web_present.is_some() {
            self.web_present = Some(Arc::new(web_present::WebPresent::new(
                &self.device,
                &self.queue,
                &self.config,
            )));
        }
        log::debug!("Surface resized to {}x{}", width, height);
    }

    /// Present in step with the display (`Fifo`), or as soon as a frame is
    /// drawn: `AutoNoVsync` takes `Immediate`, else `Mailbox`, else `Fifo`, so
    /// a surface without the first two keeps syncing. The Options page's
    /// VSync box (`[VERA] VSync=`).
    /// https://docs.rs/wgpu/30.0.1/wgpu/enum.PresentMode.html
    pub fn set_vsync(&mut self, vsync: bool) {
        let (mode, latency) = present_settings(vsync);
        if self.config.present_mode == mode && self.config.desired_maximum_frame_latency == latency
        {
            return;
        }
        // Configuring a surface needs its last frame presented.
        self.handoff.waiter().wait();
        self.config.present_mode = mode;
        self.config.desired_maximum_frame_latency = latency;
        if let Some(surface) = self
            .surface
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
        {
            surface.configure(&self.device, &self.config);
        }
        log::info!("Surface present mode: {mode:?}");
    }

    /// Take the window's next frame to draw into, or `None` when there is none
    /// this time and the next redraw should try again.
    ///
    /// A stale or suboptimal swapchain is reconfigured once and asked again;
    /// a lost surface is recreated first, as wgpu requires. Timeout and
    /// occlusion skip this redraw. Validation failures remain errors.
    /// https://docs.rs/wgpu/30.0.1/wgpu/enum.CurrentSurfaceTexture.html
    pub fn acquire_frame(&self) -> Result<Option<GpuFrame>> {
        // A surface gives out one frame at a time, and drawing keeps plain
        // queue clones that do not wait for themselves (`HandoffQueue`).
        self.handoff.waiter().wait();
        let mut surface = self.surface.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(surface) = surface.as_mut() else {
            return Ok(None);
        };
        if let Some(target) = &self.capture_target {
            return Ok(Some(target.frame()));
        }
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
                match &self.web_present {
                    Some(present) => GpuFrame {
                        texture: present.texture.clone(),
                        web_present: Some(Arc::clone(present)),
                        ..frame
                    },
                    None => frame,
                }
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        texture.create_view(&Default::default())
    }
}

/// What the driver said when it lost the process's graphics device
/// (`watch_device_loss`), for [`GpuContext::device_lost`] and for the panic
/// hook: a panic after the loss (wgpu's own, on the first buffer it maps on
/// the dead device, or any renderer's) is the loss's consequence, and the
/// player is told about the driver, not about an internal error
/// (`app::system_dialog::report_panic`). The game has one device; a second
/// context would share this.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
static LOST_DEVICE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The process's graphics device: the API and adapter it runs on, for the
/// start record (`persistence::startup_backend`) when the device is lost
/// before startup finishes, which the panic hook reports too.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
#[derive(Clone)]
pub(crate) struct DeviceIdentity {
    pub(crate) backend: wgpu::Backend,
    /// PCI vendor and device ids, the same on every API.
    pub(crate) vendor: u32,
    pub(crate) device: u32,
    pub(crate) name: String,
}

/// The last adapter chosen wins: a start whose first API could not start goes
/// on with another (`persistence::startup_backend::after_failure`), and the
/// record must hear of the API the game runs on, not the one it gave up.
/// Process-wide, behind a lock, because the panic hook reads it from
/// whichever thread panicked (`persistence::startup_backend::record_loss`).
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
static DEVICE_IDENTITY: Mutex<Option<DeviceIdentity>> = Mutex::new(None);

/// The device the game runs on, once `GpuContext::new` chose it.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn device_identity() -> Option<DeviceIdentity> {
    DEVICE_IDENTITY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The driver's message once the graphics device is lost; `None` while it
/// works. See [`GpuContext::device_lost`].
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn lost_device() -> Option<&'static str> {
    LOST_DEVICE.get().map(String::as_str)
}

/// Watch `device` for its loss. wgpu reports a loss once, on whichever thread
/// met the driver's error (this one, the frame hand-off thread or a loading
/// thread); its message is kept for [`GpuContext::device_lost`] and logged.
/// Every resource is invalid after a loss, so the work recorded until the
/// frame loop sees it fails validation: those errors are logged, not fatal.
/// Before a loss an error still ends the game, as wgpu's default handler
/// (`default_error_handler`) does.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn watch_device_loss(device: &wgpu::Device) {
    device.set_device_lost_callback(move |reason, message| {
        log::error!("Graphics device lost ({reason:?}): {message}");
        let _ = LOST_DEVICE.set(message);
    });
    let logged = std::sync::atomic::AtomicBool::new(false);
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
        if lost_device().is_none() {
            log::error!("Handling wgpu errors as fatal by default");
            panic!("wgpu error: {error}\n");
        }
        if !logged.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!("Ignoring wgpu errors on the lost device, the first: {error}");
        }
    }));
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

/// The present mode and queued pictures for the VSync box: `Fifo` in step
/// with the display, else `AutoNoVsync` (`Immediate`, else `Mailbox`, else
/// `Fifo`).
///
/// VSync keeps wgpu's default two pictures queued, as every build before the
/// box did. With one, the CPU and GPU no longer overlap: a picture whose work
/// takes longer than one refresh waits for the next, and since a lent tick
/// starts once per picture, the game itself slows to half speed. Without VSync
/// a present never waits for the screen, so one queued picture only cuts the
/// input delay.
/// https://docs.rs/wgpu/30.0.1/wgpu/struct.SurfaceConfiguration.html#structfield.desired_maximum_frame_latency
fn present_settings(vsync: bool) -> (wgpu::PresentMode, u32) {
    if vsync {
        (wgpu::PresentMode::Fifo, 2)
    } else {
        (wgpu::PresentMode::AutoNoVsync, 1)
    }
}

/// The adapter request every start makes: one that can draw to `surface`,
/// the discrete card where a machine has two.
fn adapter_options<'a, 'b>(surface: &'a wgpu::Surface<'b>) -> wgpu::RequestAdapterOptions<'a, 'b> {
    wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(surface),
        force_fallback_adapter: false,
        // wgpu 30 can round the adapter's reported limits down to the
        // WebGPU bucket it belongs to; off keeps the exact 29.x limits.
        apply_limit_buckets: false,
    }
}

/// A wgpu instance over `backends` whose DirectX 12 shaders `compiler`
/// builds. wgpu requires the display at instance creation for Wayland EGL:
/// https://docs.rs/wgpu-types/30.0.1/wgpu_types/struct.InstanceDescriptor.html
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn instance_with(
    backends: wgpu::Backends,
    display: &OwnedDisplayHandle,
    compiler: wgpu::Dx12Compiler,
) -> wgpu::Instance {
    let mut descriptor =
        wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display.clone()));
    descriptor.backends = backends;
    descriptor.flags = instance_flags();
    descriptor.backend_options.dx12.shader_compiler = compiler;
    wgpu::Instance::new(descriptor)
}

/// wgpu's flags for the build without its indirect-call check. That check
/// guards `draw_indirect` and `dispatch_workgroups_indirect`, which VERA never
/// calls, yet wgpu builds its compute pipeline with every device, and a
/// device whose pipeline fails is refused: an Intel Iris Pro's Metal compiler
/// failed on it ("Compiler encountered an internal error"), and the game
/// could not start. `WGPU_VALIDATION_INDIRECT_CALL=1` turns it back on.
fn instance_flags() -> wgpu::InstanceFlags {
    wgpu::InstanceFlags::from_build_config()
        .difference(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL)
        .with_env()
}

/// Whether `dxcompiler.dll` sits beside the game, where wgpu's
/// `Dx12Compiler::Auto` loads it from. Only a DirectX 12 start asks.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn dxc_beside_game() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("dxcompiler.dll")))
        .is_some_and(|dll| dll.is_file())
}

/// The instance, surface and adapter a start draws through, and whether the
/// adapter came through FXC after DXC had hidden it.
///
/// Where a driver offers Vulkan and DirectX 12, wgpu takes Vulkan, the first
/// API it lists. An Intel card is the exception: its Vulkan driver has hung
/// in the device request, old and current, so the start asks DirectX 12 for
/// the card it found through Vulkan (`prefer_dx12_for`), with DXC and then
/// with FXC, and keeps Vulkan only when DirectX 12 offers no card. A start
/// record asks for one API alone (`persistence::startup_backend`). wgpu's
/// own override (`WGPU_BACKEND`) wins over either, for reproducible shroud
/// GPU comparisons.
///
/// wgpu's DirectX 12 backend compiles shaders with Microsoft's DXC where
/// `dxcompiler.dll` sits beside the game, and then lists no card whose driver
/// stops at Shader Model 5.1, since DXC's output needs 6.0 (wgpu-hal
/// `dx12/adapter.rs`: "don't expose this adapter if it doesn't support
/// DXIL"). Intel's drivers before 15.60 and NVIDIA's before 387.92, both of
/// late 2017, are such: an HD 620 on 21.20.16.4574 drew a match through
/// DirectX 12 before the DLL shipped and found only Windows' software
/// renderer, which draws with the processor, after. So a start that finds no
/// card, or only the processor, asks DirectX 12 again with FXC, Windows' own
/// compiler, which every such driver serves, and keeps the card that answers.
/// The second instance costs milliseconds, on a start that was headed for
/// the processor anyway.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
async fn choose_adapter(
    window: &Arc<Window>,
    display: OwnedDisplayHandle,
    backends: wgpu::Backends,
) -> Result<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter, bool)> {
    let backends = backends.with_env();
    let instance = instance_with(backends, &display, wgpu::Dx12Compiler::Auto);
    let surface = instance
        .create_surface(window.clone())
        .context("Failed to create wgpu surface from window")?;
    let first = instance.request_adapter(&adapter_options(&surface)).await;
    let found = first.as_ref().ok().map(wgpu::Adapter::get_info);
    if cfg!(target_os = "windows")
        && let Some(info) = &found
        && prefer_dx12_for(info.backend, info.vendor, backends)
    {
        for compiler in [wgpu::Dx12Compiler::Auto, wgpu::Dx12Compiler::Fxc] {
            let fxc = matches!(compiler, wgpu::Dx12Compiler::Fxc);
            if let Some((dx12, dx12_surface, card)) = dx12_card(window, &display, compiler).await {
                let card_info = card.get_info();
                log::warn!(
                    "Vulkan offered {} (driver {} {}), an Intel driver that has hung in the device request; DirectX 12 offers the card as {} (driver {} {}), so the game draws through DirectX 12{}",
                    info.name,
                    info.driver,
                    info.driver_info,
                    card_info.name,
                    card_info.driver,
                    card_info.driver_info,
                    if fxc {
                        " with FXC: its driver has no Shader Model 6 for DXC"
                    } else {
                        ""
                    }
                );
                log_adapters(&instance, backends).await;
                return Ok((dx12, dx12_surface, card, fxc));
            }
        }
    }
    if cfg!(target_os = "windows")
        && retry_dx12_with_fxc(found.as_ref().map(|info| info.device_type), backends)
        && let Some((fxc, fxc_surface, adapter)) =
            dx12_card(window, &display, wgpu::Dx12Compiler::Fxc).await
    {
        let info = adapter.get_info();
        log::warn!(
            "DirectX 12 with DXC offered {}; {} (driver {} {}) answers with FXC, so its driver has no Shader Model 6 and the game draws through it with FXC. A graphics driver from late 2017 or newer would allow DXC",
            match found {
                Some(_) => "only the software renderer",
                None => "no adapter",
            },
            info.name,
            info.driver,
            info.driver_info
        );
        log_adapters(&fxc, wgpu::Backends::DX12).await;
        return Ok((fxc, fxc_surface, adapter, true));
    }
    if !first.as_ref().is_ok_and(graphics_card)
        && let Some((gl, gl_surface, adapter)) = gl_compatibility(window, &display, backends).await
    {
        log_adapters(&instance, backends).await;
        return Ok((gl, gl_surface, adapter, false));
    }
    let adapter = first.context("No suitable GPU adapter found — is a GPU available?")?;
    log_adapters(&instance, backends).await;
    Ok((instance, surface, adapter, false))
}

/// Whether `adapter` is a graphics card rather than the processor's software
/// renderer (`GpuContext::software_renderer`).
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn graphics_card(adapter: &wgpu::Adapter) -> bool {
    adapter.get_info().device_type != wgpu::DeviceType::Cpu
}

/// A graphics card through DirectX 12 alone, shaders compiled by `compiler`:
/// the instance, its window surface and the adapter, or `None` when DirectX
/// 12 offers no card that way (no adapter, or only the software renderer).
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
async fn dx12_card(
    window: &Arc<Window>,
    display: &OwnedDisplayHandle,
    compiler: wgpu::Dx12Compiler,
) -> Option<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter)> {
    let instance = instance_with(wgpu::Backends::DX12, display, compiler);
    let surface = instance.create_surface(window.clone()).ok()?;
    let adapter = instance
        .request_adapter(&adapter_options(&surface))
        .await
        .ok()?;
    graphics_card(&adapter).then_some((instance, surface, adapter))
}

/// The PCI vendor of Intel's graphics.
const INTEL_VENDOR: u32 = 0x8086;

/// Whether a start whose first adapter is an Intel card through Vulkan asks
/// DirectX 12 for the same card instead (`choose_adapter`). Intel's Vulkan
/// driver has hung in the device request on an HD 620, on its 2017 package
/// (21.20.16.4574) and on its 2023 one (31.0.101.2145), where DirectX 12 on
/// the same card starts; other vendors keep wgpu's preference, Vulkan. A
/// start that asks for Vulkan alone (`[VERA] GraphicsAPI=`, `WGPU_BACKEND`)
/// keeps it.
fn prefer_dx12_for(backend: wgpu::Backend, vendor: u32, backends: wgpu::Backends) -> bool {
    backend == wgpu::Backend::Vulkan
        && vendor == INTEL_VENDOR
        && backends.contains(wgpu::Backends::DX12)
}

/// OpenGL in compatibility mode, for a start whose APIs (`choose_adapter`)
/// found no graphics card: wgpu leaves only the software renderer then,
/// which draws nothing playable. Older integrated graphics end up there:
/// Intel's 4th generation has DirectX 12 resource binding tier 1, which
/// wgpu-hal 30 `dx12/adapter.rs` refuses ("We require Tier 2"), and no
/// Vulkan driver. Its OpenGL driver is the card's, and the renderers' WebGL2
/// fallback draws through it (`webgl_fallback`).
///
/// `None`, keeping the start's own adapter, where the start asked for OpenGL
/// itself (`WGPU_BACKEND=gl`), where it asked for one API and the others
/// have a card (the record or the player chose that API), and where OpenGL
/// too offers only a software renderer or no sRGB window surface, which the
/// compatibility presentation needs (`web_present`).
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
async fn gl_compatibility(
    window: &Arc<Window>,
    display: &OwnedDisplayHandle,
    backends: wgpu::Backends,
) -> Option<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter)> {
    if backends.contains(wgpu::Backends::GL) {
        return None;
    }
    if backends != wgpu::Backends::PRIMARY
        && instance_with(wgpu::Backends::PRIMARY, display, wgpu::Dx12Compiler::Auto)
            .enumerate_adapters(wgpu::Backends::PRIMARY)
            .await
            .iter()
            .any(graphics_card)
    {
        return None;
    }
    let gl = instance_with(wgpu::Backends::GL, display, wgpu::Dx12Compiler::Auto);
    let surface = match gl.create_surface(window.clone()) {
        Ok(surface) => surface,
        Err(error) => {
            log::info!("OpenGL compatibility mode unavailable: {error}");
            return None;
        }
    };
    let adapter = match gl.request_adapter(&adapter_options(&surface)).await {
        Ok(adapter) => adapter,
        Err(error) => {
            log::info!("OpenGL compatibility mode unavailable: {error}");
            return None;
        }
    };
    let srgb = surface
        .get_capabilities(&adapter)
        .formats
        .iter()
        .any(|format| format.is_srgb());
    let name = adapter.get_info().name;
    if !graphics_card(&adapter) || !srgb {
        log::info!(
            "OpenGL compatibility mode unavailable: {name} is a software renderer or offers no sRGB window surface"
        );
        return None;
    }
    log::warn!(
        "No graphics card qualified for Vulkan, DirectX 12 or Metal; using {name} through OpenGL in compatibility mode"
    );
    Some((gl, surface, adapter))
}

/// Whether a start that found no graphics card through its first instance
/// asks DirectX 12 again with FXC (`choose_adapter`): it asked for DirectX
/// 12 and found no adapter, or only the processor's software renderer, which
/// wgpu sorts last (`request_adapter`), so a card on any API would have won.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn retry_dx12_with_fxc(found: Option<wgpu::DeviceType>, backends: wgpu::Backends) -> bool {
    backends.contains(wgpu::Backends::DX12)
        && found.is_none_or(|kind| kind == wgpu::DeviceType::Cpu)
}

/// One line naming every adapter `instance` offers through `backends`, so a
/// player's log says what their machine had besides the one chosen.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
async fn log_adapters(instance: &wgpu::Instance, backends: wgpu::Backends) {
    let offered: Vec<String> = instance
        .enumerate_adapters(backends)
        .await
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            format!(
                "{} ({:?}, {:?}, driver {} {})",
                info.name, info.backend, info.device_type, info.driver, info.driver_info
            )
        })
        .collect();
    log::info!("Graphics adapters offered: {}", offered.join("; "));
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::Poll;

    use wgpu::CurrentSurfaceTexture::{Lost, Occluded, Outdated, Timeout, Validation};

    use super::{
        CaptureTarget, SurfaceRecovery, acquire_with_recovery, block_on, lost_device,
        present_settings, requested_texture_dim, watch_device_loss,
    };

    /// With VSync a picture over one refresh must not wait a whole refresh
    /// more: two pictures stay queued. Without it, one keeps input delay low.
    #[test]
    fn vsync_keeps_two_pictures_queued() {
        assert_eq!(present_settings(true), (wgpu::PresentMode::Fifo, 2));
        assert_eq!(present_settings(false), (wgpu::PresentMode::AutoNoVsync, 1));
    }

    /// A start that DirectX 12 with DXC left on the processor, or with no
    /// adapter at all, asks again with FXC; one that found a card, or never
    /// asked for DirectX 12, does not.
    #[test]
    fn only_a_start_without_a_card_asks_directx_12_again_with_fxc() {
        use super::retry_dx12_with_fxc;
        use wgpu::DeviceType::{Cpu, IntegratedGpu};
        assert!(retry_dx12_with_fxc(Some(Cpu), wgpu::Backends::DX12));
        assert!(retry_dx12_with_fxc(None, wgpu::Backends::PRIMARY));
        assert!(!retry_dx12_with_fxc(
            Some(IntegratedGpu),
            wgpu::Backends::DX12
        ));
        assert!(!retry_dx12_with_fxc(Some(Cpu), wgpu::Backends::VULKAN));
    }

    /// wgpu's indirect-call check stays off: its compute pipeline, built
    /// with every device, kept an Intel Iris Pro Mac from starting.
    #[test]
    fn the_game_asks_wgpu_for_no_indirect_call_check() {
        assert!(!super::instance_flags().contains(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL));
    }

    /// An Intel card found through Vulkan is asked for through DirectX 12
    /// where the start may ask for DirectX 12 at all; another vendor's card,
    /// or one found through DirectX 12 already, is kept.
    #[test]
    fn an_intel_card_on_vulkan_is_asked_for_through_directx_12() {
        use super::{INTEL_VENDOR, prefer_dx12_for};
        use wgpu::Backend::{Dx12, Vulkan};
        assert!(prefer_dx12_for(
            Vulkan,
            INTEL_VENDOR,
            wgpu::Backends::PRIMARY
        ));
        assert!(!prefer_dx12_for(Vulkan, 0x10de, wgpu::Backends::PRIMARY));
        assert!(!prefer_dx12_for(
            Dx12,
            INTEL_VENDOR,
            wgpu::Backends::PRIMARY
        ));
        assert!(!prefer_dx12_for(
            Vulkan,
            INTEL_VENDOR,
            wgpu::Backends::VULKAN
        ));
    }

    /// A lost device reaches the frame loop, and a working one does not. The
    /// frame recorded meanwhile, as in a player's DirectX 12 log, writes its
    /// uploads into staging chunks made on the lost device and unmaps them:
    /// neither may end the game first. A driver's loss can't be staged;
    /// `destroy` loses the device through the same callback, at the next poll.
    #[test]
    #[ignore = "requires a wgpu adapter; loses a real device"]
    fn a_lost_device_reaches_the_frame_loop_without_a_panic() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("this test requires a wgpu adapter");
        let (device, _queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        watch_device_loss(&device);
        let poll = || device.poll(wgpu::PollType::wait_indefinitely());
        poll().unwrap();
        assert_eq!(lost_device(), None);
        device.destroy();
        let _ = poll();
        assert!(lost_device().is_some());

        let mut encoder = device.create_command_encoder(&Default::default());
        let mut pool = crate::render::batch::InstanceBufferPool::new();
        pool.record_uploads(&device, &mut encoder)
            .upload("units", &[crate::render::batch::SpriteInstance::default()]);
        drop(encoder.finish());
    }

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
    fn shared_memory_graphics_get_smaller_texture_pages_outside_metal() {
        use wgpu::Backend::{Dx12, Metal, Vulkan};
        use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu};
        assert_eq!(requested_texture_dim(Dx12, IntegratedGpu, 16_384), 8_192);
        assert_eq!(requested_texture_dim(Vulkan, Cpu, 16_384), 8_192);
        assert_eq!(requested_texture_dim(Dx12, DiscreteGpu, 16_384), 16_384);
        assert_eq!(requested_texture_dim(Metal, IntegratedGpu, 16_384), 16_384);
        // The adapter's own limit still bounds every request.
        assert_eq!(requested_texture_dim(Dx12, IntegratedGpu, 4_096), 4_096);
        assert_eq!(requested_texture_dim(Dx12, DiscreteGpu, 32_768), 16_384);
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
        use crate::render::frame_readback::PendingTextureReadback;

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
            color_space: wgpu::SurfaceColorSpace::Auto,
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
                let pending = PendingTextureReadback::encode(
                    &device,
                    &mut encoder,
                    &frame.texture,
                    config.format,
                    config.width,
                    config.height,
                )
                .expect("read back the actual capture frame");
                let submission = queue.submit([encoder.finish()]);
                frame.present(&queue);
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
