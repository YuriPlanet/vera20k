//! Custom-rendered in-game sidebar layout/model.
//!
//! This module stays render-agnostic: it describes sidebar geometry, tab/item
//! state, and hit-testing, while the app/render layers decide how to draw it.
//!
//! Native screen geometry comes from `6A5090`/`6A5130`, with shape sizes
//! from the loaded assets and fixed 60×48 cameo hit zones (`6A8220`).

pub(crate) mod cameo_order;
#[cfg(all(test, any(tests_ui, tests_app_presentation)))]
mod cameo_order_tests;
pub mod command_bar;
mod frame;
pub mod gadget_flash;
mod layout_spec;
pub mod power_bar_anim;
mod sidebar_view;

use crate::sim::production::ProductionCategory;

pub use frame::{BOTTOM_STRIP_HEIGHT_PX, Edge, PanelPlaces, SidebarFrame};
pub use layout_spec::{ArtKey, PartKind, SidebarChromeLayoutSpec, SidebarPart, SidebarTheme};
pub use power_bar_anim::PowerBarAnimState;
pub(crate) use sidebar_view::ArmedSidebarEntry;
#[cfg(test)]
pub(crate) use sidebar_view::build_sidebar_view;
pub(crate) use sidebar_view::append_added_grids;
pub(crate) use sidebar_view::apply_observer_strip;
pub(crate) use sidebar_view::build_sidebar_view_with_spec;
pub(crate) use sidebar_view::{CameoFactoryStatus, ObjectCameoDrawInputs, draw_object_cameo};

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

    /// This rect for a part the layout has, and an empty one in its place for
    /// a part it leaves out: nothing hits it and nothing is drawn in it.
    pub fn shown(self, shown: bool) -> Self {
        if shown {
            self
        } else {
            Self {
                w: 0.0,
                h: 0.0,
                ..self
            }
        }
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
    /// TypeToTab 6ABC60 / GetObjectTabIdx 6ABCD0: buildings 0, or 1 with a
    /// combat BuildCat (production_tech's Defense), infantry 2, units and
    /// aircraft 3. Super weapons take strip 1 in `CameoStrips`.
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

#[derive(Debug, Clone, PartialEq)]
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
    /// True when the House's primary Factory for this type's kind holds this
    /// entry's type, finished or not. `StripClass::Draw` reads that Factory's
    /// `+0x58` and compares its type with the requested one at
    /// `0x006A9CBF..0x006A9CC3` to decide whether a count of one is the held
    /// product rather than a queued copy.
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

#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarToggleButton {
    pub rect: Rect,
    pub action: SidebarAction,
    /// Externally driven latch state for the toggle gadget.
    pub active: bool,
    /// SHP frame index (0..=4) for the button's per-theme SHP atlas.
    pub frame_index: u8,
}

/// View entry for one of the two SHP-driven strip scroll buttons.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarScrollButton {
    pub rect: Rect,
    /// Retained gadget disabled bit. Scroll capacity comes from `6A6610`.
    pub disabled: bool,
    /// SHP frame index (0..=2) for the button's per-theme atlas.
    pub frame_index: u8,
}

impl SidebarLayout {
    /// The top-left corner of `part` on the layout screen: its place in its
    /// panel's corner, the sidebar's for a part with no panel, and for the
    /// scroll buttons the cameo strip's left edge at the bottom of its last
    /// row.
    pub fn origin(&self, spec: &SidebarChromeLayoutSpec, part: SidebarPart) -> [f32; 2] {
        let [x, y] = spec.place(part);
        match part.panel() {
            None => [self.sidebar_x + x, y],
            Some(SidebarPart::Cameos) => [
                self.sidebar_x + spec.place(SidebarPart::Cameos)[0] + x,
                self.cameo_grid_bottom + y,
            ],
            Some(panel) => {
                let [panel_x, panel_y] = self.origin(spec, panel);
                [panel_x + x, panel_y + y]
            }
        }
    }

    /// The radar's map picture: the minimap's corner in the radar housing,
    /// which the GDI/NOD tables move, and the aperture's size
    /// (`RadarClass::One_Time @ 0x00652CF0`). Empty for a layout without the
    /// radar.
    pub fn minimap_rect(&self, spec: &SidebarChromeLayoutSpec) -> Rect {
        let [x, y] = self.origin(spec, SidebarPart::Minimap);
        Rect {
            x,
            y,
            w: spec.radar_width,
            h: spec.radar_height,
        }
        .shown(spec.shows(SidebarPart::Radar))
    }

    /// The height of the cameo strip's rows.
    pub fn cameo_grid_height(&self) -> f32 {
        self.cameo_grid_bottom - self.cameo_grid_top
    }

    /// The top-left corner of the cameo grid's first picture: the strip's
    /// corner and the cameo inset.
    pub fn grid_origin(&self, spec: &SidebarChromeLayoutSpec) -> [f32; 2] {
        [
            self.origin(spec, SidebarPart::Cameos)[0] + spec.cameo_inset_x,
            self.cameo_grid_top + spec.cameo_inset_y,
        ]
    }
}

impl SidebarView {
    /// A button part's rect and the frame its picture shows: the top
    /// buttons, Repair and Sell, the tabs and the scroll buttons. `None` for
    /// any other part, or a tab the view has no button for.
    pub fn button(&self, part: SidebarPart) -> Option<(Rect, u8)> {
        let scroll = |button: &SidebarScrollButton| (button.rect, button.frame_index);
        let toggle = |button: &SidebarToggleButton| (button.rect, button.frame_index);
        match part {
            SidebarPart::Diplomacy => Some(scroll(&self.top_buttons[0])),
            SidebarPart::Options => Some(scroll(&self.top_buttons[1])),
            SidebarPart::Repair => Some(toggle(&self.repair_button)),
            SidebarPart::Sell => Some(toggle(&self.sell_button)),
            SidebarPart::ScrollDown => Some(scroll(&self.scroll_down_button)),
            SidebarPart::ScrollUp => Some(scroll(&self.scroll_up_button)),
            _ => {
                let index = SidebarPart::TABS.iter().position(|&tab| tab == part)?;
                self.tabs
                    .iter()
                    .find(|button| button.tab.tab_index() == index)
                    .map(|button| (button.rect, button.frame_index))
            }
        }
    }
}

/// Where each chrome section sits on the layout screen, computed from the
/// layout table and the screen size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarLayout {
    pub sidebar_x: f32,
    /// The top and the bottom of the cameo strip's rows.
    pub cameo_grid_top: f32,
    pub cameo_grid_bottom: f32,
    pub side3_y: f32,
    /// How many side2 tiles fit in the cameo region.
    pub side2_tile_count: usize,
}

/// A grid of build pictures a layout adds beside the sidebar's own: one
/// tab's strip from `scroll_rows` rows down, in `rows` rows of `columns`,
/// its first picture at `origin` in the sidebar's layout pixels. `part` is
/// the grid's place in the layout's list of added parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddedGrid {
    pub part: usize,
    pub tab: SidebarTab,
    pub origin: [f32; 2],
    pub columns: usize,
    pub rows: usize,
    pub scroll_rows: usize,
}

/// Where a view holds an added grid's cameos: `len` items from `first`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddedGridView {
    pub part: usize,
    pub first: usize,
    pub len: usize,
    /// The grid's whole area, for the wheel.
    pub rect: Rect,
    /// The row it shows first, and the last row it can start at.
    pub scroll_rows: usize,
    pub max_scroll_rows: usize,
}

/// One House on the observer's strip (`StripClass::Draw @ 0x006A9540`,
/// `0x006AA043..0x006AA59B`), two cameo rows tall.
#[derive(Debug, Clone, PartialEq)]
pub struct ObserverEntry {
    /// Its background's top left, in the sidebar's layout pixels: the cameo
    /// grid's origin, two rows a House.
    pub origin: [f32; 2],
    /// The country's side (`HouseType+0xBC`): 0, 1 and 2 draw `OBSALLI`,
    /// `OBSSOVI` and `OBSYURI`, any other none.
    pub side: i32,
    /// The country's index (`HouseType+0xB8`), which picks the flag.
    pub country: i32,
    pub name: String,
    pub rgb: [u8; 3],
    /// Rank (online only), then kills, units and credits, a line each.
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidebarView {
    pub panel_rect: Rect,
    pub layout: SidebarLayout,
    /// The credits counter's value; the observer's elapsed seconds while
    /// [`Self::observer`] is set (`CreditsClass::AI @ 0x004A2600`).
    pub credits: i32,
    /// The observer's strip, drawn over the cameo grid while the local
    /// player is an observer: the Houses its rows show.
    pub observer: Option<Vec<ObserverEntry>>,
    pub power_produced: i32,
    pub power_drained: i32,
    pub low_power: bool,
    pub scroll_rows: usize,
    pub max_scroll_rows: usize,
    pub tabs: Vec<SidebarTabButton>,
    /// The cameos of the sidebar's own grid, then those of each grid a
    /// layout adds ([`Self::added_grids`]).
    pub items: Vec<SidebarItem>,
    /// Empty slots of the sidebar's own grid that a cameo flash darkens this
    /// frame ([`cameo_order::CameoStrips::empty_slot_flash_darkened`]).
    pub flash_empty_slots: Vec<Rect>,
    pub added_grids: Vec<AddedGridView>,
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

/// Where the chrome sections sit on `frame`'s layout screen
/// ([`SidebarFrame::layout_screen`]): the sidebar at the frame's left edge
/// for it, filling the screen's height. The item count only affects
/// scrolling.
pub(crate) fn compute_layout_with_spec(
    spec: SidebarChromeLayoutSpec,
    frame: SidebarFrame,
) -> SidebarLayout {
    let (_, screen_h) = frame.layout_screen();
    // Original 6A5090/6A5130: capacity depends on screen height and side,
    // never on build-item count. Integer division truncates toward zero.
    // Native keeps 60 select buttons per strip (0x3C stride, InitSelectZones
    // 6A8220). Above 30 rows (screens 1810 px or taller) strips 0..2 write into
    // the next strip's buttons, and the Vehicle strip runs past the array's
    // end at B0B300 (a static coordinate, then the R-DN ShapeButton at B0B328).
    // Activate 6A8330 then adds that vtable-less memory as gadgets. Native has
    // no defined result here, so VERA keeps a working uncapped grid; the
    // maintainer chose to keep it (2026-11-15).
    // GetVisibleSlotCount 6AC430 and StripClass Activate 6A8330 count 2 * rows
    // the same way: footer 26, or 18 when Scenario +0x34B8 (the side) is set,
    // plus 7, which the table carries as one height.
    let row_height = (spec.cameo_row_height as i32).max(1);
    let strip_y = spec.place(SidebarPart::Cameos)[1];
    let rows = ((screen_h as i32 - strip_y as i32 - spec.strip_footer as i32) / row_height).max(0)
        as usize;
    SidebarLayout {
        sidebar_x: frame.layout_left(),
        cameo_grid_top: strip_y,
        cameo_grid_bottom: strip_y + (rows as i32 * row_height) as f32,
        // Native 6A6C30 advances by the loaded SIDE2 canvas height. VERA's
        // columns shrink the row, so there SIDE3 follows the row's own
        // height instead; the original table sets both to the same 50.
        side3_y: strip_y
            + rows as f32
                * if spec.cameo_columns > 2.0 {
                    row_height as f32
                } else {
                    spec.side2_height
                },
        side2_tile_count: rows,
    }
}

/// Native PowerClass tooltip (`6403A0`). The 8-pixel hit width is distinct
/// from POWERP's actual canvas width and the 3-pixel paint stride.
pub fn power_bar_rect(layout: &SidebarLayout, spec: SidebarChromeLayoutSpec) -> Rect {
    let [x, y] = layout.origin(&spec, SidebarPart::Power);
    Rect {
        x,
        y,
        w: 8.0,
        h: layout.cameo_grid_height(),
    }
    .shown(spec.shows(SidebarPart::Power))
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
// (A3) — is now owned by the match's chrome (`app::input::chrome_input`, the
// GUI's chrome layer). `hit_test_item` (the cameo click→action map) stays
// public for it.

#[cfg(all(test, tests_ui))]
mod native_geometry_tests;
