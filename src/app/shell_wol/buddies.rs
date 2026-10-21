//! The game's mirror of the server-side buddy list and the buddies'
//! statuses: one owner for the names `0xB73AA8`, the locate queue
//! `0xB76980` with its pending locate `[0xB73ACC]`/`[0xB779D0]`, the status
//! map `0xB76990`, the channel map `0xB76608`, the ladder queue `0xB779B0`
//! and the game-list refresh time `[0xB778C8]`. The globals outlive every
//! WOL visit; `0x116`'s init and the lobby runner's entry clear them.
//!
//! Writers: the buddy-list sink `0x007AC220`, Add Buddy `0x0079DFE0`,
//! Remove `0x0079CBA0`, the status refresh `0x0079CE70` and `0x116`'s timer,
//! and the find sink's buddy branch (`0x007A9970`, taken whenever a buddy
//! locate is pending). The lobby's `DisplayBuddies` filter (`0x0077D4C0`)
//! and `0x116`'s rows (`0x0079AF10`) read them. No RNG is drawn here.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::service::{self, WolLadderRecord, WolServiceRequest};
use crate::app::AppState;
use crate::ui::wol_lobby::{GAME_CHANNEL, WolChannel};

/// The statuses the find sink stores (`0xB76990`); 6 has no writer.
pub(crate) const FIND_DISABLED: i32 = 1;
pub(crate) const OFFLINE: i32 = 2;
pub(crate) const ONLINE: i32 = 3;
pub(crate) const IN_CHAT: i32 = 4;
pub(crate) const IN_GAME: i32 = 5;
/// `0x84A21C` `Lob_40_` and `0x84A224` `Lob_35_` (Quick Match lobbies),
/// or a listed game's flags `0x800`/`0x1000` while `GameMode` is 4.
pub(crate) const QUICK_MATCH: i32 = 7;
pub(crate) const QUICK_MATCH_COOP: i32 = 8;

/// A pending locate younger than this (`time()` seconds) is waited for.
const LOCATE_HOLD: Duration = Duration::from_secs(5);
/// `0x116`'s timer re-requests the game list once `time() - [0xB778C8] >
/// 30`.
const GAMES_REFRESH: Duration = Duration::from_secs(30);
/// `0x0079D110` takes at most ten names off the ladder queue.
const LADDER_BATCH: usize = 10;

/// The mirror and its queues.
#[derive(Debug, Default)]
pub(crate) struct WolBuddies {
    names: Vec<String>,
    queue: VecDeque<String>,
    locating: Option<(String, Instant)>,
    statuses: HashMap<String, i32>,
    channels: HashMap<String, WolChannel>,
    ladder_queue: VecDeque<String>,
    games_at: Option<Instant>,
    /// `[0xB76E24]`: the selected buddy (static Wstring `0x0077CA30`);
    /// `0x116`'s init leaves it, and `0x11E`'s reply-to falls back to it.
    pub(crate) selected: String,
}

impl WolBuddies {
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    pub(crate) fn status(&self, name: &str) -> Option<i32> {
        self.statuses.get(name).copied()
    }

    pub(crate) fn channel(&self, name: &str) -> Option<&WolChannel> {
        self.channels.get(name)
    }

    /// `0x0079C520`: a name that may become a buddy.
    pub(crate) fn valid_new(&self, name: &str, local: &str) -> bool {
        valid_new_buddy(&self.names, name, local)
    }

    /// `0x116`'s init and the lobby runner's entry: the names, the locate
    /// queue, the status map and the channel map.
    fn clear(&mut self) {
        self.names.clear();
        self.queue.clear();
        self.statuses.clear();
        self.channels.clear();
    }

    /// The add paths: the name joins the buddies, the locate queue and the
    /// ladder queue.
    fn append(&mut self, name: String) {
        self.queue.push_back(name.clone());
        self.ladder_queue.push_back(name.clone());
        self.names.push(name);
    }

    /// The locate kick every add path and the find sink share: a pending
    /// locate younger than 5 s is waited for; otherwise the queue's head
    /// becomes the pending locate.
    fn next_locate(&mut self, now: Instant) -> Option<String> {
        if let Some((_, sent)) = &self.locating
            && now.saturating_duration_since(*sent) < LOCATE_HOLD
        {
            return None;
        }
        self.locating = None;
        let name = self.queue.pop_front()?;
        self.locating = Some((name.clone(), now));
        Some(name)
    }

    /// `0x0079CBA0`: the name leaves the buddies and both maps; the locate
    /// queue keeps it.
    fn remove(&mut self, name: &str) {
        self.names.retain(|known| known != name);
        self.channels.remove(name);
        self.statuses.remove(name);
    }

    /// The find sink's buddy branch (`0x007A9986..0x007AA10B`) for the
    /// pending locate: the status from the channel (`games` is the game
    /// list `[0xB76AE8]`) or the result code, else the old one.
    fn located(&mut self, name: String, result: &Result<WolChannel, u32>, games: &[WolChannel]) {
        let old = self.status(&name).unwrap_or(0);
        let status = match result {
            Ok(channel) if channel.kind == 0 => {
                let prefix = |p: &str| {
                    channel
                        .name
                        .get(..p.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(p))
                };
                if prefix("Lob_40_") {
                    QUICK_MATCH
                } else if prefix("Lob_35_") {
                    QUICK_MATCH_COOP
                } else {
                    IN_CHAT
                }
            }
            Ok(channel) => {
                // GameMode is 4 throughout WOL: a listed game's flags
                // promote it to a Quick Match status.
                let listed = games.iter().find(|game| {
                    crate::util::native_string::crt_strcmpi(&game.name, &channel.name).is_eq()
                });
                match listed.map(|game| game.game_flags) {
                    Some(flags) if flags & 0x1000 != 0 => QUICK_MATCH_COOP,
                    Some(flags) if flags & 0x800 != 0 => QUICK_MATCH,
                    _ => IN_GAME,
                }
            }
            Err(0x0004_0131) => ONLINE,
            Err(0x0004_0130) => OFFLINE,
            Err(0x0004_0132) => FIND_DISABLED,
            Err(_) => old,
        };
        match result {
            Ok(channel) => {
                self.channels.insert(name.clone(), channel.clone());
            }
            Err(_) => {
                self.channels.remove(&name);
            }
        }
        self.statuses.insert(name, status);
    }

    fn ladder_batch(&mut self) -> Vec<String> {
        let count = self.ladder_queue.len().min(LADDER_BATCH);
        self.ladder_queue.drain(..count).collect()
    }
}

/// `0x0079C520` over `names`: not empty, not this user (`[0xB73E40]`,
/// case-insensitive), none of `" ,;"` (`0x0084BA18`) and not already a
/// buddy (case-insensitive).
pub(crate) fn valid_new_buddy(names: &[String], name: &str, local: &str) -> bool {
    !name.is_empty()
        && !name.eq_ignore_ascii_case(local)
        && !name.contains([' ', ',', ';'])
        && !names.iter().any(|known| known.eq_ignore_ascii_case(name))
}

fn mirror(state: &mut AppState) -> &mut WolBuddies {
    &mut state.frontend.wol_buddies
}

/// The pending-locate kick: API+0x60 for the next queued name.
fn kick_locate(state: &mut AppState) {
    if let Some(name) = mirror(state).next_locate(Instant::now()) {
        service::request(state, WolServiceRequest::Locate(name));
    }
}

/// `0x0079D110`: up to ten names of the ladder queue to the ladder API.
pub(super) fn request_ladder(state: &mut AppState) {
    let names = mirror(state).ladder_batch();
    if !names.is_empty() {
        service::request(state, WolServiceRequest::LadderRecords(names));
    }
}

/// `0x116`'s init (`0x0079E6E6..0x0079E7FE`) and the lobby runner's entry
/// (`0x00787BC8..`): the mirror cleared, API+0xDC, then outside Quick
/// Match the status refresh `0x0079CE70`, then the ladder queue from the
/// (empty) buddies (`0x0079D2F0`).
pub(super) fn restart(state: &mut AppState) {
    mirror(state).clear();
    service::request(state, WolServiceRequest::ListBuddies);
    if !super::quick_match::mode_active(state) {
        refresh(state);
    }
    let buddies = mirror(state);
    buddies.ladder_queue = buddies.names.iter().cloned().collect();
    request_ladder(state);
}

/// `0x0079CE70` (and Refresh `0x704`, the timer body without its 30 s
/// test): the game list with autoping (`0x0077EF90(0x29, 1)`), the locate
/// queue rebuilt from the buddies and one locate started.
pub(super) fn refresh(state: &mut AppState) {
    let buddies = mirror(state);
    buddies.games_at = Some(Instant::now());
    buddies.queue = buddies.names.iter().cloned().collect();
    service::request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 1,
        },
    );
    kick_locate(state);
}

/// `0x116`'s `WM_TIMER` 0 (every 1000 ms): the refresh once more than 30 s
/// passed since the last one.
pub(super) fn timer(state: &mut AppState, now: Instant) {
    let due = mirror(state)
        .games_at
        .is_none_or(|at| now.saturating_duration_since(at) > GAMES_REFRESH);
    if due {
        refresh(state);
    }
}

/// `0x0079DFE0`'s Add after API+0xE0: a name the sink has not already added
/// joins the mirror, a locate starts and the ladder is asked.
pub(super) fn add(state: &mut AppState, name: String) {
    service::request(state, WolServiceRequest::AddBuddy(name.clone()));
    let buddies = mirror(state);
    if !buddies
        .names
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&name))
    {
        buddies.append(name);
        kick_locate(state);
    }
    request_ladder(state);
}

/// `0x0079CBA0`, then API+0xE4.
pub(super) fn remove(state: &mut AppState, name: &str) {
    mirror(state).remove(name);
    service::request(state, WolServiceRequest::RemoveBuddy(name.to_owned()));
}

/// The buddy-list sink `0x007AC220`: each valid user joins the mirror and
/// a locate starts; then the ladder is asked and an open `0x116` rebuilds
/// its rows and enables Refresh again.
pub(super) fn buddy_list_received(state: &mut AppState, result: Result<Vec<String>, u32>) {
    let Ok(users) = result else {
        return;
    };
    let local = state.frontend.wol_account.nickname.clone();
    for user in users {
        if mirror(state).valid_new(&user, &local) {
            mirror(state).append(user);
            kick_locate(state);
        }
    }
    request_ladder(state);
    super::buddy::buddies_changed(state, true);
}

/// The find sink while a buddy locate is pending: the answer becomes that
/// buddy's status, an open `0x116` rebuilds its rows, and the next queued
/// name is located at once. False when no buddy locate is pending.
pub(super) fn located(state: &mut AppState, result: &Result<WolChannel, u32>) -> bool {
    let Some((name, _)) = mirror(state).locating.take() else {
        return false;
    };
    let games = super::lobby::games(state).to_vec();
    mirror(state).located(name, result, &games);
    super::buddy::buddies_changed(state, false);
    kick_locate(state);
    true
}

/// The ladder sink `0x007A3100`: the records join the table, the next
/// batch is asked and an open `0x116` shows the statistics.
/// RESIDUAL: in a match the sink only sets a pending flag; VERA stores.
pub(super) fn ladder_received(state: &mut AppState, records: Vec<WolLadderRecord>) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    for record in records {
        visit
            .ladder
            .insert(record.name.to_ascii_lowercase(), record);
    }
    request_ladder(state);
    super::buddy::stats_changed(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(kind: u32, name: &str, flags: u32) -> WolChannel {
        WolChannel {
            kind,
            name: name.into(),
            game_flags: flags,
            ..Default::default()
        }
    }

    #[test]
    fn validity_refuses_self_separators_and_known_buddies() {
        let mut buddies = WolBuddies::default();
        buddies.append("Tanya".into());
        assert!(buddies.valid_new("Boris", "Tester"));
        for name in ["", "tester", "a b", "a,b", "a;b", "TANYA"] {
            assert!(!buddies.valid_new(name, "Tester"), "{name}");
        }
    }

    #[test]
    fn locates_wait_five_seconds_then_take_the_queue_head() {
        let mut buddies = WolBuddies::default();
        buddies.append("A".into());
        buddies.append("B".into());
        let now = Instant::now();
        assert_eq!(buddies.next_locate(now).as_deref(), Some("A"));
        assert_eq!(buddies.next_locate(now + Duration::from_secs(4)), None);
        assert_eq!(
            buddies.next_locate(now + Duration::from_secs(5)).as_deref(),
            Some("B")
        );
        assert_eq!(buddies.next_locate(now + Duration::from_secs(11)), None);
        assert!(buddies.locating.is_none());
    }

    #[test]
    fn the_find_sink_maps_channels_and_codes_to_statuses() {
        let mut buddies = WolBuddies::default();
        let games = [channel(GAME_CHANNEL, "qm's_game", 0x800)];
        let cases = [
            (Ok(channel(0, "Lob_41_3", 0)), IN_CHAT),
            (Ok(channel(0, "lob_40_1", 0)), QUICK_MATCH),
            (Ok(channel(0, "Lob_35_0", 0)), QUICK_MATCH_COOP),
            (Ok(channel(GAME_CHANNEL, "x's_game", 0)), IN_GAME),
            (Ok(channel(GAME_CHANNEL, "QM's_game", 0)), QUICK_MATCH),
            (Err(0x0004_0131), ONLINE),
            (Err(0x0004_0132), FIND_DISABLED),
            (Err(0x0004_0130), OFFLINE),
            // An unknown code keeps the old status.
            (Err(0x8000_4005), OFFLINE),
        ];
        for (result, status) in cases {
            buddies.located("A".into(), &result, &games);
            assert_eq!(buddies.status("A"), Some(status), "{result:?}");
            assert_eq!(buddies.channel("A").is_some(), result.is_ok());
        }
        buddies.remove("A");
        assert_eq!(buddies.status("A"), None);
    }

    #[test]
    fn ladder_batches_take_ten_names() {
        let mut buddies = WolBuddies::default();
        for index in 0..12 {
            buddies.append(format!("n{index}"));
        }
        assert_eq!(buddies.ladder_batch().len(), 10);
        assert_eq!(buddies.ladder_batch(), ["n10", "n11"]);
        assert!(buddies.ladder_batch().is_empty());
    }
}
