//! What the paint half of a game frame may reach.
//!
//! [`collect_game`](super::collect_game) reads the world on the main thread;
//! the paint half ([`paint_game`](super::paint_game) and the draw passes)
//! gets a [`PaintState`]: the renderer, the art its passes sample, and the
//! values [`PaintView`] copied while the frame was collected. It holds no
//! simulation, input or sound state, so the compiler rejects a draw pass
//! that reaches for one.
//!
//! The field paths mirror `AppState`'s on purpose
//! (`state.renderer.…`, `state.match_state.match_presentation.…`,
//! `state.match_state.input.zoom_level`), so the passes read the same
//! expressions they did against `AppState`.

use crate::app::presentation::sidebar_surface::ColumnText;
use crate::app::renderer_state::RendererState;
use crate::render::batch::BatchTexture;
use crate::render::bit_font::TextRaster;

/// The paint half's reach: [`Self::from_parts`] of the renderer, the art
/// [`paint_art!`] borrows field by field, and the frame's [`PaintView`].
pub(crate) struct PaintState<'a> {
    pub(crate) renderer: &'a mut RendererState,
    pub(crate) match_state: PaintMatchState<'a>,
}

/// `AppState::match_state`, as far as the paint half reads it.
pub(crate) struct PaintMatchState<'a> {
    pub(crate) match_presentation: PaintPresentation<'a>,
    pub(crate) input: PaintInput,
    pub(crate) sandbox_full_visibility: bool,
    view: PaintView,
}

/// The camera values the passes read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PaintInput {
    pub(crate) zoom_level: f32,
}

/// The match's art and GPU pictures the passes sample, each borrowed on its
/// own, and the radar's pictures as they stood when the frame was collected.
pub(crate) struct PaintPresentation<'a> {
    pub(crate) tile_atlas: &'a Option<crate::render::tile_atlas::TileAtlas>,
    pub(crate) building_zshape: &'a Option<crate::render::building_zshape::BuildingZShape>,
    pub(crate) unit_atlas: &'a Option<crate::render::unit_atlas::UnitAtlas>,
    pub(crate) palette_set: &'a Option<crate::render::palette_textures::PaletteSet>,
    pub(crate) sprite_atlas: &'a Option<crate::render::sprite_atlas::SpriteAtlas>,
    pub(crate) overlay_atlas: &'a Option<crate::render::overlay_atlas::OverlayAtlas>,
    pub(crate) bridge_atlas: &'a Option<crate::render::bridge_atlas::BridgeAtlas>,
    pub(crate) sidebar_cameo_atlas:
        &'a Option<crate::render::sidebar_cameo_atlas::SidebarCameoAtlas>,
    pub(crate) sidebar_chrome: &'a Option<crate::render::sidebar_chrome::SidebarChromeSet>,
    pub(crate) selection_overlay: &'a Option<crate::render::selection_overlay::SelectionOverlay>,
    #[cfg(feature = "phobos")]
    pub(crate) sw_sidebar_art: &'a Option<crate::render::sw_sidebar_art::SwSidebarArt>,
    pub(crate) beacon_art: &'a Option<crate::render::beacon_art::BeaconArt>,
    pub(crate) shroud_buffer: &'a Option<crate::render::shroud_buffer::ShroudBuffer>,
    #[cfg(feature = "phobos")]
    pub(crate) placement_preview_art:
        &'a std::cell::RefCell<crate::app::presentation::placement_preview::PlacementPreviewArt>,
    pub(crate) chrono_screen: &'a mut crate::app::chrono_screen::ChronoScreenPresentation,
    pub(crate) minimap: Option<PaintMinimap>,
    pub(crate) radar_anim: Option<PaintRadarAnim>,
}

/// The minimap's pictures for one frame (`MinimapRenderer`'s, after its
/// composition was encoded).
pub(crate) struct PaintMinimap {
    map_texture: BatchTexture,
    white_texture: BatchTexture,
    picture_generation: u64,
}

impl PaintMinimap {
    pub(crate) fn of(minimap: &crate::render::radar::minimap::MinimapRenderer) -> Self {
        Self {
            map_texture: minimap.map_texture().clone(),
            white_texture: minimap.white_texture().clone(),
            picture_generation: minimap.picture_generation(),
        }
    }

    pub(crate) fn map_texture(&self) -> &BatchTexture {
        &self.map_texture
    }

    pub(crate) fn white_texture(&self) -> &BatchTexture {
        &self.white_texture
    }

    pub(crate) fn picture_generation(&self) -> u64 {
        self.picture_generation
    }
}

/// The radar housing's picture for one frame (`RadarAnimState`'s, after its
/// paints were encoded).
pub(crate) struct PaintRadarAnim {
    texture: BatchTexture,
    picture_generation: u64,
}

impl PaintRadarAnim {
    pub(crate) fn of(radar: &crate::render::radar::anim::RadarAnimState) -> Self {
        Self {
            texture: radar.texture().clone(),
            picture_generation: radar.picture_generation(),
        }
    }

    pub(crate) fn texture(&self) -> &BatchTexture {
        &self.texture
    }

    pub(crate) fn picture_generation(&self) -> u64 {
        self.picture_generation
    }
}

/// What the passes asked `AppState` for through helper functions, answered
/// once while the frame was collected.
pub(crate) struct PaintView {
    pub(crate) input: PaintInput,
    pub(crate) sandbox_full_visibility: bool,
    /// `AppState::render_width` and `render_height`.
    render_size: [u32; 2],
    /// `input::camera::tactical_viewport_px`.
    tactical_viewport: (u32, u32, u32, u32),
    sidebar_frame: crate::ui::sidebar::SidebarFrame,
    /// `sidebar_surface::column_text`.
    column_text: ColumnText,
    /// `sidebar_render::current_sidebar_side`.
    sidebar_side: i32,
    /// `window_ui::battlefield_raster`.
    battlefield_raster: Option<TextRaster>,
    /// `outcome_banner::texture`.
    outcome_banner_texture: Option<BatchTexture>,
    /// `skirmish_shell_render::in_match_page_draw`.
    page_draw: Option<crate::app::frontend::skirmish_shell_render::InMatchPageDraw>,
    minimap: Option<PaintMinimap>,
    radar_anim: Option<PaintRadarAnim>,
}

impl PaintView {
    /// Answer the passes' questions from `state`, after everything else of
    /// the frame was collected and the radar's pictures were encoded.
    pub(crate) fn read(state: &crate::app::AppState) -> Self {
        let presentation = &state.match_state.match_presentation;
        Self {
            input: PaintInput {
                zoom_level: state.match_state.input.zoom_level,
            },
            sandbox_full_visibility: state.match_state.sandbox_full_visibility,
            render_size: [state.render_width(), state.render_height()],
            tactical_viewport: crate::app::input::camera::tactical_viewport_px(state),
            sidebar_frame: state.sidebar_frame(),
            column_text: crate::app::presentation::sidebar_surface::column_text(state),
            sidebar_side: crate::app::presentation::sidebar_render::current_sidebar_side(state),
            battlefield_raster: crate::app::presentation::window_ui::battlefield_raster(state),
            outcome_banner_texture: crate::app::presentation::outcome_banner::texture(state)
                .cloned(),
            page_draw: crate::app::frontend::skirmish_shell_render::in_match_page_draw(state),
            minimap: presentation.minimap.as_ref().map(PaintMinimap::of),
            radar_anim: presentation.radar_anim.as_ref().map(PaintRadarAnim::of),
        }
    }
}

impl<'a> PaintState<'a> {
    /// One frame's reach, from its parts.
    pub(crate) fn from_parts(
        renderer: &'a mut RendererState,
        art: PaintArt<'a>,
        view: PaintView,
    ) -> Self {
        let PaintArt {
            tile_atlas,
            building_zshape,
            unit_atlas,
            palette_set,
            sprite_atlas,
            overlay_atlas,
            bridge_atlas,
            sidebar_cameo_atlas,
            sidebar_chrome,
            selection_overlay,
            #[cfg(feature = "phobos")]
            sw_sidebar_art,
            beacon_art,
            shroud_buffer,
            #[cfg(feature = "phobos")]
            placement_preview_art,
            chrono_screen,
        } = art;
        let mut view = view;
        Self {
            renderer,
            match_state: PaintMatchState {
                match_presentation: PaintPresentation {
                    tile_atlas,
                    building_zshape,
                    unit_atlas,
                    palette_set,
                    sprite_atlas,
                    overlay_atlas,
                    bridge_atlas,
                    sidebar_cameo_atlas,
                    sidebar_chrome,
                    selection_overlay,
                    #[cfg(feature = "phobos")]
                    sw_sidebar_art,
                    beacon_art,
                    shroud_buffer,
                    #[cfg(feature = "phobos")]
                    placement_preview_art,
                    chrono_screen,
                    minimap: view.minimap.take(),
                    radar_anim: view.radar_anim.take(),
                },
                input: view.input,
                sandbox_full_visibility: view.sandbox_full_visibility,
                view,
            },
        }
    }

    pub(crate) fn render_width(&self) -> u32 {
        self.match_state.view.render_size[0]
    }

    pub(crate) fn render_height(&self) -> u32 {
        self.match_state.view.render_size[1]
    }

    pub(crate) fn sidebar_frame(&self) -> crate::ui::sidebar::SidebarFrame {
        self.match_state.view.sidebar_frame
    }

    pub(crate) fn tactical_viewport_px(&self) -> (u32, u32, u32, u32) {
        self.match_state.view.tactical_viewport
    }

    pub(crate) fn column_text(&self) -> ColumnText {
        self.match_state.view.column_text
    }

    pub(crate) fn sidebar_side(&self) -> i32 {
        self.match_state.view.sidebar_side
    }

    pub(crate) fn battlefield_raster(&self) -> Option<TextRaster> {
        self.match_state.view.battlefield_raster
    }

    pub(crate) fn outcome_banner_texture(&self) -> Option<&BatchTexture> {
        self.match_state.view.outcome_banner_texture.as_ref()
    }

    /// The in-match page's draw, once.
    pub(crate) fn take_page_draw(
        &mut self,
    ) -> Option<crate::app::frontend::skirmish_shell_render::InMatchPageDraw> {
        self.match_state.view.page_draw.take()
    }

    /// The sidebar chrome of the frame's side
    /// (`sidebar_render::current_sidebar_chrome`).
    #[cfg(feature = "phobos")]
    pub(crate) fn sidebar_chrome(&self) -> Option<&crate::render::sidebar_chrome::SidebarChromeAtlas> {
        self.match_state
            .match_presentation
            .sidebar_chrome
            .as_ref()
            .and_then(|set| set.resolve_side(self.match_state.view.sidebar_side))
            .map(|resolved| resolved.atlas)
    }
}

/// The presentation fields [`PaintState`] borrows, named by [`paint_art!`].
pub(crate) struct PaintArt<'a> {
    pub(crate) tile_atlas: &'a Option<crate::render::tile_atlas::TileAtlas>,
    pub(crate) building_zshape: &'a Option<crate::render::building_zshape::BuildingZShape>,
    pub(crate) unit_atlas: &'a Option<crate::render::unit_atlas::UnitAtlas>,
    pub(crate) palette_set: &'a Option<crate::render::palette_textures::PaletteSet>,
    pub(crate) sprite_atlas: &'a Option<crate::render::sprite_atlas::SpriteAtlas>,
    pub(crate) overlay_atlas: &'a Option<crate::render::overlay_atlas::OverlayAtlas>,
    pub(crate) bridge_atlas: &'a Option<crate::render::bridge_atlas::BridgeAtlas>,
    pub(crate) sidebar_cameo_atlas:
        &'a Option<crate::render::sidebar_cameo_atlas::SidebarCameoAtlas>,
    pub(crate) sidebar_chrome: &'a Option<crate::render::sidebar_chrome::SidebarChromeSet>,
    pub(crate) selection_overlay: &'a Option<crate::render::selection_overlay::SelectionOverlay>,
    #[cfg(feature = "phobos")]
    pub(crate) sw_sidebar_art: &'a Option<crate::render::sw_sidebar_art::SwSidebarArt>,
    pub(crate) beacon_art: &'a Option<crate::render::beacon_art::BeaconArt>,
    pub(crate) shroud_buffer: &'a Option<crate::render::shroud_buffer::ShroudBuffer>,
    #[cfg(feature = "phobos")]
    pub(crate) placement_preview_art:
        &'a std::cell::RefCell<crate::app::presentation::placement_preview::PlacementPreviewArt>,
    pub(crate) chrono_screen: &'a mut crate::app::chrono_screen::ChronoScreenPresentation,
}

/// Borrow the paint half's fields of a `MatchPresentationState` place, each
/// on its own, so its other fields stay free: `paint_art!(state.match_state
/// .match_presentation)`.
macro_rules! paint_art {
    ($presentation:expr) => {
        $crate::app::presentation::render::paint_state::PaintArt {
            tile_atlas: &$presentation.tile_atlas,
            building_zshape: &$presentation.building_zshape,
            unit_atlas: &$presentation.unit_atlas,
            palette_set: &$presentation.palette_set,
            sprite_atlas: &$presentation.sprite_atlas,
            overlay_atlas: &$presentation.overlay_atlas,
            bridge_atlas: &$presentation.bridge_atlas,
            sidebar_cameo_atlas: &$presentation.sidebar_cameo_atlas,
            sidebar_chrome: &$presentation.sidebar_chrome,
            selection_overlay: &$presentation.selection_overlay,
            #[cfg(feature = "phobos")]
            sw_sidebar_art: &$presentation.sw_sidebar_art,
            beacon_art: &$presentation.beacon_art,
            shroud_buffer: &$presentation.shroud_buffer,
            #[cfg(feature = "phobos")]
            placement_preview_art: &$presentation.placement_preview_art,
            chrono_screen: &mut $presentation.chrono_screen,
        }
    };
}
pub(crate) use paint_art;
