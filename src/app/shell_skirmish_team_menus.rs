//! VERA's Quick Team Setup and Bulk Seat Actions on the Skirmish page: the
//! item the player picked from one of the two lists
//! (`SkirmishShellState::menu_pick`), carried out on the rows
//! (the shell state's `team_menus`). Team
//! Starts reads the selected map's starts and Team Colors the rules'
//! colours, which only the app holds.

use super::*;
use crate::app::PlatformState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::ui::skirmish_shell::MenuPick;
use crate::ui::team_tools::{SeatAction, TeamTool};
use crate::util::config::GameConfig;

impl App {
    /// Carries out the item last picked from the page's two lists, if any.
    pub(super) fn run_skirmish_menu_pick(
        platform: &PlatformState,
        frontend: &mut FrontendState,
        match_state: &MatchState,
        process_assets: &mut ProcessAssets,
    ) {
        let Some(pick) = frontend.skirmish_shell_state.menu_pick.take() else {
            return;
        };
        match pick {
            MenuPick::Team(TeamTool::Starts) => {
                Self::skirmish_team_starts(platform.game_config.as_ref(), frontend, process_assets)
            }
            MenuPick::Team(TeamTool::Colors) => {
                let listed = crate::app::state::active_rules(frontend, match_state)
                    .map(crate::ui::team_tools::listed_hues)
                    .unwrap_or_default();
                crate::ui::skirmish_shell::apply_team_colors(
                    &mut frontend.skirmish_shell_state,
                    frontend.scenario_catalog.shell_maps(),
                    &listed,
                );
            }
            MenuPick::Team(TeamTool::Balance) => {
                crate::ui::skirmish_shell::balance_teams_by_strength(
                    &mut frontend.skirmish_shell_state,
                    frontend.scenario_catalog.shell_maps(),
                );
            }
            MenuPick::Team(TeamTool::Preset(preset)) => {
                crate::ui::skirmish_shell::apply_team_preset(
                    &mut frontend.skirmish_shell_state,
                    frontend.scenario_catalog.shell_maps(),
                    preset,
                );
            }
            MenuPick::Seats(action) => Self::skirmish_seat_action(frontend, action),
        }
        platform.window.request_redraw();
    }

    /// Team Starts on the selected map: each playing row's start by team
    /// ([`crate::ui::skirmish_shell::starts_by_team`]), from the map's start
    /// cells and how far a ground unit drives between them. A map with fewer
    /// than two starts gives none.
    fn skirmish_team_starts(
        game_config: Option<&GameConfig>,
        frontend: &mut FrontendState,
        process_assets: &mut ProcessAssets,
    ) {
        let map = frontend.skirmish_shell_state.selected_map_idx;
        let starts = Self::map_start_cells(frontend, map);
        if starts.len() < 2 {
            return;
        }
        let seats = crate::ui::skirmish_shell::team_start_seats(
            &frontend.skirmish_shell_state,
            frontend.scenario_catalog.shell_maps(),
        );
        let drive = Self::map_drive_distances(
            game_config,
            &frontend.scenario_catalog,
            process_assets,
            map,
            &starts,
        );
        let teams: Vec<i32> = seats.iter().map(|&(_, team)| team).collect();
        let given =
            crate::ui::skirmish_shell::starts_by_team(&starts, drive.as_deref(), &teams, &[]);
        let rows: Vec<usize> = seats.iter().map(|&(row, _)| row).collect();
        crate::ui::skirmish_shell::apply_team_starts(
            &mut frontend.skirmish_shell_state,
            &rows,
            &given,
        );
    }

    /// A Bulk Seat Actions item; Fill with AI's random difficulty is drawn
    /// for each row from the OS, as nothing the match replays depends on it.
    fn skirmish_seat_action(frontend: &mut FrontendState, action: SeatAction) {
        crate::ui::skirmish_shell::apply_seat_action(
            &mut frontend.skirmish_shell_state,
            frontend.scenario_catalog.shell_maps(),
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
