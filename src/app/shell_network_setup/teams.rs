//! VERA's room Teams: the host's Teams count and Team Starts. The session
//! keeps every seated player on one of the room's teams
//! (`MatchSession::set_network_teams`); Team Starts seats each team's
//! players next to each other, as it does in skirmish
//! ([`crate::ui::skirmish_shell::starts_by_team`]).

use super::*;
use crate::skirmish_launch::{LaunchStartPosition, LaunchTeam, NETWORK_PLAYER_SLOT_COUNT};

/// The host sets the room's Teams, Off or a count, and the options carry it.
pub(super) fn set_teams(state: &mut AppState, teams: Option<u8>) -> anyhow::Result<()> {
    state.match_state.session.set_network_teams(teams)?;
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// A lobby from before Teams refuses the host's options while they carry
/// it: the room goes on with Teams Off. Returns whether Teams was set.
pub(crate) fn lobby_refused_teams(state: &mut AppState) -> bool {
    let set = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.options.teams.is_some());
    set && set_teams(state, None).is_ok()
}

/// Team Starts in a room with Teams set: each player's start by team, on
/// the room's map. With `keep_picks`, as the host's Start Game does, a
/// player keeps the start they picked and only those on Random get one, in
/// place of gamemd's farthest free start (`0x005D6890`); the Team Starts
/// button gives everyone one. Returns whether it gave any out.
pub(super) fn team_starts(state: &mut AppState, keep_picks: bool) -> anyhow::Result<bool> {
    if state.match_state.session.network_teams().is_none() {
        return Ok(false);
    }
    let Some(map) = view::local_map_index(state) else {
        return Ok(false);
    };
    let starts = App::map_start_cells(state, map);
    if starts.len() < 2 {
        return Ok(false);
    }
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no setup"))?;
    // The rows that play, in row order, with their teams and starts. A pick
    // Team Starts cannot place stays out of its seating.
    let seats: Vec<(u8, LaunchTeam, LaunchStartPosition)> = (0..NETWORK_PLAYER_SLOT_COUNT as u8)
        .filter_map(|row| {
            let human = launch
                .humans
                .iter()
                .find(|human| human.raw_slot_index == Some(row) && !human.is_observer())
                .map(|human| (row, human.slot.team, human.slot.start_position));
            human.or_else(|| {
                let ai = launch
                    .opponents
                    .iter()
                    .find(|ai| ai.raw_slot_index == row)?;
                Some((row, ai.team, ai.start_position))
            })
        })
        .filter(|&(_, _, start)| {
            !keep_picks
                || !matches!(start, LaunchStartPosition::Position(start)
                    if usize::from(start) >= starts.len())
        })
        .collect();
    let kept: Vec<Option<usize>> = seats
        .iter()
        .map(|&(_, _, start)| match start {
            LaunchStartPosition::Position(start) if keep_picks => Some(usize::from(start)),
            _ => None,
        })
        .collect();
    if kept.iter().all(Option::is_some) {
        return Ok(false);
    }
    // -2 is the Team list's no team, which plays alone.
    let teams: Vec<i32> = seats
        .iter()
        .map(|&(_, team, _)| match team {
            LaunchTeam::Team(team) => i32::from(team),
            LaunchTeam::None => -2,
        })
        .collect();
    let drive = App::map_drive_distances(state, map, &starts);
    let given = crate::ui::skirmish_shell::starts_by_team(&starts, drive.as_deref(), &teams, &kept);
    let picks: Vec<(u8, LaunchStartPosition)> = seats
        .iter()
        .zip(given)
        .map(|(&(row, ..), start)| {
            let start = start.and_then(|start| u8::try_from(start).ok());
            (
                row,
                start.map_or(LaunchStartPosition::Auto, LaunchStartPosition::Position),
            )
        })
        .collect();
    state.match_state.session.set_network_starts(&picks)?;
    Ok(true)
}
