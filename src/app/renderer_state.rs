//! Process-wide renderer owner (F12 `RendererState`): the GPU context,
//! batch renderer, per-frame GPU pools/targets, presentation passes, and the
//! process-lifetime rendering caches.
//!
//! Match-scoped visuals (atlases, shroud buffer, minimap) stay outside this
//! owner — they are rebuilt per map and belong to match presentation.

use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;

pub(crate) struct RendererState {
    pub(crate) gpu: GpuContext,
    pub(crate) batch_renderer: BatchRenderer,
    pub(crate) fps_overlay: crate::app::diagnostics::fps_overlay::FpsOverlay,
    pub(crate) terrain_draw_renderer: crate::render::terrain_draw::TerrainDrawRenderer,
    pub(crate) combat_light_renderer: crate::render::combat_light::CombatLightRenderer,
    /// Process GPU resources for the native final client-pixel shake copy.
    pub(crate) screen_presentation: crate::render::screen_presentation::ScreenPresentationPass,
    /// Reusable GPU instance buffers — avoids per-frame GPU buffer allocation.
    pub(crate) instance_pool: crate::render::batch::InstanceBufferPool,
    /// GPU depth texture for back-to-front depth ordering. Recreated on window resize.
    pub(crate) depth_view: wgpu::TextureView,
    /// Encoded-byte RGB565 presentation boundary for stock shell/loading surfaces.
    pub(crate) shell_surface_presenter: crate::render::shell_surface_present::ShellSurfacePresenter,
    /// The match zoom pass, present only while the Options zoom gives a factor
    /// above 1 for this window (`App::sync_zoom_pass`). Captures never zoom.
    pub(crate) upscale_pass: Option<crate::render::upscale_pass::UpscalePass>,
    /// Wheel zoom resolves the native world before adding screen-fixed UI.
    pub(crate) world_zoom_pass: Option<crate::render::upscale_pass::WorldZoomPass>,
    /// The sidebar's own surface, made by the first match frame and following
    /// the frame and the Sidebar size (`render_game`).
    pub(crate) sidebar_surface: Option<crate::app::presentation::sidebar_surface::SidebarSurface>,
    /// The active-game shells' own surface, made by the first one painted and
    /// following the frame (`in_game_shell::render_shell_frame`).
    pub(crate) in_game_shell_surface: Option<crate::render::scaled_copy::ScaledSurface>,
    /// Immutable one-pixel fill shared by the asset-independent status cards.
    pub(super) status_screen_fill: crate::render::batch::BatchTexture,
    /// The status cards' button glow: #CE0000 core fading to black.
    pub(crate) status_card_glow: crate::render::batch::BatchTexture,
    /// GAME.FNT bitmap font (falls back to the built-in 5x7 face).
    pub(crate) bit_font: BitFont,
    /// The status cards' own vector face, baked per size on demand. Independent
    /// of the installation, so the error card renders without one.
    pub(crate) status_font: crate::render::status_font::StatusFaceCache,
    /// InitGame52C1EF..52C302's immutable GRFXTXT.SHP/PAL result surface.
    pub(crate) outcome_banner_art:
        Option<crate::app::presentation::outcome_banner::OutcomeBannerArt>,
    /// This frame's posed bodies: crash tilts and pitched rockets
    /// (`render::unit_pose_cache`).
    pub(crate) vxl_pose_frame_cache:
        std::cell::RefCell<crate::render::unit_pose_cache::VxlPoseFrameCache>,
    /// Previous presented pre-cursor composition, retained for input-time
    /// screenshot parity.
    pub(crate) retail_screenshot_frame_cache: crate::render::screenshot::PresentedFrameCache,
}

impl RendererState {
    /// Completed match frame, including chrome and cursor, in frame pixels.
    pub(crate) fn game_frame_texture(&self) -> &wgpu::Texture {
        self.world_zoom_pass.as_ref().map_or_else(
            || self.combat_light_renderer.composition_texture(),
            |zoom| zoom.color_texture(),
        )
    }

    pub(crate) fn copy_game_frame_to(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
    ) {
        let source = self.game_frame_texture();
        debug_assert_eq!(destination.size(), source.size());
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: destination,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            source.size(),
        );
    }
}
