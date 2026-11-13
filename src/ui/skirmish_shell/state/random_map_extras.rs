//! VERA's Extras view of the Generate Map dialog (offline Skirmish only);
//! gamemd's `0x105` has no counterpart.
//!
//! The Advanced toggle steps Basic, Advanced, Extras. The Extras view takes
//! the Advanced sliders' seven rows: a row is a tool the app carries out
//! ([`ExtrasAction`]) or one of the generator's opt-in choices
//! ([`Extra`]), which a press steps to its next value. With more entries
//! than six the last row turns the page.

use crate::map::rmg::extras::Extra;
use crate::map::rmg::options::ShareVersion;

use super::random_map_setup::RandomMapSetupModalState;

/// Rows the view has, the Advanced sliders' own.
pub const EXTRAS_ROWS: usize = 7;
/// Entries on a page once the last row is the pager.
const ENTRIES_PER_PAGE: usize = EXTRAS_ROWS - 1;

/// A tool of the Extras view; pressing its row asks the app for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtrasAction {
    /// Put the options' share code on the clipboard.
    CopyCode,
    /// Read a share code from the clipboard into the options.
    PasteCode,
    /// Go back to the map generated before the one on show.
    PreviousMap,
    /// Keep the options as one of the player's own presets.
    SavePreset,
}

/// What a press on an Extras row leaves for the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtrasOutcome {
    /// Carry out a tool.
    Tool(ExtrasAction),
    /// Generate from the options as they stand, without Generate's rolls:
    /// its seed draw (`0x00597260`) would make another map of a choice that
    /// redraws part of this one. Load starts its generation the same way
    /// (`0x00596963`).
    Regenerate,
}

/// What one row of the Extras view is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtrasRow {
    Action(ExtrasAction),
    Choice(Extra),
    /// Turns to the next page, the first after the last.
    Pager,
}

/// What a row shows in its face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtrasValue {
    /// A string-table key and its English.
    Label(&'static str, &'static str),
    /// A plain number.
    Number(u8),
    /// Page `.0` of `.1`, from one.
    Page(usize, usize),
}

/// Every entry, in view order: the tools and the variants on the first
/// page, then the choices about the match, then the ones about the look.
const ENTRIES: [ExtrasRow; 16] = [
    ExtrasRow::Action(ExtrasAction::CopyCode),
    ExtrasRow::Action(ExtrasAction::PasteCode),
    ExtrasRow::Action(ExtrasAction::PreviousMap),
    ExtrasRow::Action(ExtrasAction::SavePreset),
    ExtrasRow::Choice(Extra::OreVariant),
    ExtrasRow::Choice(Extra::TownVariant),
    ExtrasRow::Choice(Extra::BaseRoom),
    ExtrasRow::Choice(Extra::SeaAccess),
    ExtrasRow::Choice(Extra::FairTech),
    ExtrasRow::Choice(Extra::CentreGems),
    ExtrasRow::Choice(Extra::Roads),
    ExtrasRow::Choice(Extra::Mood),
    ExtrasRow::Choice(Extra::Ruins),
    ExtrasRow::Choice(Extra::Countryside),
    ExtrasRow::Choice(Extra::Landmark),
    ExtrasRow::Choice(Extra::Fences),
];

/// Pages the view has.
pub fn extras_page_count() -> usize {
    if ENTRIES.len() <= EXTRAS_ROWS {
        1
    } else {
        ENTRIES.len().div_ceil(ENTRIES_PER_PAGE)
    }
}

impl ExtrasRow {
    /// The row's label: a string-table key and its English.
    pub const fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Action(ExtrasAction::CopyCode) => ("VERA:RandomMapExtraCopyCode", "Share Code"),
            Self::Action(ExtrasAction::PasteCode) => ("VERA:RandomMapExtraPasteCode", "Use a Code"),
            Self::Action(ExtrasAction::PreviousMap) => {
                ("VERA:RandomMapExtraPrevious", "Previous Map")
            }
            Self::Action(ExtrasAction::SavePreset) => ("VERA:RandomMapExtraMyPreset", "My Preset"),
            Self::Choice(Extra::OreVariant) => ("VERA:RandomMapExtraOre", "Ore Fields"),
            Self::Choice(Extra::TownVariant) => ("VERA:RandomMapExtraTowns", "Towns"),
            Self::Choice(Extra::Mood) => ("VERA:RandomMapExtraMood", "Mood"),
            Self::Choice(Extra::Ruins) => ("VERA:RandomMapExtraRuins", "War Damage"),
            Self::Choice(Extra::Countryside) => ("VERA:RandomMapExtraFarms", "Countryside"),
            Self::Choice(Extra::Landmark) => ("VERA:RandomMapExtraLandmark", "Landmark"),
            Self::Choice(Extra::Fences) => ("VERA:RandomMapExtraFences", "Town Fences"),
            Self::Choice(Extra::CentreGems) => ("VERA:RandomMapExtraGems", "Centre Gems"),
            Self::Choice(Extra::FairTech) => ("VERA:RandomMapExtraTech", "Fair Derricks"),
            Self::Choice(Extra::Roads) => ("VERA:RandomMapExtraRoads", "Roads"),
            Self::Choice(Extra::BaseRoom) => ("VERA:RandomMapExtraRoom", "Base Room"),
            Self::Choice(Extra::SeaAccess) => ("VERA:RandomMapExtraSea", "Sea Access"),
            Self::Pager => ("VERA:RandomMapExtraPage", "More"),
        }
    }

    /// The status-line help for the row.
    pub const fn help(self) -> &'static str {
        match self {
            Self::Action(ExtrasAction::CopyCode) => "VERA:RandomMapExtraCopyCodeHelp",
            Self::Action(ExtrasAction::PasteCode) => "VERA:RandomMapExtraPasteCodeHelp",
            Self::Action(ExtrasAction::PreviousMap) => "VERA:RandomMapExtraPreviousHelp",
            Self::Action(ExtrasAction::SavePreset) => "VERA:RandomMapExtraMyPresetHelp",
            Self::Choice(Extra::OreVariant) => "VERA:RandomMapExtraOreHelp",
            Self::Choice(Extra::TownVariant) => "VERA:RandomMapExtraTownsHelp",
            Self::Choice(Extra::Mood) => "VERA:RandomMapExtraMoodHelp",
            Self::Choice(Extra::Ruins) => "VERA:RandomMapExtraRuinsHelp",
            Self::Choice(Extra::Countryside) => "VERA:RandomMapExtraFarmsHelp",
            Self::Choice(Extra::Landmark) => "VERA:RandomMapExtraLandmarkHelp",
            Self::Choice(Extra::Fences) => "VERA:RandomMapExtraFencesHelp",
            Self::Choice(Extra::CentreGems) => "VERA:RandomMapExtraGemsHelp",
            Self::Choice(Extra::FairTech) => "VERA:RandomMapExtraTechHelp",
            Self::Choice(Extra::Roads) => "VERA:RandomMapExtraRoadsHelp",
            Self::Choice(Extra::BaseRoom) => "VERA:RandomMapExtraRoomHelp",
            Self::Choice(Extra::SeaAccess) => "VERA:RandomMapExtraSeaHelp",
            Self::Pager => "VERA:RandomMapExtraPageHelp",
        }
    }
}

/// What a choice shows at `value`.
fn choice_value(extra: Extra, value: u8) -> ExtrasValue {
    const OFF: ExtrasValue = ExtrasValue::Label("VERA:RandomMapExtraOff", "Off");
    const ORIGINAL: ExtrasValue = ExtrasValue::Label("VERA:RandomMapExtraOriginal", "Original");
    match (extra, value) {
        (Extra::OreVariant | Extra::TownVariant | Extra::Mood, 0) => ORIGINAL,
        (Extra::OreVariant | Extra::TownVariant, variant) => ExtrasValue::Number(variant),
        (Extra::Mood, 1) => ExtrasValue::Label("VERA:RandomMapExtraMoodGolden", "Golden Hour"),
        (Extra::Mood, 2) => ExtrasValue::Label("VERA:RandomMapExtraMoodDusk", "Red Dusk"),
        (Extra::Mood, 3) => ExtrasValue::Label("VERA:RandomMapExtraMoodMoon", "Moonlight"),
        (Extra::Mood, 4) => ExtrasValue::Label("VERA:RandomMapExtraMoodOvercast", "Overcast"),
        (Extra::Mood, _) => ExtrasValue::Label("VERA:RandomMapExtraMoodToxic", "Toxic Haze"),
        (_, 0) => OFF,
        (Extra::Ruins, 1) | (Extra::Countryside, 1) => {
            ExtrasValue::Label("VERA:RandomMapExtraSome", "Some")
        }
        (Extra::Ruins, _) => ExtrasValue::Label("VERA:RandomMapExtraHeavy", "Heavy"),
        (Extra::Countryside, _) => ExtrasValue::Label("VERA:RandomMapExtraMany", "Many"),
        (Extra::CentreGems, 1) => ExtrasValue::Label("VERA:RandomMapExtraSmall", "Small"),
        (Extra::CentreGems, _) => ExtrasValue::Label("VERA:RandomMapExtraLarge", "Large"),
        (
            Extra::Landmark
            | Extra::Fences
            | Extra::FairTech
            | Extra::Roads
            | Extra::BaseRoom
            | Extra::SeaAccess,
            _,
        ) => ExtrasValue::Label("VERA:RandomMapExtraOn", "On"),
    }
}

impl RandomMapSetupModalState {
    /// The rows of the Extras page on show, top down; none outside the view.
    pub fn extras_rows(&self) -> Vec<ExtrasRow> {
        let Some(page) = self.extras_page else {
            return Vec::new();
        };
        if extras_page_count() == 1 {
            return ENTRIES.to_vec();
        }
        let mut rows: Vec<ExtrasRow> = ENTRIES
            .iter()
            .copied()
            .skip(page * ENTRIES_PER_PAGE)
            .take(ENTRIES_PER_PAGE)
            .collect();
        // The pager keeps the last row on every page.
        rows.resize(ENTRIES_PER_PAGE, ExtrasRow::Pager);
        rows.push(ExtrasRow::Pager);
        rows
    }

    /// What the row at `row` of the page on show is; a filler row (a short
    /// last page) is none.
    pub fn extras_row(&self, row: usize) -> Option<ExtrasRow> {
        let rows = self.extras_rows();
        let entry = *rows.get(row)?;
        let filler = entry == ExtrasRow::Pager && row + 1 != rows.len();
        (!filler).then_some(entry)
    }

    /// What a row shows in its face.
    pub fn extras_value(&self, row: ExtrasRow) -> ExtrasValue {
        match row {
            ExtrasRow::Action(ExtrasAction::CopyCode) => {
                ExtrasValue::Label("VERA:RandomMapExtraCopy", "Copy")
            }
            ExtrasRow::Action(ExtrasAction::PasteCode) => {
                ExtrasValue::Label("VERA:RandomMapExtraPaste", "Paste")
            }
            ExtrasRow::Action(ExtrasAction::PreviousMap) => {
                ExtrasValue::Label("VERA:RandomMapExtraBack", "Back")
            }
            ExtrasRow::Action(ExtrasAction::SavePreset) => {
                ExtrasValue::Label("VERA:RandomMapExtraSave", "Save")
            }
            ExtrasRow::Choice(extra) => choice_value(extra, self.options.extras.get(extra)),
            ExtrasRow::Pager => {
                ExtrasValue::Page(self.extras_page.unwrap_or(0) + 1, extras_page_count())
            }
        }
    }

    /// A press released on row `row`: a choice steps to its next value, as
    /// an option edit, and asks for its map at once (each redraws or adds to
    /// the map of the seed on show); the pager turns the page; a tool is
    /// handed back for the app to carry out.
    pub fn release_extras_row(&mut self, row: usize) -> Option<ExtrasOutcome> {
        match self.extras_row(row)? {
            // The way back is the dialog's own: the earlier options return
            // and the app only generates them.
            ExtrasRow::Action(ExtrasAction::PreviousMap) => {
                self.step_back().then_some(ExtrasOutcome::Regenerate)
            }
            ExtrasRow::Action(action) => Some(ExtrasOutcome::Tool(action)),
            ExtrasRow::Choice(extra) => {
                self.options.extras.cycle(extra);
                self.note_option_edit();
                Some(ExtrasOutcome::Regenerate)
            }
            ExtrasRow::Pager => {
                self.extras_page = self
                    .extras_page
                    .map(|page| (page + 1) % extras_page_count());
                None
            }
        }
    }

    /// Take a share code's options, as an option edit, and give the VERA
    /// version that made it; `None` for a text that is no whole code. The app
    /// then generates without Generate's rolls, which builds the sender's map
    /// when the versions match. The Advanced values are held at the code's,
    /// as if the player had set each slider, so the sliders show them and a
    /// later Generate keeps them.
    pub fn adopt_share_code(&mut self, code: &str) -> Option<ShareVersion> {
        let (options, made_by) = self.options.from_share_code(code)?;
        self.options = options;
        self.pin_advanced_values();
        self.note_option_edit();
        Some(made_by)
    }
}
