//! Random-map setup modal state (the Create Random Map dialog).
//!
//! Render-agnostic: owns the working option set, the enable/disable state
//! machine, and the accept/cancel outcome. Depends on the rmg options model and
//! the layout control enum only — no assets, no wgpu.

use crate::map::rmg::options::RmgOptions;
use crate::map::rmg::preview::PreviewImage;
use crate::map::rmg::randomize::{
    RandomRanged, derive_from_map_type, randomize, randomize_from_combos,
};
use crate::map::rmg::settings::RmgSettings;
use crate::ui::shell::static_reveal::DialogStatics;
use vera20k_gui::shell::trackbar::TrackbarRange;

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
/// VERA lists it last, as it does the larger sizes, so the original four keep
/// their rows.
const STOCK_MAP_TYPE_ITEMS: [SetupComboItem; 5] = [
    item("TXT_MAP_CONTINENT", "Continent", 1),
    item("TXT_MAP_TEAM_CONTINENTS", "Team Continents", 2),
    item("TXT_MAP_INLAND", "Inland", 3),
    item("TXT_MAP_MOUNTAINOUS", "Mountainous", 4),
    item("TXT_MAP_ARCHIPELAGO", "Archipelago", 0),
];
/// Ares' RMG_EnableArchipelago (Ares.dll 1006ACF0, hook 0x596FFE -> 0x597008)
/// starts the fill at the table's first label with item data 0, so every
/// dialog lists archipelago first when Ares.dll is loaded (the game is read
/// the Ares way).
const ARES_MAP_TYPE_ITEMS: [SetupComboItem; 5] = [
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
/// cliff or road tiles for the generator to place.
const STOCK_THEATER_ITEMS: [SetupComboItem; 4] = [
    item("Name:Temperate", "Temperate", 0),
    item("Name:Snow", "Snow", 1),
    item("Name:Urban", "Urban", 2),
    item("TXT_BIOME_DESERT", "Desert", 3),
];
/// Ares' RMG_EnableDesert (Ares.dll 1006AD10, hook 0x5970EA) adds
/// `Name:Desert` with item data 3 after the two native entries; VERA adds
/// URBAN after it. Listed when Ares.dll is loaded (the game is read the Ares
/// way).
const ARES_THEATER_ITEMS: [SetupComboItem; 4] = [
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

/// The entries a combo is filled with, in list order: the original's, then
/// VERA's, in Skirmish, LAN and online dialogs alike. `ares`: Ares.dll is
/// loaded (the game is read the Ares way), which lists its landform and
/// theater orders.
pub fn setup_combo_items(combo: SetupCombo, ares: bool) -> &'static [SetupComboItem] {
    match combo {
        SetupCombo::MapType => {
            if ares {
                &ARES_MAP_TYPE_ITEMS
            } else {
                &STOCK_MAP_TYPE_ITEMS
            }
        }
        SetupCombo::Time => &TIME_ITEMS,
        SetupCombo::Theater => {
            if ares {
                &ARES_THEATER_ITEMS
            } else {
                &STOCK_THEATER_ITEMS
            }
        }
        SetupCombo::Size => &SIZE_ITEMS,
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

/// The Players slider's range in steps of 1. The original runs 2..8
/// (`TBM_SETRANGE` at `0x0059722C`; no `0x4AB`); VERA's runs to the offline
/// Skirmish row count. A network dialog keeps the room’s player limit.
pub const PLAYERS_RANGE: TrackbarRange = TrackbarRange::new(2, crate::map::rmg::MAX_PLAYERS, 1);

/// VERA extension: a ready-made option set the Presets button applies in one
/// press. Each sets the players, landform, size, resources, the seven
/// Advanced values (`advanced`, by [`AdvancedField::row`]) and Fair Starts;
/// the theater changes only where the preset is about it. The seed, time
/// and description stay the player's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomMapPreset {
    /// Two players on open inland ground with matched starts.
    Duel,
    /// Thirty players on the largest size, little water.
    Brawl,
    /// Four players in a city that covers the map.
    CityWar,
    /// Four players across an archipelago.
    Islands,
    /// Eight players on Team Continents, the landform that gives each team
    /// a continent.
    Teams,
    /// One of the player's own, by slot from 0: the options the Extras
    /// view's My Preset tool saved (`custom_presets`), all but the seed and
    /// the description.
    Mine(u8),
}

/// The player's own presets a dialog keeps (`RandomMapPreset::Mine`).
pub const CUSTOM_PRESET_SLOTS: usize = 5;

impl RandomMapPreset {
    /// The ready-made ones, in the order the button steps through them; the
    /// player's own follow.
    pub const ALL: [Self; 5] = [
        Self::Duel,
        Self::Brawl,
        Self::CityWar,
        Self::Islands,
        Self::Teams,
    ];

    /// The button caption while this preset stands. One of the player's own
    /// is "My Preset" with its number, which the caption's drawer fills in.
    pub const fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Duel => ("VERA:RandomMapPresetDuel", "1v1 Duel"),
            Self::Brawl => ("VERA:RandomMapPresetBrawl", "30-Player Brawl"),
            Self::CityWar => ("VERA:RandomMapPresetCity", "City War"),
            Self::Islands => ("VERA:RandomMapPresetIslands", "Islands"),
            Self::Teams => ("VERA:RandomMapPresetTeams", "Team Battle"),
            Self::Mine(_) => ("VERA:RandomMapPresetMine", "My Preset {number}"),
        }
    }

    /// A ready-made preset's players, landform (`map_type`), size, resources,
    /// theater and Advanced values: water, hills, buildings, vegetation, ore
    /// fields, plateau size, accessibility. None for one of the player's own.
    const fn values(self) -> Option<(i32, i32, i32, i32, Option<i32>, [i32; 13])> {
        // The trailing six zeros are VERA's Advanced additions: the ready-made
        // presets predate those sliders and leave them at their default.
        Some(match self {
            Self::Duel => (
                2,
                3,
                1,
                1,
                None,
                [10, 25, 0, 40, 60, 50, 80, 0, 0, 0, 0, 0, 0],
            ),
            Self::Brawl => (
                30,
                3,
                7,
                2,
                None,
                [5, 20, 0, 30, 70, 50, 90, 0, 0, 0, 0, 0, 0],
            ),
            Self::CityWar => (
                4,
                3,
                2,
                1,
                Some(2),
                [5, 10, 100, 20, 50, 60, 90, 0, 0, 0, 0, 0, 0],
            ),
            Self::Islands => (
                4,
                0,
                2,
                2,
                Some(0),
                [60, 20, 0, 50, 60, 50, 50, 0, 0, 0, 0, 0, 0],
            ),
            Self::Teams => (
                8,
                2,
                4,
                2,
                None,
                [35, 25, 0, 40, 60, 50, 70, 0, 0, 0, 0, 0, 0],
            ),
            Self::Mine(_) => return None,
        })
    }
}

/// The Create Random Map dialog.
#[derive(Debug, Clone)]
pub struct RandomMapSetupModalState {
    /// The working option set the controls edit.
    pub options: RmgOptions,
    pub players_range: TrackbarRange,
    /// True once Generate has run and no option has changed since.
    pub generated: bool,
    /// True while the synchronous generate block owns the dialog.
    pub generating: bool,
    /// Last percentage reported by the active map generator.
    pub(crate) progress_percent: u8,
    /// Load/Delete enablement. Starts at saved-seed availability, but the
    /// generate action turns it on unconditionally, as the original does.
    pub saved_seed_buttons_enabled: bool,
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
    /// VERA extension: the option rows show the Advanced sliders instead of
    /// the original controls.
    pub advanced: bool,
    /// VERA extension: the Advanced values the player set, by
    /// [`AdvancedField::row`]. `None` until a slider moves; Surprise Me clears
    /// them. While set they replace the map-type-derived values that every
    /// Generate re-rolls, after the re-roll's draws.
    pub advanced_values: Option<[i32; 13]>,
    /// VERA extension: which page of the Advanced sliders is on show. The
    /// dialog has seven rows; VERA's three sliders are scrolled into view.
    pub advanced_page: usize,
    /// VERA extension: the generated map on show was written as a map file;
    /// Export Map rests until the next Generate.
    pub exported: bool,
    /// VERA extension: where the last export wrote the map, as the player
    /// reads it, shown where the progress text goes while `exported` holds.
    pub exported_to: Option<String>,
    /// VERA extension: the preset the options still stand at; any edit of
    /// theirs clears it.
    pub preset: Option<RandomMapPreset>,
    /// VERA extension: the Extras page on show (`random_map_extras`), in
    /// place of the Advanced sliders; `advanced` is set with it.
    pub extras_page: Option<usize>,
    /// VERA extension: what the last Extras tool did, shown where the
    /// progress text goes until the next press: a string-table key and its
    /// English.
    pub extras_note: Option<(&'static str, &'static str)>,
    /// VERA extension: what the generated map on show offers each start
    /// (`rmg::fairness`), for the readout under the options.
    pub fairness: Option<crate::map::rmg::fairness::FairnessReport>,
    /// VERA extension: the options of the maps generated since the dialog
    /// opened, oldest first, the one on show last ([`Self::step_back`]).
    pub history: Vec<RmgOptions>,
    /// VERA extension: the player's own presets, by slot
    /// ([`RandomMapPreset::Mine`]); the app loads and saves their files.
    pub custom_presets: Vec<RmgOptions>,
    /// The slot [`Self::save_custom_preset`] takes once every slot is taken.
    pub next_custom_slot: usize,
    /// Ares.dll is loaded (the game is read the Ares way): the combos list
    /// Ares' entries, Surprise Me draws from them, and the Urban Areas box
    /// shows. The shell sets it when it opens the dialog; never saved.
    pub ares: bool,
}

/// Maps [`RandomMapSetupModalState::history`] remembers.
const HISTORY_MAPS: usize = 12;

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
        AdvancedField::TechBuildings => &mut options.tech_building_amount,
        AdvancedField::Derelicts => &mut options.derelict_amount,
        AdvancedField::Damage => &mut options.damage_amount,
        AdvancedField::Civilians => &mut options.civilian_amount,
        AdvancedField::Animals => &mut options.animal_amount,
        AdvancedField::Wild => &mut options.wild_amount,
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
        AdvancedField::TechBuildings => options.tech_building_amount,
        AdvancedField::Derelicts => options.derelict_amount,
        AdvancedField::Damage => options.damage_amount,
        AdvancedField::Civilians => options.civilian_amount,
        AdvancedField::Animals => options.animal_amount,
        AdvancedField::Wild => options.wild_amount,
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
    ) -> Self {
        assert_ne!(
            options.seed, -1,
            "random-map options must be initialized by the app Scenario owner"
        );
        let mut modal = Self {
            options,
            players_range: PLAYERS_RANGE,
            generated: false,
            generating: false,
            progress_percent: 0,
            saved_seed_buttons_enabled: saved_seeds_available,
            generated_preview: None,
            preview_generation: 0,
            previous_selection,
            statics: DialogStatics::default(),
            advanced: false,
            advanced_values: None,
            advanced_page: 0,
            exported: false,
            preset: None,
            extras_page: None,
            extras_note: None,
            exported_to: None,
            fairness: None,
            history: Vec::new(),
            custom_presets: Vec::new(),
            next_custom_slot: 0,
            ares: false,
        };
        modal.options.normalize();
        modal
    }

    /// A room can publish start staging for its network seats only.
    pub fn use_network_player_limit(&mut self) {
        self.players_range.max = crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32;
        self.options.num_players = self.options.num_players.min(self.players_range.max);
    }

    fn normalize_options(&mut self) {
        self.options.normalize();
        self.options.num_players = self.options.num_players.min(self.players_range.max);
    }

    /// An option edit made by the Extras view.
    pub(super) fn note_option_edit(&mut self) {
        self.on_option_changed();
    }

    /// Hold every Advanced value at what the options hold, as if the player
    /// had set each slider.
    pub(super) fn pin_advanced_values(&mut self) {
        self.advanced_values =
            Some(AdvancedField::ALL.map(|field| advanced_field(&self.options, field)));
    }

    /// The entries `combo` lists in this dialog.
    pub fn combo_items(&self, combo: SetupCombo) -> &'static [SetupComboItem] {
        setup_combo_items(combo, self.ares)
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
            C::Advanced(AdvancedField::PlateauSize) => matches!(self.options.map_type, 3 | 4),
            C::ExtrasRow(row) => self.extras_row(row).is_some(),
            C::ExportMap => self.generated && !self.exported,
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

    /// The Players slider's new position. The slider notifies (`WM_HSCROLL`,
    /// case `0x00596B04`) only when its position changes (`0x0061E609`).
    pub(crate) fn change_num_players(&mut self, value: i32) -> bool {
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

    /// Commit a picked entry.
    pub fn set_combo_value(&mut self, combo: SetupCombo, value: i32) {
        match combo {
            SetupCombo::MapType => self.set_map_type(value),
            SetupCombo::Time => self.set_time(value),
            SetupCombo::Theater => self.set_theater(value),
            SetupCombo::Size => self.set_size(value),
            SetupCombo::Resources => self.set_resources(value),
        }
    }

    fn on_option_changed(&mut self) {
        self.normalize_options();
        self.generated = false;
        self.generated_preview = None;
        self.preset = None;
        self.fairness = None;
    }

    /// VERA extension: apply the preset after the one that stands (the first
    /// when none does), as one option edit. The Advanced values are set as
    /// the player's own, so Generate keeps them over the landform's rolls.
    pub fn cycle_preset(&mut self) {
        // The ready-made ones, then the player's own.
        let presets: Vec<RandomMapPreset> = RandomMapPreset::ALL
            .into_iter()
            .chain((0..self.custom_presets.len()).map(|slot| RandomMapPreset::Mine(slot as u8)))
            .collect();
        let next = match self.preset {
            Some(current) => {
                let at = presets
                    .iter()
                    .position(|preset| *preset == current)
                    .unwrap_or(0);
                presets[(at + 1) % presets.len()]
            }
            None => presets[0],
        };
        match (next, next.values()) {
            (_, Some((players, map_type, size, resources, theater, advanced))) => {
                self.options.num_players = players;
                self.options.map_type = map_type;
                self.options.width = size;
                self.options.height = size;
                self.options.resources = resources;
                if let Some(theater) = theater {
                    self.options.theater = theater;
                }
                for field in AdvancedField::ALL {
                    *advanced_field_mut(&mut self.options, field) = advanced[field.row()];
                }
                self.advanced_values = Some(advanced);
                self.options.urban_enabled = advanced[AdvancedField::Buildings.row()] > 0;
                self.options.fair_starts = true;
            }
            (RandomMapPreset::Mine(slot), _) => {
                let Some(saved) = self.custom_presets.get(usize::from(slot)) else {
                    return;
                };
                self.options = RmgOptions {
                    seed: self.options.seed,
                    description: self.options.description.clone(),
                    ..saved.clone()
                };
                self.pin_advanced_values();
            }
            _ => return,
        }
        self.on_option_changed();
        self.preset = Some(next);
    }

    /// VERA extension: keep the options as one of the player's own presets
    /// and return its slot: the next free one, or the first again once all
    /// are taken (the oldest goes). The app writes the slot's file.
    pub fn save_custom_preset(&mut self) -> usize {
        let slot = if self.custom_presets.len() < CUSTOM_PRESET_SLOTS {
            self.custom_presets.push(self.options.clone());
            self.custom_presets.len() - 1
        } else {
            let slot = self.next_custom_slot % CUSTOM_PRESET_SLOTS;
            self.custom_presets[slot] = self.options.clone();
            slot
        };
        self.next_custom_slot = (slot + 1) % CUSTOM_PRESET_SLOTS;
        self.preset = Some(RandomMapPreset::Mine(slot as u8));
        slot
    }

    /// VERA extension: go back to the map generated before the one on show:
    /// its options return, with their seed, for the app to generate without
    /// Generate's rolls. False when there is none to go back to.
    pub fn step_back(&mut self) -> bool {
        if self.history.len() < 2 {
            return false;
        }
        self.history.pop();
        let Some(previous) = self.history.last().cloned() else {
            return false;
        };
        self.options = previous;
        self.pin_advanced_values();
        self.on_option_changed();
        true
    }

    /// VERA: Load hands its seed over. The sliders let go of the values the
    /// player set, so they show the seed's.
    pub fn adopt_saved_seed(&mut self, options: RmgOptions) {
        self.options = options;
        self.normalize_options();
        self.advanced_values = None;
        self.preset = None;
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
        self.preset = None;
        if self.ares {
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
        } else {
            randomize(&mut self.options, settings, rng, description);
        }
        self.generated = false;
        self.generated_preview = None;
    }

    /// Toggle the Ares Urban Areas box (control 5000). The box is
    /// `RMG::UrbanAreas` itself: Ares copies it in at SyncOptionsFromControls
    /// (MapSeedClass_DialogFunc_GetData, Ares.dll 1006A940, hook 0x596C81) and
    /// out at SyncControlsFromOptions (_SetData 1006A980, hook 0x5971EA). A
    /// click alone neither rerolls nor invalidates the generated map. The box
    /// shows only when Ares.dll is loaded ([`Self::ares`]).
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
        self.options.fair_starts = !self.options.fair_starts;
        self.generated = false;
        self.preset = None;
    }

    /// Step the option rows to their next view: the original controls, the
    /// Advanced sliders (scrolled with their own scrollbar), then the Extras,
    /// whose first page shows. A held scrollbar lets go (the hidden widgets
    /// let go of theirs).
    pub fn toggle_advanced(&mut self) {
        match (self.advanced, self.extras_page) {
            (false, _) => {
                self.advanced = true;
                self.extras_page = None;
                self.advanced_page = 0;
            }
            (true, None) => {
                self.extras_page = Some(0);
                self.advanced_page = 0;
            }
            (true, Some(_)) => {
                self.advanced = false;
                self.extras_page = None;
                self.advanced_page = 0;
            }
        }
        self.extras_note = None;
    }

    /// Scroll the Advanced sliders by arrow-button steps, clamped to the
    /// seven-row window. Returns whether the visible rows changed. The
    /// Advanced scrollbar widget steps through [`Self::set_advanced_scroll`].
    #[cfg(test)]
    pub fn scroll_advanced(&mut self, delta: i32) -> bool {
        let next = (i32::try_from(self.advanced_page).unwrap_or(i32::MAX) + delta)
            .clamp(0, AdvancedField::PAGE_COUNT as i32 - 1) as usize;
        if next == self.advanced_page {
            return false;
        }
        self.advanced_page = next;
        true
    }

    /// Set the scroll offset from a track press or a thumb drag.
    pub fn set_advanced_scroll(&mut self, page: usize) -> bool {
        let page = page.min(AdvancedField::PAGE_COUNT - 1);
        if page == self.advanced_page {
            return false;
        }
        self.advanced_page = page;
        true
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
        self.progress_percent = 0;
    }

    /// Release a failed worker without presenting an old/partial map as a
    /// successful result. Only finish_generate may enable Save and Accept.
    pub fn fail_generate(&mut self) {
        self.generating = false;
        self.progress_percent = 0;
        self.generated = false;
        self.generated_preview = None;
        self.exported = false;
        self.extras_note = Some((
            "VERA:RandomMapGenerateFailed",
            "Could not generate this map. Try again or choose another map.",
        ));
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
        self.progress_percent = 100;
        self.generated = true;
        self.exported = false;
        self.exported_to = None;
        self.saved_seed_buttons_enabled = true;
        if preview.is_some() {
            self.preview_generation = self.preview_generation.wrapping_add(1);
        }
        self.generated_preview = preview;
        // VERA: remember the map's options, for the Extras view's way back.
        if self.history.last() != Some(&self.options) {
            if self.history.len() == HISTORY_MAPS {
                self.history.remove(0);
            }
            self.history.push(self.options.clone());
        }
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

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::page::PageCue;

    /// Always returns `max`, so draws are identifiable.
    struct MaxRng;
    impl RandomRanged for MaxRng {
        fn ranged(&mut self, _min: i32, max: i32) -> i32 {
            max
        }

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
        )
    }

    /// VERA: a loaded seed takes over the sliders.
    #[test]
    fn a_loaded_seed_resets_the_sliders() {
        let saved = RmgOptions {
            seed: 0x1234,
            urban_enabled: true,
            urban_presence: 30,
            num_players: crate::map::rmg::MAX_PLAYERS,
            ..Default::default()
        };
        let mut offline = opened();
        assert!(offline.change_advanced(AdvancedField::Water, 77));
        offline.adopt_saved_seed(saved);
        assert_eq!(offline.advanced_values, None);
        assert_eq!(offline.advanced_value(AdvancedField::Buildings), 30);
        assert_eq!(offline.options.num_players, crate::map::rmg::MAX_PLAYERS);
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
    fn fair_starts_and_presets_edit_the_options() {
        let mut offline = opened();
        assert!(offline.is_enabled(RandomMapSetupControl::AdvancedToggle));
        offline.generated = true;
        offline.toggle_fair_starts();
        assert!(offline.options.fair_starts);
        assert!(!offline.generated, "the shown map no longer matches");

        // Presets: one press sets the whole option set, the presses go round,
        // and a player's own edit lets the preset go.
        offline.cycle_preset();
        assert_eq!(offline.preset, Some(RandomMapPreset::Duel));
        assert_eq!(offline.options.num_players, 2);
        offline.cycle_preset();
        offline.cycle_preset();
        assert_eq!(offline.preset, Some(RandomMapPreset::CityWar));
        assert!(offline.options.urban_enabled && offline.options.fair_starts);
        assert_eq!(
            (offline.options.urban_presence, offline.options.theater),
            (100, 2)
        );
        assert_eq!(offline.advanced_value(AdvancedField::Buildings), 100);
        offline.cycle_preset();
        offline.cycle_preset();
        assert_eq!(offline.preset, Some(RandomMapPreset::Teams));
        assert_eq!(
            (offline.options.num_players, offline.options.map_type),
            (8, 2)
        );
        offline.cycle_preset();
        assert_eq!(offline.preset, Some(RandomMapPreset::Duel), "round again");
        assert!(!offline.options.urban_enabled);
        offline.set_num_players(3);
        assert_eq!(offline.preset, None);
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
    }

    /// VERA's Extras view: the toggle steps Basic, Advanced p1, Advanced p2,
    /// Extras; a
    /// variant row steps its value as an option edit and asks for its map at
    /// once; a share code brings another dialog's options, seed included.
    #[test]
    fn the_extras_view_steps_choices_and_takes_share_codes() {
        use super::super::random_map_extras::{ExtrasAction, ExtrasOutcome, ExtrasRow};
        use crate::map::rmg::extras::Extra;
        let mut dialog = opened();
        assert!(dialog.extras_rows().is_empty(), "no rows outside the view");
        dialog.toggle_advanced();
        assert_eq!(
            (dialog.advanced, dialog.extras_page, dialog.advanced_page),
            (true, None, 0),
            "first toggle opens Advanced"
        );
        dialog.toggle_advanced();
        assert_eq!((dialog.advanced, dialog.extras_page), (true, Some(0)));
        let rows = dialog.extras_rows();
        let ore = rows
            .iter()
            .position(|row| *row == ExtrasRow::Choice(Extra::OreVariant))
            .expect("the first page lists the ore variant");
        dialog.generated = true;
        assert_eq!(
            dialog.release_extras_row(ore),
            Some(ExtrasOutcome::Regenerate),
            "Generate's seed roll would make another map"
        );
        assert_eq!(dialog.options.extras.get(Extra::OreVariant), 1);
        assert!(!dialog.generated, "a choice is an option edit");
        let copy = rows
            .iter()
            .position(|row| *row == ExtrasRow::Action(ExtrasAction::CopyCode))
            .expect("the first page has the share tools");
        assert_eq!(
            dialog.release_extras_row(copy),
            Some(ExtrasOutcome::Tool(ExtrasAction::CopyCode))
        );
        assert_eq!(dialog.release_extras_row(EXTRAS_ROWS_PAST), None);

        // The receiver takes the sender's options whole and shows the
        // sender's slider values.
        dialog.change_advanced(AdvancedField::Water, 37);
        let code = dialog.options.share_code(dialog.ares);
        let mut receiver = opened();
        assert_eq!(receiver.adopt_share_code("not a code"), None);
        assert_eq!(
            receiver.adopt_share_code(&code),
            Some(crate::map::rmg::options::ShareVersion::this_build())
        );
        assert_eq!(receiver.options.extras.get(Extra::OreVariant), 1);
        assert_eq!(receiver.options.seed, dialog.options.seed);
        assert_eq!(receiver.advanced_value(AdvancedField::Water), 37);

        dialog.toggle_advanced();
        assert_eq!((dialog.advanced, dialog.extras_page), (false, None));
    }

    /// VERA: the Advanced scrollbar moves the seven-row window over all
    /// thirteen sliders, stops at both ends, and lets go when the view changes.
    #[test]
    fn the_advanced_scrollbar_steps_and_clamps_the_visible_rows() {
        let mut dialog = opened();
        dialog.toggle_advanced();
        assert!(!dialog.scroll_advanced(-1));
        assert_eq!(dialog.advanced_page, 0);
        assert!(dialog.scroll_advanced(1));
        assert_eq!(dialog.advanced_page, 1);
        assert!(!dialog.scroll_advanced(1), "the last page clamps");
        assert!(dialog.set_advanced_scroll(0));
        assert_eq!(dialog.advanced_page, 0);
        assert!(dialog.set_advanced_scroll(99));
        assert_eq!(dialog.advanced_page, AdvancedField::PAGE_COUNT - 1);
        assert!(
            !dialog.set_advanced_scroll(99),
            "no change at the last page"
        );
        dialog.toggle_advanced();
        assert_eq!(dialog.advanced_page, 0);
    }

    /// A row past the view's last.
    const EXTRAS_ROWS_PAST: usize = 99;

    /// VERA: the way back returns the options of the map before the one on
    /// show, seed and all; a preset of the player's own keeps the options
    /// but not the seed, follows the ready-made ones on the button, and the
    /// sixth saved takes the first slot again.
    #[test]
    fn the_way_back_and_the_players_own_presets() {
        let mut dialog = opened();
        assert!(!dialog.step_back(), "nothing generated yet");
        dialog.set_num_players(3);
        dialog.options.seed = 111;
        dialog.finish_generate(None);
        dialog.set_num_players(5);
        dialog.options.seed = 222;
        dialog.finish_generate(None);
        assert!(dialog.step_back());
        assert_eq!((dialog.options.num_players, dialog.options.seed), (3, 111));
        assert!(!dialog.generated, "the app generates the earlier map again");
        dialog.finish_generate(None);
        assert!(!dialog.step_back(), "the first map has none before it");

        dialog.set_num_players(6);
        assert_eq!(dialog.save_custom_preset(), 0);
        assert_eq!(dialog.preset, Some(RandomMapPreset::Mine(0)));
        dialog.set_num_players(2);
        dialog.options.seed = 333;
        // Past the four ready-made presets comes the player's own.
        for _ in 0..RandomMapPreset::ALL.len() + 1 {
            dialog.cycle_preset();
        }
        assert_eq!(dialog.preset, Some(RandomMapPreset::Mine(0)));
        assert_eq!((dialog.options.num_players, dialog.options.seed), (6, 333));
        let slots: Vec<usize> = (0..CUSTOM_PRESET_SLOTS + 1)
            .map(|_| dialog.save_custom_preset())
            .collect();
        assert_eq!(slots, [1, 2, 3, 4, 0, 1]);
        assert_eq!(dialog.custom_presets.len(), CUSTOM_PRESET_SLOTS);
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
        );
        assert!(some.is_enabled(RandomMapSetupControl::Load0x6c2));
        assert!(some.is_enabled(RandomMapSetupControl::Delete0x6c4));
    }

    /// Both dialogs offer the extended controls. A network room retains its
    /// 30-player limit when it opens a 120-player offline seed.
    #[test]
    fn network_dialog_keeps_thirty_players_when_opening_an_offline_seed() {
        let mut options = RmgOptions {
            seed: 1,
            num_players: 120,
            width: 7,
            height: 7,
            theater: 3,
            urban_enabled: true,
            fair_starts: true,
            ..Default::default()
        };
        options
            .extras
            .set(crate::map::rmg::extras::Extra::ALL[0], 1);
        options.normalize();
        let mut modal = RandomMapSetupModalState::open(options.clone(), None, false);
        assert_eq!(modal.options, options);
        assert_eq!(modal.combo_items(SetupCombo::Size).len(), 8);
        assert!(modal.is_enabled(RandomMapSetupControl::AdvancedToggle));
        assert!(modal.is_enabled(RandomMapSetupControl::PresetCycle));
        assert_eq!(modal.options.num_players, 120);
        modal.use_network_player_limit();
        assert_eq!(modal.options.num_players, 30);
        modal.adopt_saved_seed(options);
        assert_eq!(modal.options.num_players, 30);
        modal.set_num_players(12);
        assert_eq!(modal.options.num_players, 12);
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
    /// 0x5970EA): archipelago leads and Desert follows Snow;
    /// Surprise Me draws the map type, then the theater, from those entries
    /// and takes the Urban Areas box from a raw draw's low bit (hook
    /// 0x596786).
    #[test]
    fn ares_lists_archipelago_and_desert_and_surprise_me_draws_from_them() {
        let values = |combo| {
            setup_combo_items(combo, true)
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>()
        };
        assert_eq!(values(SetupCombo::MapType), [0, 1, 2, 3, 4]);
        assert_eq!(values(SetupCombo::Theater), [0, 1, 3, 2]);

        let mut modal = opened();
        modal.ares = true;
        modal.randomize_options(&RmgSettings::default(), &mut MaxRng, "Random Map");
        assert_eq!(modal.options.map_type, 4, "the last map-type entry");
        assert_eq!(modal.options.theater, 2, "the last theater entry");
        assert!(modal.options.urban_areas, "u32::MAX has its low bit set");
        modal.generated = true;
        modal.toggle_urban_areas();
        assert!(!modal.options.urban_areas);
        assert!(modal.generated, "the box alone keeps the generated map");
    }

    #[test]
    fn archipelago_and_the_added_theaters_are_listed_after_the_original_entries() {
        let values = |combo| {
            setup_combo_items(combo, false)
                .iter()
                .map(|item| item.value)
                .collect::<Vec<_>>()
        };
        assert_eq!(values(SetupCombo::MapType), [1, 2, 3, 4, 0]);
        assert_eq!(values(SetupCombo::Theater), [0, 1, 2, 3]);
    }

    #[test]
    fn size_selection_reflects_the_width_axis() {
        let mut state = opened();
        state.set_combo_value(SetupCombo::Size, 2);
        assert_eq!((state.options.width, state.options.height), (2, 2));
        assert_eq!(state.selected_item_index(SetupCombo::Size), Some(2));
    }

    /// The dialog's widgets, driven as the app drives them: a press and its
    /// release at `at`; the combo cue it played.
    fn click(
        ui: &mut crate::ui::shell::controller::DialogController,
        state: &mut RandomMapSetupModalState,
        layout: &super::super::super::layout::RandomMapSetupLayout,
        at: (i32, i32),
    ) -> Option<PageCue> {
        use super::super::super::page::Pointer;
        use super::super::super::random_map_page::pointer;
        let (sounds, _) = pointer(ui, state, layout, Pointer::Down, at.0, at.1);
        pointer(ui, state, layout, Pointer::Up, at.0, at.1);
        sounds
            .into_iter()
            .find(|sound| matches!(sound, PageCue::ComboOpen | PageCue::ComboClose))
    }

    fn open_combo(
        ui: &crate::ui::shell::controller::DialogController,
        state: &RandomMapSetupModalState,
    ) -> Option<SetupCombo> {
        super::super::super::random_map_page::view(ui, state).0
    }

    #[test]
    fn picking_an_entry_closes_the_dropdown_and_invalidates_the_result() {
        let layout = super::super::super::layout::compute_random_map_setup_layout(800, 600);
        let mut ui = crate::ui::shell::controller::DialogController::default();
        let mut state = opened();
        let face = layout.control_rects[SetupCombo::Resources.row()];
        click(
            &mut ui,
            &mut state,
            &layout,
            (face.x + face.w - 4, face.y + 4),
        );
        assert_eq!(open_combo(&ui, &state), Some(SetupCombo::Resources));
        state.generated = true;
        let list = super::super::super::layout::random_map_setup_dropdown_rect(
            &layout,
            SetupCombo::Resources.row(),
            RESOURCE_ITEMS.len(),
        );
        let extreme = list.y + 3 * vera20k_gui::shell::combobox::COMBO_DROPDOWN_ROW_H + 10;
        click(&mut ui, &mut state, &layout, (list.x + 4, extreme));
        assert_eq!(open_combo(&ui, &state), None);
        assert_eq!(state.options.resources, 3);
        assert!(!state.generated);
    }

    /// A face press plays the open cue but only the arrow opens the list. With
    /// a list open, a press on a row picks it and one anywhere else only
    /// closes it; both play the close cue.
    #[test]
    fn combo_presses_open_from_the_arrow_and_close_from_anywhere() {
        use crate::ui::shell::page::PageCue::{ComboClose, ComboOpen};
        let layout = super::super::super::layout::compute_random_map_setup_layout(800, 600);
        let mut ui = crate::ui::shell::controller::DialogController::default();
        let mut state = opened();
        let time = layout.control_rects[SetupCombo::Time.row()];
        let (text, arrow) = ((time.x + 4, time.y + 4), (time.x + time.w - 4, time.y + 4));
        assert_eq!(click(&mut ui, &mut state, &layout, text), Some(ComboOpen));
        assert_eq!(open_combo(&ui, &state), None);
        assert_eq!(click(&mut ui, &mut state, &layout, arrow), Some(ComboOpen));
        assert_eq!(open_combo(&ui, &state), Some(SetupCombo::Time));
        let list = super::super::super::layout::random_map_setup_dropdown_rect(
            &layout,
            SetupCombo::Time.row(),
            setup_combo_items(SetupCombo::Time, false).len(),
        );
        let dusk_row = list.y + list.h * 5 / 8;
        assert_eq!(
            click(&mut ui, &mut state, &layout, (list.x + 4, dusk_row)),
            Some(ComboClose)
        );
        assert_eq!(
            (
                open_combo(&ui, &state),
                state.selected_item_index(SetupCombo::Time)
            ),
            (None, Some(2))
        );
        assert_eq!(click(&mut ui, &mut state, &layout, arrow), Some(ComboOpen));
        let outside = (layout.rect(0x621).x + 1, layout.rect(0x621).y + 1);
        assert_eq!(
            click(&mut ui, &mut state, &layout, outside),
            Some(ComboClose)
        );
        assert_eq!(
            (
                open_combo(&ui, &state),
                state.selected_item_index(SetupCombo::Time)
            ),
            (None, Some(2))
        );
        assert_eq!(click(&mut ui, &mut state, &layout, outside), None);
    }
}
