//! Skirmish shell state and hit testing.

mod choose_map;
mod combos;
mod hit_test;
mod launch;
mod player_name;
mod random_map_setup;
mod saved_seed_browser;
mod trackbars;

#[cfg(test)]
use self::combos::{combo_dropdown_max_top_index, top_index_from_scrollbar_track_click};
use self::combos::{handle_combo_mouse_down, set_open_combo_top_index, top_index_from_thumb_y};

pub use combos::{
    combo_dropdown_content_rect, combo_dropdown_needs_scrollbar, combo_dropdown_rect,
    combo_dropdown_scroll_thumb_rect, combo_dropdown_scrollbar_rect,
    combo_dropdown_visible_row_count, combo_enabled, combo_items, combo_rect, selected_combo_item,
    selected_combo_item_index,
};
pub use hit_test::{
    action_for_owner_draw_button, apply_action, hit_test, hit_test_owner_draw_button,
    hovered_choose_map_modal_control, hovered_shell_control, status_help_key_for_choose_map_hover,
    status_help_key_for_hover,
};
pub use launch::{launch_session, launch_settings, pack_launch_session_without_start_validation};

pub use choose_map::{
    ChooseMapListPress, ChooseMapModalState, ChooseMapSelection, EjectPrompt, EjectPromptButton,
    ai_rows_beyond_limit_occupied,
};
pub use player_name::{
    PLAYER_NAME_CARET_MARGIN_PX, PLAYER_NAME_DEFAULT, PLAYER_NAME_EDIT_LIMIT_BYTES,
    PlayerNameEditState, SkirmishShellState, accept_selected_map, blur_player_name_edit,
    clear_status_help_text, combo_dropdown_open, dismiss_validation_modal, drain_pending_ui_sounds,
    focus_player_name_edit, handle_player_name_backspace, handle_player_name_delete,
    handle_player_name_end, handle_player_name_home, handle_player_name_left,
    handle_player_name_right, handle_player_name_tab, initialize_rows_for_selected_map,
    insert_player_name_text, player_name_caret_prefix, player_name_edit_rect_hit,
    player_row_visible, repair_teams_for_selected_mode, set_status_help_text,
    update_player_name_scroll_for_caret,
};
pub use random_map_setup::{
    AcceptOutcome, RandomMapSetupModalState, SETUP_COMBO_ROWS, SetupCombo, SetupComboItem,
    setup_combo_items,
};
pub use saved_seed_browser::{SAVED_SEED_DESCRIPTION_MAX_UNITS, SavedSeedBrowserState, SavedSeedBrowserRow, SavedSeedOutcome, SavedSeedPrompt, SavedSeedPromptPurpose};
pub use trackbars::{
    SkirmishTrackbarBounds, SkirmishTrackbarHScrollNotification, TrackbarDragState,
    game_speed_from_visual_position, game_speed_visual_position, handle_option_mouse_down,
    handle_option_mouse_move, handle_option_mouse_up, handle_option_mouse_wheel,
    trackbar_mouse_allowed_y, trackbar_mouse_value, trackbar_thumb_hit, trackbar_visual_value,
};

use crate::skirmish_launch::{AiDifficulty, HOUSE_COLOR_COUNT};
use crate::ui::main_menu::{SkirmishCountry, StartPosition};

use self::player_name::inactive_ai_team_default;
#[cfg(test)]
use self::trackbars::{CREDITS_MAX, CREDITS_MIN, CREDITS_STEP};
use super::layout::{ChooseMapModalButton, ColorComboId, SkirmishCheckboxId, SkirmishTrackbarId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerDrawButton {
    StartGame0x617,
    ChooseMap0x5aa,
    Back0x5c0,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishShellAction {
    None,
    StartGame,
    BackOrExit,
    ChooseMap,
    SelectColor(ColorComboId),
    SelectMap(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishShellUiSound {
    GuiCheckboxSound,
    GenericClick,
    GuiComboOpenSound,
    GuiComboCloseSound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishComboId {
    AiType(usize),
    Side(usize),
    Color(usize),
    Start(usize),
    Team(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishCountryChoice {
    Random,
    Country(SkirmishCountry),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishComboItem {
    AiType(SkirmishAiRowType),
    Country(SkirmishCountryChoice),
    ColorSentinel(i32),
    Color(usize),
    Start(StartPosition),
    Team(i32),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapHoverTarget {
    ModeList0x6eb,
    ModeListRow0x6eb { mode_id: i32 },
    MapList0x553,
    Preview0x468,
    StatusHelp0x695,
    Button(ChooseMapModalButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenComboDropdown {
    pub id: SkirmishComboId,
    pub top_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropdownScrollDragState {
    pub id: SkirmishComboId,
    pub grab_offset_y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropdownScrollbarPart {
    UpArrow,
    DownArrow,
    Thumb,
    Track,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropdownScrollbarPressState {
    pub id: SkirmishComboId,
    pub part: DropdownScrollbarPart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishValidationModalState {
    pub message: String,
    pub ok_button: String,
    pub ok_button_pressed: bool,
}

impl SkirmishValidationModalState {
    pub fn new(message: impl Into<String>, ok_button: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ok_button: ok_button.into(),
            ok_button_pressed: false,
        }
    }
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
    pub country: SkirmishCountry,
    pub country_random: bool,
    pub color_index: usize,
    pub color_claimed: bool,
    pub start_position: StartPosition,
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
        self.start_position = StartPosition::Auto;
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
fn default_opponents(first_country: SkirmishCountry) -> Vec<SkirmishShellOpponent> {
    let countries = [
        first_country,
        SkirmishCountry::Cuba,
        SkirmishCountry::Libya,
        SkirmishCountry::Iraq,
        SkirmishCountry::America,
        SkirmishCountry::Korea,
        SkirmishCountry::Germany,
    ];

    countries
        .into_iter()
        .enumerate()
        .map(|(idx, country)| {
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
                start_position: StartPosition::Auto,
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

#[cfg(test)]
mod tests;
