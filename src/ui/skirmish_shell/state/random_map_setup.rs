//! Random-map setup modal state (the Create Random Map dialog).
//!
//! Render-agnostic: owns the working option set, the enable/disable state
//! machine, and the accept/cancel outcome. Depends on the rmg options model and
//! the layout control enum only — no assets, no wgpu.

use crate::map::rmg::options::RmgOptions;
use crate::map::rmg::preview::PreviewImage;
#[cfg(not(feature = "ares"))]
use crate::map::rmg::randomize::randomize;
#[cfg(feature = "ares")]
use crate::map::rmg::randomize::randomize_from_combos;
use crate::map::rmg::randomize::{RandomRanged, derive_from_map_type};
use crate::map::rmg::settings::RmgSettings;
use crate::ui::shell::combobox::ComboPress;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::static_reveal::DialogStatics;
use crate::ui::shell::trackbar::{TrackbarHold, TrackbarPress, TrackbarRange};

use super::super::layout::{AdvancedField, RandomMapSetupControl};
use super::choose_map::ChooseMapSelection;

/// Process Main as the dialog's `RandomRanged` source. The app supplies process
/// Main (the installed simulation's continuation, or the frontend before the
/// first scenario). Map remains sim-independent (F05), so this trait binding
/// lives with the UI adapter.
pub struct MainRngRanged(pub crate::sim::rng::MainRngDraws);

impl RandomRanged for MainRngRanged {
    fn ranged(&mut self, min: i32, max: i32) -> i32 {
        self.0.ranged(min, max)
    }

    #[cfg(feature = "ares")]
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }
}

/// Which combo is currently dropped open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCombo {
    MapType,
    Time,
    Theater,
    Size,
    Resources,
}

/// The combos in layout row order. Row 5 of the option block is the players
/// trackbar, which is not a combo and is absent here.
pub const SETUP_COMBO_ROWS: [SetupCombo; 5] = [
    SetupCombo::MapType,
    SetupCombo::Time,
    SetupCombo::Theater,
    SetupCombo::Size,
    SetupCombo::Resources,
];

impl SetupCombo {
    /// The combo a control id addresses, if it is one.
    pub const fn from_control(control: RandomMapSetupControl) -> Option<Self> {
        match control {
            RandomMapSetupControl::MapType0x405 => Some(Self::MapType),
            RandomMapSetupControl::Time0x3ea => Some(Self::Time),
            RandomMapSetupControl::Theater0x407 => Some(Self::Theater),
            RandomMapSetupControl::Size0x406 => Some(Self::Size),
            RandomMapSetupControl::Resources0x408 => Some(Self::Resources),
            _ => None,
        }
    }

    /// The control id this combo is.
    pub const fn control(self) -> RandomMapSetupControl {
        match self {
            Self::MapType => RandomMapSetupControl::MapType0x405,
            Self::Time => RandomMapSetupControl::Time0x3ea,
            Self::Theater => RandomMapSetupControl::Theater0x407,
            Self::Size => RandomMapSetupControl::Size0x406,
            Self::Resources => RandomMapSetupControl::Resources0x408,
        }
    }

    /// Index into the layout's option rows.
    pub const fn row(self) -> usize {
        match self {
            Self::MapType => 0,
            Self::Time => 1,
            Self::Theater => 2,
            Self::Size => 3,
            Self::Resources => 4,
        }
    }
}

/// One combo entry: the label to resolve, an English fallback for a missing
/// string, and the value the entry carries into its option field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupComboItem {
    pub key: &'static str,
    pub fallback: &'static str,
    pub value: i32,
}

const fn item(key: &'static str, fallback: &'static str, value: i32) -> SetupComboItem {
    SetupComboItem {
        key,
        fallback,
        value,
    }
}

/// Landform entries. The original lists values 1..4 only: archipelago (0)
/// is absent, though the generator builds it and the string table names it.
/// VERA lists it last in an offline Skirmish dialog, as it does the larger
/// sizes, so the original four keep their rows; network dialogs keep four.
#[cfg(not(feature = "ares"))]
const NATIVE_MAP_TYPE_ITEMS: usize = 4;
#[cfg(not(feature = "ares"))]
const MAP_TYPE_ITEMS: [SetupComboItem; 5] = [
    item("TXT_MAP_CONTINENT", "Continent", 1),
    item("TXT_MAP_TEAM_CONTINENTS", "Team Continents", 2),
    item("TXT_MAP_INLAND", "Inland", 3),
    item("TXT_MAP_MOUNTAINOUS", "Mountainous", 4),
    item("TXT_MAP_ARCHIPELAGO", "Archipelago", 0),
];
/// Ares' RMG_EnableArchipelago (Ares.dll 1006ACF0, hook 0x596FFE -> 0x597008)
/// starts the fill at the table's first label with item data 0, so every
/// dialog lists archipelago first.
#[cfg(feature = "ares")]
const NATIVE_MAP_TYPE_ITEMS: usize = 5;
#[cfg(feature = "ares")]
const MAP_TYPE_ITEMS: [SetupComboItem; 5] = [
    item("TXT_MAP_ARCHIPELAGO", "Archipelago", 0),
    item("TXT_MAP_CONTINENT", "Continent", 1),
    item("TXT_MAP_TEAM_CONTINENTS", "Team Continents", 2),
    item("TXT_MAP_INLAND", "Inland", 3),
    item("TXT_MAP_MOUNTAINOUS", "Mountainous", 4),
];

const TIME_ITEMS: [SetupComboItem; 4] = [
    item("TXT_TIME_MORNING", "Morning", 0),
    item("TXT_TIME_AFTERNOON", "Afternoon", 1),
    item("TXT_TIME_DUSK", "Dusk", 2),
    item("TXT_TIME_NIGHT", "Night", 3),
];

/// The original offers two theaters, labelled from the theater table's own
/// name strings rather than the dialog's string list. VERA also offers the
/// generator's URBAN and DESERT (`rmg::emit::theater_name` 2 and 3), which
/// the string table names. LUNAR is not offered: it authors no water, shore,
/// cliff or road tiles for the generator to place. Like the larger sizes,
/// the added theaters are offered only in an offline Skirmish dialog.
#[cfg(not(feature = "ares"))]
const NATIVE_THEATER_ITEMS: usize = 2;
#[cfg(not(feature = "ares"))]
const THEATER_ITEMS: [SetupComboItem; 4] = [
    item("Name:Temperate", "Temperate", 0),
    item("Name:Snow", "Snow", 1),
    item("Name:Urban", "Urban", 2),
    item("TXT_BIOME_DESERT", "Desert", 3),
];
/// Ares' RMG_EnableDesert (Ares.dll 1006AD10, hook 0x5970EA) adds
/// `Name:Desert` with item data 3 after the two native entries, in every
/// dialog; the offline Skirmish dialog adds URBAN after it.
#[cfg(feature = "ares")]
const NATIVE_THEATER_ITEMS: usize = 3;
#[cfg(feature = "ares")]
const THEATER_ITEMS: [SetupComboItem; 4] = [
    item("Name:Temperate", "Temperate", 0),
    item("Name:Snow", "Snow", 1),
    item("Name:Desert", "Desert", 3),
    item("Name:Urban", "Urban", 2),
];

/// The original's four sizes, then VERA's larger ones (`rmg::EXTENDED_SIZES`).
const SIZE_ITEMS: [SetupComboItem; 8] = [
    item("TXT_MAPSIZE_SMALL", "Small", 0),
    item("TXT_MAPSIZE_MEDIUM", "Medium", 1),
    item("TXT_MAPSIZE_LARGE", "Large", 2),
    item("TXT_MAPSIZE_VERY_LARGE", "Very Large", 3),
    item("VERA:MapSizeHuge", "Huge", 4),
    item("VERA:MapSizeGiant", "Giant", 5),
    item("VERA:MapSizeEnormous", "Enormous", 6),
    item("VERA:MapSizeColossal", "Colossal", 7),
];

const RESOURCE_ITEMS: [SetupComboItem; 4] = [
    item("TXT_RESOURCE_LOW", "Low", 0),
    item("TXT_RESOURCE_MODERATE", "Moderate", 1),
    item("TXT_RESOURCE_HIGH", "High", 2),
    item("TXT_RESOURCE_EXTREME", "Extreme", 3),
];

/// The entries a combo is filled with, in list order. Size lists VERA's larger
/// maps only for an `extended` (offline Skirmish) dialog.
pub fn setup_combo_items(combo: SetupCombo, extended: bool) -> &'static [SetupComboItem] {
    match combo {
        SetupCombo::MapType if extended => &MAP_TYPE_ITEMS,
        SetupCombo::MapType => &MAP_TYPE_ITEMS[..NATIVE_MAP_TYPE_ITEMS],
        SetupCombo::Time => &TIME_ITEMS,
        SetupCombo::Theater if extended => &THEATER_ITEMS,
        SetupCombo::Theater => &THEATER_ITEMS[..NATIVE_THEATER_ITEMS],
        SetupCombo::Size if extended => &SIZE_ITEMS,
        SetupCombo::Size => &SIZE_ITEMS[..=crate::map::rmg::NATIVE_MAX_SIZE_OPTION as usize],
        SetupCombo::Resources => &RESOURCE_ITEMS,
    }
}

/// What closing the dialog should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome {
    /// Accepted: commit these options.
    Commit(Box<RmgOptions>),
    /// Rejected because nothing has been generated yet.
    NeedsGenerate,
}

/// The Players slider's range, 2..8 in steps of 1 (`TBM_SETRANGE` at
/// `0x0059722C`; no `0x4AB`).
pub const PLAYERS_RANGE: TrackbarRange =
    TrackbarRange::new(2, crate::map::rmg::NATIVE_MAX_PLAYERS, 1);
/// VERA extension: an offline Skirmish dialog's slider runs to thirty players.
pub const EXTENDED_PLAYERS_RANGE: TrackbarRange =
    TrackbarRange::new(2, crate::map::rmg::MAX_PLAYERS, 1);

/// The Create Random Map dialog.
#[derive(Debug, Clone)]
pub struct RandomMapSetupModalState {
    /// The working option set the controls edit.
    pub options: RmgOptions,
    /// True once Generate has run and no option has changed since.
    pub generated: bool,
    /// True while the synchronous generate block owns the dialog.
    pub generating: bool,
    /// Load/Delete enablement. Starts at saved-seed availability, but the
    /// generate action turns it on unconditionally, as the original does.
    pub saved_seed_buttons_enabled: bool,
    pub open_combo: Option<SetupCombo>,
    /// The Players slider holding the mouse since a press on it.
    pub players_hold: Option<TrackbarHold>,
    pub pressed_control: Option<RandomMapSetupControl>,
    /// The rasterised preview of the last generated map, shown in the preview
    /// box. `None` until a generate has produced one; any option edit clears it,
    /// because it no longer describes the configured map.
    pub generated_preview: Option<PreviewImage>,
    /// Bumped every time a generate produces a preview. The renderer keys its
    /// cached texture on this so it uploads once per generate rather than once
    /// per frame, and so a regenerated map of identical size still refreshes.
    pub preview_generation: u32,
    /// Restored verbatim if the player cancels.
    pub previous_selection: Option<ChooseMapSelection>,
    /// Heading `0x694` and status line `0x695`, alive while the seed browser
    /// hides the dialog.
    pub(crate) statics: DialogStatics,
    /// VERA extension: offline Skirmish offers more than eight players and the
    /// sizes past Very Large. A LAN or online dialog keeps the original limits,
    /// which every peer and the lobby accept.
    pub extended: bool,
    /// VERA extension (offline Skirmish): the option rows show the Advanced
    /// sliders instead of the original controls.
    pub advanced: bool,
    /// VERA extension: the Advanced values the player set, by
    /// [`AdvancedField::row`]. `None` until a slider moves; Surprise Me clears
    /// them. While set they replace the map-type-derived values that every
    /// Generate re-rolls, after the re-roll's draws.
    pub advanced_values: Option<[i32; 7]>,
    /// An Advanced slider holding the mouse since a press on it.
    pub advanced_hold: Option<TrackbarHold<AdvancedField>>,
    /// VERA extension: the generated map on show was written as a map file;
    /// Export Map rests until the next Generate.
    pub exported: bool,
}

/// The Advanced sliders' range, the fields' own clamp (`RmgOptions::normalize`).
pub const ADVANCED_RANGE: TrackbarRange = TrackbarRange::new(0, 100, 1);

/// The option field an Advanced slider sets.
fn advanced_field_mut(options: &mut RmgOptions, field: AdvancedField) -> &mut i32 {
    match field {
        AdvancedField::Water => &mut options.water_amount,
        AdvancedField::Hills => &mut options.ruggedness,
        AdvancedField::Buildings => &mut options.urban_presence,
        AdvancedField::Vegetation => &mut options.vegetation,
        AdvancedField::OreFields => &mut options.tiberium_layout,
        AdvancedField::PlateauSize => &mut options.region_size,
        AdvancedField::Accessibility => &mut options.accessibility,
    }
}

fn advanced_field(options: &RmgOptions, field: AdvancedField) -> i32 {
    match field {
        AdvancedField::Water => options.water_amount,
        AdvancedField::Hills => options.ruggedness,
        AdvancedField::Buildings => options.urban_presence,
        AdvancedField::Vegetation => options.vegetation,
        AdvancedField::OreFields => options.tiberium_layout,
        AdvancedField::PlateauSize => options.region_size,
        AdvancedField::Accessibility => options.accessibility,
    }
}

impl RandomMapSetupModalState {
    /// Open the dialog over the current selection.
    ///
    /// The app-layer Scenario owner initializes the process-persistent seed
    /// before opening this presentation state. Accept starts disabled: the
    /// player must generate first.
    pub fn open(
        options: RmgOptions,
        previous_selection: Option<ChooseMapSelection>,
        saved_seeds_available: bool,
        extended: bool,
    ) -> Self {
        assert_ne!(
            options.seed, -1,
            "random-map options must be initialized by the app Scenario owner"
        );
        let mut modal = Self {
            options,
            generated: false,
            generating: false,
            saved_seed_buttons_enabled: saved_seeds_available,
            open_combo: None,
            players_hold: None,
            pressed_control: None,
            generated_preview: None,
            preview_generation: 0,
            previous_selection,
            statics: DialogStatics::default(),
            extended,
            advanced: false,
            advanced_values: None,
            advanced_hold: None,
            exported: false,
        };
        modal.normalize_options();
        modal
    }

    /// [`RmgOptions::normalize`], then the original 8-player and Very Large
    /// ceilings unless the dialog is `extended`.
    fn normalize_options(&mut self) {
        self.options.normalize();
        if !self.extended {
            use crate::map::rmg::{NATIVE_MAX_PLAYERS, NATIVE_MAX_SIZE_OPTION};
            self.options.num_players = self.options.num_players.min(NATIVE_MAX_PLAYERS);
            self.options.width = self.options.width.min(NATIVE_MAX_SIZE_OPTION);
            self.options.height = self.options.height.min(NATIVE_MAX_SIZE_OPTION);
            // The options record is shared with offline Skirmish, so a theater
            // only that dialog offers falls back to the first native entry.
            let theater = self.options.theater;
            if theater >= 2
                && !THEATER_ITEMS[..NATIVE_THEATER_ITEMS]
                    .iter()
                    .any(|item| item.value == theater)
            {
                self.options.theater = 0;
            }
            // VERA's towns are offline-only, like the Advanced view.
            self.options.urban_enabled = false;
            self.options.fair_starts = false;
        }
    }

    /// The Players slider's range for this dialog.
    pub const fn players_range(&self) -> TrackbarRange {
        if self.extended {
            EXTENDED_PLAYERS_RANGE
        } else {
            PLAYERS_RANGE
        }
    }

    /// The entries `combo` lists in this dialog.
    pub fn combo_items(&self, combo: SetupCombo) -> &'static [SetupComboItem] {
        setup_combo_items(combo, self.extended)
    }

    /// Whether a control is currently interactive. Every control is inert
    /// during the synchronous generate block, including Cancel.
    pub fn is_enabled(&self, control: RandomMapSetupControl) -> bool {
        use RandomMapSetupControl as C;
        if self.generating {
            return false;
        }
        match control {
            // Accept and save both require a generated result.
            C::Ok0x6c5 | C::Save0x6c3 => self.generated,
            C::Load0x6c2 | C::Delete0x6c4 => self.saved_seed_buttons_enabled,
            // Plateau size reaches only the generator's plateau split, which
            // runs for Inland and Mountainous (`SplitOversizedRegions`).
            C::Advanced(AdvancedField::PlateauSize) => {
                self.extended && matches!(self.options.map_type, 3 | 4)
            }
            C::AdvancedToggle | C::Advanced(_) | C::FairStartsToggle => self.extended,
            C::ExportMap => self.extended && self.generated && !self.exported,
            _ => true,
        }
    }

    /// Apply an option edit. Any change invalidates the generated result, so
    /// accept is disabled until the next generate.
    pub fn set_map_type(&mut self, value: i32) {
        self.options.map_type = value;
        self.on_option_changed();
    }

    pub fn set_time(&mut self, value: i32) {
        self.options.time = value;
        self.on_option_changed();
    }

    pub fn set_theater(&mut self, value: i32) {
        self.options.theater = value;
        self.on_option_changed();
    }

    /// One size selection drives both axes.
    pub fn set_size(&mut self, value: i32) {
        self.options.width = value;
        self.options.height = value;
        self.on_option_changed();
    }

    pub fn set_resources(&mut self, value: i32) {
        self.options.resources = value;
        self.on_option_changed();
    }

    pub fn set_num_players(&mut self, value: i32) {
        self.options.num_players = value;
        self.on_option_changed();
    }

    /// A press at device `(x, y)` on the Players slider window. Returns whether
    /// the value changed; the slider then clicks (GenericClick, `0x0061E6DD`),
    /// since `0x105` never turns its cue off (`0x4AE`).
    pub fn press_players(&mut self, x: i32, y: i32, window: RectPx, scale: ShellScale) -> bool {
        let press = self
            .players_range()
            .press_in(self.options.num_players, x, y, window, scale);
        self.players_hold = press.hold(());
        match press {
            TrackbarPress::Jump(value) => self.change_num_players(value),
            _ => false,
        }
    }

    /// A move while the Players slider holds the mouse: a held thumb follows
    /// device `x`. Returns whether the value changed.
    pub fn drag_players(&mut self, x: i32, window: RectPx, scale: ShellScale) -> bool {
        self.players_hold.is_some_and(|hold| hold.dragging)
            && self.change_num_players(self.players_range().value_at_in(x, window, scale))
    }

    /// The slider notifies (`WM_HSCROLL`, case `0x00596B04`) only when its
    /// position changes (`0x0061E609`).
    fn change_num_players(&mut self, value: i32) -> bool {
        let changed = self.options.num_players != value;
        if changed {
            self.set_num_players(value);
        }
        changed
    }

    /// The option field a combo reflects. Size reads the width axis, which is
    /// the one the original resyncs the combo from.
    pub const fn combo_value(&self, combo: SetupCombo) -> i32 {
        match combo {
            SetupCombo::MapType => self.options.map_type,
            SetupCombo::Time => self.options.time,
            SetupCombo::Theater => self.options.theater,
            SetupCombo::Size => self.options.width,
            SetupCombo::Resources => self.options.resources,
        }
    }

    /// Which entry is highlighted, or `None` when the current value has no entry
    /// — the original resolves the selection by matching the entry, so an
    /// unlisted value simply clears the selection instead of clamping.
    pub fn selected_item_index(&self, combo: SetupCombo) -> Option<usize> {
        let value = self.combo_value(combo);
        self.combo_items(combo)
            .iter()
            .position(|entry| entry.value == value)
    }

    /// Commit a picked entry and close the dropdown.
    pub fn set_combo_value(&mut self, combo: SetupCombo, value: i32) {
        match combo {
            SetupCombo::MapType => self.set_map_type(value),
            SetupCombo::Time => self.set_time(value),
            SetupCombo::Theater => self.set_theater(value),
            SetupCombo::Size => self.set_size(value),
            SetupCombo::Resources => self.set_resources(value),
        }
        self.open_combo = None;
    }

    /// A mouse press on the combos, and the cue it plays; `None` when the
    /// press is not theirs. [`ComboPress`] holds the native face and drop
    /// window rules; this dialog keeps its own list rows.
    pub fn press_combos(
        &mut self,
        layout: &super::super::layout::RandomMapSetupLayout,
        x: i32,
        y: i32,
    ) -> Option<super::SkirmishShellUiSound> {
        let press = if let Some(combo) = self.open_combo {
            let items = self.combo_items(combo);
            let press = super::super::layout::random_map_setup_dropdown_row_at(
                layout,
                combo.row(),
                items.len(),
                x,
                y,
            )
            .map_or(ComboPress::Dismiss, ComboPress::Pick);
            if let ComboPress::Pick(index) = press {
                self.set_combo_value(combo, items[index].value);
            }
            self.open_combo = None;
            press
        } else {
            if self.advanced {
                // VERA: the Advanced view hides the combos.
                return None;
            }
            let combo = super::super::layout::random_map_setup_control_at(layout, x, y)
                .and_then(SetupCombo::from_control)
                .filter(|combo| self.is_enabled(combo.control()))?;
            let face = layout.control_rects[combo.row()];
            let press = ComboPress::on_face(face, layout.scale, x, y);
            if press == ComboPress::Open {
                self.open_combo = Some(combo);
            }
            press
        };
        press.cue().map(super::SkirmishShellUiSound::combo)
    }

    fn on_option_changed(&mut self) {
        self.normalize_options();
        self.generated = false;
        self.generated_preview = None;
    }

    /// VERA: Load hands its seed over. The sliders let go of the values the
    /// player set, so they show the seed's, and a LAN or online dialog holds
    /// the seed to its own limits, towns off.
    pub fn adopt_saved_seed(&mut self, options: RmgOptions) {
        self.options = options;
        self.normalize_options();
        self.advanced_values = None;
        self.advanced_hold = None;
    }

    /// Surprise Me: randomize the option subset and invalidate the result.
    pub fn randomize_options(
        &mut self,
        settings: &RmgSettings,
        rng: &mut impl RandomRanged,
        description: &str,
    ) {
        // VERA: Surprise Me also drops the Advanced values, so the derived
        // fields are random again.
        self.advanced_values = None;
        self.options.urban_enabled = false;
        #[cfg(not(feature = "ares"))]
        randomize(&mut self.options, settings, rng, description);
        #[cfg(feature = "ares")]
        {
            let values = |combo| {
                self.combo_items(combo)
                    .iter()
                    .map(|item| item.value)
                    .collect::<Vec<_>>()
            };
            let (map_types, theaters) = (values(SetupCombo::MapType), values(SetupCombo::Theater));
            randomize_from_combos(
                &mut self.options,
                settings,
                rng,
                description,
                &map_types,
                &theaters,
            );
        }
        self.generated = false;
        self.generated_preview = None;
        self.open_combo = None;
    }

    /// Toggle the Ares Urban Areas box (control 5000). The box is
    /// `RMG::UrbanAreas` itself: Ares copies it in at SyncOptionsFromControls
    /// (MapSeedClass_DialogFunc_GetData, Ares.dll 1006A940, hook 0x596C81) and
    /// out at SyncControlsFromOptions (_SetData 1006A980, hook 0x5971EA). A
    /// click alone neither rerolls nor invalidates the generated map.
    #[cfg(feature = "ares")]
    pub fn toggle_urban_areas(&mut self) {
        self.options.urban_areas = !self.options.urban_areas;
    }

    /// Refresh the map-type-derived fields immediately before Generate.
    /// The caller supplies the process Main stream that also seeded this dialog.
    pub fn reroll_derived_for_generate(
        &mut self,
        settings: &RmgSettings,
        rng: &mut impl RandomRanged,
    ) {
        derive_from_map_type(&mut self.options, settings, rng);
        // VERA: Advanced values the player set replace the re-rolled ones;
        // the re-roll still runs, so it draws from Main exactly as before.
        if let Some(values) = self.advanced_values {
            for field in AdvancedField::ALL {
                *advanced_field_mut(&mut self.options, field) = values[field.row()];
            }
        }
    }

    /// VERA extension: switch Fair Starts. Like any option edit it makes the
    /// generated result stale.
    pub fn toggle_fair_starts(&mut self) {
        if !self.extended {
            return;
        }
        self.options.fair_starts = !self.options.fair_starts;
        self.generated = false;
    }

    /// Switch the option rows between the original controls and the
    /// Advanced sliders. Any open list or held slider lets go.
    pub fn toggle_advanced(&mut self) {
        self.advanced = !self.advanced;
        self.open_combo = None;
        self.players_hold = None;
        self.advanced_hold = None;
    }

    /// The value an Advanced slider shows: the player's, else the option's.
    /// Buildings reads 0 until the player turns the towns on, since the
    /// presence the map type rolls places nothing by itself.
    pub fn advanced_value(&self, field: AdvancedField) -> i32 {
        if field == AdvancedField::Buildings && !self.options.urban_enabled {
            return 0;
        }
        self.advanced_values.map_or_else(
            || advanced_field(&self.options, field),
            |values| values[field.row()],
        )
    }

    /// A press at device `(x, y)` on an Advanced slider window. Returns
    /// whether the value changed.
    pub fn press_advanced(
        &mut self,
        field: AdvancedField,
        x: i32,
        y: i32,
        window: RectPx,
        scale: ShellScale,
    ) -> bool {
        let press = ADVANCED_RANGE.press_in(self.advanced_value(field), x, y, window, scale);
        self.advanced_hold = press.hold(field);
        match press {
            TrackbarPress::Jump(value) => self.change_advanced(field, value),
            _ => false,
        }
    }

    /// A move while an Advanced slider holds the mouse. Returns whether the
    /// value changed.
    pub fn drag_advanced(&mut self, x: i32, window: RectPx, scale: ShellScale) -> bool {
        let Some(hold) = self.advanced_hold.filter(|hold| hold.dragging) else {
            return false;
        };
        self.change_advanced(hold.id, ADVANCED_RANGE.value_at_in(x, window, scale))
    }

    /// Set one Advanced value. The first change captures every field's
    /// current value, so the sliders the player did not touch keep theirs.
    pub fn change_advanced(&mut self, field: AdvancedField, value: i32) -> bool {
        let value = value.clamp(ADVANCED_RANGE.min, ADVANCED_RANGE.max);
        if self.advanced_value(field) == value {
            return false;
        }
        let mut values = self
            .advanced_values
            .unwrap_or_else(|| AdvancedField::ALL.map(|field| self.advanced_value(field)));
        values[field.row()] = value;
        self.advanced_values = Some(values);
        *advanced_field_mut(&mut self.options, field) = value;
        // Moving Buildings above zero turns the towns on (`urban_enabled`).
        if field == AdvancedField::Buildings {
            self.options.urban_enabled = value > 0;
        }
        self.on_option_changed();
        true
    }

    /// Begin the synchronous generate block: every control goes inert.
    pub fn begin_generate(&mut self) {
        self.generating = true;
        self.open_combo = None;
        self.players_hold = None;
        self.advanced_hold = None;
    }

    /// Release a failed worker without presenting an old/partial map as a
    /// successful result. Only finish_generate may enable Save and Accept.
    pub fn fail_generate(&mut self) {
        self.generating = false;
        self.generated = false;
        self.generated_preview = None;
    }

    /// Show the map as it stands part-way through the generate block.
    ///
    /// The original draws its preview repeatedly while generating, so the player
    /// watches the map build up rather than waiting on an empty box. The block
    /// is still running: nothing here unlocks a control or marks a result
    /// available.
    pub fn show_progress_preview(&mut self, preview: PreviewImage) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.generated_preview = Some(preview);
    }

    /// End the generate block, marking a result available so accept unlocks.
    ///
    /// This also switches Load/Delete on unconditionally: the original
    /// re-enables the whole control set afterwards without re-testing whether
    /// any saved seed actually exists.
    pub fn finish_generate(&mut self, preview: Option<PreviewImage>) {
        self.generating = false;
        self.generated = true;
        self.exported = false;
        self.saved_seed_buttons_enabled = true;
        if preview.is_some() {
            self.preview_generation = self.preview_generation.wrapping_add(1);
        }
        self.generated_preview = preview;
    }

    /// Accept. The original generates first when nothing has been generated
    /// yet, so a caller receiving `NeedsGenerate` must generate then retry.
    pub fn accept(&self) -> AcceptOutcome {
        if !self.generated {
            return AcceptOutcome::NeedsGenerate;
        }
        let mut committed = self.options.clone();
        committed.normalize();
        AcceptOutcome::Commit(Box::new(committed))
    }

    /// Cancel. Returns the selection to restore; the caller performs no other
    /// side effects.
    pub const fn cancel(&self) -> Option<ChooseMapSelection> {
        self.previous_selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Always returns `max`, so draws are identifiable.
    struct MaxRng;
    impl RandomRanged for MaxRng {
        fn ranged(&mut self, _min: i32, max: i32) -> i32 {
            max
        }

        #[cfg(feature = "ares")]
        fn next_u32(&mut self) -> u32 {
            u32::MAX
        }
    }

    fn opened() -> RandomMapSetupModalState {
        RandomMapSetupModalState::open(
            RmgOptions {
                seed: 0x1234,
                ..Default::default()
            },
            None,
            false,
            true,
        )
    }

    /// VERA: a loaded seed takes over the sliders, and a LAN or online dialog
    /// holds it to its own limits, towns off.
    #[test]
    fn a_loaded_seed_resets_the_sliders_and_obeys_the_dialog() {
        let saved = RmgOptions {
            seed: 0x1234,
            urban_enabled: true,
            urban_presence: 30,
            num_players: crate::map::rmg::MAX_PLAYERS,
            ..Default::default()
        };
        let mut offline = opened();
        assert!(offline.change_advanced(AdvancedField::Water, 77));
        offline.adopt_saved_seed(saved.clone());
        assert_eq!(offline.advanced_values, None);
        assert_eq!(offline.advanced_value(AdvancedField::Buildings), 30);
        assert_eq!(offline.options.num_players, crate::map::rmg::MAX_PLAYERS);

        let mut network = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 0x1234,
                ..Default::default()
            },
            None,
            true,
            false,
        );
        network.adopt_saved_seed(saved);
        assert!(!network.options.urban_enabled);
        assert_eq!(
            network.options.num_players,
            crate::map::rmg::NATIVE_MAX_PLAYERS
        );
    }

    /// VERA: Advanced values survive Generate's re-roll, which still draws as
    /// before; untouched sliders keep the values they showed; Surprise Me
    /// makes them random again.
    #[test]
    fn advanced_values_replace_the_generate_reroll_until_surprise_me() {
        struct Counting(i32);
        impl RandomRanged for Counting {
            fn ranged(&mut self, min: i32, _max: i32) -> i32 {
                self.0 += 1;
                min
            }

            #[cfg(feature = "ares")]
            fn next_u32(&mut self) -> u32 {
                0
            }
        }
        let mut state = opened();
        state.options.ruggedness = 40;
        assert!(state.change_advanced(AdvancedField::Water, 77));
        assert_eq!(
            state.advanced_values.unwrap()[AdvancedField::Hills.row()],
            40
        );

        let mut rng = Counting(0);
        state.reroll_derived_for_generate(&RmgSettings::default(), &mut rng);
        assert_eq!(rng.0, 8, "the re-roll still makes all eight draws");
        assert_eq!(state.options.water_amount, 77);
        assert_eq!(
            state.options.ruggedness, 40,
            "an untouched slider keeps its value"
        );

        state.randomize_options(&RmgSettings::default(), &mut MaxRng, "Random Map");
        assert_eq!(state.advanced_values, None);
        assert_ne!(state.advanced_value(AdvancedField::Water), 77);
    }

    /// VERA: only moving Buildings above zero turns the towns on, and the
    /// slider reads 0 until then; Surprise Me turns them off again.
    #[test]
    fn only_the_buildings_slider_turns_the_towns_on() {
        let mut state = opened();
        state.options.urban_presence = 63;
        assert_eq!(state.advanced_value(AdvancedField::Buildings), 0);
        state.change_advanced(AdvancedField::Water, 30);
        assert!(
            !state.options.urban_enabled,
            "another slider leaves them off"
        );
        assert!(state.change_advanced(AdvancedField::Buildings, 40));
        assert!(state.options.urban_enabled);
        assert_eq!(state.options.urban_presence, 40);
        state.randomize_options(&RmgSettings::default(), &mut MaxRng, "Random Map");
        assert!(!state.options.urban_enabled);
    }

    #[test]
    fn the_advanced_toggle_is_offline_skirmish_only() {
        let mut network = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 0x1234,
                ..Default::default()
            },
            None,
            false,
            false,
        );
        assert!(!network.is_enabled(RandomMapSetupControl::AdvancedToggle));
        network.toggle_fair_starts();
        assert!(!network.options.fair_starts, "Fair Starts is offline-only");
        let mut offline = opened();
        assert!(offline.is_enabled(RandomMapSetupControl::AdvancedToggle));
        offline.generated = true;
        offline.toggle_fair_starts();
        assert!(offline.options.fair_starts);
        assert!(!offline.generated, "the shown map no longer matches");
        let plateau = RandomMapSetupControl::Advanced(AdvancedField::PlateauSize);
        offline.set_map_type(1);
        assert!(
            !offline.is_enabled(plateau),
            "continents have no plateau split"
        );
        offline.set_map_type(4);
        assert!(offline.is_enabled(plateau));
        offline.toggle_advanced();
        assert!(offline.advanced);
        network.toggle_advanced();
        let _ = network;
    }

    #[test]
    fn gsi_04_02_dialog_open_is_rng_pure_and_generate_reroll_uses_process_main_only() {
        let process_main = crate::sim::rng::MainRng::new(0);
        let reference_main = crate::sim::rng::MainRng::new(0);
        let scenario = crate::sim::rng::SimRng::new(0x1234);
        let scenario_before = scenario.state();
        let mut mapgen = crate::map::rmg::RmgRng::new(0x5678);
        let mut mapgen_reference = mapgen.clone();

        let mut modal = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 0x2468,
                ..Default::default()
            },
            None,
            false,
            true,
        );
        assert_eq!(process_main.logical_state(), reference_main.logical_state());

        let mut expected_options = modal.options.clone();
        derive_from_map_type(
            &mut expected_options,
            &RmgSettings::default(),
            &mut MainRngRanged(reference_main.draws()),
        );
        modal.reroll_derived_for_generate(
            &RmgSettings::default(),
            &mut MainRngRanged(process_main.draws()),
        );

        assert_eq!(modal.options, expected_options);
        assert_eq!(process_main.logical_state(), reference_main.logical_state());
        assert_eq!(scenario.state(), scenario_before);
        assert_eq!(mapgen.next_u32(), mapgen_reference.next_u32());
    }

    #[test]
    fn ok_and_save_start_disabled() {
        let state = opened();
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
        assert!(!state.is_enabled(RandomMapSetupControl::Save0x6c3));
    }

    #[test]
    fn load_and_delete_follow_saved_seed_availability_at_open() {
        let none = opened();
        assert!(!none.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(!none.is_enabled(RandomMapSetupControl::Delete0x6c4));

        let some = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 1,
                ..Default::default()
            },
            None,
            true,
            true,
        );
        assert!(some.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(some.is_enabled(RandomMapSetupControl::Delete0x6c4));
    }

    /// VERA extension: only an offline Skirmish dialog offers more than eight
    /// players and the sizes past Very Large.
    #[test]
    fn only_a_skirmish_dialog_offers_thirty_players_and_large_sizes() {
        let options = RmgOptions {
            seed: 1,
            num_players: 30,
            width: 7,
            height: 7,
            theater: 3,
            ..Default::default()
        };
        let mut network = RandomMapSetupModalState::open(options.clone(), None, false, false);
        assert_eq!(
            (
                network.options.num_players,
                network.options.width,
                network.options.theater
            ),
            // Ares lists Desert (3) in every dialog (hook 0x5970EA).
            (8, 3, if cfg!(feature = "ares") { 3 } else { 0 })
        );
        assert_eq!(network.combo_items(SetupCombo::Size).len(), 4);
        network.set_num_players(12);
        assert_eq!(network.options.num_players, 8);

        let skirmish = RandomMapSetupModalState::open(options, None, false, true);
        assert_eq!(
            (
                skirmish.options.num_players,
                skirmish.options.width,
                skirmish.options.theater
            ),
            (30, 7, 3)
        );
        assert_eq!(skirmish.combo_items(SetupCombo::Size).len(), 8);
    }

    #[test]
    fn changing_an_option_disables_accept_again() {
        let mut state = opened();
        state.generated = true;
        state.set_resources(2);
        assert!(!state.generated, "an edit invalidates the generated result");
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
    }

    #[test]
    fn randomize_invalidates_the_generated_result() {
        let mut state = opened();
        state.generated = true;
        state.randomize_options(&RmgSettings::default(), &mut MaxRng, "Random Map");
        assert!(!state.generated);
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
        assert_eq!(state.options.description, "Random Map");
    }

    #[test]
    fn every_control_is_inert_during_generate_including_cancel() {
        let mut state = opened();
        state.begin_generate();
        for control in [
            RandomMapSetupControl::MapType0x405,
            RandomMapSetupControl::Theater0x407,
            RandomMapSetupControl::Players0x3eb,
            RandomMapSetupControl::Randomize0x621,
            RandomMapSetupControl::Generate0x620,
            RandomMapSetupControl::Ok0x6c5,
            RandomMapSetupControl::Cancel0x5c0,
        ] {
            assert!(!state.is_enabled(control), "{control:?} must be inert");
        }
    }

    fn one_pixel_preview() -> PreviewImage {
        PreviewImage {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        }
    }

    #[test]
    fn progress_previews_show_without_ending_the_generate_block() {
        let mut state = opened();
        state.begin_generate();
        state.show_progress_preview(one_pixel_preview());

        assert!(state.generated_preview.is_some(), "the map is on screen");
        // Still generating: nothing has unlocked, and accept must not become
        // available off a half-built map.
        assert!(state.generating);
        assert!(!state.generated);
        assert!(!state.is_enabled(RandomMapSetupControl::Cancel0x5c0));
        assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
    }

    #[test]
    fn each_progress_preview_bumps_the_texture_key() {
        // The renderer caches its texture on this counter, so a snapshot that
        // did not bump it would never reach the screen.
        let mut state = opened();
        state.begin_generate();
        let before = state.preview_generation;
        state.show_progress_preview(one_pixel_preview());
        state.show_progress_preview(one_pixel_preview());
        assert_eq!(state.preview_generation, before.wrapping_add(2));
    }

    #[test]
    fn failed_generation_cannot_accept_or_save_old_or_partial_results() {
        for had_prior_result in [false, true] {
            let mut state = opened();
            if had_prior_result {
                state.finish_generate(Some(one_pixel_preview()));
            }
            state.begin_generate();
            state.show_progress_preview(one_pixel_preview());
            state.fail_generate();
            assert!(!state.generating);
            assert!(!state.generated);
            assert!(state.generated_preview.is_none());
            assert!(!state.is_enabled(RandomMapSetupControl::Ok0x6c5));
            assert!(!state.is_enabled(RandomMapSetupControl::Save0x6c3));
            assert!(state.is_enabled(RandomMapSetupControl::Generate0x620));
            assert!(state.is_enabled(RandomMapSetupControl::Cancel0x5c0));
            assert_eq!(state.accept(), AcceptOutcome::NeedsGenerate);
            // Retrying and succeeding is the only path back to acceptance.
            state.begin_generate();
            state.finish_generate(Some(one_pixel_preview()));
            assert!(state.is_enabled(RandomMapSetupControl::Ok0x6c5));
            assert!(state.is_enabled(RandomMapSetupControl::Save0x6c3));
        }
    }

    #[test]
    fn generate_enables_load_and_delete_even_with_no_saved_seeds() {
        // The original re-enables the whole control set after generating
        // without re-testing saved-seed availability.
        let mut state = opened();
        assert!(!state.is_enabled(RandomMapSetupControl::Load0x6c2));
        state.begin_generate();
        state.finish_generate(None);
        assert!(state.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(state.is_enabled(RandomMapSetupControl::Delete0x6c4));
    }

    #[test]
    fn accept_after_generate_commits_normalized_options() {
        let mut state = opened();
        state.finish_generate(None);
        match state.accept() {
            AcceptOutcome::Commit(options) => {
                assert!(options.tiberium >= 1, "committed options are normalized");
                assert_eq!(options.seed, state.options.seed);
            }
            other => panic!("expected a commit, got {other:?}"),
        }
    }

    // Stock lists; Ares' hooks 0x596FFE/0x5970EA change them.
    #[cfg(not(feature = "ares"))]
    #[test]
    fn map_type_entries_outside_offline_skirmish_start_at_one_and_omit_archipelago() {
        let items = setup_combo_items(SetupCombo::MapType, false);
        assert_eq!(items.len(), 4);
        assert_eq!(
            items.iter().map(|entry| entry.value).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[cfg(not(feature = "ares"))]
    #[test]
    fn every_map_type_selects_its_entry_once_archipelago_is_listed() {
        let mut state = opened();
        state.set_map_type(0);
        assert_eq!(
            state.selected_item_index(SetupCombo::MapType),
            Some(4),
            "VERA lists archipelago last"
        );
        state.set_map_type(9);
        assert_eq!(
            state.selected_item_index(SetupCombo::MapType),
            Some(3),
            "an out-of-range value clamps to mountainous"
        );
        state.set_map_type(3);
        assert_eq!(state.selected_item_index(SetupCombo::MapType), Some(2));
    }

    /// Ares RMG_EnableArchipelago (hook 0x596FFE) and RMG_EnableDesert (hook
    /// 0x5970EA): archipelago leads and Desert follows Snow in every dialog;
    /// Surprise Me draws the map type, then the theater, from those entries
    /// and takes the Urban Areas box from a raw draw's low bit (hook
    /// 0x596786).
    #[cfg(feature = "ares")]
    #[test]
    fn ares_lists_archipelago_and_desert_and_surprise_me_draws_from_them() {
        let values = |combo, extended| {
            setup_combo_items(combo, extended)
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>()
        };
        assert_eq!(values(SetupCombo::MapType, false), [0, 1, 2, 3, 4]);
        assert_eq!(values(SetupCombo::Theater, false), [0, 1, 3]);
        assert_eq!(values(SetupCombo::Theater, true), [0, 1, 3, 2]);

        let mut network = RandomMapSetupModalState::open(
            RmgOptions {
                seed: 1,
                ..Default::default()
            },
            None,
            false,
            false,
        );
        network.randomize_options(&RmgSettings::default(), &mut MaxRng, "Random Map");
        assert_eq!(network.options.map_type, 4, "the last map-type entry");
        assert_eq!(network.options.theater, 3, "the last theater entry");
        assert!(network.options.urban_areas, "u32::MAX has its low bit set");
        network.generated = true;
        network.toggle_urban_areas();
        assert!(!network.options.urban_areas);
        assert!(network.generated, "the box alone keeps the generated map");
    }

    #[cfg(not(feature = "ares"))]
    #[test]
    fn archipelago_and_the_added_theaters_are_listed_after_the_original_entries() {
        let values = |combo, extended| {
            setup_combo_items(combo, extended)
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>()
        };
        assert_eq!(values(SetupCombo::MapType, true), [1, 2, 3, 4, 0]);
        assert_eq!(values(SetupCombo::Theater, true), [0, 1, 2, 3]);
        assert_eq!(
            values(SetupCombo::Theater, false),
            [0, 1],
            "network dialogs"
        );
    }

    #[test]
    fn size_selection_reflects_the_width_axis() {
        let mut state = opened();
        state.set_combo_value(SetupCombo::Size, 2);
        assert_eq!((state.options.width, state.options.height), (2, 2));
        assert_eq!(state.selected_item_index(SetupCombo::Size), Some(2));
    }

    #[test]
    fn picking_an_entry_closes_the_dropdown_and_invalidates_the_result() {
        let mut state = opened();
        state.open_combo = Some(SetupCombo::Resources);
        state.generated = true;
        state.set_combo_value(SetupCombo::Resources, 3);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.options.resources, 3);
        assert!(!state.generated);
    }

    /// A face press plays the open cue but only the arrow opens the list. With
    /// a list open, a press on a row picks it and one anywhere else only
    /// closes it; both play the close cue.
    #[test]
    fn combo_presses_open_from_the_arrow_and_close_from_anywhere() {
        use super::super::SkirmishShellUiSound::{GuiComboCloseSound, GuiComboOpenSound};
        let layout = super::super::super::layout::compute_random_map_setup_layout(800, 600);
        let mut state = opened();
        let time = layout.control_rects[SetupCombo::Time.row()];
        let (text, arrow) = ((time.x + 4, time.y + 4), (time.x + time.w - 4, time.y + 4));
        assert_eq!(
            state.press_combos(&layout, text.0, text.1),
            Some(GuiComboOpenSound)
        );
        assert_eq!(state.open_combo, None);
        assert_eq!(
            state.press_combos(&layout, arrow.0, arrow.1),
            Some(GuiComboOpenSound)
        );
        assert_eq!(state.open_combo, Some(SetupCombo::Time));
        let list = super::super::super::layout::random_map_setup_dropdown_rect(
            &layout,
            SetupCombo::Time.row(),
            setup_combo_items(SetupCombo::Time, true).len(),
        );
        let dusk_row = list.y + list.h * 5 / 8;
        assert_eq!(
            state.press_combos(&layout, list.x + 4, dusk_row),
            Some(GuiComboCloseSound)
        );
        assert_eq!(
            (
                state.open_combo,
                state.selected_item_index(SetupCombo::Time)
            ),
            (None, Some(2))
        );
        assert_eq!(
            state.press_combos(&layout, arrow.0, arrow.1),
            Some(GuiComboOpenSound)
        );
        let outside = (layout.randomize.x + 1, layout.randomize.y + 1);
        assert_eq!(
            state.press_combos(&layout, outside.0, outside.1),
            Some(GuiComboCloseSound)
        );
        assert_eq!(
            (
                state.open_combo,
                state.selected_item_index(SetupCombo::Time)
            ),
            (None, Some(2))
        );
        assert_eq!(state.press_combos(&layout, outside.0, outside.1), None);
    }
}
