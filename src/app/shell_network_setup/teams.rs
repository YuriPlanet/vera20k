//! VERA's room Teams: the host's Teams count, Team Starts and, online, the
//! balance. The session keeps every seated player on one of the room's teams
//! (`MatchSession::set_network_teams`); Team Starts seats each team's
//! players next to each other, as it does in skirmish
//! ([`crate::ui::skirmish_shell::starts_by_team`]). The lobby weighs the
//! players and answers a balance with each one's team
//! (`wol::balance_teams`).

use super::*;
use crate::skirmish_launch::{LaunchStartPosition, LaunchTeam, NETWORK_PLAYER_SLOT_COUNT};

/// How long Balance automatically waits after who plays last changed, as
/// BAR's room bot balances at most once every 5 seconds.
const BALANCE_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// The host sets the room's Teams, Off or a count, and the options carry it.
/// Online, setting a count where Teams was Off turns Balance automatically
/// on, as BAR's team preset has it.
pub(super) fn set_teams(state: &mut AppState, teams: Option<u8>) -> anyhow::Result<()> {
    let was_off = state.match_state.session.network_teams().is_none();
    state.match_state.session.set_network_teams(teams)?;
    if was_off && teams.is_some() && dialog(state).wol() {
        state.match_state.session.set_network_auto_balance(true)?;
    }
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// The host turns Balance automatically on or off, and the options carry it.
pub(super) fn set_auto_balance(state: &mut AppState, on: bool) -> anyhow::Result<()> {
    state.match_state.session.set_network_auto_balance(on)?;
    actions::queue(state, NetworkSetupRequest::PublishOptions);
    Ok(())
}

/// A lobby that cannot take the host's options refuses them while they carry
/// a field it does not know: the room goes on without the newest one it
/// carries, the join queue before Balance automatically before Teams, and
/// publishes again. Returns the line telling the host so, or `None` when
/// none was set.
pub(crate) fn lobby_refused_option(state: &mut AppState) -> Option<&'static str> {
    if !state.match_state.session.network_queue().is_empty() {
        // The room stops queueing, and its spectators keep watching.
        state.match_state.session.stop_network_queue().ok()?;
        actions::queue(state, NetworkSetupRequest::PublishOptions);
        return Some("VERA:QueueNotTaken");
    }
    let session = &state.match_state.session;
    if session.network_auto_balance() {
        return set_auto_balance(state, false)
            .is_ok()
            .then_some("VERA:AutoBalanceNotTaken");
    }
    let set = session
        .network_setup()
        .is_some_and(|launch| launch.options.teams.is_some());
    (set && set_teams(state, None).is_ok()).then_some("VERA:TeamsNotTaken")
}

/// The host's Balance in the Teams box: `/balance` in the room's chat, as
/// if typed.
pub(super) fn balance(state: &mut AppState) {
    actions::queue(state, NetworkSetupRequest::Balance { automatic: false });
}

/// Balance automatically in the host's game: who plays in a room that
/// balances itself, for [`NetworkSetupProtocol::time_balance`]. Returns when
/// the next balance is due.
pub(super) fn poll_balance(state: &mut AppState) -> Option<Instant> {
    let automatic = state.match_state.session.host()
        && state.match_state.session.network_auto_balance()
        && dialog(state).wol()
        && !quick_match(state)
        && !external(state);
    let session = &state.match_state.session;
    let roster = session.network_setup().filter(|_| automatic).map(|launch| {
        let playing: Vec<&str> = launch
            .humans
            .iter()
            .filter(|human| human.raw_slot_index.is_some() && !human.is_observer())
            .map(|human| human.name.as_str())
            .collect();
        let balanceable = launch.opponents.is_empty() && playing.len() >= 2;
        (
            session.network_teams().unwrap_or_default(),
            playing,
            balanceable,
        )
    });
    state
        .frontend
        .network_setup_protocol
        .as_mut()?
        .time_balance(roster, Instant::now())
}

/// Whether a balance of Balance automatically is due or waits for the
/// lobby's answer: the host's Start Game waits for it.
pub(super) fn balance_waiting(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(NetworkSetupProtocol::balance_waiting)
}

impl NetworkSetupProtocol {
    /// Balance automatically's timer. `roster` is the Teams count, the
    /// playing humans by name and whether the room has anything to balance
    /// (two or more players and no AI), or `None` while the room does not
    /// balance itself. A change starts the 5 seconds again; when they are
    /// up, the lobby's quiet `balance` command goes out, which says nothing
    /// when nothing moves. Returns when the next balance is due.
    fn time_balance(
        &mut self,
        roster: Option<(u8, Vec<&str>, bool)>,
        now: Instant,
    ) -> Option<Instant> {
        let Some((teams, playing, balanceable)) = roster else {
            self.balance_roster = None;
            self.balance_due = None;
            return None;
        };
        let same = self
            .balance_roster
            .as_ref()
            .is_some_and(|(count, names, was)| {
                (*count, *was) == (teams, balanceable)
                    && names.iter().map(String::as_str).eq(playing.iter().copied())
            });
        if !same {
            let names = playing.into_iter().map(str::to_owned).collect();
            self.balance_roster = Some((teams, names, balanceable));
            self.balance_due = balanceable.then(|| now + BALANCE_DELAY);
        }
        let due = self.balance_due?;
        if now < due {
            return Some(due);
        }
        // The lobby's answer or the start's end wakes the room again.
        if self.balance_sent || self.wol_start_pending || self.locked {
            return None;
        }
        self.balance_due = None;
        self.balance_sent = true;
        self.requests
            .push(NetworkSetupRequest::Balance { automatic: true });
        None
    }

    fn balance_waiting(&self) -> bool {
        self.balance_due.is_some() || self.balance_sent
    }
}

/// The lobby answered the host's automatic balance: its `balance` event,
/// if any, came before.
pub(crate) fn balance_answered(state: &mut AppState) {
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.balance_sent = false;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Balance automatically asks the lobby 5 seconds after who plays last
    /// changed, and the host's Start waits until the lobby answers. A room
    /// with AI has nothing to balance and does not wait.
    #[test]
    fn balance_automatically_waits_five_seconds_and_holds_start_for_the_answer() {
        let mut room = NetworkSetupProtocol::new(
            SetupDialog {
                host: true,
                flavor: SetupFlavor::Wol,
                modern: true,
            },
            false,
        );
        room.take_service_requests();
        let two = || Some((2, vec!["Host", "Guest"], true));
        let three = || Some((2, vec!["Host", "Guest", "Third"], true));
        let opened = Instant::now();
        assert_eq!(
            room.time_balance(two(), opened),
            Some(opened + BALANCE_DELAY)
        );
        // A join 3 seconds in starts the wait again.
        let join = opened + std::time::Duration::from_secs(3);
        assert_eq!(room.time_balance(three(), join), Some(join + BALANCE_DELAY));
        assert!(room.balance_waiting() && room.take_service_requests().is_empty());

        let due = join + BALANCE_DELAY;
        assert_eq!(room.time_balance(three(), due), None);
        assert_eq!(
            room.take_service_requests(),
            vec![NetworkSetupRequest::Balance { automatic: true }]
        );
        assert!(room.balance_waiting(), "Start waits for the lobby's answer");
        room.balance_sent = false;
        assert_eq!(room.time_balance(three(), due + BALANCE_DELAY), None);
        assert!(!room.balance_waiting() && room.take_service_requests().is_empty());

        let with_ai = Some((2, vec!["Host", "Guest", "Third"], false));
        assert_eq!(room.time_balance(with_ai, due), None);
        assert!(!room.balance_waiting());
    }
}
