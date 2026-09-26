//! Player-name edit state and input helpers for the skirmish shell.

use crate::map::scenario_menu::MapMenuEntry;
use crate::sim::game_options::GameOptions;
use crate::skirmish_launch::{SKIRMISH_PLAYER_SLOT_COUNT, SkirmishLaunchOptions};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};
use crate::ui::main_menu::{SkirmishCountry, SkirmishSettings, StartPosition};

use super::super::SkirmishStatics;
use super::super::layout::{SkirmishShellLayout, SkirmishTrackbarId};
use super::trackbars::{SkirmishTrackbarBounds, trackbar_control_id, trackbar_hscroll_wparam};
use super::{
    ChooseMapModalState, DropdownScrollDragState, DropdownScrollbarPressState, OpenComboDropdown,
    OwnerDrawButton, RandomMapSetupModalState, SkirmishShellOpponent, SkirmishShellUiSound,
    SkirmishTrackbarHScrollNotification, SkirmishValidationModalState, TrackbarDragState,
    default_opponents,
};

pub const PLAYER_NAME_DEFAULT: &str = "Player";
pub const PLAYER_NAME_EDIT_LIMIT_BYTES: usize = 19;
pub const PLAYER_NAME_CARET_MARGIN_PX: i32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerNameEditState {
    pub text: String,
    pub focused: bool,
    pub selection: Option<(usize, usize)>,
    pub caret: usize,
    pub scroll_x: i32,
}

impl Default for PlayerNameEditState {
    fn default() -> Self {
        Self::with_name(PLAYER_NAME_DEFAULT)
    }
}

impl PlayerNameEditState {
    /// Seed the ANSI edit from the persistent player profile. `WM_SETTEXT` is
    /// programmatic and is not truncated by the edit's later `EM_LIMITTEXT`.
    pub fn with_name(name: &str) -> Self {
        let text: String = crate::util::native_string::acp_round_trip(name)
            .chars()
            .filter(|ch| !matches!(*ch, '\r' | '\n'))
            .collect();
        let caret = text.chars().count();
        Self {
            text,
            focused: false,
            selection: None,
            caret,
            scroll_x: 0,
        }
    }
}

impl PlayerNameEditState {
    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    fn clamp_position(&self, position: usize) -> usize {
        position.min(self.char_len())
    }

    fn byte_index(&self, char_index: usize) -> usize {
        if char_index == 0 {
            return 0;
        }
        self.text
            .char_indices()
            .nth(char_index)
            .map(|(idx, _)| idx)
            .unwrap_or(self.text.len())
    }

    fn normalized_selection(&self) -> Option<(usize, usize)> {
        let (start, end) = self.selection?;
        let start = self.clamp_position(start);
        let end = self.clamp_position(end);
        if start == end {
            None
        } else if start < end {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }

    fn delete_range(&mut self, start: usize, end: usize) {
        let start_byte = self.byte_index(start);
        let end_byte = self.byte_index(end);
        self.text.replace_range(start_byte..end_byte, "");
        self.caret = start;
        self.selection = None;
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.normalized_selection() else {
            return false;
        };
        self.delete_range(start, end);
        true
    }

    fn clamp_state(&mut self) {
        let len = self.char_len();
        self.caret = self.caret.min(len);
        self.selection = self.selection.and_then(|(start, end)| {
            let start = start.min(len);
            let end = end.min(len);
            (start != end).then_some((start, end))
        });
        self.scroll_x = self.scroll_x.max(0);
    }

    pub fn focus_select_all(&mut self) -> bool {
        let changed = !self.focused
            || self.selection != Some((0, self.char_len()))
            || self.caret != self.char_len();
        self.focused = true;
        let len = self.char_len();
        self.selection = (len > 0).then_some((0, len));
        self.caret = len;
        self.scroll_x = 0;
        changed
    }

    pub fn blur(&mut self) -> bool {
        let changed = self.focused || self.selection.is_some();
        self.focused = false;
        self.selection = None;
        self.clamp_state();
        changed
    }

    pub fn insert_text(&mut self, text: &str) -> bool {
        let filtered: String = crate::util::native_string::acp_round_trip(text)
            .chars()
            .filter(|ch| !ch.is_control())
            .collect();
        if filtered.is_empty() {
            return false;
        }

        let deleted_selection = self.delete_selection();
        let capacity = PLAYER_NAME_EDIT_LIMIT_BYTES
            .saturating_sub(crate::util::native_string::acp_encode(&self.text).len());
        if capacity == 0 {
            return deleted_selection;
        }
        let mut remaining = capacity;
        let mut inserted = String::new();
        for character in filtered.chars() {
            let encoded_len = crate::util::native_string::acp_encode(&character.to_string()).len();
            if encoded_len > remaining {
                break;
            }
            remaining -= encoded_len;
            inserted.push(character);
        }
        if inserted.is_empty() {
            return deleted_selection;
        }

        let byte = self.byte_index(self.caret);
        self.text.insert_str(byte, &inserted);
        self.caret += inserted.chars().count();
        self.selection = None;
        self.clamp_state();
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            self.clamp_state();
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let end = self.caret;
        self.delete_range(end - 1, end);
        self.clamp_state();
        true
    }

    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            self.clamp_state();
            return true;
        }
        let len = self.char_len();
        if self.caret >= len {
            return false;
        }
        self.delete_range(self.caret, self.caret + 1);
        self.clamp_state();
        true
    }

    pub fn move_left(&mut self) -> bool {
        let old = (self.caret, self.selection);
        self.selection = None;
        self.caret = self.caret.saturating_sub(1);
        old != (self.caret, self.selection)
    }

    pub fn move_right(&mut self) -> bool {
        let old = (self.caret, self.selection);
        self.selection = None;
        self.caret = (self.caret + 1).min(self.char_len());
        old != (self.caret, self.selection)
    }

    pub fn move_home(&mut self) -> bool {
        let old = (self.caret, self.selection);
        self.selection = None;
        self.caret = 0;
        old != (self.caret, self.selection)
    }

    pub fn move_end(&mut self) -> bool {
        let old = (self.caret, self.selection);
        self.selection = None;
        self.caret = self.char_len();
        old != (self.caret, self.selection)
    }

    pub fn caret_prefix(&self) -> &str {
        &self.text[..self.byte_index(self.caret)]
    }
}

#[derive(Debug, Clone)]
pub struct SkirmishShellState {
    pub selected_map_idx: usize,
    pub selected_mode_id: i32,
    pub player_name_edit: PlayerNameEditState,
    pub player_country: SkirmishCountry,
    pub player_country_random: bool,
    pub player_color_index: usize,
    pub player_color_claimed: bool,
    pub player_start_position: StartPosition,
    pub player_team: i32,
    pub starting_credits: i32,
    pub game_speed: i32,
    pub unit_count: i32,
    /// Credits/Unit Count slider ranges, seeded from `[MultiplayerDialogSettings]`
    /// at lobby construction (stock-default constants until then).
    pub trackbar_bounds: SkirmishTrackbarBounds,
    /// Per-match option base, seeded from `[MultiplayerDialogSettings]` at lobby
    /// construction (stock defaults until then). The launch path overrides the
    /// fields exposed as widgets from the live state above; this base carries
    /// the non-widget toggles (tech level, bases, shroud, …) into the match.
    pub launch_options_base: SkirmishLaunchOptions,
    pub short_game: bool,
    pub super_weapons: bool,
    pub build_off_ally: bool,
    pub crates: bool,
    pub mcv_redeploy: bool,
    pub zoom_enabled: bool,
    pub opponents: Vec<SkirmishShellOpponent>,
    pub selected_mode_allies_allowed: bool,
    pub selected_mode_must_ally: bool,
    pub pressed_owner_draw_button: Option<OwnerDrawButton>,
    pub trackbar_drag: Option<TrackbarDragState>,
    pub dropdown_scroll_drag: Option<DropdownScrollDragState>,
    pub dropdown_scroll_press: Option<DropdownScrollbarPressState>,
    pub open_combo_dropdown: Option<OpenComboDropdown>,
    pub choose_map_modal: Option<ChooseMapModalState>,
    /// The Create Random Map dialog, opened over the choose-map modal.
    pub random_map_setup_modal: Option<RandomMapSetupModalState>,
    /// The Load / Save / Delete browser the setup dialog opens over itself.
    /// Sits beside the setup modal rather than inside it because it replaces
    /// the setup dialog on screen while that dialog keeps its working options.
    pub saved_seed_browser: Option<super::SavedSeedBrowserState>,
    pub validation_modal: Option<SkirmishValidationModalState>,
    pub pending_trackbar_hscrolls: Vec<SkirmishTrackbarHScrollNotification>,
    pub pending_ui_sounds: Vec<SkirmishShellUiSound>,
    /// The kind-1 statics; the status line holds the hover help.
    pub(crate) statics: SkirmishStatics,
}

/// A family dialog of the Skirmish stack. Choose Map `0x6B` hides `0x102`
/// and the random-map dialog `0x105` hides `0x6B`; the seed browser (`0xB7`,
/// `0x2B4`, `0x2B5`) opens over `0x105` without hiding it (`0x00558DD0`
/// creates and shows its own dialog, `0x00622650`, `0x00622800`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishShellDialog {
    Skirmish,
    ChooseMap,
    RandomMap,
    SeedBrowser,
}

impl SkirmishShellState {
    /// The dialog that shows on top of the Skirmish stack: the one decision
    /// for the slide target, the renderer and input. `None` while the
    /// chooser stays hidden after the random-map run, behind its Use Map's
    /// eject box.
    pub fn top_dialog(&self) -> Option<SkirmishShellDialog> {
        let Some(chooser) = self.choose_map_modal.as_ref() else {
            return Some(SkirmishShellDialog::Skirmish);
        };
        if self.saved_seed_browser.is_some() {
            Some(SkirmishShellDialog::SeedBrowser)
        } else if self.random_map_setup_modal.is_some() {
            Some(SkirmishShellDialog::RandomMap)
        } else if chooser.is_hidden() {
            None
        } else {
            Some(SkirmishShellDialog::ChooseMap)
        }
    }
}

impl Default for SkirmishShellState {
    fn default() -> Self {
        let settings = SkirmishSettings::default();
        let options = SkirmishLaunchOptions::default();
        Self {
            selected_map_idx: settings.selected_map_idx,
            selected_mode_id: 1,
            player_name_edit: PlayerNameEditState::default(),
            player_country: settings.player_country,
            player_country_random: false,
            player_color_index: 0,
            player_color_claimed: true,
            // Native population selects Random when the local row owns no
            // numbered start reservation.
            player_start_position: StartPosition::Auto,
            player_team: -2,
            starting_credits: options.starting_credits,
            game_speed: options.game_speed,
            unit_count: options.unit_count,
            trackbar_bounds: SkirmishTrackbarBounds::default(),
            launch_options_base: SkirmishLaunchOptions::default(),
            short_game: options.short_game,
            super_weapons: options.super_weapons,
            build_off_ally: options.build_off_ally,
            crates: options.crates,
            mcv_redeploy: options.mcv_redeploy,
            zoom_enabled: settings.zoom_enabled,
            opponents: default_opponents(settings.ai_country),
            selected_mode_allies_allowed: true,
            selected_mode_must_ally: false,
            pressed_owner_draw_button: None,
            trackbar_drag: None,
            dropdown_scroll_drag: None,
            dropdown_scroll_press: None,
            open_combo_dropdown: None,
            choose_map_modal: None,
            random_map_setup_modal: None,
            saved_seed_browser: None,
            validation_modal: None,
            pending_trackbar_hscrolls: Vec::new(),
            pending_ui_sounds: Vec::new(),
            statics: SkirmishStatics::default(),
        }
    }
}

impl SkirmishShellState {
    pub const TRACKBAR_WM_HSCROLL_MESSAGE: u32 = 0x114;
    pub const TRACKBAR_HSCROLL_CHANGED_LOW_WORD: u16 = 5;

    /// Seed the lobby from per-match options parsed from
    /// `[MultiplayerDialogSettings]`. The values the setup dialog exposes as
    /// widgets are copied into the live fields so each control opens on the
    /// configured value; the full set is also retained as the launch base so
    /// the non-widget toggles reach the match unchanged. `GameSpeed` is stored
    /// as parsed — the trackbar inverts it only for display.
    pub fn apply_multiplayer_dialog_values(&mut self, options: &GameOptions) {
        self.starting_credits = options.starting_credits;
        self.unit_count = options.unit_count;
        self.game_speed = options.game_speed;
        self.short_game = options.short_game;
        self.super_weapons = options.super_weapons;
        self.build_off_ally = options.build_off_ally;
        self.crates = options.crates;
        self.mcv_redeploy = options.mcv_redeploy;
        self.launch_options_base = SkirmishLaunchOptions::from_game_options(options);
    }

    pub(super) fn push_ui_sound(&mut self, sound: SkirmishShellUiSound) {
        self.pending_ui_sounds.push(sound);
    }

    pub(super) fn push_trackbar_hscroll(&mut self, id: SkirmishTrackbarId, visual_value: i32) {
        self.pending_trackbar_hscrolls.push((
            trackbar_control_id(id),
            visual_value,
            trackbar_hscroll_wparam(visual_value),
        ));
    }

    pub fn pending_trackbar_hscrolls(&self) -> &[SkirmishTrackbarHScrollNotification] {
        &self.pending_trackbar_hscrolls
    }

    pub fn drain_pending_trackbar_hscrolls(&mut self) -> Vec<SkirmishTrackbarHScrollNotification> {
        std::mem::take(&mut self.pending_trackbar_hscrolls)
    }

    pub fn pending_ui_sounds(&self) -> &[SkirmishShellUiSound] {
        &self.pending_ui_sounds
    }

    pub fn drain_pending_ui_sounds(&mut self) -> Vec<SkirmishShellUiSound> {
        std::mem::take(&mut self.pending_ui_sounds)
    }
}

/// A hover message to the status line ([`SkirmishStatics::hover`]). Returns
/// whether the help changed.
pub fn set_status_help_text(state: &mut SkirmishShellState, text: impl Into<String>) -> bool {
    state.statics.hover(&text.into(), std::time::Instant::now())
}

pub fn clear_status_help_text(state: &mut SkirmishShellState) -> bool {
    set_status_help_text(state, String::new())
}

pub fn dismiss_validation_modal(state: &mut SkirmishShellState) -> bool {
    state.validation_modal.take().is_some()
}

pub fn drain_pending_ui_sounds(state: &mut SkirmishShellState) -> Vec<SkirmishShellUiSound> {
    state.drain_pending_ui_sounds()
}

pub(super) fn inactive_ai_team_default(state: &SkirmishShellState) -> i32 {
    if state.selected_mode_allies_allowed {
        3
    } else {
        -2
    }
}

/// Whether a complete player-row control group is visible for the selected map.
/// Slot zero is the local row and never hides; opponent rows are bounded by the
/// selected map's native player-capacity query.
pub fn player_row_visible(state: &SkirmishShellState, maps: &[MapMenuEntry], row: usize) -> bool {
    if row == 0 {
        return true;
    }
    let capacity = maps
        .get(state.selected_map_idx)
        .map(|map| visible_player_capacity(map.player_capacity))
        .unwrap_or(SKIRMISH_PLAYER_SLOT_COUNT)
        .min(SKIRMISH_PLAYER_SLOT_COUNT);
    row < capacity
}

fn visible_player_capacity(capacity: i32) -> usize {
    capacity.clamp(0, SKIRMISH_PLAYER_SLOT_COUNT as i32) as usize
}

fn reset_rows_hidden_by_capacity(state: &mut SkirmishShellState, capacity: i32) {
    let capacity = visible_player_capacity(capacity);
    let visible_opponents = capacity.saturating_sub(1);
    let team_default = inactive_ai_team_default(state);
    for opponent in state.opponents.iter_mut().skip(visible_opponents) {
        opponent.row_type = super::SkirmishAiRowType::None;
        opponent.apply_inactive_combo_defaults(team_default);
    }

    let open_row = state.open_combo_dropdown.map(|open| match open.id {
        super::SkirmishComboId::AiType(idx) => idx + 1,
        super::SkirmishComboId::Side(row)
        | super::SkirmishComboId::Color(row)
        | super::SkirmishComboId::Start(row)
        | super::SkirmishComboId::Team(row) => row,
    });
    if open_row.is_some_and(|row| row != 0 && row >= capacity) {
        state.open_combo_dropdown = None;
        state.dropdown_scroll_drag = None;
        state.dropdown_scroll_press = None;
    }
}

/// Apply the accepted Choose Map selection as one row-lifecycle transaction.
/// A shrink resets every now-hidden opponent row before the selected map becomes
/// observable; growth only reveals rows, whose prior hide reset remains intact.
pub fn accept_selected_map(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    map_idx: usize,
) -> bool {
    let Some(new_map) = maps.get(map_idx) else {
        return false;
    };
    let old_capacity = maps
        .get(state.selected_map_idx)
        .map(|map| map.player_capacity)
        .unwrap_or(SKIRMISH_PLAYER_SLOT_COUNT as i32);
    if new_map.player_capacity < old_capacity {
        reset_rows_hidden_by_capacity(state, new_map.player_capacity);
    }
    state.selected_map_idx = map_idx;
    true
}

/// Apply the native first-open row hide pass for the initially selected map.
pub fn initialize_rows_for_selected_map(state: &mut SkirmishShellState, maps: &[MapMenuEntry]) {
    let capacity = maps
        .get(state.selected_map_idx)
        .map(|map| map.player_capacity)
        .unwrap_or(SKIRMISH_PLAYER_SLOT_COUNT as i32);
    if capacity < SKIRMISH_PLAYER_SLOT_COUNT as i32 {
        reset_rows_hidden_by_capacity(state, capacity);
    }
}

#[cfg(test)]
mod multiplayer_dialog_value_tests {
    use super::*;

    #[test]
    fn default_shell_launch_base_matches_hardcoded_defaults() {
        // The safety-net default path must stay on the hardcoded fallback so a
        // default()-constructed shell is byte-identical without an INI.
        let shell = SkirmishShellState::default();
        assert_eq!(shell.launch_options_base, SkirmishLaunchOptions::default());
    }

    #[test]
    fn apply_multiplayer_dialog_values_seeds_widgets_and_launch_base() {
        let mut shell = SkirmishShellState::default();
        let options = GameOptions {
            starting_credits: 7400,
            unit_count: 4,
            game_speed: 4,
            short_game: false,
            super_weapons: false,
            build_off_ally: false,
            crates: false,
            mcv_redeploy: false,
            tech_level: 3,
            bases: false,
            shroud: false,
            ..GameOptions::default()
        };

        shell.apply_multiplayer_dialog_values(&options);

        // Widget-backed fields are seeded into the live state.
        assert_eq!(shell.starting_credits, 7400);
        assert_eq!(shell.unit_count, 4);
        // GameSpeed is stored as parsed; the trackbar handles display inversion.
        assert_eq!(shell.game_speed, 4);
        assert!(!shell.short_game);
        assert!(!shell.super_weapons);
        assert!(!shell.build_off_ally);
        assert!(!shell.crates);
        assert!(!shell.mcv_redeploy);
        // Non-widget toggles ride on the launch base into the match.
        assert_eq!(shell.launch_options_base.tech_level, 3);
        assert!(!shell.launch_options_base.bases);
        assert!(!shell.launch_options_base.shroud);
    }
}

pub fn repair_teams_for_selected_mode(state: &mut SkirmishShellState, modes: &[SkirmishGameMode]) {
    let mode = mode_by_id(modes, state.selected_mode_id)
        .or_else(|| mode_by_id(modes, 1))
        .or_else(|| modes.first());
    if let Some(mode) = mode {
        state.selected_mode_allies_allowed = mode.allies_allowed;
        state.selected_mode_must_ally = mode.must_ally;
    }

    let inactive_default = inactive_ai_team_default(state);
    // Rebuilding the local Team combo selects its first row: Team A when
    // MustAlly suppresses None, otherwise None. Accepted map/type refreshes are
    // constructor-like and do not preserve a prior explicit local selection.
    state.player_team = if state.selected_mode_must_ally { 0 } else { -2 };
    for opponent in &mut state.opponents {
        // Native selected-mode refresh writes the mode default to every AI Team
        // control. This is a refresh event, not a per-frame repair.
        opponent.team = inactive_default;
    }
}

pub fn combo_dropdown_open(state: &SkirmishShellState) -> bool {
    state.open_combo_dropdown.is_some()
}

pub fn player_name_edit_rect_hit(layout: &SkirmishShellLayout, x: i32, y: i32) -> bool {
    layout.player_name.contains(x, y)
}

pub fn focus_player_name_edit(state: &mut SkirmishShellState) -> bool {
    state.open_combo_dropdown = None;
    state.dropdown_scroll_drag = None;
    state.dropdown_scroll_press = None;
    state.trackbar_drag = None;
    state.pressed_owner_draw_button = None;
    state.player_name_edit.focus_select_all()
}

pub fn blur_player_name_edit(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.blur()
}

pub fn insert_player_name_text(state: &mut SkirmishShellState, text: &str) -> bool {
    state.player_name_edit.insert_text(text)
}

pub fn handle_player_name_backspace(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.backspace()
}

pub fn handle_player_name_delete(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.delete()
}

pub fn handle_player_name_left(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.move_left()
}

pub fn handle_player_name_right(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.move_right()
}

pub fn handle_player_name_home(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.move_home()
}

pub fn handle_player_name_end(state: &mut SkirmishShellState) -> bool {
    state.player_name_edit.move_end()
}

pub fn player_name_caret_prefix(state: &SkirmishShellState) -> &str {
    state.player_name_edit.caret_prefix()
}

pub fn update_player_name_scroll_for_caret(
    state: &mut SkirmishShellState,
    visible_width: i32,
    caret_prefix_width: u32,
) -> bool {
    let visible_width = visible_width.max(0);
    let caret_x = caret_prefix_width as i32;
    let old = state.player_name_edit.scroll_x;
    let mut scroll = old.max(0);

    if visible_width <= PLAYER_NAME_CARET_MARGIN_PX * 2 {
        scroll = caret_x;
    } else {
        let right_limit = scroll + visible_width - PLAYER_NAME_CARET_MARGIN_PX;
        if caret_x > right_limit {
            scroll = caret_x - visible_width + PLAYER_NAME_CARET_MARGIN_PX;
        }
        let left_limit = scroll + PLAYER_NAME_CARET_MARGIN_PX;
        if caret_x < left_limit {
            scroll = (caret_x - PLAYER_NAME_CARET_MARGIN_PX).max(0);
        }
    }

    state.player_name_edit.scroll_x = scroll.max(0);
    state.player_name_edit.scroll_x != old
}

/// Handle Tab while the player-name edit has focus. The original moves keyboard
/// focus to the next dialog tab-stop control; the skirmish shell currently
/// models keyboard focus only for this edit, so the observable effect we can
/// reproduce is that focus leaves the edit (its caret/selection clear). Full
/// focus advancement to a specific next control awaits a shell-wide keyboard
/// focus/tab-order model. Returns true when focus state changed.
pub fn handle_player_name_tab(state: &mut SkirmishShellState) -> bool {
    blur_player_name_edit(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_shell_starts_with_an_unreserved_random_position() {
        assert_eq!(
            SkirmishShellState::default().player_start_position,
            StartPosition::Auto
        );
    }
}
