//! Process-wide renderer owner (F12 `RendererState`): the GPU context,
//! batch renderer, per-frame GPU pools/targets, presentation passes, and the
//! process-lifetime rendering caches.
//!
//! Match-scoped visuals (atlases, shroud buffer, minimap) stay outside this
//! owner — they are rebuilt per map and belong to match presentation.

use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::egui_integration::EguiIntegration;
use crate::render::gpu::GpuContext;

pub(crate) struct RendererState {
    pub(crate) gpu: GpuContext,
    pub(crate) batch_renderer: BatchRenderer,
    pub(crate) terrain_draw_renderer: crate::render::terrain_draw::TerrainDrawRenderer,
    pub(crate) combat_light_renderer: crate::render::combat_light::CombatLightRenderer,
    /// Reusable GPU instance buffers — avoids per-frame GPU buffer allocation.
    pub(crate) instance_pool: crate::render::batch::InstanceBufferPool,
    /// GPU depth texture for back-to-front depth ordering. Recreated on window resize.
    pub(crate) depth_view: wgpu::TextureView,
    /// Encoded-byte RGB565 presentation boundary for stock shell/loading surfaces.
    pub(crate) shell_surface_presenter: crate::render::shell_surface_present::ShellSurfacePresenter,
    /// Optional Catmull-Rom bicubic upscale pass (render at lower res, upscale to window).
    pub(crate) upscale_pass: Option<crate::render::upscale_pass::UpscalePass>,
    /// egui integration — input handling + GPU rendering.
    pub(super) egui: EguiIntegration,
    /// GAME.FNT bitmap font (falls back to the built-in 5x7 face).
    pub(crate) bit_font: BitFont,
    pub(crate) vxl_slope_transition_cache:
        std::cell::RefCell<crate::render::unit_slope_transition_cache::VxlSlopeTransitionCache>,
    /// Previous presented pre-cursor composition, retained for input-time
    /// screenshot parity.
    pub(crate) retail_screenshot_frame_cache: crate::render::screenshot::PresentedFrameCache,
}
