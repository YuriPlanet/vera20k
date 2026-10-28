//! Custom-rendered in-game sidebar layout/model.
//!
//! This module stays render-agnostic: it describes sidebar geometry, tab/item
//! state, and hit-testing, while the app/render layers decide how to draw it.
//!
//! Native screen geometry comes from `6A5090`/`6A5130`, with shape sizes
//! from the loaded assets and fixed 60×48 cameo hit zones (`6A8220`).

pub(crate) mod cameo_order;
#[cfg(test)]
mod cameo_order_tests;
pub mod command_bar;
mod frame;
pub mod gadget_flash;
mod layout_spec;
pub mod power_bar_anim;
mod sidebar_view;

use crate::sim::production::ProductionCategory;

pub use frame::{BOTTOM_STRIP_HEIGHT_PX, SIDEBAR_WIDTH_PX, SidebarFrame};
pub use layout_spec::{SidebarChromeLayoutSpec, SidebarTheme};
pub use power_bar_anim::PowerBarAnimState;
pub(crate) use sidebar_view::ArmedSidebarEntry;
pub(crate) use sidebar_view::build_sidebar_view_with_spec;
pub(crate) use sidebar_view::{CameoFactoryStatus, ObjectCameoDrawInputs, draw_object_cameo};

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
    Rect {
        x: screen_w - spec.sidebar_width + 16.0,
        y: 49.0,
        w: 140.0,
        h: 108.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Building,
    Defense,
    Infantry,
    Vehicle,
}

impl SidebarTab {
    pub(crate) fn for_category(category: ProductionCategory) -> Self {
        match category {
            ProductionCategory::Building => Self::Building,
            ProductionCategory::Defense => Self::Defense,
            ProductionCategory::Infantry => Self::Infantry,
            ProductionCategory::Vehicle
            | ProductionCategory::Aircraft
            | ProductionCategory::Ship => Self::Vehicle,
        }
    }

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
    /// A press on a build cameo; `app::input::sidebar_eva::cameo_click` decides
    /// what it does. `at_build_limit` is the cameo's build-limit state
    /// (the only reason a shown build cameo is not buildable).
    CameoPress {
        type_id: String,
        right: bool,
        shift: bool,
        at_build_limit: bool,
    },
    /// Forward the press to the live SuperClass::CanFire/click owner.
    SuperWeaponPress {
        section: String,
        right: bool,
    },
    /// Toggle Repair-mode (cursor stays armed for clicking buildings to repair).
    /// Clears Sell; pending building placement refuses the transition.
    ToggleRepairMode,
    /// Toggle Sell-mode (cursor stays armed for clicking buildings to sell).
    /// Clears Repair; pending building placement refuses the transition.
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
    /// Draw the type's `AltCameo=` image (`TechnoTypeClass::GetCameo`).
    pub alt_cameo: bool,
    /// Native retained-entry flash phase (StripDraw6A9B59).
    pub flash_darkened: bool,
    pub queue_category: ProductionCategory,
    /// Draw's unavailable overlay; it is not an input admission gate. For an
    /// object cameo, [`draw_object_cameo`] writes it and the drawn state below
    /// from the projection's live inputs on every rebuild and frame, as
    /// `StripClass::Draw` does.
    pub enabled: bool,
    pub at_build_limit: bool,
    pub progress: f32,
    pub queued_count: usize,
    /// True when this cameo's linked Factory is actively producing.
    pub is_building_this_type: bool,
    /// True when the linked Factory's held product IS this entry's type,
    /// finished or not. `StripClass::Draw` reads `Factory+0x58` and compares its
    /// type index with the requested one at `0x006A9CBF..0x006A9CC3` to decide
    /// whether a count of one is the held product rather than a queued copy.
    pub is_held_this_type: bool,
    pub is_ready: bool,
    /// True when the linked Factory has no rate or is suspended. A cash stall
    /// retains its running rate and draws a frozen clock without `TXT_HOLD`.
    pub is_on_hold: bool,
    pub is_armed: bool,
    /// True if this cameo represents a superweapon (not a buildable).
    pub is_superweapon: bool,
    /// Unique SW INI section name (e.g., "LightningStormSpecial"). Set
    /// only when `is_superweapon=true`. Multiple SWs may share `type_id`
    /// (SidebarImage), but section names are unique.
    pub super_weapon_section: Option<String>,
    /// Derived native Super display; factories keep their own status owner.
    pub super_weapon_display: Option<crate::sim::superweapon::SuperWeaponDisplay>,
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
/// 5-frame SHP state table. Native never disables either button
/// (see [`gadget_flash::SidebarGadgetState::repair_frame`]).
#[derive(Debug, Clone)]
pub struct SidebarToggleButton {
    pub rect: Rect,
    pub action: SidebarAction,
    /// Externally driven latch state for the toggle gadget.
    pub active: bool,
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
    Rect {
        x: layout.sidebar_x + spec.power_bar_x,
        y: 227.0,
        w: 8.0,
        h: layout.cameo_grid_bottom - layout.cameo_grid_top,
    }
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
    (
        Rect { x, y, w: dw, h: dh },
        Rect {
            x: x + spec.scroll_pitch,
            y,
            w: uw,
            h: uh,
        },
    )
}

/// Forward cameo presses to the live Super or Factory click decision.
/// Draw flags and the currently armed mode do not decide admission here.
pub(crate) fn hit_test_item(item: &SidebarItem, right_click: bool, shift: bool) -> SidebarAction {
    if !item.is_superweapon {
        return SidebarAction::CameoPress {
            type_id: item.type_id.clone(),
            right: right_click,
            shift,
            at_build_limit: item.at_build_limit,
        };
    }
    // Draw flags are not input admission. The app resolves a fresh sim view;
    // right presses also reach the owner while the Super is held/charging.
    SidebarAction::SuperWeaponPress {
        section: item
            .super_weapon_section
            .clone()
            .unwrap_or_else(|| item.display_name.clone()),
        right: right_click,
    }
}

// `sidebar::hit_test` (the legacy press-path hit-test) was retired in A6: every
// in-game surface — tabs/repair/sell/scroll (A1), cameos (A2) and tactical/minimap
// (A3) — is now owned by the `app::input::gadget_input`
// retained list (one order = hit + draw, R7 complete). `hit_test_item` (the cameo
// click→action map) stays public for the driver.

#[cfg(test)]
mod native_geometry_tests;
