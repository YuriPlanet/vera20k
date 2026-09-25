//! Choose Map `0x6B` state (proc `0x005E6920`): the game-type list `0x6EB`,
//! the map list `0x553` and the three buttons.
//!
//! Both lists are the shell list subclass `0x00618D40` (`ui::shell::list`):
//! a press on a row selects it and plays GenericClick (`0x0061AA5A`), a
//! press below the last row is ignored, the scrollbar captures and repeats,
//! and no key or wheel case exists (`0x00618D40..0x0061C600`).

use std::time::{Duration, Instant};

use crate::map::skirmish_scenarios::{
    SkirmishScenarioRecord, filter_records_for_mode, upsert_random_map_sentinel,
};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};
use crate::ui::shell::list::{ListScrollInteraction, ShellListGeometry};

use super::super::layout::{ChooseMapModalButton, ChooseMapModalLayout, RectPx};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChooseMapSelection {
    pub mode_id: i32,
    pub record_index: Option<usize>,
}

/// What a press on the chooser's lists did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapListPress {
    /// Not on either list.
    Missed,
    /// On a scrollbar or below the last row: no sound.
    Consumed,
    /// On a row: the list plays GenericClick. `rebuilt` when a game-type
    /// click rebuilt the map list.
    RowClicked { rebuilt: bool },
}

/// The buttons of Use Map's eject box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EjectPromptButton {
    Ok,
    Cancel,
}

/// Use Map's box when occupied AI rows would not fit the chosen map
/// (`0x005E7285..0x005E7336`): `GUI:EjectAIPlayers` with OK and Cancel. OK
/// goes on with the selection; Cancel keeps the chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EjectPrompt {
    pub selection: ChooseMapSelection,
    pub pressed: Option<EjectPromptButton>,
}

/// Whether any AI row at or past the map's player limit is occupied
/// (`0x006ACCA0`: rows from slot `limit` on whose type is Easy, Normal or
/// Hard; a limit of 8 or more never ejects).
pub fn ai_rows_beyond_limit_occupied(
    opponents: &[super::SkirmishShellOpponent],
    limit: i32,
) -> bool {
    if limit >= 8 {
        return false;
    }
    opponents
        .iter()
        .skip(limit.max(1) as usize - 1)
        .any(|opponent| opponent.row_type != super::SkirmishAiRowType::None)
}

/// A list's paint surface: one pixel wider and taller than its window, like
/// every family list.
fn list_surface(window: RectPx) -> RectPx {
    RectPx::new(window.x, window.y, window.w + 1, window.h + 1)
}

#[derive(Debug, Clone)]
pub struct ChooseMapModalState {
    pub saved_selection: ChooseMapSelection,
    /// The game-type list's rows: the ids of the modes Skirmish lists, in
    /// mode order (`0x005D6130`).
    mode_rows: Vec<i32>,
    /// The game type whose maps are listed; the list cursor is on its row
    /// when it has one.
    pub selected_mode_id: i32,
    /// `0x583`'s enable, set when the map list is built (`0x005E6CB3`).
    random_maps_allowed: bool,
    pub filtered_record_indices: Vec<usize>,
    pub highlighted_filtered_index: Option<usize>,
    pub mode_top_index: usize,
    pub map_top_index: usize,
    pub mode_scroll: ListScrollInteraction,
    pub map_scroll: ListScrollInteraction,
    pub pressed_button: Option<ChooseMapModalButton>,
    /// The status line's help text. A child sets it from its own mouse
    /// moves; `0x6B` has no dialog hit test that clears it, so it keeps the
    /// last text over the background (`0x00611CBA..0x00611E8B`).
    pub status_help: String,
    /// Use Map's eject box while it shows.
    pub eject_prompt: Option<EjectPrompt>,
    /// The list window and time/point of the last press on list rows, for
    /// Windows' double-click test.
    last_list: Option<ChooseMapList>,
    last_press: Option<(Instant, i32, i32)>,
}

/// One of the two list windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChooseMapList {
    Mode,
    Map,
}

impl ChooseMapModalState {
    /// `0x497` (`0x005E6EA6`): the current game type (or the default) is
    /// selected, its maps listed with the current map selected and scrolled
    /// to the top of the list (`0x005E7000..0x005E701B`; the list clamps the
    /// top). The game-type list keeps its top at 0.
    ///
    /// Skirmish lists only the modes whose class answers vtable `+0x40`
    /// (`0x005D6263`). The fill's observer check (`0x005D6239`, vtable
    /// `+0xBC`) rejects only Cooperative, which that test already drops.
    pub fn open(
        current_mode_id: i32,
        current_record_index: Option<usize>,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) -> Self {
        let selected_mode_id = mode_by_id(modes, current_mode_id)
            .or_else(|| modes.first())
            .map(|mode| mode.id)
            .unwrap_or(current_mode_id);
        let mut state = Self {
            saved_selection: ChooseMapSelection {
                mode_id: current_mode_id,
                record_index: current_record_index,
            },
            mode_rows: modes
                .iter()
                .filter(|mode| mode.class.lists_in_skirmish())
                .map(|mode| mode.id)
                .collect(),
            selected_mode_id,
            random_maps_allowed: false,
            filtered_record_indices: Vec::new(),
            highlighted_filtered_index: None,
            mode_top_index: 0,
            map_top_index: 0,
            mode_scroll: ListScrollInteraction::default(),
            map_scroll: ListScrollInteraction::default(),
            pressed_button: None,
            status_help: String::new(),
            eject_prompt: None,
            last_list: None,
            last_press: None,
        };
        state.build_map_list(modes, records);
        // The fill selects only the current scenario; when the list lacks it
        // no row is selected (`0x005D6504`, `0x005E6F99`).
        state.highlighted_filtered_index = current_record_index.and_then(|record| {
            state
                .filtered_record_indices
                .iter()
                .position(|idx| *idx == record)
        });
        state.scroll_map_selection_to_top(layout);
        state
    }

    /// The ids of the listed game types, top row first.
    pub fn mode_rows(&self) -> &[i32] {
        &self.mode_rows
    }

    pub fn mode_geometry(&self, layout: &ChooseMapModalLayout) -> ShellListGeometry {
        ShellListGeometry::new(
            list_surface(layout.mode_list),
            self.mode_rows.len(),
            self.mode_top_index,
        )
    }

    pub fn map_geometry(&self, layout: &ChooseMapModalLayout) -> ShellListGeometry {
        ShellListGeometry::new(
            list_surface(layout.map_list),
            self.filtered_record_indices.len(),
            self.map_top_index,
        )
    }

    pub fn selected_record_index(&self) -> Option<usize> {
        self.highlighted_filtered_index
            .and_then(|idx| self.filtered_record_indices.get(idx).copied())
    }

    /// The row of the selected game type in the game-type list.
    pub fn selected_mode_row(&self) -> Option<usize> {
        self.mode_rows
            .iter()
            .position(|id| *id == self.selected_mode_id)
    }

    fn build_map_list(&mut self, modes: &[SkirmishGameMode], records: &[SkirmishScenarioRecord]) {
        let mode = mode_by_id(modes, self.selected_mode_id);
        self.filtered_record_indices = mode
            .map(|mode| filter_records_for_mode(records, mode))
            .unwrap_or_default();
        self.random_maps_allowed = mode.is_some_and(|mode| mode.random_maps_allowed);
        self.map_scroll.cancel();
    }

    /// `LB_SETTOPINDEX(selection)`, clamped by the list.
    fn scroll_map_selection_to_top(&mut self, layout: &ChooseMapModalLayout) {
        let max_top = self.map_geometry(layout).max_top;
        self.map_top_index = self.highlighted_filtered_index.unwrap_or(0).min(max_top);
    }

    /// Rebuild the map list for the selected game type and reselect the map
    /// whose name matches the highlighted one (`[0xAC0EC8]`; the last match
    /// wins, row 0 when none), scrolled to the top (`0x005E6C81..0x005E6D43`).
    pub fn select_mode(
        &mut self,
        mode_id: i32,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) -> bool {
        if mode_by_id(modes, mode_id).is_none() {
            return false;
        }
        let name = self
            .selected_record_index()
            .and_then(|record| records.get(record))
            .map(|record| record.display_name.clone())
            .unwrap_or_default();
        self.selected_mode_id = mode_id;
        self.build_map_list(modes, records);
        let matched = self.filtered_record_indices.iter().rposition(|record| {
            records
                .get(*record)
                .is_some_and(|record| record.display_name == name)
        });
        self.highlighted_filtered_index =
            matched.or_else(|| (!self.filtered_record_indices.is_empty()).then_some(0));
        self.scroll_map_selection_to_top(layout);
        true
    }

    /// Whether a press is the second click of a double-click on one list's
    /// rows. Windows then sends `WM_LBUTTONDBLCLK`, which the list subclass
    /// only forwards as `LBN_DBLCLK` (`0x0061A904..0x0061A945`; `0x6B`
    /// ignores it): no selection, no sound. A press anywhere else, or on the
    /// other list, starts a new sequence.
    pub fn is_double_click(
        &mut self,
        layout: &ChooseMapModalLayout,
        (x, y): (i32, i32),
        now: Instant,
        limits: (Duration, i32, i32),
    ) -> bool {
        let list = if self.mode_geometry(layout).content.contains(x, y) {
            ChooseMapList::Mode
        } else if self.map_geometry(layout).content.contains(x, y) {
            ChooseMapList::Map
        } else {
            self.last_press = None;
            return false;
        };
        if self.last_list != Some(list) {
            self.last_list = Some(list);
            self.last_press = None;
        }
        crate::ui::shell::list::is_double_click(&mut self.last_press, now, x, y, limits)
    }

    /// A press on the lists (`0x0061A948`): scrollbar capture, or a row. A
    /// game-type row whose index equals the last one clicked (`[0x8316FC]`,
    /// which lives across chooser visits and starts at -1) moves the cursor
    /// without rebuilding the map list (`0x005E6BB7`).
    pub fn mouse_down(
        &mut self,
        layout: &ChooseMapModalLayout,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        last_mode_row: &mut Option<usize>,
        (x, y): (i32, i32),
        now: Instant,
    ) -> ChooseMapListPress {
        let mode_geometry = self.mode_geometry(layout);
        if let Some(part) = mode_geometry.scroll_part_at(x, y) {
            self.mode_scroll
                .press(part, mode_geometry, &mut self.mode_top_index, y, now);
            return ChooseMapListPress::Consumed;
        }
        let map_geometry = self.map_geometry(layout);
        if let Some(part) = map_geometry.scroll_part_at(x, y) {
            self.map_scroll
                .press(part, map_geometry, &mut self.map_top_index, y, now);
            return ChooseMapListPress::Consumed;
        }
        if mode_geometry.outer.contains(x, y) {
            let count = self.mode_rows.len();
            let Some(row) = mode_geometry.row_at(count, self.mode_top_index, x, y) else {
                return ChooseMapListPress::Consumed;
            };
            let mode_id = self.mode_rows[row];
            self.selected_mode_id = mode_id;
            if *last_mode_row == Some(row) {
                return ChooseMapListPress::RowClicked { rebuilt: false };
            }
            *last_mode_row = Some(row);
            self.select_mode(mode_id, modes, records, layout);
            return ChooseMapListPress::RowClicked { rebuilt: true };
        }
        if map_geometry.outer.contains(x, y) {
            let count = self.filtered_record_indices.len();
            let Some(row) = map_geometry.row_at(count, self.map_top_index, x, y) else {
                return ChooseMapListPress::Consumed;
            };
            self.highlighted_filtered_index = Some(row);
            return ChooseMapListPress::RowClicked { rebuilt: false };
        }
        ChooseMapListPress::Missed
    }

    /// Pointer motion: a captured thumb follows it.
    pub fn mouse_move(&mut self, layout: &ChooseMapModalLayout, x: i32, y: i32) {
        let geometry = self.mode_geometry(layout);
        self.mode_scroll
            .pointer_moved(geometry, &mut self.mode_top_index, x, y);
        let geometry = self.map_geometry(layout);
        self.map_scroll
            .pointer_moved(geometry, &mut self.map_top_index, x, y);
    }

    /// The button release ends any scrollbar capture.
    pub fn mouse_up(&mut self) {
        self.mode_scroll.cancel();
        self.map_scroll.cancel();
    }

    /// Arrow auto-repeat; returns whether a list moved.
    pub fn poll_scroll(
        &mut self,
        layout: &ChooseMapModalLayout,
        x: i32,
        y: i32,
        now: Instant,
    ) -> bool {
        let geometry = self.mode_geometry(layout);
        let mode = self
            .mode_scroll
            .poll(geometry, &mut self.mode_top_index, x, y, now);
        let geometry = self.map_geometry(layout);
        let map = self
            .map_scroll
            .poll(geometry, &mut self.map_top_index, x, y, now);
        mode || map
    }

    /// The earlier of the two lists' repeat deadlines.
    pub fn scroll_deadline(&self) -> Option<Instant> {
        match (self.mode_scroll.repeat_at(), self.map_scroll.repeat_at()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn accept_selection(&self) -> Option<ChooseMapSelection> {
        Some(ChooseMapSelection {
            mode_id: self.selected_mode_id,
            record_index: Some(self.selected_record_index()?),
        })
    }

    /// Whether Create Random Map is enabled: the built list's mode admits
    /// random maps (`MultiplayerGameMode::RandomMapsAllowed` `0x005D6350`,
    /// read at `0x005E6CAF` and `0x005E6F6F`).
    pub fn random_map_button_enabled(&self) -> bool {
        self.random_maps_allowed
    }

    pub fn button_enabled(&self, button: ChooseMapModalButton) -> bool {
        button != ChooseMapModalButton::CreateRandomMap0x583 || self.random_map_button_enabled()
    }

    /// Arm an enabled owner-draw button. Disabled controls consume the hit but
    /// never acquire pressed state, which also keeps their click sound silent.
    pub fn press_button(&mut self, button: ChooseMapModalButton) -> bool {
        let enabled = self.button_enabled(button);
        self.pressed_button = enabled.then_some(button);
        enabled
    }

    /// Release the captured button, rechecking admission so a stale press can
    /// never dispatch after the selected mode has disabled that command.
    pub fn release_button(
        &mut self,
        released_button: Option<ChooseMapModalButton>,
    ) -> (bool, Option<ChooseMapModalButton>) {
        let Some(pressed_button) = self.pressed_button.take() else {
            return (false, None);
        };
        let should_fire =
            Some(pressed_button) == released_button && self.button_enabled(pressed_button);
        (true, should_fire.then_some(pressed_button))
    }

    pub const fn cancel_selection(&self) -> ChooseMapSelection {
        self.saved_selection
    }

    pub fn create_random_map(
        &mut self,
        records: &mut Vec<SkirmishScenarioRecord>,
        modes: &[SkirmishGameMode],
        display_name: impl Into<String>,
        player_capacity: i32,
        layout: &ChooseMapModalLayout,
    ) -> Option<usize> {
        // Keep the late defense even though normal input is disabled earlier:
        // direct/programmatic callers must not write a disallowed sentinel.
        if !self.random_map_button_enabled() {
            return None;
        }

        let record_index = upsert_random_map_sentinel(records, display_name, player_capacity);
        self.build_map_list(modes, records);
        self.highlighted_filtered_index = self
            .filtered_record_indices
            .iter()
            .position(|idx| *idx == record_index)
            .or_else(|| (!self.filtered_record_indices.is_empty()).then_some(0));
        self.scroll_map_selection_to_top(layout);
        Some(record_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skirmish_modes::stock_skirmish_modes;
    use crate::ui::skirmish_shell::compute_choose_map_modal_layout;
    use std::time::Duration;

    fn record(name: &str) -> SkirmishScenarioRecord {
        let mut record = SkirmishScenarioRecord::concrete_from_ini(
            0,
            crate::map::skirmish_scenarios::SkirmishScenarioSource::Synthetic,
            &format!("{name}.map"),
            &crate::rules::ini_parser::IniFile::from_str(""),
        );
        record.display_name = name.to_string();
        record.game_modes.clear();
        record
    }

    #[test]
    fn random_map_button_admission_tracks_the_built_mode() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(1, None, &modes, &[], &layout);
        for mode_id in [1, 2] {
            assert!(modal.select_mode(mode_id, &modes, &[], &layout));
            assert!(modal.random_map_button_enabled());
        }
        for mode_id in 3..=9 {
            assert!(modal.select_mode(mode_id, &modes, &[], &layout));
            assert!(!modal.random_map_button_enabled());
        }
    }

    #[test]
    fn a_stale_random_map_press_cannot_dispatch() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(9, None, &modes, &[], &layout);
        let random = ChooseMapModalButton::CreateRandomMap0x583;
        assert!(!modal.press_button(random));
        assert_eq!(modal.release_button(Some(random)), (false, None));
        assert!(modal.select_mode(1, &modes, &[], &layout));
        assert!(modal.press_button(random));
        assert!(modal.select_mode(9, &modes, &[], &layout));
        assert_eq!(modal.release_button(Some(random)), (true, None));
        let mut records = Vec::new();
        assert_eq!(
            modal.create_random_map(&mut records, &modes, "Disallowed", 4, &layout),
            None
        );
        assert!(
            records.is_empty(),
            "late defense must not upsert a sentinel"
        );
    }

    #[test]
    fn opening_scrolls_the_current_map_to_the_top_clamped() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let standard = modes
            .iter()
            .find(|mode| mode.map_filter == "standard")
            .expect("a standard mode");
        let all: Vec<_> = (0..40).map(|i| record(&format!("Map {i:02}"))).collect();
        let modal = ChooseMapModalState::open(standard.id, Some(25), &modes, &all, &layout);
        let geometry = modal.map_geometry(&layout);
        assert_eq!(geometry.visible_rows, 18, "surface 345 px tall: 18 rows");
        assert_eq!(modal.highlighted_filtered_index, Some(25));
        assert_eq!(modal.map_top_index, 22, "40 rows - 18 visible");
        let modal = ChooseMapModalState::open(standard.id, Some(5), &modes, &all, &layout);
        assert_eq!(modal.map_top_index, 5);
    }

    fn record_for(name: &str, game_modes: &[&str]) -> SkirmishScenarioRecord {
        let mut record = record(name);
        record.game_modes = game_modes.iter().map(|mode| mode.to_string()).collect();
        record
    }

    #[test]
    fn a_game_type_change_keeps_the_same_named_map() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let all: Vec<_> = (0..30).map(|i| record(&format!("Map {i:02}"))).collect();
        let mut modal = ChooseMapModalState::open(1, Some(20), &modes, &all, &layout);
        assert!(modal.select_mode(2, &modes, &all, &layout));
        assert_eq!(modal.highlighted_filtered_index, Some(20));
        assert_eq!(modal.map_top_index, 12, "30 rows - 18 visible");
    }

    #[test]
    fn the_last_same_named_map_wins_and_no_match_takes_row_0() {
        // `0x005E6CDF..0x005E6D43`: `wcscmp` over the new list, the last
        // match kept; no match selects row 0.
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let all = vec![
            record_for("Alpha", &["standard"]),
            record_for("Bravo", &["standard", "teamgame"]),
            record_for("Charlie", &["teamgame"]),
            record_for("Bravo", &["teamgame"]),
            record_for("alpha", &["teamgame"]),
        ];
        let mut modal = ChooseMapModalState::open(1, Some(1), &modes, &all, &layout);
        assert!(modal.select_mode(9, &modes, &all, &layout));
        assert_eq!(modal.filtered_record_indices, [1, 2, 3, 4]);
        assert_eq!(modal.selected_record_index(), Some(3), "the last Bravo");
        let mut modal = ChooseMapModalState::open(1, Some(0), &modes, &all, &layout);
        assert!(modal.select_mode(9, &modes, &all, &layout));
        assert_eq!(
            modal.highlighted_filtered_index,
            Some(0),
            "case-sensitive: \"alpha\" is not \"Alpha\""
        );
    }

    #[test]
    fn a_current_map_outside_the_list_opens_with_no_row() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let all = vec![
            record_for("Alpha", &["standard"]),
            record_for("Charlie", &["teamgame"]),
        ];
        let modal = ChooseMapModalState::open(1, Some(1), &modes, &all, &layout);
        assert_eq!(modal.highlighted_filtered_index, None);
        assert_eq!(modal.accept_selection(), None, "Use Map does nothing");
    }

    #[test]
    fn a_double_click_on_a_row_is_not_a_second_press() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(1, None, &modes, &[], &layout);
        let limits = (Duration::from_millis(500), 4, 4);
        let now = Instant::now();
        let mode_row = modal.mode_geometry(&layout).row(0);
        let map_row = modal.map_geometry(&layout).row(0);
        let mode_point = (mode_row.x + 2, mode_row.y + 2);
        let map_point = (map_row.x + 2, map_row.y + 2);
        assert!(!modal.is_double_click(&layout, mode_point, now, limits));
        assert!(modal.is_double_click(
            &layout,
            mode_point,
            now + Duration::from_millis(100),
            limits
        ));
        // The other list starts a new sequence.
        assert!(!modal.is_double_click(&layout, mode_point, now, limits));
        assert!(!modal.is_double_click(&layout, map_point, now, limits));
        assert!(!modal.is_double_click(&layout, mode_point, now, limits));
        // A press off the lists breaks the sequence.
        assert!(!modal.is_double_click(
            &layout,
            (layout.use_map_button.x + 2, layout.use_map_button.y + 2),
            now,
            limits
        ));
        assert!(!modal.is_double_click(&layout, mode_point, now, limits));
    }

    #[test]
    fn row_clicks_select_and_scrollbar_presses_capture() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(modes[0].id, None, &modes, &[], &layout);
        modal.filtered_record_indices = (0..30).collect();
        modal.highlighted_filtered_index = Some(0);
        let mut last = None;
        let now = Instant::now();
        let geometry = modal.map_geometry(&layout);
        let row = geometry.row(2);
        assert_eq!(
            modal.mouse_down(&layout, &modes, &[], &mut last, (row.x + 2, row.y + 2), now),
            ChooseMapListPress::RowClicked { rebuilt: false }
        );
        assert_eq!(modal.highlighted_filtered_index, Some(2));
        let bar = geometry.scrollbar.expect("30 rows overflow 18");
        assert_eq!(
            modal.mouse_down(
                &layout,
                &modes,
                &[],
                &mut last,
                (bar.x + 3, bar.y + bar.h - 3),
                now
            ),
            ChooseMapListPress::Consumed
        );
        assert_eq!(modal.map_top_index, 1);
        // The arrow repeats after 500 ms while held.
        assert!(modal.poll_scroll(
            &layout,
            bar.x + 3,
            bar.y + bar.h - 3,
            now + Duration::from_millis(500)
        ));
        assert_eq!(modal.map_top_index, 2);
        modal.mouse_up();
        assert_eq!(modal.scroll_deadline(), None);
    }

    #[test]
    fn only_occupied_rows_past_the_limit_eject() {
        let mut shell = crate::ui::skirmish_shell::SkirmishShellState::default();
        for opponent in shell.opponents.iter_mut() {
            opponent.row_type = crate::ui::skirmish_shell::SkirmishAiRowType::None;
        }
        // Slots 0 (player) and 1 fit a two-player map.
        shell.opponents[0].row_type = crate::ui::skirmish_shell::SkirmishAiRowType::Hard;
        assert!(!ai_rows_beyond_limit_occupied(&shell.opponents, 2));
        shell.opponents[1].row_type = crate::ui::skirmish_shell::SkirmishAiRowType::Easy;
        assert!(ai_rows_beyond_limit_occupied(&shell.opponents, 2));
        assert!(!ai_rows_beyond_limit_occupied(&shell.opponents, 3));
        assert!(!ai_rows_beyond_limit_occupied(&shell.opponents, 8));
    }

    #[test]
    fn the_last_clicked_game_type_row_does_not_rebuild_the_list() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(modes[0].id, None, &modes, &[], &layout);
        let geometry = modal.mode_geometry(&layout);
        let row1 = geometry.row(1);
        let now = Instant::now();
        let mut last = None;
        assert_eq!(
            modal.mouse_down(
                &layout,
                &modes,
                &[],
                &mut last,
                (row1.x + 2, row1.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: true }
        );
        assert_eq!(last, Some(1));
        assert_eq!(
            modal.mouse_down(
                &layout,
                &modes,
                &[],
                &mut last,
                (row1.x + 2, row1.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: false }
        );
        // A later visit keeps the global: the same row still does not rebuild.
        let mut modal = ChooseMapModalState::open(modes[0].id, None, &modes, &[], &layout);
        assert_eq!(
            modal.mouse_down(
                &layout,
                &modes,
                &[],
                &mut last,
                (row1.x + 2, row1.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: false }
        );
        assert_eq!(
            modal.selected_mode_id,
            modal.mode_rows()[1],
            "the cursor still moves"
        );
    }

    #[test]
    fn the_lists_match_the_retail_capture() {
        // Retail `0x6B` at 800x600 (cm-6b-steady.png): selection fills at
        // x 117..=311 (game types) and 339..=513 (maps, beside the
        // scrollbar), rows from y 128; scrollbar edge at x 514; the rings
        // at x 115/116 and 312/313, y 126/127 and 471/472.
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(1, None, &modes, &[], &layout);
        assert_eq!(
            modal.mode_geometry(&layout).row(0),
            RectPx::new(117, 128, 195, 19)
        );
        modal.filtered_record_indices = (0..60).collect();
        let maps = modal.map_geometry(&layout);
        assert_eq!(maps.row(0), RectPx::new(339, 128, 175, 19));
        assert_eq!(maps.scrollbar.map(|bar| bar.x), Some(514));
        let lines = crate::ui::shell::list::list_frame_lines(layout.mode_list);
        let left = lines.iter().map(|(r, _)| r.x).min();
        let right = lines.iter().map(|(r, _)| r.x + r.w - 1).max();
        let top = lines.iter().map(|(r, _)| r.y).min();
        let bottom = lines.iter().map(|(r, _)| r.y + r.h - 1).max();
        assert_eq!(
            (left, right, top, bottom),
            (Some(115), Some(313), Some(126), Some(472))
        );
    }

    #[test]
    fn skirmish_lists_battle_free_for_all_and_team_game() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let modal = ChooseMapModalState::open(9, None, &modes, &[], &layout);
        assert_eq!(modal.mode_rows(), [1, 2, 9]);
        assert_eq!(modal.selected_mode_row(), Some(2));
        // A current mode Skirmish does not list keeps its maps, on no row.
        let modal = ChooseMapModalState::open(3, None, &modes, &[], &layout);
        assert_eq!(modal.selected_mode_id, 3);
        assert_eq!(modal.selected_mode_row(), None);
    }
}
