//! The live teams (`TeamClass::Array`), found by id without a search, with
//! an index of the objects they name.
//!
//! `TeamClass::PointerExpired @ 0x006EAE60` reaches every team at every
//! object's expiry, and almost every expiring object is one no team names.
//! VERA-only, [`TeamArray::naming`] answers which teams name an object
//! without walking them.

use std::collections::BTreeMap;
use std::collections::hash_map::Entry;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use super::{TeamScriptState, TeamTarget};
use crate::util::lookup_hash::LookupMap;

/// Live teams in id order. Ids rise with construction, so that is
/// TeamClass::Array order. Saved as the map of teams by id.
///
/// VERA-only: a team's update asks for it by id some twenty times, so the
/// teams lie in one array with each one's slot by id beside it (`slot_of`,
/// only looked up). A removed team leaves a gap, which a walk passes over,
/// until the gaps outnumber the teams and the array closes them.
///
/// VERA-only as well: the objects each team names ([`Names`]) and the teams
/// that name each object. The teams are the source of truth. Every way to a
/// team that can change one ([`Self::get_mut`], [`Self::values_mut`],
/// [`Self::insert`]) marks it unread, and [`Self::naming`] reads the unread
/// teams again before it answers. So a caller that only reads a team asks
/// [`Self::get`]: a team handed out for writing on every object's step would
/// be read again at every expiry.
///
/// Neither index is saved or hashed; a load builds the slots and starts
/// with every team unread. Debug builds under the live-read check compare
/// each slot with its id and each [`Self::naming`] answer with a walk of the
/// teams.
#[derive(Debug, Clone, Default)]
pub(super) struct TeamArray {
    slots: Vec<Slot>,
    slot_of: LookupMap<u64, u32>,
    named: NamedObjects,
}

/// One place in the array: a team, or the gap a removed one left.
#[derive(Debug, Clone)]
struct Slot {
    id: u64,
    team: Option<TeamScriptState>,
}

/// The gaps the array keeps beyond as many as it has teams before it closes
/// them.
const SPARE_GAPS: usize = 16;

/// The objects one team names, as `PointerExpired` tests them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Names {
    /// The head of its member list (`+0x54`), which only an expiry with
    /// `all` drops.
    head: Option<u64>,
    /// Its centre (`+0x34`), mission target (`+0x3C`) and move target
    /// (`+0x40`) where they are objects, and its closest member (`+0x38`).
    kept: [Option<u64>; 4],
}

impl Names {
    fn of(team: &TeamScriptState) -> Self {
        let object = |target| match target {
            Some(TeamTarget::Object(id)) => Some(id),
            Some(TeamTarget::Cell { .. }) | None => None,
        };
        Self {
            head: team.members.first().map(|member| member.id),
            kept: [
                object(team.zone),
                object(team.mission_target),
                object(team.focus),
                team.closest_member,
            ],
        }
    }

    /// Whether `PointerExpired` of `expired` changes the team.
    fn holds(&self, expired: u64, all: bool) -> bool {
        (all && self.head == Some(expired)) || self.kept.contains(&Some(expired))
    }

    /// Whether the team names `id` at all.
    fn names(&self, id: u64) -> bool {
        self.head == Some(id) || self.kept.contains(&Some(id))
    }

    /// The objects named, each once.
    fn objects(&self) -> impl Iterator<Item = u64> {
        let [zone, mission_target, focus, closest] = self.kept;
        let slots = [self.head, zone, mission_target, focus, closest];
        (0..slots.len())
            .filter_map(move |slot| slots[slot].filter(|id| !slots[..slot].contains(&Some(*id))))
    }
}

/// How many of the teams that name one object [`NamedBy`] lists: a dying
/// unit is named by its own team and the few that attack it.
const LISTED_TEAMS: usize = 3;

/// The teams that name one object: the first [`LISTED_TEAMS`] to do so, and
/// how many more, which only a search of every team's names finds.
#[derive(Debug, Clone, Copy, Default)]
struct NamedBy {
    teams: [u64; LISTED_TEAMS],
    listed: u8,
    unlisted: u32,
}

impl NamedBy {
    fn add(&mut self, team: u64) {
        match self.teams.get_mut(usize::from(self.listed)) {
            Some(place) => {
                *place = team;
                self.listed += 1;
            }
            None => self.unlisted += 1,
        }
    }

    /// Take `team` out; true when no team is left.
    fn remove(&mut self, team: u64) -> bool {
        let listed = usize::from(self.listed);
        match self.teams[..listed].iter().position(|&id| id == team) {
            Some(place) => {
                self.teams[place] = self.teams[listed - 1];
                self.listed -= 1;
            }
            None => self.unlisted = self.unlisted.saturating_sub(1),
        }
        self.listed == 0 && self.unlisted == 0
    }
}

/// What the teams named when each was last read.
#[derive(Debug, Clone, Default)]
struct NamedObjects {
    /// The teams that name each object, by object.
    by_object: LookupMap<u64, NamedBy>,
    /// What each team names. Only looked up, and, for an object with
    /// unlisted teams, searched for those that name it, which are then
    /// sorted.
    by_team: LookupMap<u64, Names>,
    /// The teams marked unread since they were read, some more than once.
    unread: Vec<u64>,
    /// Every team is unread: after a load, a walk of all for writing, or
    /// more marks than there are teams.
    all_unread: bool,
}

impl NamedObjects {
    /// Mark team `id` of `teams` unread.
    fn mark(&mut self, id: u64, teams: usize) {
        if self.all_unread || self.unread.last() == Some(&id) {
            return;
        }
        if self.unread.len() >= teams {
            self.mark_all();
        } else {
            self.unread.push(id);
        }
    }

    fn mark_all(&mut self) {
        self.all_unread = true;
        self.unread.clear();
    }

    /// Team `team` now names `now` (nothing once it is gone).
    fn read(&mut self, team: u64, now: Option<Names>) {
        let before = match now {
            Some(names) => self.by_team.insert(team, names),
            None => self.by_team.remove(&team),
        };
        if before == now {
            return;
        }
        for id in before.iter().flat_map(Names::objects) {
            if now.is_some_and(|now| now.names(id)) {
                continue;
            }
            if let Entry::Occupied(mut by) = self.by_object.entry(id)
                && by.get_mut().remove(team)
            {
                by.remove();
            }
        }
        for id in now.iter().flat_map(Names::objects) {
            if !before.is_some_and(|before| before.names(id)) {
                self.by_object.entry(id).or_default().add(team);
            }
        }
    }
}

/// Team `id` of `slots`, through its slot.
fn team_at<'a>(
    slots: &'a [Slot],
    slot_of: &LookupMap<u64, u32>,
    id: &u64,
) -> Option<&'a TeamScriptState> {
    let slot = slots.get(*slot_of.get(id)? as usize)?;
    debug_assert!(
        !crate::sim::touch_log::live_read_check_enabled() || slot.id == *id,
        "team {id}'s slot holds team {}",
        slot.id
    );
    slot.team.as_ref()
}

impl TeamArray {
    pub(super) fn get(&self, id: &u64) -> Option<&TeamScriptState> {
        team_at(&self.slots, &self.slot_of, id)
    }

    pub(super) fn get_mut(&mut self, id: &u64) -> Option<&mut TeamScriptState> {
        let slot = self.slots.get_mut(*self.slot_of.get(id)? as usize)?;
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || slot.id == *id,
            "team {id}'s slot holds team {}",
            slot.id
        );
        let team = slot.team.as_mut()?;
        self.named.mark(*id, self.slot_of.len());
        Some(team)
    }

    pub(super) fn contains_key(&self, id: &u64) -> bool {
        self.slot_of.contains_key(id)
    }

    /// Add team `id`, or replace it. Ids rise with construction, so a new
    /// team joins the end; any other id takes its place in id order.
    pub(super) fn insert(&mut self, id: u64, team: TeamScriptState) -> Option<TeamScriptState> {
        let replaced = match self.slot_of.get(&id) {
            Some(&slot) => self
                .slots
                .get_mut(slot as usize)
                .and_then(|slot| slot.team.replace(team)),
            None => {
                let place = match self.slots.last() {
                    Some(last) if last.id >= id => self.slots.partition_point(|slot| slot.id < id),
                    _ => self.slots.len(),
                };
                match self.slots.get_mut(place) {
                    // The gap this id left.
                    Some(gap) if gap.id == id => gap.team = Some(team),
                    _ => {
                        let team = Some(team);
                        self.slots.insert(place, Slot { id, team });
                    }
                }
                self.index_from(place);
                None
            }
        };
        self.named.mark(id, self.slot_of.len());
        replaced
    }

    /// Remove team `id`, which names nothing from here on.
    pub(super) fn remove(&mut self, id: &u64) -> Option<TeamScriptState> {
        let slot = self.slot_of.remove(id)? as usize;
        let team = self.slots.get_mut(slot)?.team.take();
        self.named.read(*id, None);
        if self.slots.len() > 2 * self.slot_of.len() + SPARE_GAPS {
            self.slots.retain(|slot| slot.team.is_some());
            self.slot_of.clear();
            self.index_from(0);
        }
        team
    }

    /// Note the slot of every team from slot `first` on.
    fn index_from(&mut self, first: usize) {
        for (slot, held) in self.slots.iter().enumerate().skip(first) {
            if held.team.is_some() {
                self.slot_of.insert(held.id, slot as u32);
            }
        }
    }

    pub(super) fn len(&self) -> usize {
        self.slot_of.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.slot_of.is_empty()
    }

    /// The teams with their ids, in id order.
    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = (&u64, &TeamScriptState)> {
        self.slots
            .iter()
            .filter_map(|slot| Some((&slot.id, slot.team.as_ref()?)))
    }

    pub(super) fn keys(&self) -> impl DoubleEndedIterator<Item = &u64> {
        self.iter().map(|(id, _)| id)
    }

    pub(super) fn values(&self) -> impl DoubleEndedIterator<Item = &TeamScriptState> {
        self.iter().map(|(_, team)| team)
    }

    pub(super) fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut TeamScriptState> {
        self.named.mark_all();
        self.slots.iter_mut().filter_map(|slot| slot.team.as_mut())
    }

    /// The teams `PointerExpired` of `expired` changes, in team order: those
    /// that keep it as their centre, closest member, mission target or move
    /// target, and with `all` those whose list it heads.
    pub(super) fn naming(&mut self, expired: u64, all: bool) -> Vec<u64> {
        self.read_unread();
        let named = &self.named;
        let mut naming = Vec::new();
        if let Some(by) = named.by_object.get(&expired) {
            if by.unlisted == 0 {
                let listed = by.teams[..usize::from(by.listed)].iter().copied();
                naming.extend(listed.filter(|team| {
                    named
                        .by_team
                        .get(team)
                        .is_some_and(|names| names.holds(expired, all))
                }));
            } else {
                naming.extend(
                    named
                        .by_team
                        .iter()
                        .filter(|(_, names)| names.holds(expired, all))
                        .map(|(&team, _)| team),
                );
            }
            naming.sort_unstable();
        }
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || naming.iter().copied().eq(self
                    .iter()
                    .filter(|(_, team)| TeamScriptState::clone(team).forget(expired, all).0)
                    .map(|(&id, _)| id)),
            "the teams naming {expired} are not the {naming:?} their index lists"
        );
        naming
    }

    /// Bring the index of named objects up to the teams marked unread.
    fn read_unread(&mut self) {
        let named = &mut self.named;
        if named.all_unread {
            named.all_unread = false;
            for slot in &self.slots {
                if let Some(team) = &slot.team {
                    named.read(slot.id, Some(Names::of(team)));
                }
            }
            return;
        }
        let mut unread = std::mem::take(&mut named.unread);
        for id in unread.drain(..) {
            let team = team_at(&self.slots, &self.slot_of, &id);
            named.read(id, team.map(Names::of));
        }
        // The emptied list keeps its allocation.
        named.unread = unread;
    }
}

/// A test's read of a team it knows to be live.
#[cfg(test)]
impl std::ops::Index<&u64> for TeamArray {
    type Output = TeamScriptState;

    fn index(&self, id: &u64) -> &TeamScriptState {
        self.get(id).expect("the team is live")
    }
}

impl Serialize for TeamArray {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (id, team) in self.iter() {
            map.serialize_entry(id, team)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for TeamArray {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let teams = BTreeMap::<u64, TeamScriptState>::deserialize(deserializer)?;
        let mut array = Self::default();
        array.slots.extend(teams.into_iter().map(|(id, team)| {
            let team = Some(team);
            Slot { id, team }
        }));
        array.index_from(0);
        array.named.mark_all();
        Ok(array)
    }
}

#[cfg(all(test, tests_sim_t_z))]
mod tests {
    use super::*;
    use crate::sim::intern::InternedId;

    fn team(id: u64) -> TeamScriptState {
        let name = InternedId::from_index(1);
        TeamScriptState::new(id, name, None, None, name, 0)
    }

    fn ids(array: &TeamArray) -> Vec<u64> {
        array.keys().copied().collect()
    }

    /// The array answers as the map of teams by id it is saved as: in id
    /// order whatever order the ids came in, through removals, an id's
    /// return to its gap, the closing of the gaps, and a save.
    #[test]
    fn the_array_answers_as_the_map_it_is_saved_as() {
        let mut array = TeamArray::default();
        let mut map = BTreeMap::new();
        for id in [3, 5, 9, 4, 1, 12] {
            assert!(array.insert(id, team(id)).is_none());
            map.insert(id, team(id));
        }
        assert_eq!(ids(&array), [1, 3, 4, 5, 9, 12]);
        assert_eq!(array.remove(&4).map(|team| team.id), Some(4));
        assert!(array.remove(&4).is_none());
        assert!(array.get(&4).is_none() && !array.contains_key(&4));
        assert_eq!(ids(&array), [1, 3, 5, 9, 12]);
        array.insert(4, team(4));
        array.get_mut(&4).expect("back in its gap").cursor = 7;
        map.get_mut(&4).expect("the map kept 4").cursor = 7;
        assert_eq!(array.insert(9, team(9)).map(|team| team.id), Some(9));
        assert_eq!(ids(&array), [1, 3, 4, 5, 9, 12]);
        let walked: Vec<u64> = array.values().map(|team| team.id).collect();
        assert_eq!(walked, ids(&array));
        let backwards: Vec<u64> = array.values_mut().rev().map(|team| team.id).collect();
        assert_eq!(backwards, [12, 9, 5, 4, 3, 1]);

        // Enough removals close the gaps; every team is still found.
        for id in 20..60 {
            array.insert(id, team(id));
            map.insert(id, team(id));
        }
        for id in 20..58 {
            array.remove(&id);
            map.remove(&id);
        }
        assert!(array.slots.len() < 20, "{} slots", array.slots.len());
        assert_eq!(array.len(), map.len());
        assert_eq!(ids(&array), map.keys().copied().collect::<Vec<_>>());
        for id in map.keys() {
            assert_eq!(array[id].id, *id);
        }
        assert_eq!(array[&4].cursor, 7);

        let saved = serde_json::to_string(&array).unwrap();
        assert_eq!(saved, serde_json::to_string(&map).unwrap());
        let loaded: TeamArray = serde_json::from_str(&saved).unwrap();
        assert_eq!(ids(&loaded), ids(&array));
        assert_eq!(loaded[&4].cursor, 7);
    }
}
