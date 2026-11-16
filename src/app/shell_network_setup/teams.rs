//! VERA's room Teams: the host's Teams count, Team Starts, Team Colors and,
//! online, the balance. With Teams set the session keeps every seated player
//! on one of the room's teams (`MatchSession::set_network_teams`); with it
//! Off the room is one free-for-all box. Team Starts seats each
//! team's players next to each other, as it does in skirmish
//! ([`crate::ui::skirmish_shell::starts_by_team`]), and Team Colors gives
//! each team its own range of hues ([`colors_by_team`]). The lobby weighs
//! the players and answers a balance with each one's team
//! (`wol::balance_teams`).

use super::*;
use crate::app::match_runtime::session::SeatPick;
use crate::rules::ruleset::RuleSet;
use crate::rules::skirmish_colors::RETAIL_COLOR_COUNT;
use crate::skirmish_launch::{
    LaunchStartPosition, LaunchTeam, NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchSession,
};

/// The host sets the room's Teams, Off or a count, and the options carry it.
pub(super) fn set_teams(state: &mut AppState, teams: Option<u8>) -> anyhow::Result<()> {
    state.match_state.session.set_network_teams(teams)?;
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// A lobby that cannot take the host's options refuses them while they carry
/// a field it does not know: the room goes on without the newest one it
/// carries, Superweapon Limit before the rest of the room's rules before the
/// join queue before Teams, and publishes again.
/// Returns the line telling the host so, or `None` when none was set.
///
/// A random map for more than eight players, which a lobby from before them
/// refuses, comes first: the room keeps its options, and the host picks
/// another map.
pub(crate) fn lobby_refused_option(state: &mut AppState) -> Option<&'static str> {
    let random_map = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| launch.selected_map_file.as_deref())
        .is_some_and(|file| file.eq_ignore_ascii_case("RandMap.Sed"));
    if random_map
        && state
            .frontend
            .multiplayer_shell_runtime
            .start_staging()
            .count
            > crate::map::preview::NATIVE_START_STAGING_SLOTS as i32
    {
        return Some("VERA:BigRandomMapNotTaken");
    }
    if let Some(mut options) = state
        .match_state
        .session
        .network_setup()
        .map(|launch| launch.options.clone())
        .filter(|options| options.host_rules.superweapon_limit != 0)
    {
        // A lobby from before the limit: the room keeps its other rules.
        options.host_rules.superweapon_limit = 0;
        state
            .match_state
            .session
            .set_network_options(options)
            .ok()?;
        actions::queue(state, NetworkSetupRequest::PublishOptions);
        return Some("VERA:SuperweaponLimitNotTaken");
    }
    if let Some(mut options) = state
        .match_state
        .session
        .network_setup()
        .map(|launch| launch.options.clone())
        .filter(|options| !options.host_rules.is_empty())
    {
        // Every player then plays the original rules there.
        options.host_rules = Default::default();
        state
            .match_state
            .session
            .set_network_options(options)
            .ok()?;
        actions::queue(state, NetworkSetupRequest::PublishOptions);
        return Some("VERA:HostRulesNotTaken");
    }
    if !state.match_state.session.network_queue().is_empty() {
        // The room stops queueing, and its spectators keep watching.
        state.match_state.session.stop_network_queue().ok()?;
        actions::queue(state, NetworkSetupRequest::PublishOptions);
        return Some("VERA:QueueNotTaken");
    }
    let set = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.options.teams.is_some());
    (set && set_teams(state, None).is_ok()).then_some("VERA:TeamsNotTaken")
}

/// The host's Balance in the team bar: `/balance` in the room's chat, as
/// if typed.
pub(super) fn balance(state: &mut AppState) {
    actions::queue(state, NetworkSetupRequest::Balance);
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
    // A pick Team Starts cannot place stays out of its seating.
    let seats: Vec<(u8, LaunchTeam, LaunchStartPosition)> = seats(launch)
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
        .map(|&(_, team, _)| match team {
            LaunchTeam::Team(team) => i32::from(team),
            LaunchTeam::None => -2,
        })
        .collect();
    let drive = App::map_drive_distances(state, map, &starts);
    let given = crate::ui::skirmish_shell::starts_by_team(&starts, drive.as_deref(), &teams, &kept);
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

/// The rows that play, in row order, with their teams and starts: the
/// seated people and the AI.
fn seats(launch: &SkirmishLaunchSession) -> Vec<(u8, LaunchTeam, LaunchStartPosition)> {
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
/// team ([`colors_by_team`]), from the room's colour list and the loaded
/// rules' `[Colors]`. Players can still change theirs. Returns whether it
/// gave any out.
pub(super) fn team_colors(state: &mut AppState) -> anyhow::Result<bool> {
    let Some(count) = state.match_state.session.network_teams() else {
        return Ok(false);
    };
    let Some(rules) = state.rules() else {
        return Ok(false);
    };
    let listed = listed_hues(rules);
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
        .zip(colors_by_team(&listed, &teams, count))
        .filter_map(|(&(row, _), color)| Some((row, SeatPick::Color(color?))))
        .collect();
    if picks.is_empty() {
        return Ok(false);
    }
    state.match_state.session.set_network_seat_picks(&picks)?;
    Ok(true)
}

/// The colours a room lists ([`crate::rules::skirmish_colors::SkirmishColorMode`]),
/// YR's eight first, each with the hue of its `[Colors]` entry.
fn listed_hues(rules: &RuleSet) -> Vec<(u8, u8)> {
    let colors = rules.skirmish_colors();
    colors
        .lobby_priorities()
        .filter_map(|color| {
            let entry = colors.entry_for_priority(i32::from(color));
            Some((color, rules.color_schemes.get(entry)?.hsv[0]))
        })
        .collect()
}

/// `[Colors]` hues run 0..=254 round the circle, and 255 is red again, as
/// `HSV_To_RGB` (`0x00517440`, [`crate::rules::color_scheme::hsv_to_rgb`])
/// reads them.
const HUES: i32 = 255;

/// Team Colors: each seat's colour, given each seat's team in row order and
/// the room's colours with their hues (`listed`, YR's eight first). The
/// colours, sorted round the hue circle, are cut into one range per team
/// ([`HueCircle`]), and each team's players get colours spread across its
/// range. While every team fits in its range of YR's eight, only the eight
/// are used, as a Random colour prefers them; a bigger room uses the whole
/// list. A team with more players than its range takes the free colours
/// nearest it, and once none is free, teammates share. `None` is a seat on
/// none of the `count` teams, or a room without colours.
fn colors_by_team(listed: &[(u8, u8)], teams: &[u8], count: u8) -> Vec<Option<u8>> {
    let count = usize::from(count);
    let mut players = vec![0; count];
    for &team in teams {
        if let Some(players) = players.get_mut(usize::from(team)) {
            *players += 1;
        }
    }
    let fits = |circle: &HueCircle| {
        circle.ranges.len() == count
            && circle
                .ranges
                .iter()
                .zip(&players)
                .all(|(&(_, len), &players)| players <= len)
    };
    let eight: Vec<(u8, u8)> = listed
        .iter()
        .copied()
        .filter(|&(color, _)| usize::from(color) < RETAIL_COLOR_COUNT)
        .collect();
    let circle = Some(HueCircle::new(&eight, count))
        .filter(fits)
        .unwrap_or_else(|| HueCircle::new(listed, count));
    let given = circle.hand_out(&players);
    let mut next = vec![0; count];
    teams
        .iter()
        .map(|&team| {
            let team = usize::from(team);
            let place = *given.get(team)?.get(next[team])?;
            next[team] += 1;
            Some(circle.colors[place].0)
        })
        .collect()
}

/// A room's colours sorted round the hue circle, with each team's range of
/// them: its first place and how many it holds. The ranges hold the same
/// number of colours, the first teams one more while they do not divide
/// evenly, and follow each other round the circle from the first team's,
/// whose middle is nearest red: two teams get warm against cool.
struct HueCircle {
    colors: Vec<(u8, i32)>,
    ranges: Vec<(usize, usize)>,
}

impl HueCircle {
    fn new(listed: &[(u8, u8)], count: usize) -> Self {
        let mut colors: Vec<(u8, i32)> = listed
            .iter()
            .map(|&(color, hue)| (color, i32::from(hue) % HUES))
            .collect();
        colors.sort_by_key(|&(_, hue)| hue);
        let mut circle = Self {
            colors,
            ranges: Vec::new(),
        };
        let places = circle.colors.len();
        if places == 0 || count == 0 {
            return circle;
        }
        let lengths: Vec<usize> = (0..count)
            .map(|team| places / count + usize::from(team < places % count))
            .collect();
        let first = lengths.first().copied().unwrap_or_default();
        let start = (0..places)
            .min_by_key(|&start| hue_gap(circle.middle(start, first), 0))
            .unwrap_or_default();
        let mut place = start;
        circle.ranges = lengths
            .into_iter()
            .map(|len| {
                let range = (place, len);
                place = (place + len) % places;
                range
            })
            .collect();
        circle
    }

    /// The middle hue of the `len` colours round the circle from `start`,
    /// in half hue steps.
    fn middle(&self, start: usize, len: usize) -> i32 {
        let hue = |place: usize| self.colors[place % self.colors.len()].1;
        let first = hue(start);
        let last = hue(start + len.max(1) - 1);
        2 * first + (last - first).rem_euclid(HUES)
    }

    /// Each team's places on the circle, one per player: spread across its
    /// range, then the free places nearest its range, then its own again.
    fn hand_out(&self, players: &[usize]) -> Vec<Vec<usize>> {
        let places = self.colors.len();
        let mut taken = vec![false; places];
        let mut given = vec![Vec::new(); self.ranges.len()];
        let teams = || self.ranges.iter().zip(players);
        for (own, (&(start, len), &players)) in given.iter_mut().zip(teams()) {
            let wanted = players.min(len);
            for player in 0..wanted {
                let place = (start + (2 * player + 1) * len / (2 * wanted)) % places;
                taken[place] = true;
                own.push(place);
            }
        }
        for (own, (&(start, len), &players)) in given.iter_mut().zip(teams()) {
            let middle = self.middle(start, len);
            while own.len() < players {
                let Some(place) = (0..places)
                    .filter(|&place| !taken[place])
                    .min_by_key(|&place| hue_gap(2 * self.colors[place].1, middle))
                else {
                    break;
                };
                taken[place] = true;
                own.push(place);
            }
        }
        for (own, (&(start, len), &players)) in given.iter_mut().zip(teams()) {
            let distinct = own.len();
            for player in distinct..players {
                let place = match distinct {
                    0 => (start + len / 2) % places,
                    _ => own[player % distinct],
                };
                own.push(place);
            }
        }
        given
    }
}

/// How far apart two hues lie round the circle, both in half hue steps.
fn hue_gap(a: i32, b: i32) -> i32 {
    let gap = (a - b).rem_euclid(2 * HUES);
    gap.min(2 * HUES - gap)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::rules::skirmish_colors::FIRST_EXTRA_PRIORITY;

    /// The `[Colors]` names of the colours Team Colors gives seats on
    /// `teams`.
    fn team_color_names(
        rules: &RuleSet,
        listed: &[(u8, u8)],
        teams: &[u8],
        count: u8,
    ) -> Vec<String> {
        let colors = rules.skirmish_colors();
        colors_by_team(listed, teams, count)
            .into_iter()
            .map(|color| {
                let entry = colors.entry_for_priority(i32::from(color.expect("a team's colour")));
                rules.color_schemes[entry].name.clone()
            })
            .collect()
    }

    /// Two teams of four get YR's eight, warm against cool, as a Random
    /// colour prefers them. Five a side take the whole list: every player
    /// apart, each team's hues nearer its own end of the circle.
    #[test]
    fn team_colors_give_two_teams_warm_against_cool() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let listed = listed_hues(&rules);
        let four = team_color_names(&rules, &listed, &[0, 1, 0, 1, 0, 1, 0, 1], 2);
        let team = |names: &[String], team: usize| -> Vec<String> {
            names.iter().skip(team).step_by(2).cloned().collect()
        };
        assert_eq!(team(&four, 0), ["Magenta", "DarkRed", "Orange", "Gold"]);
        assert_eq!(
            team(&four, 1),
            ["DarkGreen", "DarkSky", "DarkBlue", "Purple"]
        );

        let teams = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
        let five: Vec<u8> = colors_by_team(&listed, &teams, 2)
            .into_iter()
            .flatten()
            .collect();
        let distinct: std::collections::BTreeSet<u8> = five.iter().copied().collect();
        assert_eq!(distinct.len(), 10);
        assert!(five.iter().any(|&color| color >= FIRST_EXTRA_PRIORITY));
        for (&team, color) in teams.iter().zip(five) {
            let (_, hue) = listed.iter().find(|(listed, _)| *listed == color).unwrap();
            // Red is 0, cyan half the circle away, in half hue steps.
            let hue = 2 * i32::from(*hue);
            let warm = hue_gap(hue, 0) < hue_gap(hue, HUES);
            assert_eq!(warm, team == 0, "colour {color} on Team {team}");
        }
    }

    /// A full room of four teams keeps all 30 players apart, though one
    /// team has a player more than its range holds. With more players than
    /// colours, as with a mod that lists only the eight, only allies share.
    #[test]
    fn team_colors_keep_players_apart_and_share_only_with_allies() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let listed = listed_hues(&rules);
        assert_eq!(listed.len(), 30);
        let teams: Vec<u8> = [(0, 8), (1, 8), (2, 8), (3, 6)]
            .into_iter()
            .flat_map(|(team, players)| std::iter::repeat_n(team, players))
            .collect();
        let full = team_color_names(&rules, &listed, &teams, 4);
        let distinct: std::collections::BTreeSet<&String> = full.iter().collect();
        assert_eq!(distinct.len(), 30);

        let teams = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
        let shared = colors_by_team(&listed[..RETAIL_COLOR_COUNT], &teams, 2);
        let team = |team: u8| -> std::collections::BTreeSet<u8> {
            teams
                .iter()
                .zip(&shared)
                .filter(|(on, _)| **on == team)
                .map(|(_, color)| color.expect("a team's colour"))
                .collect()
        };
        assert_eq!((team(0).len(), team(1).len()), (4, 4));
        assert!(team(0).is_disjoint(&team(1)));
    }
}
