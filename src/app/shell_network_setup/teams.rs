//! VERA's room Teams: the host's Teams count, and the Quick Team Setup and
//! Bulk Seat Actions lists in its team bar ([`super::team_menus`]). With
//! Teams set the session keeps every seated player on one of the room's
//! teams (`MatchSession::set_network_teams`); with it Off the room is one
//! free-for-all box. Team Starts seats each team's players next to each
//! other, as it does in skirmish
//! ([`crate::ui::skirmish_shell::starts_by_team`]), and Team Colors gives
//! each team its own range of hues
//! ([`crate::ui::team_tools::colors_by_team`]). The lobby weighs the players
//! and answers a balance with each one's team (`wol::balance_teams`).

use super::*;
use crate::app::match_runtime::session::{MatchSession, SeatPick};
use crate::skirmish_launch::{
    LaunchStartPosition, LaunchTeam, NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchSession,
};

/// The host sets the room's Teams, Off or a count, and the options carry it.
pub(super) fn set_teams(state: &mut AppState, teams: Option<u8>) -> anyhow::Result<()> {
    state.match_state.session.set_network_teams(teams)?;
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// Only a lobby known to predate watching may need a room rule omitted.
pub(crate) fn lobby_refused_option(state: &mut AppState) -> Option<&'static str> {
    let lobby_watches = crate::app::shell_wol::lobby_watches(state);
    if lobby_watches == Some(false)
        && super::maps::generated_selection(&state.match_state.session)
        && state
            .frontend
            .multiplayer_shell_runtime
            .start_staging()
            .count
            > crate::map::preview::NATIVE_START_STAGING_SLOTS as i32
    {
        return Some("VERA:BigRandomMapNotTaken");
    }
    let line = refused_room_rule(&mut state.match_state.session, lobby_watches)?;
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Some(line)
}

/// A lobby from before the room's Building Production Queue refuses options
/// that carry it, whether or not it lists matches: the host turns the queue
/// off and sends its options again. `None` when the refused options did not
/// carry it, so the refusal stands.
pub(crate) fn lobby_refused_building_queue(state: &mut AppState) -> Option<&'static str> {
    let line = refused_building_queue(&mut state.match_state.session)?;
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Some(line)
}

fn refused_building_queue(session: &mut MatchSession) -> Option<&'static str> {
    if !session.host() {
        return None;
    }
    let mut options = session.network_setup()?.options.clone();
    if !options.host_rules.building_production_queue {
        return None;
    }
    options.host_rules.building_production_queue = false;
    session.set_network_options(options).ok()?;
    Some("VERA:BuildingQueueNotTaken")
}

fn refused_room_rule(
    session: &mut MatchSession,
    lobby_watches: Option<bool>,
) -> Option<&'static str> {
    // All these keys predate watching. An unknown capability is rechecked
    // before retry; an ordinary refusal from a current lobby keeps the room.
    if lobby_watches != Some(false) || !session.host() {
        return None;
    }
    let mut options = session.network_setup()?.options.clone();
    if options.host_rules.superweapon_limit != 0 {
        options.host_rules.superweapon_limit = 0;
        session.set_network_options(options).ok()?;
        Some("VERA:SuperweaponLimitNotTaken")
    } else if !options.host_rules.is_empty() {
        options.host_rules = Default::default();
        session.set_network_options(options).ok()?;
        Some("VERA:HostRulesNotTaken")
    } else if !session.network_queue().is_empty() {
        session.stop_network_queue().ok()?;
        Some("VERA:QueueNotTaken")
    } else if options.teams.is_some() {
        session.set_network_teams(None).ok()?;
        Some("VERA:TeamsNotTaken")
    } else {
        None
    }
}

/// The host's Balance Teams in Quick Team Setup, online: `/balance` in the
/// room's chat, as if typed.
pub(super) fn balance(state: &mut AppState) {
    actions::queue(state, NetworkSetupRequest::Balance);
}

/// Team Starts in a room with Teams set: each player's start by team, on
/// the room's map. With `keep_picks`, as the host's Start Game does, a
/// player keeps the start they picked and only those on Random get one, in
/// place of gamemd's farthest free start (`0x005D6890`); the Team Starts
/// item gives everyone one. Where the room seats more players than the
/// map has starts, a team's people share its bases ([`shared_bases`]) and
/// everyone gets one. Returns whether it gave any out.
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
    let all = seats(launch);
    // Where bases are shared, a start a later team change left to players
    // of two teams is dealt again with every other: each team shares only
    // its own.
    let mixed = all.iter().enumerate().any(|(index, &(_, team, start))| {
        matches!(start, LaunchStartPosition::Position(_))
            && all[..index].iter().any(|&(_, other, held)| {
                held == start && (other != team || team == LaunchTeam::None)
            })
    });
    let keep_picks = keep_picks && !(mixed && state.match_state.session.network_shares_bases());
    // A pick Team Starts cannot place stays out of its seating.
    let seats: Vec<(u8, LaunchTeam, LaunchStartPosition)> = all
        .into_iter()
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
        .map(|&(_, team, _)| team.shell_value())
        .collect();
    let session = &state.match_state.session;
    let bases = starts
        .len()
        .min(session.network_setup_capacity().unwrap_or(0).max(0) as usize);
    let shared = (session.network_shares_bases() && seats.len() > bases)
        .then(|| shared_bases(&teams, bases))
        .flatten();
    let drive = App::map_drive_distances(state, map, &starts);
    let given = if let Some((base_of, base_teams)) = shared {
        let free = vec![None; base_teams.len()];
        let given = crate::ui::skirmish_shell::starts_by_team(
            &starts,
            drive.as_deref(),
            &base_teams,
            &free,
        );
        base_of.into_iter().map(|base| given[base]).collect()
    } else {
        crate::ui::skirmish_shell::starts_by_team(&starts, drive.as_deref(), &teams, &kept)
    };
    let picks: Vec<(u8, SeatPick)> = seats
        .iter()
        .zip(given)
        .map(|(&(row, ..), start)| {
            let start = start.and_then(|start| u8::try_from(start).ok());
            let start = start.map_or(LaunchStartPosition::Auto, LaunchStartPosition::Position);
            (row, SeatPick::Start(start))
        })
        .collect();
    state.match_state.session.set_network_seat_picks(&picks)?;
    Ok(true)
}

/// A room sharing bases (`MatchSession::network_shares_bases`) that seats
/// more players than its map's `starts`: each player on no team (-2) needs
/// a base of its own, each team at least one, and every start left over
/// goes to the team with the most players for each base, the first on a
/// tie. A team's players are dealt over its bases in row order. The people
/// on one start share its House
/// ([`crate::skirmish_launch::SkirmishLaunchSession::house_leader`]), and
/// an AI there starts beside them in its own. Takes each seat's team;
/// returns each seat's base and each base's team, or `None` when the map
/// has too few starts even for that.
fn shared_bases(seats: &[i32], starts: usize) -> Option<(Vec<usize>, Vec<i32>)> {
    // Each team with players and how many, in the order they first sit.
    let mut teams: Vec<(i32, usize)> = Vec::new();
    let mut alone = 0;
    for &team in seats {
        match teams.iter_mut().find(|(on, _)| *on == team) {
            _ if team < 0 => alone += 1,
            Some((_, players)) => *players += 1,
            None => teams.push((team, 1)),
        }
    }
    let mut spare = starts.checked_sub(alone + teams.len())?;
    let mut bases = vec![1; teams.len()];
    while spare > 0 {
        // a/b players for each base is more than c/d when a*d > c*b.
        let crowded = (0..teams.len())
            .filter(|&team| bases[team] < teams[team].1)
            .max_by(|&a, &b| {
                (teams[a].1 * bases[b])
                    .cmp(&(teams[b].1 * bases[a]))
                    .then(b.cmp(&a))
            });
        let Some(team) = crowded else {
            break;
        };
        bases[team] += 1;
        spare -= 1;
    }
    let mut base_teams = Vec::new();
    let mut first = Vec::with_capacity(teams.len());
    for (&(team, _), &count) in teams.iter().zip(&bases) {
        first.push(base_teams.len());
        base_teams.extend(std::iter::repeat_n(team, count));
    }
    let mut dealt = vec![0; teams.len()];
    let base_of = seats
        .iter()
        .map(|&team| match teams.iter().position(|(on, _)| *on == team) {
            Some(index) => {
                dealt[index] += 1;
                first[index] + (dealt[index] - 1) % bases[index]
            }
            None => {
                base_teams.push(team);
                base_teams.len() - 1
            }
        })
        .collect();
    Some((base_of, base_teams))
}

/// A room needs a start per playing participant, or one per playing team
/// plus a start per unteamed participant when the room shares bases.
pub(crate) fn starts_needed(
    launch: &SkirmishLaunchSession,
    shared: bool,
    flavor: SetupFlavor,
) -> i32 {
    let people = launch.humans.iter().filter(|human| !human.is_observer());
    if !shared {
        let ai = match flavor {
            SetupFlavor::Wol => launch.opponents.len() as i32,
            SetupFlavor::Lan => launch.ai_player_count,
        };
        return people.count() as i32 + ai;
    }
    // Online the lobby counts every published player. LAN shares bases among
    // the people bound to rows; a node awaiting its row occupies no base yet.
    let people =
        people.filter(|human| flavor == SetupFlavor::Wol || human.raw_slot_index.is_some());
    let mut teams = Vec::new();
    let mut alone = 0;
    for team in people
        .map(|human| human.slot.team)
        .chain(launch.opponents.iter().map(|ai| ai.team))
    {
        match team {
            LaunchTeam::Team(team) if !teams.contains(&team) => teams.push(team),
            LaunchTeam::Team(_) => {}
            LaunchTeam::None => alone += 1,
        }
    }
    (teams.len() + alone) as i32
}

/// The rows that play, in row order, with their teams and starts: the
/// seated people and the AI.
pub(super) fn seats(launch: &SkirmishLaunchSession) -> Vec<(u8, LaunchTeam, LaunchStartPosition)> {
    (0..NETWORK_PLAYER_SLOT_COUNT as u8)
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
        .collect()
}

/// Team Colors in a room with Teams set: every seated player's colour by
/// team ([`crate::ui::team_tools::colors_by_team`]), from the room's colour
/// list and the loaded rules' `[Colors]`. Players can still change theirs.
/// Returns whether it gave any out.
pub(super) fn team_colors(state: &mut AppState) -> anyhow::Result<bool> {
    let Some(count) = state.match_state.session.network_teams() else {
        return Ok(false);
    };
    let Some(rules) = state.rules() else {
        return Ok(false);
    };
    let listed = crate::ui::team_tools::listed_hues(rules);
    let launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no setup"))?;
    // Every seated player is on one of the room's teams.
    let seats: Vec<(u8, u8)> = seats(launch)
        .into_iter()
        .filter_map(|(row, team, _)| match team {
            LaunchTeam::Team(team) => Some((row, team)),
            LaunchTeam::None => None,
        })
        .collect();
    let teams: Vec<u8> = seats.iter().map(|&(_, team)| team).collect();
    let picks: Vec<(u8, SeatPick)> = seats
        .iter()
        .zip(crate::ui::team_tools::colors_by_team(
            &listed, &teams, count,
        ))
        .filter_map(|(&(row, _), color)| Some((row, SeatPick::Color(color?))))
        .collect();
    if picks.is_empty() {
        return Ok(false);
    }
    state.match_state.session.set_network_seat_picks(&picks)?;
    Ok(true)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// Any lobby that refuses options carrying the building queue may be
    /// from before it, so the host drops the queue and keeps its other rules;
    /// a refusal of options without it stands.
    #[test]
    fn a_refused_building_queue_is_dropped_and_the_other_rules_kept() {
        use crate::app::match_runtime::session::NetworkReturnContext;
        let rooms = crate::test_fixture::json("tests/fixtures/lobby/options.json");
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(rooms["rooms"]["generated_map_for_thirty"]["launch"].clone())
                .unwrap();
        launch.options.host_rules.building_production_queue = true;
        launch.options.host_rules.superweapon_limit = 7;
        let mut room = MatchSession::default();
        room.enter_network(NetworkReturnContext::Wol);
        room.accept_network_setup(launch, true, None, true).unwrap();
        assert_eq!(
            refused_building_queue(&mut room),
            Some("VERA:BuildingQueueNotTaken")
        );
        let rules = &room.network_setup().unwrap().options.host_rules;
        assert!(!rules.building_production_queue);
        assert_eq!(rules.superweapon_limit, 7);
        assert_eq!(refused_building_queue(&mut room), None);
    }

    #[test]
    fn only_a_confirmed_legacy_lobby_can_drop_a_host_rule() {
        use crate::app::match_runtime::session::NetworkReturnContext;
        let rooms = crate::test_fixture::json("tests/fixtures/lobby/options.json");
        let mut launch: SkirmishLaunchSession =
            serde_json::from_value(rooms["rooms"]["generated_map_for_thirty"]["launch"].clone())
                .unwrap();
        launch.options.host_rules.superweapon_limit = 7;
        let mut room = MatchSession::default();
        room.enter_network(NetworkReturnContext::Wol);
        room.accept_network_setup(launch, true, None, true).unwrap();
        assert_eq!(refused_room_rule(&mut room, None), None);
        assert_eq!(refused_room_rule(&mut room, Some(true)), None);
        assert_eq!(
            room.network_setup()
                .unwrap()
                .options
                .host_rules
                .superweapon_limit,
            7
        );
        assert_eq!(
            refused_room_rule(&mut room, Some(false)),
            Some("VERA:SuperweaponLimitNotTaken")
        );
        assert_eq!(
            room.network_setup()
                .unwrap()
                .options
                .host_rules
                .superweapon_limit,
            0
        );
    }

    /// Four players on Team A, three on B and one on no team on a
    /// four-start map: the lone player keeps a base of its own, each team
    /// gets one, and the spare start goes to A, with more players for each
    /// base. Three teams on two starts get none.
    #[test]
    fn shared_bases_give_the_spare_start_to_the_crowded_team() {
        // Rows: A, B, A, B, none, A, B, A.
        let seats = [0, 1, 0, 1, -2, 0, 1, 0];
        let (base_of, base_teams) = shared_bases(&seats, 4).unwrap();
        assert_eq!(base_teams, [0, 0, 1, -2]);
        assert_eq!(base_of, [0, 2, 1, 2, 3, 0, 2, 1]);
        assert_eq!(shared_bases(&[0, 1, 2], 2), None);
    }
}
