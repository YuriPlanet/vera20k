//! VERA's Extras view of the Generate Map dialog (offline Skirmish only);
//! gamemd's `0x105` has no counterpart.
//!
//! The Advanced toggle steps Basic, Advanced, Extras. The Extras view takes
//! the Advanced sliders' seven rows: a row is a tool the app carries out
//! ([`ExtrasAction`]) or one of the generator's opt-in choices
//! ([`Extra`]), which a press steps to its next value. With more entries
//! than six the last row turns the page.

use crate::map::rmg::extras::Extra;

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

/// Every entry, in view order: the tools, then the choices.
const ENTRIES: [ExtrasRow; 4] = [
    ExtrasRow::Action(ExtrasAction::CopyCode),
    ExtrasRow::Action(ExtrasAction::PasteCode),
    ExtrasRow::Choice(Extra::OreVariant),
    ExtrasRow::Choice(Extra::TownVariant),
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
            Self::Choice(Extra::OreVariant) => ("VERA:RandomMapExtraOre", "Ore Fields"),
            Self::Choice(Extra::TownVariant) => ("VERA:RandomMapExtraTowns", "Towns"),
            Self::Pager => ("VERA:RandomMapExtraPage", "More"),
        }
    }

    /// The status-line help for the row.
    pub const fn help(self) -> &'static str {
        match self {
            Self::Action(ExtrasAction::CopyCode) => "VERA:RandomMapExtraCopyCodeHelp",
            Self::Action(ExtrasAction::PasteCode) => "VERA:RandomMapExtraPasteCodeHelp",
            Self::Choice(Extra::OreVariant) => "VERA:RandomMapExtraOreHelp",
            Self::Choice(Extra::TownVariant) => "VERA:RandomMapExtraTownsHelp",
            Self::Pager => "VERA:RandomMapExtraPageHelp",
        }
    }
}

/// What a choice shows at `value`.
fn choice_value(extra: Extra, value: u8) -> ExtrasValue {
    match (extra, value) {
        (Extra::OreVariant | Extra::TownVariant, 0) => {
            ExtrasValue::Label("VERA:RandomMapExtraOriginal", "Original")
        }
        (Extra::OreVariant | Extra::TownVariant, variant) => ExtrasValue::Number(variant),
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
            ExtrasRow::Choice(extra) => choice_value(extra, self.options.extras.get(extra)),
            ExtrasRow::Pager => {
                ExtrasValue::Page(self.extras_page.unwrap_or(0) + 1, extras_page_count())
            }
        }
    }

    /// A press released on row `row`: a choice steps to its next value, as
    /// an option edit, and a variant choice asks for its map at once; the
    /// pager turns the page; a tool is handed back for the app to carry out.
    pub fn release_extras_row(&mut self, row: usize) -> Option<ExtrasOutcome> {
        match self.extras_row(row)? {
            ExtrasRow::Action(action) => Some(ExtrasOutcome::Tool(action)),
            ExtrasRow::Choice(extra) => {
                self.options.extras.cycle(extra);
                self.note_option_edit();
                matches!(extra, Extra::OreVariant | Extra::TownVariant)
                    .then_some(ExtrasOutcome::Regenerate)
            }
            ExtrasRow::Pager => {
                self.extras_page = self
                    .extras_page
                    .map(|page| (page + 1) % extras_page_count());
                None
            }
        }
    }

    /// Take a share code's options, as an option edit; false for a text that
    /// is no whole code. The app then generates without Generate's rolls,
    /// which builds the sender's map. The Advanced values are held at the
    /// code's, as if the player had set each slider, so the sliders show
    /// them and a later Generate keeps them.
    pub fn adopt_share_code(&mut self, code: &str) -> bool {
        let Some(options) = self.options.from_share_code(code) else {
            return false;
        };
        self.options = options;
        self.pin_advanced_values();
        self.note_option_edit();
        true
    }
}
