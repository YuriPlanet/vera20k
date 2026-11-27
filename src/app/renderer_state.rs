//! Process-wide renderer owner (F12 `RendererState`): the GPU context,
//! batch renderer, per-frame GPU pools/targets, presentation passes, and the
//! process-lifetime rendering caches.
//!
//! Match-scoped visuals (atlases, shroud buffer, minimap) stay outside this
//! owner — they are rebuilt per map and belong to match presentation.

use crate::render::batch::BatchRenderer;
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;
use crate::ui::game_screen::GameScreen;

pub(crate) struct RendererState {
    pub(crate) gpu: GpuContext,
    pub(crate) batch_renderer: BatchRenderer,
    pub(crate) performance_overlay:
        crate::app::diagnostics::performance_overlay::PerformanceOverlay,
    pub(crate) terrain_draw_renderer: crate::render::terrain_draw::TerrainDrawRenderer,
    /// Logs each frame's render pass and submission counts while they are
    /// new or growing, for a player's log of a GPU that cannot keep up.
    pub(crate) pass_log: crate::app::presentation::render::FramePassLog,
    pub(crate) combat_light_renderer: crate::render::combat_light::CombatLightRenderer,
    /// Process GPU resources for the native final client-pixel shake copy.
    pub(crate) screen_presentation: crate::render::screen_presentation::ScreenPresentationPass,
    /// Reusable GPU instance buffers — avoids per-frame GPU buffer allocation.
    pub(crate) instance_pool: crate::render::batch::InstanceBufferPool,
    /// GPU depth texture for back-to-front depth ordering. Recreated on window resize.
    pub(crate) depth_view: wgpu::TextureView,
    /// Encoded-byte RGB565 presentation boundary for stock shell/loading surfaces.
    pub(crate) shell_surface_presenter: crate::render::shell_surface_present::ShellSurfacePresenter,
    /// The match zoom pass, present only while the match frame is not the
    /// whole window at its own pixels: the Options zoom above 100% or the 4:3
    /// shape's bars (`App::sync_zoom_pass`). Captures zoom only when their
    /// profile asks (`CapturePresentation`).
    pub(crate) upscale_pass: Option<crate::render::upscale_pass::UpscalePass>,
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
    /// Every interface text's font: GAME.FNT, with the letters it lacks drawn
    /// in its style and the bundled fonts behind them, drawn in whole pixels.
    pub(crate) bit_font: BitFont,
    /// A zoomed match's text and cursor at the window's own pixels.
    pub(crate) window_ui: crate::app::presentation::window_ui::WindowUi,
    /// InitGame52C1EF..52C302's immutable GRFXTXT.SHP/PAL result surface.
    pub(crate) outcome_banner_art:
        Option<crate::app::presentation::outcome_banner::OutcomeBannerArt>,
    /// This frame's posed bodies: crash tilts and pitched rockets
    /// (`render::unit_pose_cache`). A `RefCell`: the unit and building
    /// visitors, alive at once over the shared renderer, pose bodies through it.
    pub(crate) vxl_pose_frame_cache:
        std::cell::RefCell<crate::render::unit_pose_cache::VxlPoseFrameCache>,
    /// VERA-only, experimental: this frame's declared models
    /// (`render::model_draw`), untouched while no type declares one.
    pub(crate) model_frame: std::cell::RefCell<crate::render::model_draw::ModelFrame>,
    /// Previous presented pre-cursor composition, retained for input-time
    /// screenshot parity.
    pub(crate) retail_screenshot_frame_cache: crate::render::screenshot::PresentedFrameCache,
    /// The finished terrain layer, kept while its inputs hold still.
    pub(crate) terrain_picture: crate::app::presentation::render::terrain_picture::TerrainPicture,
    /// gamemd's Z-buffer row seed, which lives as long as its Z buffer: one
    /// per process, until the view's size changes.
    pub(crate) native_z_seed: crate::render::native_z::ZSeed,
    /// Whether the camera has zoomed in during this run: voxels then keep
    /// making their 2x pictures (`vxl_raster::set_hd_voxels`), so zooming in
    /// again shows them at once.
    pub(crate) hd_voxels_seen: bool,
    /// Set when a match is still to show its first frame: with HD art on, that
    /// frame eases the wheel zoom to `render::hd_switch::DEFAULT_ZOOM`. A loaded
    /// game keeps its saved zoom.
    pub(crate) hd_art_zoom_pending: bool,
}

impl RendererState {
    /// Current projection on `screen`: frontend/result use physical window
    /// pixels; tactical rendering and pending tactical installation use the
    /// optional zoom pass. Loading artwork explicitly uses GPU/window
    /// dimensions in loading::pump.
    pub(crate) fn render_size(&self, screen: &GameScreen) -> (u32, u32) {
        render_dimensions(
            screen,
            (self.gpu.config.width, self.gpu.config.height),
            self.upscale_pass
                .as_ref()
                .map(|up| (up.src_width(), up.src_height())),
        )
    }

    pub(crate) fn render_width(&self, screen: &GameScreen) -> u32 {
        self.render_size(screen).0
    }

    pub(crate) fn render_height(&self, screen: &GameScreen) -> u32 {
        self.render_size(screen).1
    }

    /// The zoom pass a match frame goes through: the battlefield and the
    /// active-game shells draw into it at the source size. Loading and the
    /// menus never zoom.
    pub(crate) fn match_zoom_pass(
        &self,
        screen: &GameScreen,
    ) -> Option<&crate::render::upscale_pass::UpscalePass> {
        (*screen == GameScreen::InGame)
            .then_some(self.upscale_pass.as_ref())
            .flatten()
    }

    /// The zoom between the window and the match frame, whose cursor, layout
    /// and drawing all use source pixels.
    pub(crate) fn tactical_zoom(
        &self,
        screen: &GameScreen,
    ) -> Option<crate::ui::main_menu_dialogs::options::ZoomGeometry> {
        self.match_zoom_pass(screen).map(|pass| pass.geometry())
    }

    /// Window pixels per frame pixel of a match frame: the Options zoom, 1
    /// outside a match or without one.
    pub(crate) fn frame_scale(&self, screen: &GameScreen) -> f32 {
        self.tactical_zoom(screen).map_or(1.0, |zoom| zoom.scale())
    }

    /// The match composition's size: the frame's rect in the window, which
    /// the battlefield and the frame's interface are drawn at
    /// (`docs/zoom-one-pass.md`); the frame itself without a zoom.
    pub(crate) fn composition_size(&self, screen: &GameScreen) -> (u32, u32) {
        self.tactical_zoom(screen)
            .map_or_else(|| self.render_size(screen), |zoom| zoom.frame_size())
    }

    /// A rect of the match frame, `(x, y, width, height)` in frame pixels, in
    /// the composition's pixels: each edge scaled and rounded, so rects that
    /// meet in the frame (the battlefield and the sidebar column) meet in the
    /// composition, and cut to it.
    pub(crate) fn frame_to_composition(
        &self,
        screen: &GameScreen,
        rect: (u32, u32, u32, u32),
    ) -> [u32; 4] {
        scale_frame_rect(
            self.frame_scale(screen),
            self.composition_size(screen),
            rect,
        )
    }

    /// Completed match frame with its chrome, at the composition's pixels
    /// (`composition_size`). The software cursor goes over this frame's
    /// copies, so it stays cursor-free.
    pub(crate) fn game_frame_texture(&self) -> &wgpu::Texture {
        self.combat_light_renderer.composition_texture()
    }

    /// Copy the completed match frame into `destination`, a window-sized
    /// target with no zoom pass: then the frame is the window.
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

/// [`RendererState::frame_to_composition`] at `scale` composition pixels per
/// frame pixel, cut to `target`.
pub(crate) fn scale_frame_rect(
    scale: f32,
    (target_width, target_height): (u32, u32),
    (x, y, width, height): (u32, u32, u32, u32),
) -> [u32; 4] {
    let edge = |v: u32, limit: u32| ((v as f32 * scale).round() as u32).min(limit);
    let left = edge(x, target_width);
    let top = edge(y, target_height);
    let right = edge(x.saturating_add(width), target_width);
    let bottom = edge(y.saturating_add(height), target_height);
    [left, top, right - left, bottom - top]
}

/// Loading uses tactical dimensions only for pending match-resource setup;
/// its own artwork uses the window directly. Frontend layout and hit testing
/// must never inherit the match zoom's source size.
fn render_dimensions(
    screen: &GameScreen,
    window: (u32, u32),
    tactical_source: Option<(u32, u32)>,
) -> (u32, u32) {
    match screen {
        GameScreen::MainMenu | GameScreen::MissionResult { .. } => window,
        GameScreen::Loading | GameScreen::InGame => tactical_source.unwrap_or(window),
    }
}
