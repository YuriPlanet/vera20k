//! Per-team aggregation behind the in-game superweapon timer panel.
//!
//! This is a VERA presentation extension, on with `[VERA] GroupedSuperTimers=`
//! (Options), not a `TacticalClass::Draw` parity path: it replaces the native
//! one-line-per-Super lines with one line per (team, superweapon type). Off,
//! the native timer list draws as before.
//!
//! Teams are the *real launch teams* (`LaunchTeam::Team(u8)`, kept for the
//! whole match in the app-owned `SkirmishLaunchSession`), not the live
//! alliance graph: MakeAlly/MakeEnemy, triggers and the Paranoid AI rewrite
//! alliances mid-match, which splits houses that launched on the same team
//! and mislabels the rows. `Team(0)` prints "A", `Team(1)` "B", and so on; a
//! house on no team (`LaunchTeam::None`, or one the caller cannot map) shares
//! the single "-" row — the panel's one "no-team bucket" of all such houses.
//!
//! This panel lists *other* houses' timers only: the local player's own
//! Supers are always omitted (it reads those on the sidebar). A better home
//! for one's own countdown is left to a separate change. Only Supers of the
//! native timer list reach here, after the `SW.ShowTimer=` / Ares
//! `SW.TimerVisibility` gates have filtered them out.

use std::collections::{BTreeMap, HashMap};

use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperTimerView;

/// Native `TacticalClass::DrawTimer` frames-per-second divisor
/// (`0x88888889`, `sar 3`), shared with `super_timers`.
const FRAMES_PER_SECOND: i32 = 15;

/// A display team. `Named(0)` prints "A" (launch team 0), `Named(1)` "B",
/// and so on; `Unteamed` prints "-". Deriving `Ord` keeps unteamed rows last,
/// matching the "-, and yourself" ordering in the panel design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TeamKey {
    Named(u8),
    Unteamed,
}

impl TeamKey {
    /// The panel's short label: A..Z (then `T<n>` beyond 26 teams). A named
    /// team shows its letter; the no-team bucket shows its player count,
    /// `(N)`, so the row reads `(7) Nuke 43/119` rather than `- Nuke 43/119`.
    pub(crate) fn label(self, players: usize) -> String {
        match self {
            Self::Named(index) if index < 26 => ((b'A' + index) as char).to_string(),
            Self::Named(index) => format!("T{index}"),
            Self::Unteamed => format!("({players})"),
        }
    }

    /// The row key for a launch team number (`None` = no team).
    fn from_launch_team(team: Option<u8>) -> Self {
        team.map_or(Self::Unteamed, Self::Named)
    }
}

/// One aggregated `(team, superweapon type)` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeamSuperRow {
    pub(crate) team: TeamKey,
    /// A member house used for the row's colour and blink identity: the
    /// first holder of this type in house order, so it stays stable per frame.
    pub(crate) representative: InternedId,
    pub(crate) sw_type: InternedId,
    /// Houses in this team whose Super of this type is charged (`+0x6F`).
    pub(crate) ready: usize,
    /// Houses in this team that hold this type at all.
    pub(crate) total: usize,
    /// Shortest whole seconds left among the not-yet-ready houses; `0` when
    /// every holder is ready (the panel then prints "0:00").
    pub(crate) nearest_seconds: i32,
    /// Number of player houses (human or AI, not observers) sitting on this
    /// display team. The no-team row shows it as its `(N)` caption.
    pub(crate) players: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    team: TeamKey,
    sw_type: InternedId,
}

/// Group `views` into [`TeamSuperRow`]s by each owner's real launch team.
///
/// `houses` is `sim.houses().order()` (stable, native registration order),
/// used only to pick a stable representative. `team_of(house)` returns the
/// house's launch team number, or `None` when it is on no team / cannot be
/// mapped. `local`'s own Supers are never counted — this panel is other
/// houses' timers.
pub(crate) fn grouped_super_rows(
    houses: &[InternedId],
    team_of: impl Fn(InternedId) -> Option<u8>,
    views: &[SuperTimerView],
    local: Option<InternedId>,
    players_of: impl Fn(Option<u8>) -> usize,
) -> Vec<TeamSuperRow> {
    let mut index_of: HashMap<InternedId, usize> = HashMap::with_capacity(houses.len());
    for (index, &house) in houses.iter().enumerate() {
        index_of.insert(house, index);
    }

    // (ready, total, nearest_seconds, first holder's house index), with
    // i32::MAX as "no pending holder". The smallest holder index is the
    // representative, so a row keeps one house's colour/blink across frames
    // instead of picking one from a map whose order changes from call to call.
    let mut rows: BTreeMap<RowKey, (usize, usize, i32, usize)> = BTreeMap::new();
    for view in views {
        let owner = view.owner();
        // This panel lists other houses' timers only; the local player reads
        // its own on the sidebar.
        if local == Some(owner) {
            continue;
        }
        let Some(&index) = index_of.get(&owner) else {
            continue;
        };
        let team = TeamKey::from_launch_team(team_of(owner));
        let entry = rows
            .entry(RowKey {
                team,
                sw_type: view.sw_type(),
            })
            .or_insert((0, 0, i32::MAX, index));
        entry.3 = entry.3.min(index);
        entry.1 += 1;
        if view.is_ready() {
            entry.0 += 1;
        } else {
            let seconds = (view.frames_left() / FRAMES_PER_SECOND).max(0);
            entry.2 = entry.2.min(seconds);
        }
    }

    rows.into_iter()
        .map(|(key, (ready, total, nearest, holder))| TeamSuperRow {
            // The team number the label and the player count are keyed by.
            players: players_of(match key.team {
                TeamKey::Named(team) => Some(team),
                TeamKey::Unteamed => None,
            }),
            team: key.team,
            representative: houses[holder],
            sw_type: key.sw_type,
            ready,
            total,
            nearest_seconds: if nearest == i32::MAX { 0 } else { nearest },
        })
        .collect()
}

#[cfg(all(test, tests_app_presentation))]
#[path = "super_timer_groups_tests.rs"]
mod tests;
