//! VERA's Quick Team Setup and Bulk Seat Actions on the Skirmish page: the
//! item the player picked from one of the two lists
//! (`SkirmishShellState::menu_pick`), carried out on the rows
//! (the shell state's `team_menus`). Team
//! Starts reads the selected map's starts and Team Colors the rules'
//! colours, which only the app holds.

use super::*;
use crate::ui::skirmish_shell::MenuPick;
use crate::ui::team_tools::{SeatAction, TeamTool};

impl App {
    /// Carries out the item last picked from the page's two lists, if any.
    pub(super) fn run_skirmish_menu_pick(state: &mut AppState) {
        let Some(pick) = state.frontend.skirmish_shell_state.menu_pick.take() else {
            return;
        };
        match pick {
            MenuPick::Team(TeamTool::Starts) => Self::skirmish_team_starts(state),
            MenuPick::Team(TeamTool::Colors) => {
                let listed = state
                    .rules()
                    .map(crate::ui::team_tools::listed_hues)
                    .unwrap_or_default();
                crate::ui::skirmish_shell::apply_team_colors(
                    &mut state.frontend.skirmish_shell_state,
                    state.frontend.scenario_catalog.shell_maps(),
                    &listed,
                );
            }
            MenuPick::Team(TeamTool::Balance) => {
                crate::ui::skirmish_shell::balance_teams_by_strength(
                    &mut state.frontend.skirmish_shell_state,
                    state.frontend.scenario_catalog.shell_maps(),
                );
            }
            MenuPick::Team(TeamTool::Preset(preset)) => {
                crate::ui::skirmish_shell::apply_team_preset(
                    &mut state.frontend.skirmish_shell_state,
                    state.frontend.scenario_catalog.shell_maps(),
                    preset,
                );
            }
            MenuPick::Seats(action) => Self::skirmish_seat_action(state, action),
        }
        state.platform.window.request_redraw();
    }

    /// Team Starts on the selected map: each playing row's start by team
    /// ([`crate::ui::skirmish_shell::starts_by_team`]), from the map's start
    /// cells and how far a ground unit drives between them. A map with fewer
    /// than two starts gives none.
    fn skirmish_team_starts(state: &mut AppState) {
        let map = state.frontend.skirmish_shell_state.selected_map_idx;
        let starts = Self::map_start_cells(state, map);
        if starts.len() < 2 {
            return;
        }
        let seats = crate::ui::skirmish_shell::team_start_seats(
            &state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
        );
        let drive = Self::map_drive_distances(state, map, &starts);
        let teams: Vec<i32> = seats.iter().map(|&(_, team)| team).collect();
        let given =
            crate::ui::skirmish_shell::starts_by_team(&starts, drive.as_deref(), &teams, &[]);
        let rows: Vec<usize> = seats.iter().map(|&(row, _)| row).collect();
        crate::ui::skirmish_shell::apply_team_starts(
            &mut state.frontend.skirmish_shell_state,
            &rows,
            &given,
        );
    }

    /// A Bulk Seat Actions item; Fill with AI's random difficulty is drawn
    /// for each row from the OS, as nothing the match replays depends on it.
    fn skirmish_seat_action(state: &mut AppState, action: SeatAction) {
        crate::ui::skirmish_shell::apply_seat_action(
            &mut state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            action,
            random_difficulty,
        );
    }
}

/// A difficulty drawn from the OS for Fill with AI's random choice; Normal
/// where the OS gives no randomness.
pub(super) fn random_difficulty() -> crate::skirmish_launch::AiDifficulty {
    let mut bits = [0u8; 1];
    match getrandom::fill(&mut bits) {
        Ok(()) => crate::ui::team_tools::drawn_difficulty(bits[0]),
        Err(_) => crate::skirmish_launch::AiDifficulty::Normal,
    }
}
