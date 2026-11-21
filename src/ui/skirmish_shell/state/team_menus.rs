//! VERA's Quick Team Setup and Bulk Seat Actions on the Skirmish page
//! ([`crate::ui::team_tools`]): two lists under the rows, as the VERA
//! launcher's room has them. The player is the one person; every seat
//! change goes through the rows' own combo selection
//! ([`super::combos::apply_combo_selection`]), as picking it in that row
//! would. Skirmish has no closed seats: a row set to None is an open seat.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::{AiDifficulty, LaunchStartPosition, LaunchTeam, NATIVE_TEAMS};
use crate::ui::team_tools::{
    PRESETS, SeatAction, SeatCounts, TeamPreset, TeamTool, ToolRefusal, balance_teams,
    preset_teams, seat_actions,
};

use super::combos::apply_combo_selection;
use super::{
    SkirmishAiRowType, SkirmishComboId, SkirmishComboItem, SkirmishShellState, selected_player_rows,
};

/// The item picked from one of the two menus, which the app carries out:
/// Team Starts reads the map and Team Colors the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPick {
    Team(TeamTool),
    Seats(SeatAction),
}

/// How Balance weighs a seat in Skirmish, which has one person and no
/// rating: an AI by its difficulty, the player as a Hard AI.
fn strength(seat: Option<AiDifficulty>) -> u32 {
    match seat {
        Some(AiDifficulty::Easy) => 1,
        Some(AiDifficulty::Normal) => 2,
        Some(AiDifficulty::Hard) | None => 3,
    }
}

/// The rows that play, in row order: the player's (0), then each computer
/// row the map admits with an AI in it, with its difficulty.
fn playing_rows(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
) -> Vec<(usize, Option<AiDifficulty>, LaunchTeam)> {
    let rows = selected_player_rows(state, maps);
    let player = (0, None, LaunchTeam::from_shell_value(state.player_team));
    let ais = state
        .opponents
        .iter()
        .enumerate()
        .take(rows.saturating_sub(1))
        .filter(|(_, opponent)| opponent.is_active())
        .map(|(index, opponent)| {
            (
                index + 1,
                opponent.row_type.difficulty(),
                LaunchTeam::from_shell_value(opponent.team),
            )
        });
    std::iter::once(player).chain(ais).collect()
}

/// The teams the playing rows are on, in letter order.
fn teams_in_use(rows: &[(usize, Option<AiDifficulty>, LaunchTeam)]) -> Vec<u8> {
    let mut teams: Vec<u8> = rows
        .iter()
        .filter_map(|&(_, _, team)| match team {
            LaunchTeam::Team(team) => Some(team),
            LaunchTeam::None => None,
        })
        .collect();
    teams.sort_unstable();
    teams.dedup();
    teams
}

fn seats(rows: &[(usize, Option<AiDifficulty>, LaunchTeam)]) -> Vec<crate::ui::team_tools::Seat> {
    rows.iter()
        .map(|&(row, _, team)| match row {
            0 => crate::ui::team_tools::Seat::Player(team),
            _ => crate::ui::team_tools::Seat::Ai(team),
        })
        .collect()
}

/// Quick Team Setup's list: Team Starts, Team Colors and Balance, then the
/// heading over the quick teams and those, each greyed with why while it
/// cannot be had.
pub(super) fn team_menu_items(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
) -> Vec<SkirmishComboItem> {
    let rows = playing_rows(state, maps);
    let item = |tool, refusal| SkirmishComboItem::TeamTool { tool, refusal };
    let colors = (teams_in_use(&rows).len() < 2).then_some(ToolRefusal::TwoTeamsNeeded);
    let balance = (rows.len() < 2).then_some(ToolRefusal::NoAi);
    let presets = PRESETS.into_iter().map(|preset| {
        let refusal = preset_teams(
            preset,
            &seats(&rows),
            state.selected_mode_must_ally,
            NATIVE_TEAMS,
        )
        .err();
        item(TeamTool::Preset(preset), refusal)
    });
    [
        item(TeamTool::Starts, None),
        item(TeamTool::Colors, colors),
        item(TeamTool::Balance, balance),
        SkirmishComboItem::TeamHeading,
    ]
    .into_iter()
    .chain(presets)
    .collect()
}

/// Bulk Seat Actions' list: filling the open rows with AI and emptying the
/// AI rows.
pub(super) fn seat_menu_items(
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
) -> Vec<SkirmishComboItem> {
    let rows = selected_player_rows(state, maps).saturating_sub(1);
    let admitted = || state.opponents.iter().take(rows);
    let counts = SeatCounts {
        open: admitted().filter(|opponent| !opponent.is_active()).count(),
        closed: 0,
        ai: admitted().filter(|opponent| opponent.is_active()).count(),
        closable: false,
        ai_allowed: true,
    };
    seat_actions(counts)
        .into_iter()
        .map(|(action, enabled)| SkirmishComboItem::SeatAction { action, enabled })
        .collect()
}

/// Puts `team` on player row `row` through its Team list.
fn set_team(state: &mut SkirmishShellState, row: usize, team: LaunchTeam) {
    apply_combo_selection(
        state,
        SkirmishComboId::Team(row),
        SkirmishComboItem::Team(team.shell_value()),
    );
}

/// A quick team on the playing rows. Returns whether it could be had.
pub fn apply_team_preset(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    preset: TeamPreset,
) -> bool {
    let rows = playing_rows(state, maps);
    let Ok(teams) = preset_teams(
        preset,
        &seats(&rows),
        state.selected_mode_must_ally,
        NATIVE_TEAMS,
    ) else {
        return false;
    };
    for (&(row, ..), team) in rows.iter().zip(teams) {
        if let Some(team) = team {
            set_team(state, row, team);
        }
    }
    true
}

/// Balance: the playing rows split over the teams in use (Teams A and B
/// while fewer are), the totals of their [`strength`] as even as can be.
pub fn balance_teams_by_strength(state: &mut SkirmishShellState, maps: &[MapMenuEntry]) -> bool {
    let rows = playing_rows(state, maps);
    if rows.len() < 2 {
        return false;
    }
    let mut teams = teams_in_use(&rows);
    if teams.len() < 2 {
        teams = vec![0, 1];
    }
    teams.truncate(rows.len());
    let ratings: Vec<u32> = rows
        .iter()
        .map(|&(_, difficulty, _)| strength(difficulty))
        .collect();
    let current: Vec<Option<usize>> = rows
        .iter()
        .map(|&(_, _, team)| match team {
            LaunchTeam::Team(team) => teams.iter().position(|on| *on == team),
            LaunchTeam::None => None,
        })
        .collect();
    let split = balance_teams(&ratings, &current, teams.len());
    for (&(row, ..), team) in rows.iter().zip(split) {
        set_team(state, row, LaunchTeam::Team(teams[team]));
    }
    true
}

/// Team Colors: the playing rows on a team get their team's colours from
/// `listed` (the rules' lobby colours with their hues,
/// [`crate::ui::team_tools::listed_hues`]). A row on no team keeps its
/// colour unless a team's takes it, which leaves it on Random. Returns
/// whether two teams or more were there to colour.
pub fn apply_team_colors(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    listed: &[(u8, u8)],
) -> bool {
    let rows = playing_rows(state, maps);
    let teams = teams_in_use(&rows);
    let Ok(count) = u8::try_from(teams.len()) else {
        return false;
    };
    if count < 2 {
        return false;
    }
    let teamed: Vec<(usize, u8)> = rows
        .iter()
        .filter_map(|&(row, _, team)| match team {
            LaunchTeam::Team(team) => Some((row, teams.iter().position(|on| *on == team)? as u8)),
            LaunchTeam::None => None,
        })
        .collect();
    let on: Vec<u8> = teamed.iter().map(|&(_, team)| team).collect();
    let colors = crate::ui::team_tools::colors_by_team(listed, &on, count);
    for (&(row, _), color) in teamed.iter().zip(colors) {
        if let Some(color) = color {
            apply_combo_selection(
                state,
                SkirmishComboId::Color(row),
                SkirmishComboItem::Color(usize::from(color)),
            );
        }
    }
    true
}

/// Team Starts' seats: each playing row with its team as the Team list's
/// item data (-2 for none), for [`super::starts_by_team`].
pub fn team_start_seats(state: &SkirmishShellState, maps: &[MapMenuEntry]) -> Vec<(usize, i32)> {
    playing_rows(state, maps)
        .into_iter()
        .map(|(row, _, team)| (row, team.shell_value()))
        .collect()
}

/// Gives each of `rows` its start from Team Starts (`None`: Random).
pub fn apply_team_starts(state: &mut SkirmishShellState, rows: &[usize], starts: &[Option<usize>]) {
    for (&row, start) in rows.iter().zip(starts) {
        let start = start
            .and_then(|start| u8::try_from(start).ok())
            .map_or(LaunchStartPosition::Auto, LaunchStartPosition::Position);
        apply_combo_selection(
            state,
            SkirmishComboId::Start(row),
            SkirmishComboItem::Start(start),
        );
    }
}

/// A Bulk Seat Actions item: Fill with AI puts an AI in every open row the
/// map admits, of the picked difficulty or one `draw` gives each; Remove
/// all AI empties every AI row. Skirmish has no closed rows to open or
/// close.
pub fn apply_seat_action(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    action: SeatAction,
    mut draw: impl FnMut() -> AiDifficulty,
) {
    let rows = selected_player_rows(state, maps).saturating_sub(1);
    for index in 0..rows.min(state.opponents.len()) {
        let active = state.opponents[index].is_active();
        let row_type = match action {
            SeatAction::FillAi(_) if active => continue,
            SeatAction::FillAi(difficulty) => match difficulty.unwrap_or_else(&mut draw) {
                AiDifficulty::Easy => SkirmishAiRowType::Easy,
                AiDifficulty::Normal => SkirmishAiRowType::Normal,
                AiDifficulty::Hard => SkirmishAiRowType::Hard,
            },
            SeatAction::RemoveAi if active => SkirmishAiRowType::None,
            SeatAction::RemoveAi | SeatAction::CloseAll | SeatAction::OpenClosed => continue,
        };
        apply_combo_selection(
            state,
            SkirmishComboId::AiType(index),
            SkirmishComboItem::AiType(row_type),
        );
    }
}
