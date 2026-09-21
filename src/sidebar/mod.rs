//! Custom-rendered in-game sidebar layout/model.
//!
//! This module stays render-agnostic: it describes sidebar geometry, tab/item
//! state, and hit-testing, while the app/render layers decide how to draw it.
//!
//! Native screen geometry comes from `6A5090`/`6A5130`, with shape sizes
//! from the loaded assets and fixed 60×48 cameo hit zones (`6A8220`).

pub mod gadget_flash;
pub mod command_bar;
mod layout_spec;
pub mod power_bar_anim;
mod sidebar_view;

use crate::sim::production::ProductionCategory;

pub use layout_spec::{SidebarChromeLayoutSpec, SidebarTheme};
pub use power_bar_anim::PowerBarAnimState;
#[cfg(test)]
pub(crate) use sidebar_view::build_sidebar_view;
pub(crate) use sidebar_view::build_sidebar_view_with_spec;
pub(crate) use sidebar_view::ArmedSidebarEntry;

/// Original RA2 sidebar chrome width (all SHPs are 168px wide).
pub const SIDEBAR_WIDTH: f32 = 168.0;
/// Cameo hit zones are fixed 60×48 (`6A8220`), independent of artwork size.
pub(crate) const CAMEO_COLUMNS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[cfg(test)]
pub fn radar_minimap_rect(screen_w: f32) -> Rect {
    radar_minimap_rect_with_spec(screen_w, SidebarChromeLayoutSpec::stock())
}

pub fn radar_minimap_rect_with_spec(screen_w: f32, spec: SidebarChromeLayoutSpec) -> Rect {
    // Radar One_Time 652CF0 and Init_For_House 652E90: fixed aperture.
    Rect { x: screen_w - spec.sidebar_width + 16.0, y: 49.0, w: 140.0, h: 108.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Building,
    Defense,
    Infantry,
    Vehicle,
}

impl SidebarTab {
    pub fn all() -> [Self; 4] {
        [Self::Building, Self::Defense, Self::Infantry, Self::Vehicle]
    }

    pub fn category(self) -> ProductionCategory {
        match self {
            Self::Building => ProductionCategory::Building,
            Self::Defense => ProductionCategory::Defense,
            Self::Infantry => ProductionCategory::Infantry,
            Self::Vehicle => ProductionCategory::Vehicle,
        }
    }

    pub fn default_active_tab() -> Self {
        default_active_tab()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Building => "Build",
            Self::Defense => "Def",
            Self::Infantry => "Inf",
            Self::Vehicle => "Veh",
        }
    }

    /// Index into tab00..tab03 SHP array.
    pub fn tab_index(self) -> usize {
        match self {
            Self::Building => 0,
            Self::Defense => 1,
            Self::Infantry => 2,
            Self::Vehicle => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarAction {
    None,
    OpenPauseMenu,
    OpenDiplomacy,
    SelectTab(SidebarTab),
    BuildType(String),
    ArmPlacement(String),
    ClearPlacementMode,
    /// Arm the targeting cursor for a charged superweapon.
    /// Payload: SW INI section name (e.g., "LightningStormSpecial").
    ArmSuperWeapon(String),
    /// Clear the SW targeting cursor (toggle off / second click on cameo).
    ClearSuperWeaponMode,
    TogglePauseQueue(ProductionCategory),
    CycleProducer(ProductionCategory),
    CancelBuild(String),
    CancelLastBuild,
    CycleOwner,
    PlaceStarterBase,
    SpawnTestUnits,
    /// Toggle Repair-mode (cursor stays armed for clicking buildings to repair).
    /// Mutually exclusive with `ToggleSellMode` and any active `TargetingMode`.
    ToggleRepairMode,
    /// Toggle Sell-mode (cursor stays armed for clicking buildings to sell).
    /// Mutually exclusive with `ToggleRepairMode` and any active `TargetingMode`.
    ToggleSellMode,
    Deploy,
}

#[derive(Debug, Clone)]
pub struct SidebarItem {
    pub rect: Rect,
    pub type_id: String,
    pub display_name: String,
    pub cost: Option<i32>,
    pub has_cameo_art: bool,
    pub queue_category: ProductionCategory,
    pub enabled: bool,
    pub progress: f32,
    pub queued_count: usize,
    /// True when this type is the one actively being produced in its category.
    pub is_building_this_type: bool,
    pub is_ready: bool,
    /// True while this type's production is suspended — the player paused the
    /// queue or ran short of cash mid-build. gamemd draws the `TXT_HOLD`
    /// status text with the same dark strip as `TXT_READY` in that state, and
    /// it is the game's only visual cue that a build has stalled.
    pub is_on_hold: bool,
    pub is_armed: bool,
    /// True if this cameo represents a superweapon (not a buildable).
    pub is_superweapon: bool,
    /// Unique SW INI section name (e.g., "LightningStormSpecial"). Set
    /// only when `is_superweapon=true`. Multiple SWs may share `type_id`
    /// (SidebarImage), but section names are unique.
    pub super_weapon_section: Option<String>,
}

impl SidebarItem {
    /// The cameo art slot inside this item (the full item rect IS the cameo).
    pub fn cameo_rect(&self) -> Rect {
        self.rect
    }
}

#[derive(Debug, Clone)]
pub struct SidebarTabButton {
    pub tab: SidebarTab,
    pub rect: Rect,
    /// True when this is the currently-selected tab. Used by hit-test
    /// disambiguation; the rendered visual is driven by `frame_index`.
    pub active: bool,
    /// Mirrors the retained gadget disabled bit used to select the visual and
    /// to synchronize hit-testing/tooltips from this immutable projection.
    pub disabled: bool,
    /// SHP frame index (0..=4) for the per-theme tab SHP atlas. Picked by
    /// `SidebarGadgetState::tab_frame` each frame.
    pub frame_index: u8,
}

/// View entry for an SHP-driven toggle button (Repair, Sell).
/// Rect for hit-testing, action to dispatch on click, frame index for the
/// 5-frame SHP state table.
#[derive(Debug, Clone)]
pub struct SidebarToggleButton {
    pub rect: Rect,
    pub action: SidebarAction,
    /// Externally driven latch state for the toggle gadget.
    pub active: bool,
    /// Retained gadget disabled bit for hit-testing and tooltip presentation.
    pub disabled: bool,
    /// SHP frame index (0..=4) for the button's per-theme SHP atlas.
    pub frame_index: u8,
}

/// View entry for one of the two SHP-driven strip scroll buttons.
#[derive(Debug, Clone)]
pub struct SidebarScrollButton {
    pub rect: Rect,
    /// Retained gadget disabled bit. Scroll capacity comes from `6A6610`.
    pub disabled: bool,
    /// SHP frame index (0..=2) for the button's per-theme atlas.
    pub frame_index: u8,
}

#[derive(Debug, Clone)]
pub struct SidebarControlButton {
    pub rect: Rect,
    pub action: SidebarAction,
    pub label: String,
}

/// Computed vertical offsets for each chrome section, based on screen height.
#[derive(Debug, Clone, Copy)]
pub struct SidebarLayout {
    pub sidebar_x: f32,
    pub radar_y: f32,
    pub side1_y: f32,
    pub tabs_y: f32,
    pub cameo_grid_top: f32,
    pub cameo_grid_bottom: f32,
    pub side3_y: f32,
    /// How many side2 tiles fit in the cameo region.
    pub side2_tile_count: usize,
}

#[derive(Debug, Clone)]
pub struct SidebarView {
    pub panel_rect: Rect,
    pub layout: SidebarLayout,
    pub credits: i32,
    pub power_produced: i32,
    pub power_drained: i32,
    pub credits_frac: f32,
    pub power_frac: f32,
    pub low_power: bool,
    pub scroll_rows: usize,
    pub max_scroll_rows: usize,
    pub tabs: Vec<SidebarTabButton>,
    pub items: Vec<SidebarItem>,
    /// Repair button (toggle mode). Rendered from the per-theme atlas's
    /// `repair_frames[frame_index]`. Hit-test routes to
    /// `SidebarAction::ToggleRepairMode`.
    pub repair_button: SidebarToggleButton,
    /// Sell button (toggle mode). Rendered from the per-theme atlas's
    /// `sell_frames[frame_index]`. Hit-test routes to
    /// `SidebarAction::ToggleSellMode`.
    pub sell_button: SidebarToggleButton,
    /// DIPLOBTN (left), OPTBTN (right), native IDs F2/F3.
    pub top_buttons: [SidebarScrollButton; 2],
    pub scroll_down_button: SidebarScrollButton,
    pub scroll_up_button: SidebarScrollButton,
    pub cancel_button: SidebarControlButton,
    pub cycle_owner_button: SidebarControlButton,
    pub starter_base_button: SidebarControlButton,
    pub spawn_test_units_button: SidebarControlButton,
    pub pause_button: Option<SidebarControlButton>,
    pub producer_button: Option<SidebarControlButton>,
}

pub fn default_active_tab() -> SidebarTab {
    SidebarTab::Building
}

/// Compute the vertical layout of chrome sections for a given screen height.
/// The sidebar shell fills the available height; item count only affects scrolling.
pub fn compute_layout(screen_w: f32, screen_h: f32, item_rows: usize) -> SidebarLayout {
    compute_layout_with_spec(
        SidebarChromeLayoutSpec::stock(),
        screen_w,
        screen_h,
        item_rows,
    )
}

pub(crate) fn compute_layout_with_spec(
    spec: SidebarChromeLayoutSpec,
    screen_w: f32,
    screen_h: f32,
    _item_rows: usize,
) -> SidebarLayout {
    // Original 6A5090/6A5130: capacity depends on screen height and side,
    // never on build-item count. Integer division truncates toward zero.
    let rows = ((screen_h as i32 - 227 - spec.footer_allowance - 7) / 50).max(0) as usize;
    SidebarLayout {
        sidebar_x: screen_w - spec.sidebar_width,
        radar_y: spec.top_inset,
        side1_y: 158.0,
        tabs_y: 197.0,
        cameo_grid_top: 227.0,
        cameo_grid_bottom: 227.0 + rows as f32 * 50.0,
        // Native 6A6C30 advances by the loaded SIDE2 canvas height.
        side3_y: 227.0 + rows as f32 * spec.side2_height,
        side2_tile_count: rows,
    }
}

/// Native PowerClass tooltip (`6403A0`). The 8-pixel hit width is distinct
/// from POWERP's actual canvas width and the 3-pixel paint stride.
pub fn power_bar_rect(layout: &SidebarLayout, spec: SidebarChromeLayoutSpec) -> Rect {
    Rect { x: layout.sidebar_x + spec.power_bar_x, y: 227.0, w: 8.0,
        h: layout.cameo_grid_bottom - layout.cameo_grid_top }
}

/// Native R-DN/R-UP anchors (`6ABD30`), with actual SHP canvas hit sizes.
pub fn scroll_button_rects(
    layout: &SidebarLayout,
    spec: SidebarChromeLayoutSpec,
    down_size: Option<[f32; 2]>,
    up_size: Option<[f32; 2]>,
) -> (Rect, Rect) {
    let [dw, dh] = down_size.unwrap_or([0.0, 0.0]);
    let [uw, uh] = up_size.unwrap_or([0.0, 0.0]);
    let x = layout.sidebar_x + spec.scroll_x;
    let y = layout.cameo_grid_bottom + 7.0;
    (Rect { x, y, w: dw, h: dh },
     Rect { x: x + spec.scroll_pitch, y, w: uw, h: uh })
}

pub(crate) fn hit_test_item(item: &SidebarItem, right_click: bool) -> SidebarAction {
    if right_click {
        // SW cameos have no queue → right-click does nothing.
        if item.is_superweapon {
            return SidebarAction::None;
        }
        // Build cameo right-click: cancel one queued (or ready) item.
        return if item.queued_count > 0 || item.is_ready {
            SidebarAction::CancelBuild(item.type_id.clone())
        } else {
            SidebarAction::None
        };
    }
    // Left-click branch.
    if item.is_superweapon {
        if !item.is_ready {
            return SidebarAction::None;
        }
        return if item.is_armed {
            SidebarAction::ClearSuperWeaponMode
        } else {
            // Section name is unique; fall back to display_name (which
            // matches today for SW views) if for some reason it's not set.
            let section = item
                .super_weapon_section
                .clone()
                .unwrap_or_else(|| item.display_name.clone());
            SidebarAction::ArmSuperWeapon(section)
        };
    }
    // Build cameo branch (unchanged behavior).
    if item.is_ready {
        if item.is_armed {
            SidebarAction::ClearPlacementMode
        } else {
            SidebarAction::ArmPlacement(item.type_id.clone())
        }
    } else if item.enabled {
        SidebarAction::BuildType(item.type_id.clone())
    } else {
        SidebarAction::None
    }
}

// `sidebar::hit_test` (the legacy press-path hit-test) was retired in A6: every
// in-game surface — tabs/repair/sell/scroll (A1), cameos (A2), tactical/minimap
// (A3), and the control/dev buttons (A6) — is now owned by the `app::input::gadget_input`
// retained list (one order = hit + draw, R7 complete). `hit_test_item` (the cameo
// click→action map) stays public for the driver.

#[cfg(test)]
mod tests {
    use super::{Rect, SidebarAction, SidebarItem};
    use crate::sim::production::ProductionCategory;

    fn make_sw_item(is_ready: bool, is_armed: bool) -> SidebarItem {
        SidebarItem {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 48.0,
            },
            type_id: "INTICON".to_string(),
            display_name: "LightningStormSpecial".to_string(),
            cost: None,
            has_cameo_art: true,
            queue_category: ProductionCategory::Defense,
            enabled: true,
            progress: if is_ready { 1.0 } else { 0.5 },
            queued_count: 0,
            is_building_this_type: !is_ready,
            is_ready,
            is_on_hold: false,
            is_armed,
            is_superweapon: true,
            super_weapon_section: Some("LightningStormSpecial".to_string()),
        }
    }

    #[test]
    fn sw_ready_left_click_arms() {
        let item = make_sw_item(true, false);
        let action = super::hit_test_item(&item, false);
        assert_eq!(
            action,
            SidebarAction::ArmSuperWeapon("LightningStormSpecial".to_string())
        );
    }

    #[test]
    fn sw_ready_armed_left_click_clears() {
        let item = make_sw_item(true, true);
        let action = super::hit_test_item(&item, false);
        assert_eq!(action, SidebarAction::ClearSuperWeaponMode);
    }

    #[test]
    fn sw_charging_left_click_does_nothing() {
        let item = make_sw_item(false, false);
        let action = super::hit_test_item(&item, false);
        assert_eq!(action, SidebarAction::None);
    }

    #[test]
    fn sw_right_click_does_nothing() {
        for ready in [false, true] {
            for armed in [false, true] {
                let item = make_sw_item(ready, armed);
                let action = super::hit_test_item(&item, true);
                assert_eq!(
                    action,
                    SidebarAction::None,
                    "ready={} armed={}",
                    ready,
                    armed
                );
            }
        }
    }
}

#[cfg(test)]
mod native_geometry_tests;
