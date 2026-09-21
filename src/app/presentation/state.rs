//! Match presentation owner (F12 `MatchPresentationState`): the per-match GPU
//! atlas set, the software cursor, and the map-view data the render paths read
//! each frame (terrain/overlay/waypoint/tag projections, house colors, and the
//! transient lighting view).
//!
//! Map-load handoff replaces per-map resources; the presentation owner remains
//! allocated while the shell is active.
//! Process-lifetime GPU objects live in `app::renderer_state::RendererState`.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use crate::map::cell_tags::CellTagMap;
use crate::map::houses::{HouseColorMap, HouseRoster};
use crate::map::overlay::TerrainObject;
use crate::map::tags::TagMap;
use crate::map::terrain::TerrainGrid;
use crate::map::waypoints::Waypoint;
use crate::render::bridge_atlas::BridgeAtlas;
use crate::render::bridge_railing_atlas::BridgeRailingAtlas;
use crate::render::minimap::MinimapRenderer;
use crate::render::overlay_atlas::OverlayAtlas;
use crate::render::selection_overlay::SelectionOverlay;
use crate::render::sidebar_cameo_atlas::SidebarCameoAtlas;
use crate::render::sidebar_chrome::SidebarChromeSet;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::tile_atlas::TileAtlas;
use crate::render::unit_atlas::UnitAtlas;
use crate::sidebar::{SidebarChromeLayoutSpec, SidebarTab};

pub(crate) struct MatchPresentationState {
    pub(crate) tile_atlas: Option<TileAtlas>,
    /// BUILDNGZ.SHA z-shape bound at group 2 of the Z-writing building draw.
    pub(crate) building_zshape: Option<crate::render::building_zshape::BuildingZShape>,
    pub(crate) unit_atlas: Option<UnitAtlas>,
    /// Palette + per-house RGB ramp GPU resources for the voxel sprite shader.
    pub(crate) palette_set: Option<crate::render::palette_textures::PaletteSet>,
    pub(crate) sprite_atlas: Option<SpriteAtlas>,
    pub(crate) overlay_atlas: Option<OverlayAtlas>,
    pub(crate) bridge_atlas: Option<BridgeAtlas>,
    pub(crate) bridge_railing_atlas: Option<BridgeRailingAtlas>,
    pub(crate) sidebar_cameo_atlas: Option<SidebarCameoAtlas>,
    pub(crate) sidebar_chrome: Option<SidebarChromeSet>,
    pub(crate) software_cursor: Option<crate::app::presentation::render::SoftwareCursor>,
    pub(crate) terrain_grid: Option<TerrainGrid>,
    /// Last mutable MapClass playfield authority installed into presentation.
    /// `None` is an explicit stale gate (new map / quickload); the inner
    /// optional bounds preserves fail-closed absence without inventing a rect.
    pub(crate) installed_playfield_authority:
        Option<(Option<crate::map::playfield::PlayfieldBounds>, u64)>,
    /// Overlay entries from map for per-frame instance generation.
    pub(crate) overlays: crate::app::presentation::overlay_index::OverlayRenderIndex,
    /// Terrain objects from map for per-frame instance generation.
    pub(crate) terrain_objects: Vec<TerrainObject>,
    pub(crate) waypoints: HashMap<u32, Waypoint>,
    pub(crate) cell_tags: CellTagMap,
    pub(crate) tags: TagMap,
    /// Overlay ID → type name mapping for atlas lookups at render time.
    pub(crate) overlay_names: BTreeMap<u8, String>,
    /// Exact SHP frame-header radar RGB for each native-selected overlay frame.
    pub(crate) overlay_radar_colors: HashMap<(u8, u8), [u8; 3]>,
    /// Owner name → house color index mapping for atlas key lookups.
    pub(crate) house_color_map: HouseColorMap,
    pub(crate) house_roster: HouseRoster,
    pub(crate) lighting: super::lighting::MatchLighting,
    pub(crate) combat_lights: crate::app::presentation::combat_lights::CombatLightRuntime,
    pub(crate) minimap: Option<MinimapRenderer>,
    /// Animated radar chrome — plays 33-frame open/close animation when radar gained/lost.
    pub(crate) radar_anim: Option<crate::render::radar_anim::RadarAnimState>,
    /// Requested-versus-resolved atlas identity used to construct `radar_anim`.
    ///
    /// Kept beside the animation so tactical evidence never reconstructs
    /// provenance from the currently selected sidebar theme.
    pub(crate) radar_animation_source:
        Option<crate::render::sidebar_chrome::ResolvedSidebarChromeIdentity>,
    /// Content insets [left, top, right, bottom] derived from the transparent opening
    /// in radar.shp frame 0. Used to position the minimap inside the chrome housing.
    /// Unscaled pixels — multiply by `ui_scale` at use site.
    pub(crate) radar_content_insets: Option<[u32; 4]>,
    /// Whether the local player currently has operational radar (power-gated).
    pub(crate) has_radar: bool,
    /// Selection overlay renderer — highlights and drag rectangle.
    pub(crate) selection_overlay: Option<SelectionOverlay>,
    /// Authentic SHROUD.SHP sprite-based shroud edge renderer.
    /// GPU ABuffer — screen-resolution brightness texture for per-pixel shroud darkening.
    /// SHROUD.SHP brightness pixels blitted per-cell, then a full-screen multiply pass
    /// darkens the scene.
    pub(crate) shroud_buffer: Option<crate::render::shroud_buffer::ShroudBuffer>,
    /// Cell (rx, ry) -> high-bridge facts used by the tactical cursor inverse.
    pub(crate) tactical_bridge_inverse_map:
        BTreeMap<(u16, u16), crate::map::terrain::TacticalBridgeCell>,
    /// Active map theater name (e.g., DESERT).
    pub(crate) theater_name: String,
    /// Active map theater extension (e.g., des).
    pub(crate) theater_ext: String,
    /// Target/action lines — colored lines from selected units to command destinations.
    pub(crate) target_lines: crate::app::presentation::target_lines::TargetLineState,
    /// Active parachute animations, one per descending paradropped infantry.
    /// Polling-based lifecycle: spawned when an entity gains parachute_state
    /// in the sim, removed on landing or death. Render-only; not snapshotted.
    pub(crate) parachute_anims: Vec<crate::sim::components::ParachuteAnim>,
    /// Global elapsed time for looping terrain overlay animations.
    pub(crate) idle_anim_elapsed_ms: u32,
    // -- Reusable per-frame scratch buffers (avoid allocation each frame) --
    /// Overlay instance scratch vec — cleared and refilled each frame.
    pub(crate) cached_overlay_instances: Vec<crate::render::batch::SpriteInstance>,
    /// Unit (voxel) instance scratch vec — cleared and refilled each frame.
    pub(crate) cached_unit_instances: Vec<crate::render::batch::SpriteInstance>,
    /// UnitAtlas texture-page tags aligned with `cached_unit_instances`.
    pub(crate) cached_unit_pages: Vec<usize>,
    /// Animated power bar — segment-by-segment transition matching original PowerClass.
    pub(crate) power_bar_anim: crate::sidebar::PowerBarAnimState,
    /// Persistent flash + mode state for in-game sidebar gadgets. Ticked from
    /// `sidebar_gadgets::update_sidebar_gadget_state` once per sim tick;
    /// read each frame by the sidebar view builder to pick SHP frame indices.
    pub(crate) sidebar_gadget_state: crate::sidebar::gadget_flash::SidebarGadgetState,
    /// In-game gadget substrate (study §6.1): retained sidebar button list +
    /// capture/focus state + reusable tick output + the mouse-held record.
    pub(crate) in_game_gadgets: crate::app::input::gadget_input::InGameGadgets,
    /// Retained immutable sidebar view plus its per-owner animated credit state.
    /// Consumers read the snapshot; explicit transitions rebuild it.
    pub(crate) sidebar_projection: crate::app::sidebar_projection::SidebarProjectionState,
    /// Active tab for the custom in-game sidebar.
    pub(crate) active_sidebar_tab: SidebarTab,
    /// Native side-specific geometry in physical render pixels.
    pub(crate) sidebar_layout_spec: SidebarChromeLayoutSpec,
    /// Ordinary retail artwork uses one render pixel per source pixel.
    pub(crate) ui_scale: f32,
    /// Scroll offset for the current sidebar tab's item list.
    ///
    /// gamemd's sidebar keeps this row per build strip, not one shared value —
    /// its scroll command indexes the strip by column. This holds the live row
    /// for the active tab; the parked rows for the other tabs live in
    /// `sidebar_scroll_rows_parked` and swap in and out on a tab change, which
    /// keeps every consumer reading one field while the position stops bleeding
    /// across tabs.
    pub(crate) sidebar_scroll_rows: usize,
    /// Parked scroll row per sidebar tab, indexed by `input::dispatch::tab_scroll_slot`.
    /// One entry per `SidebarTab` variant.
    pub(crate) sidebar_scroll_rows_parked: [usize; 4],
    /// Shared tooltip service (study S1) — the model is clock-injected; only
    /// `input::tooltips` reads the wall clock.
    pub(crate) tooltips: crate::ui::tooltips::TooltipService,
    /// Epoch for the tooltip/message wall-clock (`now_ms` = elapsed since
    /// app construction).
    pub(crate) tooltip_epoch: Instant,
    /// In-game chat/system message surface (study §3.1) — re-anchored to the
    /// tactical viewport per frame by `input::messages`.
    pub(crate) message_list: crate::ui::messages::MessageList,
    /// Pause-adjusted clock for message deadlines (contract §4.2 step 8 /
    /// §4.3: the native composite timer freezes during pause). Fed pause
    /// edges by `messages::update`.
    pub(crate) message_clock: crate::ui::messages::PauseAwareClock,
    /// In-scenario modal state — the port of gamemd's in-scenario state
    /// variable. Owns the in-game menu, the abort-mission confirmation and the
    /// parent/child relationship with the `0xBBB` Options dialog.
    pub(crate) in_game_menu: crate::ui::pause_menu::InGameMenuState,
    pub(crate) pause_menu_has_saves: bool,
    pub(crate) pause_menu_interaction: crate::ui::shell::pause_menu::PauseMenuInteraction,
    pub(crate) sound_dialog: Option<crate::ui::shell::sound::SoundState>,
    pub(crate) abort_buttons:
        crate::ui::shell::button::ShellButtonInteraction<crate::ui::shell::abort::AbortButton>,
    pub(crate) saved_game_browser:
        Option<crate::ui::skirmish_shell::SavedSeedBrowserState<std::path::PathBuf>>,
    /// Client-side in-game Options (0xBBB) state: the six [Options] values plus
    /// transient interaction flags. `game_speed` mirrors the launched sim and
    /// queues an authoritative transition on close; `sim_speed_tps` is its local
    /// presentation readout. App/ui-level.
    pub(crate) in_game_options: crate::ui::shell::in_game_options_state::InGameOptionsState,
    /// Laid-out 0xBBB anchor cached by the overlay render pass each frame it draws,
    /// so the paused mouse handler hit-tests the exact rects that were rendered
    /// (the sidebar-anchored button Y is render-derived; see KD-6). None until the
    /// overlay first renders.
    pub(crate) in_game_options_anchor: Option<crate::ui::shell::layout::InGameOptionsAnchor>,
    /// Show hotkey reference overlay. Toggle with F1.
    pub(crate) show_hotkey_help: bool,
    /// Save/load panel visible. Toggle with F5.
    pub(crate) show_save_load_panel: bool,
}
