//! Per-team aggregation behind the in-game superweapon timer panel.
//!
//! This is a VERA presentation extension, on with `[VERA] GroupedSuperTimers=`
//! (Options), not a `TacticalClass::Draw` parity path: it replaces the native
//! one-line-per-Super lines with one line per (team, superweapon type). Off,
//! the native timer list draws as before.
//!
//! The runtime has no launch-team number (`LaunchTeam::Team(u8)` lives only in
//! the startup session). Teams are therefore the connected components of the
//! live alliance graph, joined only where two houses ally each other
//! (`HouseClass::IsAlliedWith @ 0x004F9A50` is one-way); an isolated house is
//! the "-" (no-team) group. Labels A/B/C/... are assigned in a
//! stable order from `houses().order()`. The local house's own Supers are
//! counted into its named team; if local has no named team (it is alone or
//! unknown) its Supers are hidden. Only Supers of the timer list
//! (`super_timer_views`, after any Ares `SW.TimerVisibility` filter) reach
//! here.
//!
//! Every isolated house shares the single "-" row key, so several unteamed
//! rivals are aggregated together even though they are not allies with each
//! other — this is the panel's one "no-team" bucket, and its counts mean
//! "how many unteamed houses have this Super", not a coordinated side.

use std::collections::{BTreeMap, HashMap};

use crate::sim::intern::InternedId;
use crate::sim::superweapon::SuperTimerView;

/// Native `TacticalClass::DrawTimer` frames-per-second divisor
/// (`0x88888889`, `sar 3`), shared with `super_timers`.
const FRAMES_PER_SECOND: i32 = 15;

/// A display team. `Named(0)` prints "A", `Named(1)` "B", and so on;
/// `Unteamed` prints "-". Deriving `Ord` keeps unteamed rows last, matching
/// the "-, and yourself" ordering in the panel design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TeamKey {
    Named(u8),
    Unteamed,
}

impl TeamKey {
    /// The panel's short label: A..Z (then `T<n>` beyond 26 teams), or "-".
    pub(crate) fn label(self) -> String {
        match self {
            Self::Named(index) if index < 26 => ((b'A' + index) as char).to_string(),
            Self::Named(index) => format!("T{index}"),
            Self::Unteamed => "-".to_string(),
        }
    }
}

/// One aggregated `(team, superweapon type)` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeamSuperRow {
    pub(crate) team: TeamKey,
    /// A member house used for the row's colour and blink identity.
    pub(crate) representative: InternedId,
    pub(crate) sw_type: InternedId,
    /// Houses in this team whose Super of this type is charged (`+0x6F`).
    pub(crate) ready: usize,
    /// Houses in this team that hold this type at all.
    pub(crate) total: usize,
    /// Shortest whole seconds left among the not-yet-ready houses; `0` when
    /// every holder is ready (the panel then prints "0:00").
    pub(crate) nearest_seconds: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    team: TeamKey,
    sw_type: InternedId,
}

/// Group `views` into [`TeamSuperRow`]s.
///
/// `houses` is `sim.houses().order()` (stable, native registration order).
/// `allied(asker, other)` is the one-way alliance test; two houses join a
/// team only when each allies the other. `local`'s own Supers are counted
/// into its named team; when it has none they are omitted (the player reads
/// those on the sidebar).
pub(crate) fn grouped_super_rows(
    houses: &[InternedId],
    allied: impl Fn(InternedId, InternedId) -> bool,
    views: &[SuperTimerView],
    local: Option<InternedId>,
) -> Vec<TeamSuperRow> {
    let count = houses.len();
    let mut index_of: HashMap<InternedId, usize> = HashMap::with_capacity(count);
    for (index, &house) in houses.iter().enumerate() {
        index_of.insert(house, index);
    }

    let mut parent: Vec<usize> = (0..count).collect();
    for left in 0..count {
        for right in left + 1..count {
            let (a, b) = (houses[left], houses[right]);
            if allied(a, b) && allied(b, a) {
                union(&mut parent, left, right);
            }
        }
    }

    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..count {
        members
            .entry(find(&mut parent, index))
            .or_default()
            .push(index);
    }

    let mut teams: Vec<(usize, TeamKey, InternedId)> = Vec::new();
    let mut multi: Vec<_> = members
        .iter()
        .filter(|(_, members)| members.len() >= 2)
        .collect();
    multi.sort_by_key(|(_, members)| members[0]);
    for (root, members) in multi {
        teams.push((*root, TeamKey::Named(teams.len() as u8), houses[members[0]]));
    }
    for (root, members) in &members {
        if members.len() == 1 {
            teams.push((*root, TeamKey::Unteamed, houses[members[0]]));
        }
    }
    let teams: HashMap<usize, (TeamKey, InternedId)> = teams
        .into_iter()
        .map(|(root, key, representative)| (root, (key, representative)))
        .collect();

    // The local house's Supers are counted into its own named team (the
    // player reads them on the sidebar too, but they belong to that team's
    // tally). If local has no named team — it is alone ("-") or unknown — its
    // Supers stay hidden, matching the original "your own on the sidebar"
    // intent. Resolve the team once here: `find` compresses paths, so it is
    // safe to call again per view below.
    let local_team = local
        .and_then(|local| index_of.get(&local).copied())
        .map(|index| find(&mut parent, index))
        .and_then(|root| teams.get(&root).map(|&(team, _)| team));

    // (ready, total, nearest_seconds, representative), with i32::MAX as "no
    // pending holder". Every one-house team shares the "-" key, so a row's
    // representative is that of its first counted holder in house order, never
    // one picked from a map whose order changes from call to call.
    let mut rows: BTreeMap<RowKey, (usize, usize, i32, (usize, InternedId))> = BTreeMap::new();
    for view in views {
        let owner = view.owner();
        // Hide own Supers only when local has no named team to count them in.
        if local == Some(owner) && !matches!(local_team, Some(TeamKey::Named(_))) {
            continue;
        }
        let Some(&index) = index_of.get(&owner) else {
            continue;
        };
        let Some(&(team, representative)) = teams.get(&find(&mut parent, index)) else {
            continue;
        };
        let entry = rows
            .entry(RowKey {
                team,
                sw_type: view.sw_type(),
            })
            .or_insert((0, 0, i32::MAX, (index, representative)));
        entry.3 = entry.3.min((index, representative));
        entry.1 += 1;
        if view.is_ready() {
            entry.0 += 1;
        } else {
            let seconds = (view.frames_left() / FRAMES_PER_SECOND).max(0);
            entry.2 = entry.2.min(seconds);
        }
    }

    rows.into_iter()
        .map(
            |(key, (ready, total, nearest, (_, representative)))| TeamSuperRow {
                team: key.team,
                representative,
                sw_type: key.sw_type,
                ready,
                total,
                nearest_seconds: if nearest == i32::MAX { 0 } else { nearest },
            },
        )
        .collect()
}

fn find(parent: &mut [usize], mut node: usize) -> usize {
    let mut root = node;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[node] != node {
        let next = parent[node];
        parent[node] = root;
        node = next;
    }
    root
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let (left, right) = (find(parent, left), find(parent, right));
    if left != right {
        parent[right] = left;
    }
}

#[cfg(all(test, tests_app_presentation))]
#[path = "super_timer_groups_tests.rs"]
mod tests;
