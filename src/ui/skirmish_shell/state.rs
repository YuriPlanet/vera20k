//! Skirmish shell state and hit testing.

mod choose_map;
mod combos;
mod hit_test;
mod launch;
mod opponent_scroll;
mod player_name;
mod random_map_extras;
mod random_map_setup;
pub(crate) mod saved_seed_browser;
mod saved_setup;
mod team_menus;
mod team_starts;
mod trackbars;

pub(crate) use combos::{apply_combo_selection, combo_open};
pub use combos::{
    available_color_choices, available_start_choices, combo_dropdown_max_visible_rows,
    combo_enabled, combo_items, combo_rect, country_choices, selected_combo_item,
    selected_combo_item_index, team_choices, team_items,
};
pub(crate) use hit_test::status_help_key_for_hover_with_catalog;
pub use hit_test::{
    status_help_key_for_choose_map_hover, status_help_key_for_hover,
    status_help_key_for_random_map_setup,
};
pub use launch::{
    launch_session, pack_launch_session_without_start_validation, start_skips_side_gates,
};
pub use opponent_scroll::{opponent_scroll_geometry, opponent_scroll_rows};

pub use choose_map::{
    ChooseMapContext, ChooseMapModalState, ChooseMapPrompt, ChooseMapPromptAnswer,
    ChooseMapPromptButton, ChooseMapPromptKind, ChooseMapSelection, FAVORITES_ROW, ONLINE_MAPS_ROW,
    RANDOM_MAPS_ROW, SEARCH_MAX_CHARS, SkirmishModeSelection, ai_rows_beyond_limit_occupied,
    favorite_key,
};
pub(crate) use player_name::CountryMenuCatalog;
pub use player_name::{
    PLAYER_NAME_DEFAULT, PLAYER_NAME_EDIT_LIMIT_BYTES, SkirmishShellDialog, SkirmishShellState,
    accept_selected_map, initialize_rows_for_selected_map, player_name_from_profile,
    player_row_visible, repair_teams_for_selected_mode, selected_player_rows,
    skirmish_session_options,
};
pub use random_map_extras::{
    EXTRAS_ROWS, ExtrasAction, ExtrasOutcome, ExtrasRow, ExtrasValue, extras_page_count,
};
pub use random_map_setup::{
    ADVANCED_RANGE, AcceptOutcome, CUSTOM_PRESET_SLOTS, MainRngRanged, PLAYERS_RANGE,
    RandomMapPreset, RandomMapSetupModalState, SETUP_COMBO_ROWS, SetupCombo, SetupComboItem,
    setup_combo_items,
};
pub use saved_seed_browser::{
    SAVED_SEED_DESCRIPTION_MAX_UNITS, SavedSeedBrowserRow, SavedSeedBrowserState,
    SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose,
};
pub use saved_setup::{SkirmishSavedSetup, SkirmishSetupComputer, SkirmishSetupSeat};
pub use team_menus::{
    MenuPick, apply_seat_action, apply_team_colors, apply_team_preset, apply_team_starts,
    balance_teams_by_strength, team_start_seats,
};
pub use team_starts::starts_by_team;
pub use trackbars::SkirmishTrackbarBounds;

use crate::skirmish_launch::{
    AiDifficulty, HOUSE_COLOR_COUNT, LaunchCountry, LaunchStartPosition, SKIRMISH_MAX_SLOT_COUNT,
};

use self::player_name::inactive_ai_team_default;
#[cfg(test)]
use self::trackbars::CREDITS_MAX;
use super::layout::{ChooseMapModalButton, SkirmishCheckboxId, SkirmishTrackbarId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerDrawButton {
    StartGame0x617,
    ChooseMap0x5aa,
    /// VERA's Game Options ([`super::game_options`]).
    GameOptions,
    /// VERA's Setups: the page that saves and loads named setups.
    Setups,
    Back0x5c0,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishShellAction {
    None,
    StartGame,
    BackOrExit,
    ChooseMap,
    /// The app opens the box on the rules the profile keeps.
    GameOptions,
    /// The app owns the saved setups.
    Setups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishComboId {
    AiType(usize),
    Side(usize),
    Color(usize),
    Start(usize),
    Team(usize),
    /// VERA's Quick Team Setup under the rows ([`team_menus`]).
    TeamMenu,
    /// VERA's Bulk Seat Actions beside it.
    SeatMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishCountryChoice {
    Random,
    Country(LaunchCountry),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishComboItem {
    AiType(SkirmishAiRowType),
    Country(SkirmishCountryChoice),
    ColorSentinel(i32),
    Color(usize),
    Start(LaunchStartPosition),
    Team(i32),
    /// A Quick Team Setup item, greyed with why while `refusal` is set.
    TeamTool {
        tool: crate::ui::team_tools::TeamTool,
        refusal: Option<crate::ui::team_tools::ToolRefusal>,
    },
    /// The heading over the quick teams, which picks nothing.
    TeamHeading,
    /// A Bulk Seat Actions item.
    SeatAction {
        action: crate::ui::team_tools::SeatAction,
        enabled: bool,
    },
}

impl SkirmishComboItem {
    /// Whether a click on this item picks it: a menu's greyed items and
    /// heading take none.
    pub fn pickable(self) -> bool {
        match self {
            Self::TeamTool { refusal, .. } => refusal.is_none(),
            Self::TeamHeading => false,
            Self::SeatAction { enabled, .. } => enabled,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishHoverTarget {
    OwnerDrawButton(OwnerDrawButton),
    PlayerName0x6a0,
    MapPreview0x468,
    StatusHelp0x695,
    Checkbox(SkirmishCheckboxId),
    Trackbar(SkirmishTrackbarId),
    ComboFace(SkirmishComboId),
    ComboItem {
        id: SkirmishComboId,
        item: SkirmishComboItem,
    },
    // Static (non-interactive) controls in dialog 0x102 that still produce
    // status help on hover. Flag pictures 0x6DA..0x6E1 share one STT key, so a
    // single variant covers all eight.
    FlagPicture,
    GameTypeLabel0x6ec,
    ScenarioLabel0x5a8,
    /// A control of the open Game Options box, which covers the page.
    GameOptions(super::game_options::GameOptionsControl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapHoverTarget {
    ModeList0x6eb,
    ModeListRow0x6eb { mode: SkirmishModeSelection },
    MapList0x553,
    Preview0x468,
    StatusHelp0x695,
    Button(ChooseMapModalButton),
}

/// The combo whose list is open, as [`super::page::open_combo`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenComboDropdown {
    pub id: SkirmishComboId,
    pub top_index: usize,
    /// The popup's preview row; the saved choice changes only on a click.
    pub hot_row: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropdownScrollbarPart {
    UpArrow,
    DownArrow,
    Thumb,
    Track,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishAiRowType {
    None,
    Easy,
    Normal,
    Hard,
}

impl SkirmishAiRowType {
    pub const fn item_data(self) -> i32 {
        match self {
            Self::None => -1,
            Self::Easy => 2,
            Self::Normal => 1,
            Self::Hard => 0,
        }
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Easy | Self::Normal | Self::Hard)
    }

    pub const fn difficulty(self) -> Option<AiDifficulty> {
        match self {
            Self::None => None,
            Self::Easy => Some(AiDifficulty::Easy),
            Self::Normal => Some(AiDifficulty::Normal),
            Self::Hard => Some(AiDifficulty::Hard),
        }
    }

    pub const fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::None => ("GUI:None", "None"),
            Self::Easy => ("GUI:AIEasy", "Easy"),
            Self::Normal => ("GUI:AINormal", "Normal"),
            Self::Hard => ("GUI:AIHard", "Hard"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishShellOpponent {
    pub enabled: bool,
    pub row_type: SkirmishAiRowType,
    pub country: LaunchCountry,
    pub country_random: bool,
    pub color_index: usize,
    pub color_claimed: bool,
    pub start_position: LaunchStartPosition,
    pub team: i32,
    pub difficulty: AiDifficulty,
}

impl SkirmishShellOpponent {
    pub const fn is_active(&self) -> bool {
        self.row_type.is_active()
    }

    fn apply_inactive_combo_defaults(&mut self, team_default: i32) {
        self.enabled = false;
        self.country_random = true;
        self.color_claimed = false;
        self.start_position = LaunchStartPosition::Auto;
        self.team = team_default;
    }
}

/// Fresh-profile opponent rows for a clean offline skirmish (no persisted slot
/// keys). The first opponent row defaults to an active Easy AI; all other rows
/// default to None (inactive). This is the net end state of the engine's
/// dialog-init: an intermediate pass resets every AI combo to None, then a
/// second pass applies the persisted/default slot type codes — the first slot's
/// default code maps to Easy and the rest map to None. Do NOT change this to
/// all-None; the player-observable clean-profile shell shows exactly one Easy
/// opponent. When persisted slot reading is added, the per-row fallbacks must be
/// the Easy code for row 1 and the None code for rows 2-7, mapped through the
/// slot type-code table, not hardcoded all-None.
///
/// VERA: the rows go on past the retail seven to 29, for maps with up to
/// thirty starts; each opens as None, as rows 2-7 do.
fn default_opponents(first_country: LaunchCountry) -> Vec<SkirmishShellOpponent> {
    let countries = [
        first_country,
        LaunchCountry::Cuba,
        LaunchCountry::Libya,
        LaunchCountry::Iraq,
        LaunchCountry::America,
        LaunchCountry::Korea,
        LaunchCountry::Germany,
    ];

    (0..SKIRMISH_MAX_SLOT_COUNT - 1)
        .map(|idx| {
            let country = countries[idx % countries.len()];
            let row_type = if idx == 0 {
                SkirmishAiRowType::Easy
            } else {
                SkirmishAiRowType::None
            };
            let mut opponent = SkirmishShellOpponent {
                enabled: idx == 0,
                row_type,
                country,
                country_random: false,
                color_index: (idx + 1) % HOUSE_COLOR_COUNT,
                color_claimed: row_type.is_active(),
                start_position: LaunchStartPosition::Auto,
                team: 3,
                difficulty: AiDifficulty::Easy,
            };
            if !row_type.is_active() {
                opponent.apply_inactive_combo_defaults(3);
            }
            opponent
        })
        .collect()
}

#[cfg(all(test, tests_ui))]
mod tests;
