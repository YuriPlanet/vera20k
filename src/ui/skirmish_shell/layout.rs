//! Dialog 0x102 shell layout recovered from gamemd.exe.

use super::state::DropdownScrollbarPart;
use crate::ui::shell::descriptor::{
    AnchorRule, BACK_BUTTON, BUTTON, LOW_HEADING_ANCHOR, MAP_TEXT_ANCHOR,
};
pub use crate::ui::shell::geom::{RectPx, RightPanelRects};
use crate::ui::shell::geom::{
    ScaledRects, ShellScale, center_offset, child_window, dialog_origin, dlu_rect,
    right_panel_rects,
};
use crate::ui::shell::layout::Placer;
use vera20k_gui::scaled_rects;
use vera20k_gui::shell::combo_scroll::ScrollModel;

pub const SHELL_BASE_W: i32 = 800;
pub const SHELL_BASE_H: i32 = 600;
pub const RIGHT_PANEL_WIDTH: i32 = crate::ui::shell::geom::RIGHT_PANEL_WIDTH;
pub const SDBTNBKGD_H: i32 = crate::ui::shell::geom::RIGHT_PANEL_TILE_H;
pub const SKIRMISH_CHECKBOX_COUNT: usize = 5;
pub const CHECKBOX_ICON_W: i32 = 18;
pub const CHECKBOX_ICON_H: i32 = 18;
pub const CHECKBOX_TEXT_LEFT_OFFSET: i32 = 26;
pub const SKIRMISH_ROW_COUNT: usize = 8;
/// VERA: the most player rows the page shows at once. On a map with more
/// than eight starts the retail eight grow into the room under the options
/// ([`SkirmishShellLayout::shown_rows`]); the rest scroll.
pub const SKIRMISH_LONG_ROW_COUNT: usize = 13;

/// VERA: the player rows the page shows for a map admitting `rows`: the
/// retail eight up to eight starts, then one per start up to
/// [`SKIRMISH_LONG_ROW_COUNT`].
pub fn shown_player_rows(rows: usize) -> usize {
    rows.clamp(SKIRMISH_ROW_COUNT, SKIRMISH_LONG_ROW_COUNT)
}

/// The `0x102` template's player rows start at y 36 DLU, 16 DLU apart; the
/// rows VERA adds past the eighth keep the pitch.
const ROW_TOP_DLU: i32 = 36;
const ROW_PITCH_DLU: i32 = 16;

/// One control column of the player rows from row `first` on: the template
/// rect of row `first + index` for each index.
fn row_column<const N: usize>(first: usize, x: i32, w: i32, h: i32) -> [RectPx; N] {
    std::array::from_fn(|index| {
        dlu_rect(
            x,
            ROW_TOP_DLU + ROW_PITCH_DLU * (first + index) as i32,
            w,
            h,
        )
    })
}
pub use vera20k_gui::shell::combobox::{
    COMBO_ARROW_RESERVE_W, COMBO_DROPDOWN_ROW_H, COMBO_DROPDOWN_SCROLLBAR_BUTTON_H,
    COMBO_DROPDOWN_SCROLLBAR_MIN_THUMB_H, COMBO_DROPDOWN_SCROLLBAR_W, COMBO_FACE_H,
    DropdownMetrics, combo_arrow_hit_rect, combo_face_rect,
};
pub const COMBO_ARROW_X_FROM_RIGHT: i32 = 19;
pub const COMBO_ARROW_Y: i32 = 1;
pub const COMBO_TEXT_LEFT_INSET: i32 = 2;
pub const COMBO_SWATCH_INSET: i32 = 2;

/// Visible row `visible_row` of an open combo dropdown, on the one row grid the
/// item text, the highlight, the colour swatches and both pointer hit tests
/// share: [`DropdownMetrics::row_h`] steps from the content top.
pub fn combo_dropdown_row_rect(content: RectPx, scale: ShellScale, visible_row: usize) -> RectPx {
    let row_h = DropdownMetrics::scaled(scale).row_h;
    RectPx::new(
        content.x,
        content.y + visible_row as i32 * row_h,
        content.w,
        row_h,
    )
}
pub const PLAYER_NAME_EDIT_CLIENT_INSET: i32 = 1;
pub const PLAYER_NAME_EDIT_TEXT_LEFT_INSET: i32 = 2;
pub const CHOOSE_MAP_MODAL_W: i32 = 533;
pub const CHOOSE_MAP_MODAL_H: i32 = 369;
// Random-map setup dialog 0x105. Same 533x369-DLU frame, font and background as
// choose-map. Its right-column controls sit 2-3 DLU left of choose-map's in the
// resource (and the preview 2 DLU right), but every right-column rule here is
// panel-anchored and discards the source x, so those differences do not reach
// the output. The left column is plain dlu_rect, where x and y both matter.
const SETUP_LABEL_X: i32 = 74;
const SETUP_LABEL_W: i32 = 93;
const SETUP_CONTROL_X: i32 = 179;
const SETUP_CONTROL_W: i32 = 150;
const SETUP_TRACKBAR_H: i32 = 13;
/// Rows map type, time, theater, size, resources, players: the control's top,
/// the label's top and height, and the control's height. The 14-DLU labels
/// sit one DLU above their control; the time combo's dropdown is two DLU
/// shorter than the others, and players is a trackbar.
const SETUP_ROWS: [(i32, i32, i32, i32); 6] = [
    (41, 40, 14, 103),
    (65, 64, 14, 101),
    (90, 90, 12, 103),
    (114, 114, 12, 103),
    (138, 138, 12, 103),
    (163, 162, 14, SETUP_TRACKBAR_H),
];
/// Right-column button tops: Use Map, Load, Save, Delete, then VERA's
/// Advanced/Basic toggle, Export Map, Fair Starts and Presets, each the
/// column's next slot at the same 27-unit pitch.
const SETUP_RIGHT_Y: [i32; 8] = [122, 149, 176, 203, 230, 257, 284, 311];
/// VERA: the Advanced view's seven sliders, at the option rows' 24-unit pitch
/// from the first row; the last ends above the progress widgets (217).
const SETUP_ADVANCED_ROW_Y: [i32; 7] = [41, 65, 90, 114, 138, 163, 187];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellControlId {
    StartGame0x617,
    ChooseMap0x5aa,
    Back0x5c0,
    MapPreview0x468,
    PlayerName0x6a0,
    PlayerColor0x6a2,
    AiColor0x522,
    AiColor0x523,
    AiColor0x524,
    AiColor0x525,
    AiColor0x526,
    AiColor0x527,
    AiColor0x528,
    Flag0x6da,
    Flag0x6db,
    Flag0x6dc,
    Flag0x6dd,
    Flag0x6de,
    Flag0x6df,
    Flag0x6e0,
    Flag0x6e1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorComboId {
    Player,
    Ai(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishCheckboxId {
    ShortGame0x54e,
    McvRepacks0x693,
    CratesAppear0x696,
    SuperWeapons0x69a,
    BuildOffAlly0x69d,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishCheckboxRect {
    pub id: SkirmishCheckboxId,
    pub rect: RectPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishTrackbarId {
    GameSpeed0x529,
    Credits0x511,
    UnitCount0x50c,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishRightPanelTextRects {
    pub title: RectPx,
    pub game_type: RectPx,
    pub map_label: RectPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishTrackbarRects {
    pub game_speed: RectPx,
    pub credits: RectPx,
    pub unit_count: RectPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishTrackbarLabelRects {
    pub game_speed: RectPx,
    pub credits: RectPx,
    pub unit_count: RectPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishColumnLabelRects {
    pub players: RectPx,
    pub side: RectPx,
    pub color: RectPx,
    pub start: RectPx,
    pub team: RectPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishRowRects {
    /// Rows 1 on: the player's own row has a name box instead.
    pub ai_type_combos: [RectPx; SKIRMISH_LONG_ROW_COUNT - 1],
    pub side_combos: [RectPx; SKIRMISH_LONG_ROW_COUNT],
    pub start_combos: [RectPx; SKIRMISH_LONG_ROW_COUNT],
    pub team_combos: [RectPx; SKIRMISH_LONG_ROW_COUNT],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishShellLayout {
    /// How far these rects sit from the 800x600 art they were authored against.
    /// `RETAIL` on the retail layout at every screen size.
    pub scale: ShellScale,
    pub screen: RectPx,
    pub right_panel: RightPanelRects,
    pub right_panel_text: SkirmishRightPanelTextRects,
    pub start_button: RectPx,
    /// gamemd's Choose Map (`0x5AA`), one tile down where VERA's Game Options
    /// takes its tile.
    pub choose_map_button: RectPx,
    /// VERA's Game Options ([`super::game_options`]), on gamemd's Choose Map
    /// tile under Start Game.
    pub game_options_button: RectPx,
    /// VERA's Setups, the tile under Choose Map, which opens the page that
    /// saves and loads named setups; gamemd's `0x102` has no such button.
    pub setups_button: RectPx,
    pub back_button: RectPx,
    pub map_preview: RectPx,
    pub column_labels: SkirmishColumnLabelRects,
    pub player_name: RectPx,
    /// Every row slot the page can show; the first [`Self::shown_rows`] show.
    pub rows: SkirmishRowRects,
    pub color_combos: [RectPx; SKIRMISH_LONG_ROW_COUNT],
    pub flags: [RectPx; SKIRMISH_LONG_ROW_COUNT],
    /// The player rows this page shows: the retail eight, or VERA's longer
    /// list for a map with more than eight starts ([`shown_player_rows`]).
    pub shown_rows: usize,
    /// VERA's scrollbar for a map with more than eight starts: the list frame
    /// round it, beside the player rows ([`opponent_scrollbar_rect`]).
    pub opponent_scrollbar: RectPx,
    pub trackbars: SkirmishTrackbarRects,
    pub trackbar_labels: SkirmishTrackbarLabelRects,
    pub checkboxes: [SkirmishCheckboxRect; SKIRMISH_CHECKBOX_COUNT],
    pub status_help: RectPx,
}

impl SkirmishShellLayout {
    /// Where the shell's own art sits: gamemd's [`dialog_origin`] on the retail
    /// layout, and the scaled dialog's top-left once the 800x600 space is
    /// scaled into the window — every child rect already carries that scale.
    pub fn shell_origin(&self) -> (i32, i32) {
        let (x, y) = dialog_origin(self.screen.w, self.screen.h);
        self.scale
            .dialog_origin_or((self.screen.x + x, self.screen.y + y))
    }

    /// The live height of the shell space: the authored 600 rows letterboxed
    /// below the strip on the retail layout, the scaled dialog's once scaled.
    pub fn shell_height(&self) -> i32 {
        if self.scale.is_retail() {
            if self.screen.h > 767 {
                600
            } else {
                self.screen.h
            }
        } else {
            self.scale.dialog_rect().h
        }
    }

    /// The width the shell *assets* are selected by. The 640-wide shapes belong to
    /// the unscaled layout only; a stretched page always takes the large art.
    pub fn authored_width(&self) -> i32 {
        if self.scale.is_retail() {
            self.screen.w
        } else {
            SHELL_BASE_W
        }
    }
}

/// Heading `0x694` of Choose Map `0x6B`.
pub const CHOOSE_MAP_TITLE_KEY: &str = "GUI:ChooseMap";
/// The random-map dialog `0x105`'s heading (its template text).
pub const RANDOM_MAP_TITLE_KEY: &str = "GUI:GenerateMap";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapModalButton {
    UseMap0x6c5,
    Cancel0x5c0,
    CreateRandomMap0x583,
    /// VERA: stars or unstars the highlighted map. `0x6B` has no such button.
    Favorite,
}

/// VERA: the generator fields the Advanced view sets, each a 0..100 slider.
/// The original derives all of them from the map type on every Generate
/// (`MapSeedClass::RandomizeDerivedFields @ 0x00597260`) and shows none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvancedField {
    Water,
    Hills,
    Buildings,
    Vegetation,
    OreFields,
    /// `RegionSize`: the largest plateau, one height level, the Inland and
    /// Mountainous map types keep before cutting it into levels with cliffs.
    PlateauSize,
    Accessibility,
    /// VERA extension (Advanced page 2): neutral tech-building amount.
    TechBuildings,
    /// VERA extension (Advanced page 2): derelict combat buildings amount.
    Derelicts,
    /// VERA extension (Advanced page 2): whole-map building damage level.
    Damage,
    /// VERA extension: extra civilian infantry and vehicles on Neutral.
    Civilians,
    /// VERA extension: `Name=Animal …` units on Neutral.
    Animals,
    /// VERA extension: player-buildable units left neutral and hostile.
    Wild,
}

impl AdvancedField {
    /// Row order in the Advanced view: the original seven, then VERA's
    /// sliders on the following pages.
    pub const ALL: [Self; 13] = [
        Self::Water,
        Self::Hills,
        Self::Buildings,
        Self::Vegetation,
        Self::OreFields,
        Self::PlateauSize,
        Self::Accessibility,
        Self::TechBuildings,
        Self::Derelicts,
        Self::Damage,
        Self::Civilians,
        Self::Animals,
        Self::Wild,
    ];

    /// Sliders on a page. The dialog has room for the original seven rows.
    pub const PAGE_SIZE: usize = 7;
    pub const PAGE_COUNT: usize = 2;

    pub const fn row(self) -> usize {
        self as usize
    }

    /// The page this slider is on: the original seven first, then VERA's.
    pub const fn page(self) -> usize {
        self.row() / Self::PAGE_SIZE
    }

    /// The row within the page, used to index the seven dialog control rects.
    pub const fn page_row(self) -> usize {
        self.row() % Self::PAGE_SIZE
    }

    /// The Advanced view's native vertical scrollbar, to the right of the
    /// reusable slider windows. Its span is exactly the sliders' span, so it
    /// stays aligned at every page scale.
    pub fn advanced_scrollbar_rect(layout: &RandomMapSetupLayout) -> RectPx {
        let first = layout.advanced_control_rects[0];
        let last = layout.advanced_control_rects[Self::PAGE_SIZE - 1];
        RectPx::new(
            first.x + first.w + 4,
            first.y,
            DropdownMetrics::scaled(layout.scale).scrollbar_w,
            last.y + last.h - first.y,
        )
    }

    /// The same scrollbar geometry the combo dropdowns use, with the seven
    /// visible Advanced rows as its page size.
    pub fn advanced_scrollbar_model(scale: ShellScale) -> ScrollModel {
        ScrollModel::combo_at(Self::PAGE_SIZE as i32, DropdownMetrics::scaled(scale))
    }

    /// Thumb position for a scroll offset clamped to the available pages.
    pub fn advanced_scrollbar_thumb(layout: &RandomMapSetupLayout, page: usize) -> RectPx {
        let bar = Self::advanced_scrollbar_rect(layout);
        let model = Self::advanced_scrollbar_model(layout.scale);
        let thumb_h = model.thumb_height(Self::PAGE_SIZE, Self::ALL.len(), bar.h);
        let top = page.min(Self::PAGE_COUNT - 1);
        let thumb_y = model.thumb_y(bar, thumb_h, top, Self::PAGE_COUNT - 1);
        RectPx::new(bar.x, thumb_y, bar.w, thumb_h)
    }

    /// Which part of the Advanced scrollbar a point is over.
    pub fn advanced_scrollbar_part_at(
        layout: &RandomMapSetupLayout,
        page: usize,
        x: i32,
        y: i32,
    ) -> Option<DropdownScrollbarPart> {
        let bar = Self::advanced_scrollbar_rect(layout);
        if !bar.contains(x, y) {
            return None;
        }
        let button_h = DropdownMetrics::scaled(layout.scale).button_h;
        if y < bar.y + button_h {
            return Some(DropdownScrollbarPart::UpArrow);
        }
        if y >= bar.y + bar.h - button_h {
            return Some(DropdownScrollbarPart::DownArrow);
        }
        let thumb = Self::advanced_scrollbar_thumb(layout, page);
        Some(if thumb.contains(x, y) {
            DropdownScrollbarPart::Thumb
        } else {
            DropdownScrollbarPart::Track
        })
    }

    /// Scroll offset a track press or thumb drag maps to.
    pub fn advanced_scrollbar_page_at_pointer(
        layout: &RandomMapSetupLayout,
        page: usize,
        y: i32,
    ) -> usize {
        let bar = Self::advanced_scrollbar_rect(layout);
        let thumb = Self::advanced_scrollbar_thumb(layout, page);
        Self::advanced_scrollbar_model(layout.scale).top_index_at_pointer(
            bar,
            thumb.h,
            Self::PAGE_COUNT - 1,
            y,
        )
    }

    /// The row label: the string table's own names where it has one.
    pub const fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Water => ("GUI:Water", "Water"),
            Self::Hills => ("GUI:Hills", "Hills"),
            Self::Buildings => ("GUI:Buildings", "Buildings"),
            Self::Vegetation => ("GUI:Vegetation", "Vegetation"),
            Self::OreFields => ("GUI:TiberiumFields", "Ore Fields"),
            Self::PlateauSize => ("VERA:RandomMapPlateauSize", "Plateau Size"),
            Self::Accessibility => ("GUI:Accessability", "Accessibility"),
            Self::TechBuildings => ("VERA:RandomMapTechBuildings", "Tech Buildings"),
            Self::Derelicts => ("VERA:RandomMapDerelicts", "Derelicts"),
            Self::Damage => ("VERA:RandomMapDamage", "Damage"),
            Self::Civilians => ("VERA:RandomMapCivilians", "Civilians"),
            Self::Animals => ("VERA:RandomMapAnimals", "Animals"),
            Self::Wild => ("VERA:RandomMapWild", "Neutral Defenders"),
        }
    }
}

/// Every interactive control in the random-map setup dialog `0x105`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomMapSetupControl {
    MapType0x405,
    Time0x3ea,
    Theater0x407,
    Size0x406,
    Resources0x408,
    Players0x3eb,
    Randomize0x621,
    Generate0x620,
    Ok0x6c5,
    Load0x6c2,
    Save0x6c3,
    Delete0x6c4,
    Cancel0x5c0,
    /// VERA: switches the option rows between the original controls and
    /// the Advanced sliders.
    AdvancedToggle,
    /// VERA: one of the Advanced view's sliders.
    Advanced(AdvancedField),
    /// VERA: the vertical scrollbar that pages the Advanced sliders.
    AdvancedScrollbar,
    /// VERA: writes the generated map as a `.yrm` file.
    ExportMap,
    /// VERA: turns Fair Starts on or off.
    FairStartsToggle,
    /// VERA: applies the next ready-made option set.
    PresetCycle,
    /// VERA: a row of the Extras view, top down from 0.
    ExtrasRow(usize),
    /// Ares' Urban Areas box (`ARES_CHK_RMG_URBAN_AREAS`, Ares' dialog 261).
    /// Hit-tested only when Ares.dll is loaded (the game is read the Ares
    /// way).
    UrbanAreas5000,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChooseMapModalLayout {
    /// The 800x600 -> window mapping these rects went through.
    pub scale: ShellScale,
    pub screen: RectPx,
    pub dialog: RectPx,
    pub mode_list: RectPx,
    pub map_list: RectPx,
    /// VERA: the search box between the Game Map heading and the map list,
    /// framed like the list. `0x6B` has none; its list starts where the box
    /// does and is one row longer.
    pub map_search: RectPx,
    pub use_map_button: RectPx,
    pub cancel_button: RectPx,
    pub create_random_map_button: RectPx,
    /// VERA: the favorite button, in the right column's slot under Create
    /// Random Map.
    pub favorite_button: RectPx,
    pub title: RectPx,
    pub select_engagement: RectPx,
    pub game_type_heading: RectPx,
    pub game_map_heading: RectPx,
    pub status_help: RectPx,
    pub preview: RectPx,
    /// VERA: the selected map's author under the preview, two pixels above
    /// where `0x102` shows its game type ([`game_type_rect`]), to sit closer
    /// to the preview. `0x6B` has no such line.
    pub author: RectPx,
}

impl ChooseMapModalLayout {
    /// The width the chooser's background assets are selected by: they exist at
    /// 640 and 800, so a stretched chooser takes the 800 shape.
    pub fn authored_width(&self) -> i32 {
        if self.scale.is_retail() {
            self.screen.w
        } else {
            SHELL_BASE_W
        }
    }
}

/// Pixel rects for the random-map setup dialog `0x105`.
///
/// Rows are ordered map type, time, theater, size, resources, players.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RandomMapSetupLayout {
    /// The 800x600 -> window mapping these rects went through.
    pub scale: ShellScale,
    pub screen: RectPx,
    pub dialog: RectPx,
    pub label_rects: [RectPx; 6],
    pub control_rects: [RectPx; 6],
    pub randomize: RectPx,
    pub generate: RectPx,
    pub seed_field: RectPx,
    pub title: RectPx,
    pub preview: RectPx,
    pub use_map: RectPx,
    pub load: RectPx,
    pub save: RectPx,
    pub delete: RectPx,
    pub cancel: RectPx,
    /// VERA: the Advanced/Basic toggle in the right column.
    pub advanced: RectPx,
    /// VERA: the Export Map button below the toggle.
    pub export: RectPx,
    /// VERA: the Fair Starts toggle in the right column.
    pub fair_starts: RectPx,
    /// VERA: the Presets button below it.
    pub preset: RectPx,
    /// VERA: the Advanced view's labels and slider windows, by
    /// [`AdvancedField::row`].
    pub advanced_label_rects: [RectPx; 7],
    pub advanced_control_rects: [RectPx; 7],
    /// Status line `0x695` (the family static, template 303x12 DLU).
    pub status_help: RectPx,
    pub progress_text: RectPx,
    pub progress_bar: RectPx,
    /// Ares' Urban Areas checkbox: `177, 192, 150, 14` in Ares' dialog 261
    /// (RMGDialog.rc), which keeps every stock control's position. Drawn and
    /// hit-tested only when Ares.dll is loaded (the game is read the Ares way).
    pub urban_areas: RectPx,
}

const fn translate_checkbox(rect: SkirmishCheckboxRect, dx: i32, dy: i32) -> SkirmishCheckboxRect {
    SkirmishCheckboxRect {
        id: rect.id,
        rect: rect.rect.translate(dx, dy),
    }
}

pub fn translate_layout(mut layout: SkirmishShellLayout, dx: i32, dy: i32) -> SkirmishShellLayout {
    layout.right_panel = layout.right_panel.translate(dx, dy);
    let text = &mut layout.right_panel_text;
    let labels = &mut layout.column_labels;
    let rows = &mut layout.rows;
    let rects = [
        &mut layout.screen,
        &mut text.title,
        &mut text.game_type,
        &mut text.map_label,
        &mut layout.start_button,
        &mut layout.choose_map_button,
        &mut layout.game_options_button,
        &mut layout.setups_button,
        &mut layout.back_button,
        &mut layout.map_preview,
        &mut labels.players,
        &mut labels.side,
        &mut labels.color,
        &mut labels.start,
        &mut labels.team,
        &mut layout.player_name,
        &mut layout.opponent_scrollbar,
        &mut layout.trackbars.game_speed,
        &mut layout.trackbars.credits,
        &mut layout.trackbars.unit_count,
        &mut layout.trackbar_labels.game_speed,
        &mut layout.trackbar_labels.credits,
        &mut layout.trackbar_labels.unit_count,
        &mut layout.status_help,
    ];
    let columns = rows.ai_type_combos.iter_mut().chain(&mut rows.side_combos);
    let columns = columns
        .chain(&mut rows.start_combos)
        .chain(&mut rows.team_combos);
    let columns = columns
        .chain(&mut layout.color_combos)
        .chain(&mut layout.flags);
    for rect in rects.into_iter().chain(columns) {
        *rect = rect.translate(dx, dy);
    }
    for rect in &mut layout.checkboxes {
        *rect = translate_checkbox(*rect, dx, dy);
    }
    layout
}

pub fn compute_fixed_800_layout(screen_w: u32, screen_h: u32) -> SkirmishShellLayout {
    translate_layout(
        compute_layout(SHELL_BASE_W as u32, SHELL_BASE_H as u32),
        center_offset(screen_w as i32, SHELL_BASE_W),
        center_offset(screen_h as i32, SHELL_BASE_H),
    )
}

/// The icon plate inside a checkbox. `scale` maps the control's own device rect
/// back to its authored 18x18 plate, so paint and hit-testing agree on one rect.
pub fn checkbox_icon_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        RectPx::new(rect.x, rect.y, CHECKBOX_ICON_W, CHECKBOX_ICON_H)
    })
}

pub fn checkbox_text_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        RectPx::new(
            rect.x + CHECKBOX_TEXT_LEFT_OFFSET,
            rect.y,
            rect.w - CHECKBOX_TEXT_LEFT_OFFSET,
            rect.h,
        )
    })
}

pub fn player_name_edit_client_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        RectPx::new(
            rect.x + PLAYER_NAME_EDIT_CLIENT_INSET,
            rect.y + PLAYER_NAME_EDIT_CLIENT_INSET,
            rect.w - PLAYER_NAME_EDIT_CLIENT_INSET * 2,
            rect.h - PLAYER_NAME_EDIT_CLIENT_INSET * 2,
        )
    })
}

pub fn player_name_edit_text_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        let client = player_name_edit_client_rect(rect, ShellScale::RETAIL);
        RectPx::new(
            client.x + PLAYER_NAME_EDIT_TEXT_LEFT_INSET,
            client.y,
            client.w - PLAYER_NAME_EDIT_TEXT_LEFT_INSET,
            client.h,
        )
    })
}

/// The combo arrow's top-left anchor: a zero-size rect, because the arrow glyph
/// is placed from it rather than fitted into it.
pub fn combo_arrow_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        RectPx::new(
            rect.x + rect.w - COMBO_ARROW_X_FROM_RIGHT,
            rect.y + COMBO_ARROW_Y,
            0,
            0,
        )
    })
}

pub fn combo_text_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        let w = rect.w - COMBO_ARROW_RESERVE_W;
        RectPx::new(
            rect.x + COMBO_TEXT_LEFT_INSET,
            rect.y,
            if w > 0 { w } else { 0 },
            COMBO_FACE_H,
        )
    })
}

pub fn combo_swatch_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        let w = rect.w - COMBO_ARROW_RESERVE_W - COMBO_SWATCH_INSET * 2;
        let h = COMBO_FACE_H - COMBO_SWATCH_INSET * 2;
        RectPx::new(
            rect.x + COMBO_SWATCH_INSET,
            rect.y + COMBO_SWATCH_INSET,
            if w > 0 { w } else { 0 },
            if h > 0 { h } else { 0 },
        )
    })
}

/// The retail `0x102` page, its eight player rows.
pub fn compute_layout(screen_w: u32, screen_h: u32) -> SkirmishShellLayout {
    compute_layout_for_rows(screen_w, screen_h, SKIRMISH_ROW_COUNT)
}

/// The `0x102` page for a map admitting `player_rows` rows: the retail page
/// up to eight; past eight, VERA's longer list
/// ([`SkirmishShellLayout::with_rows`]).
pub fn compute_layout_for_rows(
    screen_w: u32,
    screen_h: u32,
    player_rows: usize,
) -> SkirmishShellLayout {
    let (screen_w, screen_h) = (screen_w as i32, screen_h as i32);
    let place = Placer::new(screen_w, screen_h);
    let mut player_name = dlu_rect(38, 36, 100, 14);
    player_name.x += 1;
    player_name.w += 1;
    let rows = SkirmishRowRects {
        ai_type_combos: row_column(1, 39, 100, 74),
        side_combos: row_column(0, 191, 78, 74),
        start_combos: row_column(0, 324, 25, 73),
        team_combos: row_column(0, 364, 25, 73),
    };
    // Not retail. VERA's Game Options takes gamemd's Choose Map tile, under
    // Start Game, Choose Map moves one tile down and VERA's Setups takes the
    // tile under it. Every panel has those two tiles free above Back, the
    // six-tile one of the smallest window (640x480) too.
    let game_options_button = place.at(BUTTON, 425, 176, 108, 23);
    let choose_map_button = game_options_button.translate(0, SDBTNBKGD_H);
    let checkbox = |id, x, y, w, h, dx| SkirmishCheckboxRect {
        id,
        rect: dlu_rect(x, y, w, h).translate(dx, 0),
    };

    SkirmishShellLayout {
        scale: ShellScale::RETAIL,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        right_panel: right_panel_rects(screen_w, screen_h),
        // The executed relayout places them at (635, 3, 163, 17), (650, 167,
        // 136, 17) and (650, 189, 136, 34) at 800x600.
        right_panel_text: SkirmishRightPanelTextRects {
            title: place.at(LOW_HEADING_ANCHOR, 425, 1, 108, 10),
            game_type: place.at(MAP_TEXT_ANCHOR, 432, 103, 90, 10),
            map_label: place.at(MAP_TEXT_ANCHOR, 432, 116, 90, 20),
        },
        start_button: place.at(BUTTON, 425, 149, 108, 23),
        choose_map_button,
        game_options_button,
        setups_button: choose_map_button.translate(0, SDBTNBKGD_H),
        back_button: place.rule(BACK_BUTTON),
        map_preview: place.at(AnchorRule::RightAnchor, 429, 23, 96, 69),
        column_labels: SkirmishColumnLabelRects {
            players: dlu_rect(39, 21, 97, 10),
            side: dlu_rect(191, 21, 73, 10),
            color: dlu_rect(283, 21, 42, 10),
            start: dlu_rect(325, 21, 34, 10),
            team: dlu_rect(363, 21, 34, 10),
        },
        player_name,
        opponent_scrollbar: opponent_scrollbar_rect(&rows, SKIRMISH_ROW_COUNT),
        rows,
        color_combos: row_column(0, 282, 29, 73),
        flags: row_column(0, 150, 32, 12),
        shown_rows: SKIRMISH_ROW_COUNT,
        trackbars: SkirmishTrackbarRects {
            game_speed: child_window(269, 176, 85, 13),
            credits: child_window(269, 193, 85, 13),
            // The fix-up pass `0x0060B950` lifts Unit Count one pixel.
            unit_count: child_window(269, 210, 85, 13).translate(0, -1),
        },
        trackbar_labels: SkirmishTrackbarLabelRects {
            game_speed: RectPx::new(302, 286, 90, 16),
            credits: RectPx::new(302, 314, 90, 16),
            unit_count: RectPx::new(302, 341, 90, 16),
        },
        checkboxes: [
            checkbox(SkirmishCheckboxId::ShortGame0x54e, 48, 176, 100, 10, -1),
            checkbox(SkirmishCheckboxId::McvRepacks0x693, 48, 193, 100, 10, -1),
            checkbox(SkirmishCheckboxId::CratesAppear0x696, 48, 210, 100, 10, -1),
            checkbox(SkirmishCheckboxId::SuperWeapons0x69a, 48, 228, 103, 10, -1),
            checkbox(SkirmishCheckboxId::BuildOffAlly0x69d, 201, 227, 166, 11, 0),
        ],
        status_help: place.at(AnchorRule::StatusLine, 2, 355, 410, 12),
    }
    .with_rows(player_rows)
}

impl SkirmishShellLayout {
    /// VERA: the page for a map admitting `rows` player rows. gamemd's
    /// `0x102` shows eight; past eight the rows grow one per start, up to
    /// [`SKIRMISH_LONG_ROW_COUNT`], into the empty room between the options
    /// and the status line. The options move down by as many rows and the
    /// scrollbar beside the computer rows grows with them. Eight or fewer
    /// keep the retail page. On the unscaled template, before any scaling.
    fn with_rows(mut self, rows: usize) -> Self {
        let shown = shown_player_rows(rows);
        let pitch = self.rows.side_combos[1].y - self.rows.side_combos[0].y;
        let dy = (shown - SKIRMISH_ROW_COUNT) as i32 * pitch;
        for checkbox in &mut self.checkboxes {
            *checkbox = translate_checkbox(*checkbox, 0, dy);
        }
        for rect in [
            &mut self.trackbars.game_speed,
            &mut self.trackbars.credits,
            &mut self.trackbars.unit_count,
            &mut self.trackbar_labels.game_speed,
            &mut self.trackbar_labels.credits,
            &mut self.trackbar_labels.unit_count,
        ] {
            *rect = rect.translate(0, dy);
        }
        self.shown_rows = shown;
        self.opponent_scrollbar = opponent_scrollbar_rect(&self.rows, shown);
        self
    }
}

/// How far right of the Team column VERA's scrollbar frame starts.
const OPPONENT_SCROLLBAR_GAP: i32 = 12;

/// VERA's scrollbar right of Team, from the player's row to row `shown`: the
/// shared list's frame window round the native scrollbar child
/// (`0x0061C690`), its outer ring level with the rows' faces. It scrolls only
/// the computer rows, but its top is flush with the top row. The width is
/// the retail scrollbar's; a scaled page takes the scaled scrollbar's.
fn opponent_scrollbar_rect(rows: &SkirmishRowRects, shown: usize) -> RectPx {
    let first = rows.team_combos[0];
    let last = rows.team_combos[shown - 1];
    let top = first.y + 1;
    let bottom = last.y + COMBO_FACE_H - 1;
    RectPx::new(
        first.x + first.w + OPPONENT_SCROLLBAR_GAP,
        top,
        crate::ui::shell::list::ListMetrics::RETAIL.scrollbar_w + 2,
        bottom - top,
    )
}

/// VERA: the search box's height, and how far it pushes the map list's top
/// down: the box and the two frames between them. The box's top is flush
/// with the game-type list's.
const MAP_SEARCH_H: i32 = 17;
const MAP_SEARCH_LIST_SHIFT: i32 = MAP_SEARCH_H + 3;

/// VERA: both lists and their headings are 20 px (a tenth) wider than
/// `0x6B`'s, for long map names. The game-type column grows to the left and
/// the map column to the right, so the gap between them stays.
const LIST_WIDEN: i32 = 20;

fn widen_left(rect: RectPx) -> RectPx {
    RectPx::new(rect.x - LIST_WIDEN, rect.y, rect.w + LIST_WIDEN, rect.h)
}

fn widen_right(rect: RectPx) -> RectPx {
    RectPx::new(rect.x, rect.y, rect.w + LIST_WIDEN, rect.h)
}

pub fn compute_choose_map_modal_layout(screen_w: u32, screen_h: u32) -> ChooseMapModalLayout {
    let (screen_w, screen_h) = (screen_w as i32, screen_h as i32);
    let place = Placer::new(screen_w, screen_h);
    // `0x6B`'s map list window; VERA's search box takes its top.
    let map_list = widen_right(child_window(225, 78, 130, 211));

    ChooseMapModalLayout {
        scale: ShellScale::RETAIL,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        dialog: RectPx::new(0, 0, screen_w, screen_h),
        mode_list: widen_left(child_window(77, 78, 130, 211)),
        map_list: RectPx::new(
            map_list.x,
            map_list.y + MAP_SEARCH_LIST_SHIFT,
            map_list.w,
            map_list.h - MAP_SEARCH_LIST_SHIFT,
        ),
        map_search: RectPx::new(map_list.x, map_list.y, map_list.w, MAP_SEARCH_H),
        use_map_button: place.at(BUTTON, 425, 122, 108, 23),
        cancel_button: place.rule(BACK_BUTTON),
        create_random_map_button: place.at(BUTTON, 425, 149, 108, 23),
        // VERA: one slot (27 dialog units) under Create Random Map.
        favorite_button: place.at(BUTTON, 425, 176, 108, 23),
        title: place.at(LOW_HEADING_ANCHOR, 425, 1, 108, 10),
        select_engagement: dlu_rect(80, 20, 257, 12),
        game_type_heading: widen_left(dlu_rect(77, 60, 130, 10)),
        game_map_heading: widen_right(dlu_rect(225, 60, 130, 10)),
        // The family static (template 303x12 DLU).
        status_help: place.at(AnchorRule::StatusLine, 2, 355, 303, 12),
        preview: place.at(AnchorRule::RightAnchor, 428, 23, 96, 69),
        // `0x102`'s game type static, two pixels higher.
        author: place.at(MAP_TEXT_ANCHOR, 432, 103, 90, 10).translate(0, -2),
    }
}

/// Layout for the random-map setup dialog `0x105`.
///
/// Shares choose-map's frame and right-panel anchoring; only the left column
/// differs. Rows are map type, time, theater, size, resources, players — the
/// players row is a trackbar, so it is shorter than the five combos.
pub fn compute_random_map_setup_layout(screen_w: u32, screen_h: u32) -> RandomMapSetupLayout {
    let (screen_w, screen_h) = (screen_w as i32, screen_h as i32);
    let place = Placer::new(screen_w, screen_h);
    let [
        use_map,
        load,
        save,
        delete,
        advanced,
        export,
        fair_starts,
        preset,
    ] = SETUP_RIGHT_Y.map(|y| place.at(BUTTON, 422, y, 108, 23));

    RandomMapSetupLayout {
        scale: ShellScale::RETAIL,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        dialog: RectPx::new(0, 0, screen_w, screen_h),
        label_rects: SETUP_ROWS.map(|(_, y, h, _)| dlu_rect(SETUP_LABEL_X, y, SETUP_LABEL_W, h)),
        // The players trackbar takes its runtime window; the combos keep the
        // resource conversion, which their face paint expects.
        control_rects: SETUP_ROWS.map(|(y, _, _, h)| {
            let window = if h == SETUP_TRACKBAR_H {
                child_window
            } else {
                dlu_rect
            };
            window(SETUP_CONTROL_X, y, SETUP_CONTROL_W, h)
        }),
        randomize: dlu_rect(74, 257, 83, 15),
        generate: dlu_rect(246, 257, 83, 15),
        // Display-only seed field.
        seed_field: dlu_rect(279, 287, 50, 12),
        title: place.at(LOW_HEADING_ANCHOR, 422, 1, 108, 10),
        preview: place.at(AnchorRule::RightAnchor, 430, 23, 96, 69),
        use_map,
        load,
        save,
        delete,
        cancel: place.rule(BACK_BUTTON),
        advanced,
        export,
        fair_starts,
        preset,
        advanced_label_rects: SETUP_ADVANCED_ROW_Y
            .map(|y| dlu_rect(SETUP_LABEL_X, y - 1, SETUP_LABEL_W, SETUP_TRACKBAR_H + 2)),
        advanced_control_rects: SETUP_ADVANCED_ROW_Y
            .map(|y| child_window(SETUP_CONTROL_X, y, SETUP_CONTROL_W, SETUP_TRACKBAR_H)),
        status_help: place.at(AnchorRule::StatusLine, 2, 355, 303, 12),
        // Hidden until the generate block runs.
        progress_text: dlu_rect(74, 219, 150, 11),
        progress_bar: dlu_rect(229, 217, 100, 21),
        urban_areas: dlu_rect(177, 192, 150, 14),
    }
}

/// The open list of a combo row: directly under the face, as tall as its
/// entries. The resource reserves a taller extent per combo, but a Windows
/// dropdown sizes itself to the item count up to that reserve, and none of
/// these lists come close to it.
pub fn random_map_setup_dropdown_rect(
    layout: &RandomMapSetupLayout,
    row: usize,
    item_count: usize,
) -> RectPx {
    let face = combo_face_rect(layout.control_rects[row], layout.scale);
    let top = face.y + face.h;
    let mut bottom = top + DropdownMetrics::scaled(layout.scale).row_h * item_count as i32;
    // VERA: a list longer than the original's four rows (Archipelago, the added
    // theaters) can end inside a lower row's text; it then runs on to that
    // text's bottom so the row is hidden whole, never cut. The original lists
    // never end inside one, so they keep their height.
    for (index, &control) in layout.control_rects.iter().enumerate().skip(row + 1) {
        let text = if index + 1 == layout.control_rects.len() {
            crate::ui::shell::trackbar::value_text_rect(control, layout.scale)
        } else {
            combo_text_rect(control, layout.scale)
        };
        if text.y < bottom && bottom < text.y + text.h {
            bottom = text.y + text.h;
        }
    }
    RectPx::new(face.x, top, face.w, bottom - top)
}

// Saved-seed browser (Load 0xB7 / Save 0x2B4 / Delete 0x2B5). All three share
// the 533x369 frame, the right column and the bottom status line with the
// chooser; only the list extent, the prompt and the action button differ.
/// List rects per mode: Load, Save, Delete. Save's is shorter to leave room for
/// the file-name edit beneath it.
const SEED_LIST_RECTS: [(i32, i32, i32, i32); 3] =
    [(79, 78, 266, 187), (79, 80, 266, 157), (78, 76, 266, 187)];
/// Prompt static above each list.
const SEED_PROMPT_RECTS: [(i32, i32, i32, i32); 3] =
    [(79, 44, 266, 24), (79, 46, 266, 24), (78, 42, 266, 24)];
/// The file-name edit, Save only.
const SEED_NAME_EDIT_RECT: (i32, i32, i32, i32) = (80, 253, 266, 14);
/// Load and Delete put the status line one pixel lower than Save does.
const SEED_BLANK_RECTS: [(i32, i32, i32, i32); 3] =
    [(2, 355, 303, 12), (2, 356, 303, 12), (2, 355, 303, 12)];

/// Which of the three saved-seed dialogs is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedSeedMode {
    Load,
    Save,
    Delete,
}

impl SavedSeedMode {
    const fn index(self) -> usize {
        match self {
            Self::Load => 0,
            Self::Save => 1,
            Self::Delete => 2,
        }
    }

    /// Caption key and English fallback for the action button.
    pub const fn action_label(self) -> (&'static str, &'static str) {
        match self {
            Self::Load => ("GUI:Load", "Load"),
            Self::Save => ("GUI:Save", "Save"),
            Self::Delete => ("GUI:Delete", "Delete"),
        }
    }

    /// Caption key and fallback for the top-right title.
    pub const fn title_label(self) -> (&'static str, &'static str) {
        match self {
            Self::Load => ("GUI:LoadMapMenu", "Load Map"),
            Self::Save => ("GUI:SaveMapMenu", "Save Map"),
            Self::Delete => ("GUI:DeleteMapMenu", "Delete Map"),
        }
    }

    /// Caption key and fallback for the prompt above the list.
    pub const fn prompt_label(self) -> (&'static str, &'static str) {
        match self {
            Self::Load => ("GUI:SelectMission", "Select a mission"),
            Self::Save => ("GUI:SaveMission", "Save mission"),
            Self::Delete => ("GUI:DeleteMission", "Delete mission"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedSeedLayout {
    /// The 800x600 -> window mapping these rects went through.
    pub scale: ShellScale,
    pub screen: RectPx,
    pub dialog: RectPx,
    pub title: RectPx,
    pub prompt: RectPx,
    pub list: RectPx,
    /// Present only in Save mode.
    pub name_edit: Option<RectPx>,
    pub action: RectPx,
    /// VERA's Setups page only: Load and Delete under the action, Save.
    pub load: Option<RectPx>,
    pub delete: Option<RectPx>,
    pub back: RectPx,
    pub blank: RectPx,
}

pub fn compute_saved_seed_layout(
    mode: SavedSeedMode,
    screen_w: u32,
    screen_h: u32,
) -> SavedSeedLayout {
    let (screen_w, screen_h) = (screen_w as i32, screen_h as i32);
    let place = Placer::new(screen_w, screen_h);
    let index = mode.index();
    let rect_of = |(x, y, w, h): (i32, i32, i32, i32)| dlu_rect(x, y, w, h);

    SavedSeedLayout {
        scale: ShellScale::RETAIL,
        screen: RectPx::new(0, 0, screen_w, screen_h),
        dialog: RectPx::new(0, 0, screen_w, screen_h),
        title: dlu_rect(425, 1, 108, 10),
        prompt: rect_of(SEED_PROMPT_RECTS[index]),
        list: rect_of(SEED_LIST_RECTS[index]),
        name_edit: (mode == SavedSeedMode::Save).then(|| rect_of(SEED_NAME_EDIT_RECT)),
        // The action button sits where the chooser puts Use Map, but is one
        // DLU shorter than Back -- 22 against 23.
        action: place.at(BUTTON, 425, 122, 108, 22),
        load: None,
        delete: None,
        back: place.rule(BACK_BUTTON),
        blank: rect_of(SEED_BLANK_RECTS[index]),
    }
}

// --- Responsive (window-filling) variants -------------------------------
//
// Each layout scales the verified 800x600 dialog to fit the live window. The
// `ScaledRects` impls below are exhaustive struct literals, so a rect field
// added to a layout must be scaled too or the crate stops building. `screen`
// is the exception: it stays the whole window, as on the retail layout.

scaled_rects!(SkirmishRightPanelTextRects {
    title,
    game_type,
    map_label
});
scaled_rects!(SkirmishTrackbarRects {
    game_speed,
    credits,
    unit_count
});
scaled_rects!(SkirmishTrackbarLabelRects {
    game_speed,
    credits,
    unit_count
});
scaled_rects!(SkirmishColumnLabelRects {
    players,
    side,
    color,
    start,
    team
});
scaled_rects!(SkirmishRowRects {
    ai_type_combos,
    side_combos,
    start_combos,
    team_combos
});
scaled_rects!(SkirmishCheckboxRect { rect; keep: id });
scaled_rects!(SkirmishShellLayout {
    [scale, screen]
    right_panel, right_panel_text, start_button, choose_map_button, game_options_button,
    setups_button, back_button, map_preview, column_labels, player_name, rows, color_combos,
    flags, opponent_scrollbar, trackbars, trackbar_labels, checkboxes, status_help;
    keep: shown_rows
});
scaled_rects!(ChooseMapModalLayout {
    [scale, screen]
    dialog, mode_list, map_list, map_search, use_map_button, cancel_button,
    create_random_map_button, favorite_button, title, select_engagement, game_type_heading,
    game_map_heading, status_help, preview, author
});
scaled_rects!(RandomMapSetupLayout {
    [scale, screen]
    dialog, label_rects, control_rects, randomize, generate, seed_field, title, preview,
    use_map, load, save, delete, cancel, advanced, export, fair_starts, preset,
    advanced_label_rects, advanced_control_rects, status_help, progress_text, progress_bar,
    urban_areas
});
scaled_rects!(SavedSeedLayout {
    [scale, screen]
    dialog, title, prompt, list, name_edit, action, load, delete, back, blank
});

/// The `0x102` common shell on the mapping a page already holds: the retail
/// layout for [`ShellScale::RETAIL`], the responsive one otherwise. Pages drawn
/// over the common shell (Launcher Options, Keyboard) rebuild it through this,
/// so they cannot pick a different mapping than their own rects use.
pub fn compute_layout_on(scale: ShellScale, screen_w: u32, screen_h: u32) -> SkirmishShellLayout {
    if scale.is_retail() {
        compute_layout(screen_w, screen_h)
    } else {
        compute_layout(SHELL_BASE_W as u32, SHELL_BASE_H as u32).scaled_rects(scale)
    }
}

/// VERA's Setups page: the Save browser (list, name field, Save), with Load
/// and Delete on the two tiles under Save, above Back at 640x480 too.
pub fn compute_setups_layout(screen_w: u32, screen_h: u32) -> SavedSeedLayout {
    let mut layout = compute_saved_seed_layout(SavedSeedMode::Save, screen_w, screen_h);
    layout.load = Some(layout.action.translate(0, SDBTNBKGD_H));
    layout.delete = Some(layout.action.translate(0, 2 * SDBTNBKGD_H));
    layout
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::AdvancedField;
    use super::RandomMapSetupControl;
    use crate::ui::skirmish_shell::random_map_page::control_at as random_map_setup_control_at_view;

    /// The basic view's control the pointer at `x`, `y` hovers.
    fn random_map_setup_control_at(
        layout: &super::RandomMapSetupLayout,
        x: i32,
        y: i32,
        ares: bool,
    ) -> Option<RandomMapSetupControl> {
        random_map_setup_control_at_view(layout, x, y, false, false, ares, 0)
    }
    use super::{
        ChooseMapModalButton, RectPx, SavedSeedMode, ShellScale, SkirmishCheckboxId,
        checkbox_icon_rect, checkbox_text_rect, combo_arrow_rect, combo_face_rect,
        combo_swatch_rect, combo_text_rect, compute_choose_map_modal_layout,
        compute_fixed_800_layout, compute_layout, compute_random_map_setup_layout,
        compute_saved_seed_layout, dlu_rect, player_name_edit_client_rect,
        player_name_edit_text_rect,
    };
    use crate::ui::shell::geom::fit_window;

    /// The chooser button the pointer at `x`, `y` hovers on its widgets.
    fn choose_map_modal_button_at(
        layout: &super::ChooseMapModalLayout,
        x: i32,
        y: i32,
    ) -> Option<ChooseMapModalButton> {
        use crate::ui::skirmish_shell::{ChooseMapHoverTarget, choose_map_page};
        let modes = crate::skirmish_modes::stock_skirmish_modes();
        let modal = crate::ui::skirmish_shell::ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            None,
            &modes,
            &[],
            layout,
        );
        let mut ui = crate::ui::shell::controller::DialogController::default();
        choose_map_page::sync_page(&mut ui, &modal, layout);
        ui.on_pointer_move(x, y);
        match choose_map_page::hover_target(ui.gui(), &modal, layout, x, y) {
            Some(ChooseMapHoverTarget::Button(button)) => Some(button),
            _ => None,
        }
    }

    #[test]
    fn responsive_layouts_keep_the_window_as_screen_and_centre_the_shell() {
        // 16:9: the height limits the factor to 1.8, so the 1440x1080 shell sits
        // 240 px in from either side.
        let shell = fit_window(1920, 1080, compute_layout);
        assert_eq!(shell.screen, RectPx::new(0, 0, 1920, 1080));
        assert_eq!(shell.shell_origin(), (240, 0));
        assert_eq!(shell.shell_height(), 1080);
        // The seed prompts and the eject box centre in `screen`, as their hit
        // tests centre in the window.
        let seeds = fit_window(1920, 1080, |w, h| {
            compute_saved_seed_layout(SavedSeedMode::Save, w, h)
        });
        assert_eq!(seeds.screen, RectPx::new(0, 0, 1920, 1080));
        let chooser = fit_window(1920, 1080, compute_choose_map_modal_layout);
        assert_eq!(chooser.screen, RectPx::new(0, 0, 1920, 1080));
        // Its own full-size dialog is the centred shell, not the window.
        assert_eq!(chooser.dialog, RectPx::new(240, 0, 1440, 1080));
    }

    /// VERA's Setups page puts Load and Delete on the two tiles under Save,
    /// above Back on the smallest window too.
    #[test]
    fn setups_page_stacks_load_and_delete_under_save_above_back() {
        use super::{SDBTNBKGD_H, compute_setups_layout};
        for layout in [
            compute_setups_layout(640, 480),
            compute_setups_layout(800, 600),
        ] {
            let load = layout.load.unwrap();
            let delete = layout.delete.unwrap();
            assert_eq!(load, layout.action.translate(0, SDBTNBKGD_H));
            assert_eq!(delete, load.translate(0, SDBTNBKGD_H));
            assert!(delete.y + delete.h <= layout.back.y, "{layout:?}");
        }
        // The retail browsers have neither.
        let seeds = compute_saved_seed_layout(SavedSeedMode::Save, 800, 600);
        assert_eq!((seeds.load, seeds.delete), (None, None));
    }

    #[test]
    fn key_rects_match_800x600() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.start_button, RectPx::new(644, 241, 156, 42));
        // VERA's Game Options takes gamemd's Choose Map tile; Choose Map
        // moves one down.
        assert_eq!(layout.game_options_button, RectPx::new(644, 283, 156, 42));
        assert_eq!(layout.choose_map_button, RectPx::new(644, 325, 156, 42));
        assert_eq!(layout.map_preview, RectPx::new(644, 37, 144, 112));
        assert_eq!(layout.back_button, RectPx::new(644, 535, 156, 42));
    }

    #[test]
    fn fixed_800_layout_centers_native_shell_without_rescaling() {
        let layout = compute_fixed_800_layout(1024, 768);

        assert_eq!(layout.screen, RectPx::new(112, 84, 800, 600));
        assert_eq!(layout.start_button, RectPx::new(756, 325, 156, 42));
        assert_eq!(layout.game_options_button, RectPx::new(756, 367, 156, 42));
        assert_eq!(layout.choose_map_button, RectPx::new(756, 409, 156, 42));
        assert_eq!(layout.map_preview, RectPx::new(756, 121, 144, 112));
        assert_eq!(layout.back_button, RectPx::new(756, 619, 156, 42));
        assert_eq!(layout.column_labels.players, RectPx::new(171, 118, 146, 16));
        assert_eq!(
            layout.rows.ai_type_combos[0],
            RectPx::new(171, 169, 150, 120)
        );
    }

    #[test]
    fn key_rects_match_640x480_formula() {
        let layout = compute_layout(640, 480);
        assert_eq!(layout.start_button, RectPx::new(484, 241, 156, 42));
        // VERA's Game Options takes gamemd's Choose Map tile; Choose Map
        // moves one down.
        assert_eq!(layout.game_options_button, RectPx::new(484, 283, 156, 42));
        assert_eq!(layout.choose_map_button, RectPx::new(484, 325, 156, 42));
        // VERA's Setups takes the last tile above Back.
        assert_eq!(layout.setups_button, RectPx::new(484, 367, 156, 42));
        assert_eq!(layout.map_preview, RectPx::new(484, 37, 144, 112));
        assert_eq!(layout.back_button, RectPx::new(484, 409, 156, 42));
    }

    #[test]
    fn represented_0102_player_name_one_pixel_fixup_is_applied() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.player_name, RectPx::new(58, 59, 151, 23));
    }

    #[test]
    fn player_name_edit_text_rect_uses_verified_client_and_text_insets() {
        let layout = compute_layout(800, 600);
        assert_eq!(
            player_name_edit_client_rect(layout.player_name, ShellScale::RETAIL),
            RectPx::new(59, 60, 149, 21)
        );
        assert_eq!(
            player_name_edit_text_rect(layout.player_name, ShellScale::RETAIL),
            RectPx::new(61, 60, 147, 21)
        );
    }

    #[test]
    fn color_combos_and_flags_do_not_right_anchor() {
        let layout_800 = compute_layout(800, 600);
        let layout_1024 = compute_layout(1024, 768);
        assert_eq!(layout_800.color_combos, layout_1024.color_combos);
        assert_eq!(layout_800.flags, layout_1024.flags);
        assert_eq!(layout_800.color_combos[0], RectPx::new(423, 59, 44, 119));
        assert_eq!(layout_800.flags[0], RectPx::new(225, 59, 48, 20));
    }

    #[test]
    fn a_map_past_eight_starts_grows_the_rows_into_the_room_under_the_options() {
        use super::{COMBO_FACE_H, SKIRMISH_LONG_ROW_COUNT, compute_layout_for_rows};
        let retail = compute_layout(800, 600);
        assert_eq!(compute_layout_for_rows(800, 600, 8), retail);
        let ten = compute_layout_for_rows(800, 600, 10).shown_rows;
        let thirty = compute_layout_for_rows(800, 600, 30);
        assert_eq!(
            (retail.shown_rows, ten, thirty.shown_rows),
            (8, 10, SKIRMISH_LONG_ROW_COUNT)
        );
        // The rows keep the template's pitch; the options move down five rows,
        // below the thirteenth row and above the status line.
        let pitch = retail.rows.side_combos[1].y - retail.rows.side_combos[0].y;
        assert_eq!(
            thirty.rows.side_combos[12].y,
            retail.rows.side_combos[7].y + 5 * pitch
        );
        let moved = |rect: RectPx| rect.translate(0, 5 * pitch);
        for (long, short) in thirty.checkboxes.iter().zip(retail.checkboxes) {
            assert_eq!(long.rect, moved(short.rect));
        }
        assert_eq!(
            thirty.trackbars.unit_count,
            moved(retail.trackbars.unit_count)
        );
        assert_eq!(
            thirty.trackbar_labels.credits,
            moved(retail.trackbar_labels.credits)
        );
        let last_row_bottom = thirty.rows.side_combos[12].y + COMBO_FACE_H;
        assert!(last_row_bottom < thirty.trackbars.game_speed.y);
        let options_bottom = thirty
            .checkboxes
            .iter()
            .map(|c| c.rect.y + c.rect.h)
            .max()
            .unwrap();
        assert!(options_bottom < thirty.status_help.y);
    }

    /// VERA's Game Options sits second in every panel, under Start Game, with
    /// Choose Map and VERA's Setups a tile apart under it and above Back: on
    /// the smallest window, 640x480, and on a page grown to thirteen rows.
    #[test]
    fn game_options_sits_second_in_every_panel() {
        use super::{SDBTNBKGD_H, compute_layout_for_rows};
        for layout in [
            compute_layout(640, 480),
            compute_layout_for_rows(800, 600, 30),
        ] {
            let column = [
                layout.start_button,
                layout.game_options_button,
                layout.choose_map_button,
                layout.setups_button,
            ];
            for pair in column.windows(2) {
                assert_eq!(pair[1], pair[0].translate(0, SDBTNBKGD_H));
            }
            let setups = layout.setups_button;
            assert!(setups.y + setups.h <= layout.back_button.y);
        }
    }

    #[test]
    fn row_combo_rects_match_800x600_resource_geometry() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.column_labels.players, RectPx::new(59, 34, 146, 16));
        assert_eq!(layout.rows.ai_type_combos[0], RectPx::new(59, 85, 150, 120));
        assert_eq!(layout.rows.side_combos[0], RectPx::new(287, 59, 117, 120));
        assert_eq!(layout.color_combos[0], RectPx::new(423, 59, 44, 119));
        assert_eq!(layout.rows.start_combos[0], RectPx::new(486, 59, 38, 119));
        assert_eq!(layout.rows.team_combos[0], RectPx::new(546, 59, 38, 119));
    }

    #[test]
    fn setup_left_column_matches_800x600_resource_geometry() {
        let setup = compute_random_map_setup_layout(800, 600);
        // Rows: map type, time, theater, size, resources, players.
        assert_eq!(setup.control_rects[0], dlu_rect(179, 41, 150, 103));
        assert_eq!(
            setup.control_rects[1],
            dlu_rect(179, 65, 150, 101),
            "the time combo's dropdown is two DLU shorter"
        );
        assert_eq!(setup.control_rects[2], dlu_rect(179, 90, 150, 103));
        assert_eq!(
            setup.control_rects[5],
            RectPx::new(269, 265, 226, 22),
            "players is a trackbar, in its runtime window"
        );
        assert_eq!(setup.randomize, dlu_rect(74, 257, 83, 15));
        assert_eq!(setup.generate, dlu_rect(246, 257, 83, 15));
        assert_eq!(setup.seed_field, dlu_rect(279, 287, 50, 12));
        assert_eq!(setup.progress_text, dlu_rect(74, 219, 150, 11));
        assert_eq!(setup.progress_bar, dlu_rect(229, 217, 100, 21));
    }

    /// VERA: the seven Advanced sliders sit at the option rows' pitch above
    /// the progress widgets, and the toggle takes the column slot below
    /// Delete; in the Advanced view the sliders replace the option rows.
    #[test]
    fn advanced_sliders_and_toggle_fit_the_dialog() {
        let setup = compute_random_map_setup_layout(800, 600);
        let sliders = setup.advanced_control_rects;
        for pair in sliders.windows(2) {
            assert!(pair[0].y + pair[0].h <= pair[1].y, "{pair:?} overlap");
        }
        assert!(sliders[6].y + sliders[6].h <= setup.progress_text.y);
        assert!(setup.advanced.y >= setup.delete.y + setup.delete.h);
        assert!(setup.advanced.y + setup.advanced.h <= setup.export.y);
        assert!(setup.export.y + setup.export.h <= setup.cancel.y);
        assert!(setup.advanced.y + setup.advanced.h <= setup.cancel.y);
        assert!(setup.fair_starts.y >= setup.advanced.y + 2 * setup.advanced.h);
        assert!(setup.fair_starts.y + setup.fair_starts.h <= setup.preset.y);
        assert!(setup.preset.y + setup.preset.h <= setup.cancel.y);

        let water = sliders[AdvancedField::Water.row()];
        let (cx, cy) = (water.x + water.w / 2, water.y + water.h / 2);
        assert_eq!(
            random_map_setup_control_at_view(&setup, cx, cy, true, false, false, 0),
            Some(RandomMapSetupControl::Advanced(AdvancedField::Water))
        );
        assert_eq!(
            random_map_setup_control_at_view(&setup, cx, cy, true, true, false, 0),
            Some(RandomMapSetupControl::ExtrasRow(AdvancedField::Water.row())),
            "the Extras view takes the sliders' rows"
        );
        assert_eq!(
            random_map_setup_control_at_view(&setup, cx, cy, false, false, false, 0),
            Some(RandomMapSetupControl::MapType0x405),
            "the basic view keeps the original rows"
        );
        let toggle = (setup.advanced.x + 4, setup.advanced.y + 4);
        assert_eq!(
            random_map_setup_control_at_view(&setup, toggle.0, toggle.1, false, false, false, 0),
            Some(RandomMapSetupControl::AdvancedToggle)
        );
    }

    /// VERA: the Advanced scrollbar sits beside the sliders, its thumb moves
    /// between the two seven-row windows, and the Extras view keeps its rows.
    #[test]
    fn advanced_scrollbar_sits_beside_the_sliders_and_maps_to_pages() {
        use super::DropdownScrollbarPart;
        let setup = compute_random_map_setup_layout(800, 600);
        let bar = AdvancedField::advanced_scrollbar_rect(&setup);
        let sliders = setup.advanced_control_rects;
        assert!(bar.x > sliders[0].x + sliders[0].w);
        assert_eq!(bar.y, sliders[0].y);
        assert_eq!(bar.y + bar.h, sliders[6].y + sliders[6].h);
        let top = AdvancedField::advanced_scrollbar_thumb(&setup, 0);
        let bottom = AdvancedField::advanced_scrollbar_thumb(&setup, 1);
        assert!(top.y < bottom.y, "the thumb travels with the page");
        let (px, py) = (bar.x + bar.w / 2, bar.y + bar.h / 2);
        assert_eq!(
            random_map_setup_control_at_view(&setup, px, py, true, false, false, 0),
            Some(RandomMapSetupControl::AdvancedScrollbar)
        );
        assert_eq!(
            AdvancedField::advanced_scrollbar_part_at(&setup, 0, px, bar.y + 2),
            Some(DropdownScrollbarPart::UpArrow)
        );
        assert_eq!(
            AdvancedField::advanced_scrollbar_part_at(&setup, 0, px, bar.y + bar.h - 2),
            Some(DropdownScrollbarPart::DownArrow)
        );
        assert_eq!(
            AdvancedField::advanced_scrollbar_page_at_pointer(&setup, 0, bar.y + bar.h - 2),
            1
        );
    }

    #[test]
    fn setup_label_heights_are_not_uniform() {
        // The resource gives rows 0/1/5 a 14-DLU label and rows 2/3/4 a 12-DLU
        // one; a single shared height would make three rows too tall.
        let setup = compute_random_map_setup_layout(800, 600);
        assert_eq!(setup.label_rects[0], dlu_rect(74, 40, 93, 14));
        assert_eq!(setup.label_rects[1], dlu_rect(74, 64, 93, 14));
        assert_eq!(setup.label_rects[2], dlu_rect(74, 90, 93, 12));
        assert_eq!(setup.label_rects[4], dlu_rect(74, 138, 93, 12));
        assert_eq!(setup.label_rects[5], dlu_rect(74, 162, 93, 14));
    }

    #[test]
    fn setup_shares_the_choose_map_frame_and_right_column() {
        let choose = compute_choose_map_modal_layout(800, 600);
        let setup = compute_random_map_setup_layout(800, 600);

        assert_eq!(setup.screen, choose.screen);
        assert_eq!(setup.dialog, choose.dialog);
        // The 0x105 resource puts these 2-3 DLU left of 0x6B's (and the preview
        // 2 DLU right), but right_anchor / snap_button_biased_truncate / back_rect
        // are all panel-anchored and discard the source x, so the pixel output is
        // identical. Sharing the chrome is therefore safe.
        assert_eq!(setup.use_map, choose.use_map_button);
        assert_eq!(setup.cancel, choose.cancel_button);
        assert_eq!(setup.title, choose.title);
        assert_eq!(setup.preview, choose.preview);
        // Load reuses the slot 0x6B gives Create Random Map (both at y=149).
        assert_eq!(setup.load, choose.create_random_map_button);
    }

    #[test]
    fn setup_hit_test_returns_controls_in_draw_order() {
        let setup = compute_random_map_setup_layout(800, 600);
        let centre = |rect: RectPx| (rect.x + rect.w / 2, rect.y + 2);

        let (x, y) = centre(setup.control_rects[0]);
        assert_eq!(
            random_map_setup_control_at(&setup, x, y, false),
            Some(RandomMapSetupControl::MapType0x405)
        );
        let (x, y) = centre(setup.control_rects[5]);
        assert_eq!(
            random_map_setup_control_at(&setup, x, y, false),
            Some(RandomMapSetupControl::Players0x3eb)
        );
        let (x, y) = centre(setup.randomize);
        assert_eq!(
            random_map_setup_control_at(&setup, x, y, false),
            Some(RandomMapSetupControl::Randomize0x621)
        );
        let (x, y) = centre(setup.generate);
        assert_eq!(
            random_map_setup_control_at(&setup, x, y, false),
            Some(RandomMapSetupControl::Generate0x620)
        );
        assert_eq!(random_map_setup_control_at(&setup, 0, 0, false), None);
    }

    #[test]
    fn collapsed_combo_helpers_follow_owner_draw_constants() {
        let rect = RectPx::new(423, 59, 44, 119);

        assert_eq!(
            combo_face_rect(rect, ShellScale::RETAIL),
            RectPx::new(423, 59, 44, 24)
        );
        assert_eq!(
            combo_arrow_rect(rect, ShellScale::RETAIL),
            RectPx::new(448, 60, 0, 0)
        );
        assert_eq!(
            combo_text_rect(rect, ShellScale::RETAIL),
            RectPx::new(425, 59, 24, 24)
        );
        assert_eq!(
            combo_swatch_rect(rect, ShellScale::RETAIL),
            RectPx::new(425, 61, 20, 20)
        );
    }

    #[test]
    fn trackbar_windows_match_the_executed_relayout() {
        // One pixel wider and taller than the resource conversion, like every
        // family child in the retail stills; the frames and the plaque sit
        // around this window.
        use vera20k_gui::fixtures::executed_child_window as executed;
        for (w, h) in [(640, 480), (800, 600), (1024, 768)] {
            let layout = compute_layout(w as u32, h as u32);
            assert_eq!(layout.trackbars.game_speed, executed(0x102, 0x529, w, h));
            assert_eq!(layout.trackbars.credits, executed(0x102, 0x511, w, h));
            assert_eq!(layout.trackbars.unit_count, executed(0x102, 0x50C, w, h));
            let setup = compute_random_map_setup_layout(w as u32, h as u32);
            assert_eq!(setup.control_rects[5], executed(0x105, 0x3EB, w, h));
        }
    }

    #[test]
    fn option_label_static_rects_preserve_resource_positions() {
        for layout in [
            compute_layout(640, 480),
            compute_layout(800, 600),
            compute_layout(1024, 768),
        ] {
            assert_eq!(
                layout.trackbar_labels.game_speed,
                RectPx::new(302, 286, 90, 16)
            );
            assert_eq!(
                layout.trackbar_labels.credits,
                RectPx::new(302, 314, 90, 16)
            );
            assert_eq!(
                layout.trackbar_labels.unit_count,
                RectPx::new(302, 341, 90, 16)
            );
        }
    }

    #[test]
    fn right_panel_statics_and_status_line_match_the_executed_relayout() {
        use vera20k_gui::fixtures::executed_child_window as executed;
        for (w, h) in [(640, 480), (800, 600), (1024, 768)] {
            let layout = compute_layout(w as u32, h as u32);
            assert_eq!(layout.right_panel_text.title, executed(0x102, 0x694, w, h));
            assert_eq!(
                layout.right_panel_text.game_type,
                executed(0x102, 0x6EC, w, h)
            );
            assert_eq!(
                layout.right_panel_text.map_label,
                executed(0x102, 0x5A8, w, h)
            );
            assert_eq!(layout.status_help, executed(0x102, 0x695, w, h));
        }
        let chooser = compute_choose_map_modal_layout(800, 600);
        assert_eq!(chooser.title, executed(0x6B, 0x694, 800, 600));
        assert_eq!(chooser.status_help, executed(0x6B, 0x695, 800, 600));
        for (w, h) in [(800, 600), (1024, 768)] {
            let setup = compute_random_map_setup_layout(w as u32, h as u32);
            assert_eq!(setup.title, executed(0x105, 0x694, w, h));
            assert_eq!(setup.status_help, executed(0x105, 0x695, w, h));
        }
    }

    #[test]
    fn checkbox_rects_match_800x600_final_geometry() {
        let layout = compute_layout(800, 600);
        assert_eq!(layout.checkboxes.len(), 5);
        assert_eq!(layout.checkboxes[0].id, SkirmishCheckboxId::ShortGame0x54e);
        assert_eq!(layout.checkboxes[0].rect, RectPx::new(71, 286, 150, 16));
        assert_eq!(layout.checkboxes[1].id, SkirmishCheckboxId::McvRepacks0x693);
        assert_eq!(layout.checkboxes[1].rect, RectPx::new(71, 314, 150, 16));
        assert_eq!(
            layout.checkboxes[2].id,
            SkirmishCheckboxId::CratesAppear0x696
        );
        assert_eq!(layout.checkboxes[2].rect, RectPx::new(71, 341, 150, 16));
        assert_eq!(
            layout.checkboxes[3].id,
            SkirmishCheckboxId::SuperWeapons0x69a
        );
        assert_eq!(layout.checkboxes[3].rect, RectPx::new(71, 371, 155, 16));
        assert_eq!(
            layout.checkboxes[4].id,
            SkirmishCheckboxId::BuildOffAlly0x69d
        );
        assert_eq!(layout.checkboxes[4].rect, RectPx::new(302, 369, 249, 18));
    }

    #[test]
    fn checkbox_icon_and_text_rects_follow_owner_draw_constants() {
        let rect = RectPx::new(71, 286, 150, 16);

        assert_eq!(
            checkbox_icon_rect(rect, ShellScale::RETAIL),
            RectPx::new(71, 286, 18, 18)
        );
        assert_eq!(checkbox_text_rect(rect, ShellScale::RETAIL).x, rect.x + 26);
    }

    #[test]
    fn right_panel_globals_match_research_modes() {
        let a = compute_layout(800, 600);
        assert_eq!(a.right_panel.top, RectPx::new(632, 0, 168, 199));
        assert_eq!(a.right_panel.tile, RectPx::new(632, 199, 168, 42));
        assert_eq!(a.right_panel.tile_count, 9);
        assert_eq!(a.right_panel.bottom, RectPx::new(632, 577, 168, 23));

        let b = compute_layout(1024, 768);
        assert_eq!(b.right_panel.top, RectPx::new(744, 84, 168, 199));
        assert_eq!(b.right_panel.tile, RectPx::new(744, 283, 168, 42));
        assert_eq!(b.right_panel.tile_count, 9);
        assert_eq!(b.right_panel.bottom, RectPx::new(744, 661, 168, 23));

        let c = compute_layout(640, 480);
        assert_eq!(c.right_panel.top, RectPx::new(472, 0, 168, 199));
        assert_eq!(c.right_panel.tile, RectPx::new(472, 199, 168, 42));
        assert_eq!(c.right_panel.tile_count, 6);
        assert_eq!(c.right_panel.bottom, RectPx::new(472, 451, 168, 29));
    }

    #[test]
    fn choose_map_modal_layout_matches_verified_0x6b_geometry() {
        let layout = compute_choose_map_modal_layout(800, 600);

        assert_eq!(layout.screen, RectPx::new(0, 0, 800, 600));
        assert_eq!(layout.dialog, RectPx::new(0, 0, 800, 600));
        // VERA: 20 px wider than `0x6B`'s 196, to the left.
        assert_eq!(layout.mode_list, RectPx::new(96, 127, 216, 344));
        // VERA: the search box takes the top 20 px of `0x6B`'s 344 px list.
        assert_eq!(layout.map_list, RectPx::new(338, 147, 216, 324));
        assert_eq!(layout.use_map_button, RectPx::new(644, 199, 156, 42));
        assert_eq!(
            layout.create_random_map_button,
            RectPx::new(644, 241, 156, 42)
        );
        assert_eq!(layout.cancel_button, RectPx::new(644, 535, 156, 42));
        assert_eq!(layout.title, RectPx::new(635, 3, 163, 17));
        assert_eq!(layout.select_engagement, RectPx::new(120, 33, 386, 20));
        assert_eq!(layout.game_type_heading, RectPx::new(96, 98, 215, 16));
        assert_eq!(layout.game_map_heading, RectPx::new(338, 98, 215, 16));
        // The box's top is the game-type list's; its frame meets the map list's.
        assert_eq!(layout.map_search, RectPx::new(338, 127, 216, 17));
        assert_eq!(layout.favorite_button, RectPx::new(644, 283, 156, 42));
        assert_eq!(layout.status_help, RectPx::new(10, 578, 456, 21));
        assert_eq!(layout.preview, RectPx::new(644, 37, 144, 112));
    }

    #[test]
    fn choose_map_modal_high_res_preserves_lists_and_offsets_shell_helpers() {
        let layout = compute_choose_map_modal_layout(1024, 768);

        assert_eq!(layout.screen, RectPx::new(0, 0, 1024, 768));
        assert_eq!(layout.dialog, RectPx::new(0, 0, 1024, 768));
        // VERA: 20 px wider than `0x6B`'s 196, to the left.
        assert_eq!(layout.mode_list, RectPx::new(96, 127, 216, 344));
        assert_eq!(layout.map_list, RectPx::new(338, 147, 216, 324));
        assert_eq!(layout.use_map_button, RectPx::new(756, 283, 156, 42));
        assert_eq!(
            layout.create_random_map_button,
            RectPx::new(756, 325, 156, 42)
        );
        assert_eq!(layout.cancel_button, RectPx::new(756, 619, 156, 42));
        assert_eq!(layout.title, RectPx::new(747, 87, 163, 17));
        assert_eq!(layout.select_engagement, RectPx::new(120, 33, 386, 20));
        assert_eq!(layout.game_type_heading, RectPx::new(96, 98, 215, 16));
        assert_eq!(layout.game_map_heading, RectPx::new(338, 98, 215, 16));
        assert_eq!(layout.status_help, RectPx::new(122, 662, 456, 21));
        assert_eq!(layout.preview, RectPx::new(756, 121, 144, 112));
    }

    /// VERA: the author line sits in the top panel art between the preview
    /// and Use Map, wherever the right panel goes.
    #[test]
    fn choose_map_author_line_fits_between_the_preview_and_use_map() {
        for (w, h) in [(800, 600), (1024, 768)] {
            let chooser = compute_choose_map_modal_layout(w, h);
            let top = compute_layout(w, h).right_panel.top;
            let author = chooser.author;
            assert!(author.y >= chooser.preview.y + chooser.preview.h);
            assert!(author.y + author.h <= chooser.use_map_button.y);
            assert!(author.x >= top.x && author.x + author.w <= top.x + top.w);
        }
    }

    #[test]
    fn choose_map_modal_button_hit_test_uses_control_rects() {
        let layout = compute_choose_map_modal_layout(800, 600);

        assert_eq!(
            choose_map_modal_button_at(&layout, layout.use_map_button.x, layout.use_map_button.y),
            Some(ChooseMapModalButton::UseMap0x6c5)
        );
        assert_eq!(
            choose_map_modal_button_at(&layout, layout.cancel_button.x, layout.cancel_button.y),
            Some(ChooseMapModalButton::Cancel0x5c0)
        );
        assert_eq!(
            choose_map_modal_button_at(
                &layout,
                layout.create_random_map_button.x,
                layout.create_random_map_button.y
            ),
            Some(ChooseMapModalButton::CreateRandomMap0x583)
        );
        assert_eq!(
            choose_map_modal_button_at(&layout, layout.dialog.x, layout.dialog.y),
            None
        );
        assert_eq!(
            choose_map_modal_button_at(&layout, layout.dialog.x + 374, layout.dialog.y + 80),
            None
        );
        assert_eq!(
            choose_map_modal_button_at(&layout, layout.dialog.x + 374, layout.dialog.y + 116),
            None
        );
        assert_eq!(
            choose_map_modal_button_at(&layout, layout.dialog.x + 374, layout.dialog.y + 152),
            None
        );
    }

    /// A 2x stretch is exact, so every responsive rect is the base one doubled.
    fn double(rect: RectPx) -> RectPx {
        RectPx::new(rect.x * 2, rect.y * 2, rect.w * 2, rect.h * 2)
    }

    #[test]
    fn responsive_layouts_are_identity_at_the_base_size() {
        assert_eq!(
            fit_window(800, 600, compute_layout),
            compute_layout(800, 600)
        );
        assert_eq!(
            fit_window(800, 600, compute_choose_map_modal_layout),
            compute_choose_map_modal_layout(800, 600)
        );
        assert_eq!(
            fit_window(800, 600, compute_random_map_setup_layout),
            compute_random_map_setup_layout(800, 600)
        );
        for mode in [
            SavedSeedMode::Load,
            SavedSeedMode::Save,
            SavedSeedMode::Delete,
        ] {
            assert_eq!(
                fit_window(800, 600, |w, h| compute_saved_seed_layout(mode, w, h)),
                compute_saved_seed_layout(mode, 800, 600)
            );
        }
    }

    #[test]
    fn responsive_skirmish_layout_doubles_the_verified_base_geometry() {
        let base = compute_layout(800, 600);
        let layout = fit_window(1600, 1200, compute_layout);

        assert_eq!(layout.scale, ShellScale::from_screen(1600, 1200));
        assert_eq!(layout.screen, RectPx::new(0, 0, 1600, 1200));
        for (rect, base_rect) in [
            (layout.start_button, base.start_button),
            (layout.choose_map_button, base.choose_map_button),
            (layout.back_button, base.back_button),
            (layout.map_preview, base.map_preview),
            (layout.player_name, base.player_name),
            (layout.status_help, base.status_help),
            (layout.right_panel_text.title, base.right_panel_text.title),
            (layout.column_labels.players, base.column_labels.players),
            (layout.rows.side_combos[0], base.rows.side_combos[0]),
            (layout.rows.team_combos[7], base.rows.team_combos[7]),
            (layout.color_combos[0], base.color_combos[0]),
            (layout.flags[0], base.flags[0]),
            (layout.trackbars.game_speed, base.trackbars.game_speed),
            (
                layout.trackbar_labels.unit_count,
                base.trackbar_labels.unit_count,
            ),
            (layout.checkboxes[0].rect, base.checkboxes[0].rect),
        ] {
            assert_eq!(rect, double(base_rect));
        }
        // The panel keeps its chrome art whole: flush to the window's right edge
        // and twice as tall, with the tile count unchanged for the painter.
        assert_eq!(layout.right_panel.top.x + layout.right_panel.top.w, 1600);
        assert_eq!(layout.right_panel.top.h, 199 * 2);
        assert_eq!(layout.right_panel.tile_count, base.right_panel.tile_count);
    }

    #[test]
    fn responsive_hit_tests_follow_the_stretched_rects() {
        let base = compute_random_map_setup_layout(800, 600);
        let setup = fit_window(1600, 1200, compute_random_map_setup_layout);
        let face = setup.control_rects[0];

        assert_eq!(
            random_map_setup_control_at(&setup, face.x + face.w / 2, face.y + 2, false),
            Some(RandomMapSetupControl::MapType0x405)
        );
        // The base dialog's own map-type spot is empty chrome once stretched: a
        // click there must not still land on the control.
        let base_face = base.control_rects[0];
        assert_eq!(
            random_map_setup_control_at(&setup, base_face.x + 5, base_face.y + 2, false),
            None
        );
    }
}
