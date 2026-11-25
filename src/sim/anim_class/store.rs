//! `AnimStore`, the id-keyed animation storage with its touch logs, owner
//! counts and local-anim queue, and the effect-anim presentation choice.

use crate::sim::world::Simulation;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{AnimId, AnimObject, AnimRemap};
use crate::sim::intern::InternedId;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::util::lookup_hash::LookupMap;

/// Animation objects by id. Every `&mut AnimObject` goes through the store,
/// which notes it in a touch log for each [`AnimTouchReader`].
pub struct AnimStore {
    pub(super) anims: BTreeMap<AnimId, AnimObject>,
    /// VERA-only: how many stored anims name each owner at `+0xCC`
    /// (`owner_entity`), so [`Self::another_owned_by`] answers the Anim
    /// array scans without walking it. The anims' owners are the truth:
    /// [`Self::insert`] and [`Self::remove`] keep it, and
    /// [`Self::set_owner_entity`] is the owners' one writer. Rebuilt on load,
    /// never saved, compared or hashed; only looked up.
    owner_counts: LookupMap<u64, u32>,
    /// Which anims may have changed since each reader last took its log
    /// ([`Self::take_touched`]). Transient: never saved, compared or hashed.
    touched: TouchLogs<{ AnimTouchReader::COUNT }>,
    /// Which anims may draw differently since the presentation last took the
    /// log ([`Self::take_looks`]): stored, or given a new colour remap or, by
    /// [`Self::note_look`], a new type. The simulation never reads it; never
    /// saved, compared or hashed.
    looks: TouchLogs<1>,
    /// The local anims, which only this game runs and draws: a LAN or online
    /// MoveFlash, which `FootClass::ClickedAction_Cell` moves out of
    /// AnimClass::Array (`0x00A8E9A8`) into its own list (`0x00A83E00`,
    /// `0x004D7FD7..0x004D8062`). They take ids from their own range
    /// ([`is_local_anim_id`]). The by-id reads and writes find them; the
    /// Anim array's readers ([`Self::iter`], [`Self::len`], [`Self::key_at`],
    /// [`Self::values_mut`]), comparison, saves and the world hash leave them
    /// out. They have no owner: the owner writers refuse them.
    local: BTreeMap<AnimId, AnimObject>,
    /// The id the next local anim takes.
    next_local: AnimId,
    /// Local anims Destroy has retired, which the end-of-frame drain removes
    /// ([`Self::drain_local_deletes`]): the shared pending-delete queue is
    /// hashed, so they stay out of it.
    local_deletes: Vec<AnimId>,
}

/// How a world makes the effect anims (muzzle flashes, impacts, trailers)
/// that change nothing but their own picture: in native order, as a new world
/// does, or as presentation anims ([`AnimConstructorInput::Presentation`])
/// once [`Simulation::present_effect_anims`] has asked. The choice decides
/// the unique ids later objects take, so a save carries it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EffectAnims {
    presentation: bool,
}

/// The first local anim id ([`AnimStore`]'s `local`). Shared ids count up
/// from 1 ([`crate::sim::world::ObjectSubstrate::allocate_stable_id`]) and
/// never reach it, and it sits below the sound-handle owner tags
/// (`1 << 57` to `1 << 62`), so a local anim's sound owner stays its own.
const LOCAL_ANIM_ID_BASE: AnimId = 1 << 56;

/// Whether `id` names a local anim, one only this game runs and draws.
pub(crate) fn is_local_anim_id(id: u64) -> bool {
    id >= LOCAL_ANIM_ID_BASE
}

/// A product derived from the anims that re-derives from the touch log.
/// Each reads its own log, so one reader's take leaves the others' intact.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AnimTouchReader {
    /// The pointer-expiry broadcast's listener index.
    ExpiryListeners = 0,
}

impl AnimTouchReader {
    const COUNT: usize = 1;
}

impl AnimStore {
    pub fn get(&self, id: AnimId) -> Option<&AnimObject> {
        if is_local_anim_id(id) {
            return self.local.get(&id);
        }
        self.anims.get(&id)
    }

    pub fn get_mut(&mut self, id: AnimId) -> Option<&mut AnimObject> {
        let stored = self.anims.len();
        let anim = if is_local_anim_id(id) {
            self.local.get_mut(&id)?
        } else {
            self.anims.get_mut(&id)?
        };
        self.touched.note(id, stored);
        Some(anim)
    }

    /// A house's colour conversion as the anim's remap (`+0xD4`), frozen at
    /// the write; later disguise or owner changes must not recolor the anim.
    /// Unit deploy739C1A/739DFE store the one Techno705D70 returns (its
    /// custom-palette arm is unported there: no retail deploying Unit sets
    /// art `Palette=`); InfantryClass::ReceiveDamage's InfDeath 9 arm
    /// the house's colour scheme's (`0x00518AB5..0x00518AC9`,
    /// `0x00518AE4..0x00518AF9`).
    pub fn set_anim_house_remap(&mut self, id: AnimId, house: InternedId) -> bool {
        self.set_anim_remap(id, AnimRemap::House(house))
    }

    /// Assign the producer's frozen Convert identity (`Anim+0xD4`). Custom
    /// groups also retain their resource name, independently of any owner.
    pub fn set_anim_remap(&mut self, id: AnimId, remap: AnimRemap) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        anim.remap = Some(remap);
        self.looks.note(id, self.anims.len());
        true
    }

    /// Drop the anim's Convert (`Anim+0xD4 = 0`): Ares
    /// InfantryClass_ReceiveDamage_InfantryVirus2 (10025F50, hook 0x51887B).
    pub(crate) fn clear_anim_remap(&mut self, id: AnimId) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        anim.remap = None;
        true
    }

    pub(crate) fn insert(&mut self, object: AnimObject) -> Option<AnimObject> {
        self.touched.note(object.stable_id, self.anims.len());
        self.looks.note(object.stable_id, self.anims.len());
        if is_local_anim_id(object.stable_id) {
            debug_assert!(object.owner_entity.is_none());
            return self.local.insert(object.stable_id, object);
        }
        count_owner(&mut self.owner_counts, object.owner_entity);
        let replaced = self.anims.insert(object.stable_id, object);
        if let Some(replaced) = &replaced {
            uncount_owner(&mut self.owner_counts, replaced.owner_entity);
        }
        replaced
    }

    pub(crate) fn remove(&mut self, id: AnimId) -> Option<AnimObject> {
        if is_local_anim_id(id) {
            let removed = self.local.remove(&id);
            if removed.is_some() {
                self.touched.note(id, self.anims.len());
            }
            return removed;
        }
        let removed = self.anims.remove(&id);
        if let Some(removed) = &removed {
            uncount_owner(&mut self.owner_counts, removed.owner_entity);
            self.touched.note(id, self.anims.len());
        }
        removed
    }

    /// Write `id`'s owner (`+0xCC`), returning the one it replaced, or `None`
    /// when `id` is not stored. The owners' one writer.
    pub(crate) fn set_owner_entity(
        &mut self,
        id: AnimId,
        owner: Option<u64>,
    ) -> Option<Option<u64>> {
        // A presentation anim follows its owner too. No Anim array scan
        // counts it ([`Self::another_owned_by`]): it raises no owner flag.
        if is_local_anim_id(id) {
            let anim = self.local.get_mut(&id)?;
            debug_assert!(anim.presentation.is_some() || owner.is_none());
            return Some(std::mem::replace(&mut anim.owner_entity, owner));
        }
        let stored = self.anims.len();
        let anim = self.anims.get_mut(&id)?;
        self.touched.note(id, stored);
        let replaced = std::mem::replace(&mut anim.owner_entity, owner);
        uncount_owner(&mut self.owner_counts, replaced);
        count_owner(&mut self.owner_counts, owner);
        Some(replaced)
    }

    /// Write `id`'s terrain owner (`+0xCC` naming a TerrainClass), returning
    /// the one it replaced, or `None` when `id` is not stored.
    pub(crate) fn set_owner_terrain(
        &mut self,
        id: AnimId,
        owner: Option<u64>,
    ) -> Option<Option<u64>> {
        let stored = self.anims.len();
        let anim = self.anims.get_mut(&id)?;
        self.touched.note(id, stored);
        Some(std::mem::replace(&mut anim.owner_terrain, owner))
    }

    /// Whether a stored anim other than `id` names the terrain object
    /// `owner`: the Anim array scan of SetOwnerObject424B79..424BB6 for a
    /// terrain owner. Only burning trees own anims, so it walks the store.
    pub(crate) fn another_owned_by_terrain(&self, id: AnimId, owner: u64) -> bool {
        self.anims
            .iter()
            .any(|(&other, anim)| other != id && anim.owner_terrain == Some(owner))
    }

    /// Ares terrain fire: the anims a terrain object owns, in array order.
    pub(crate) fn owned_by_terrain(&self, owner: u64) -> Vec<AnimId> {
        self.anims
            .iter()
            .filter(|(_, anim)| anim.owner_terrain == Some(owner))
            .map(|(&id, _)| id)
            .collect()
    }

    /// `AnimClass::ClearLoopsForOwner @ 0x00422B80`: every anim naming
    /// `owner` at `+0xCC` ends after its current loop (`+0x195 = 0`).
    pub(crate) fn clear_loops_for_terrain_owner(&mut self, owner: u64) {
        let stored = self.anims.len();
        for (&id, anim) in self.anims.iter_mut() {
            if anim.owner_terrain == Some(owner) {
                anim.end_loops();
                self.touched.note(id, stored);
            }
        }
    }

    /// The terrain object an anim is attached to (`+0xCC`).
    pub(crate) fn owner_terrain(&self, id: AnimId) -> Option<u64> {
        self.anims.get(&id).and_then(|anim| anim.owner_terrain)
    }

    /// Install a fixture's owner (`+0xCC`) on a stored anim.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owner_entity_for_test(&mut self, id: AnimId, owner: Option<u64>) {
        self.set_owner_entity(id, owner)
            .expect("the fixture's anim is stored");
    }

    /// Whether a stored anim other than `id` names `owner` at `+0xCC`: the
    /// Anim array scan of SetOwnerObject424B79..424BB6 and the destructor
    ///42291A..42295B, which counts every retained entry (inactive,
    /// concealed and pending-delete ones too), answered from the owners'
    /// counts. Debug builds check it against that scan (the live-read check).
    pub(crate) fn another_owned_by(&self, id: AnimId, owner: u64) -> bool {
        let own = u32::from(self.get(id).and_then(|anim| anim.owner_entity) == Some(owner));
        let another = self
            .owner_counts
            .get(&owner)
            .is_some_and(|&count| count > own);
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || another
                    == self
                        .anims
                        .iter()
                        .any(|(&other, anim)| other != id && anim.owner_entity == Some(owner)),
            "the Anim owner counts disagree with the Anim array for owner {owner}"
        );
        another
    }

    /// Store the independently owned attached-Bullet pointer (+0x17C).
    pub(super) fn set_attached_bullet(&mut self, id: AnimId, bullet: Option<u64>) -> bool {
        let Some(anim) = self.get_mut(id) else {
            return false;
        };
        anim.attached_bullet = bullet;
        true
    }

    fn from_anims(anims: BTreeMap<AnimId, AnimObject>) -> Self {
        let mut owner_counts = LookupMap::default();
        for anim in anims.values() {
            count_owner(&mut owner_counts, anim.owner_entity);
        }
        Self {
            anims,
            owner_counts,
            touched: TouchLogs::everything(),
            looks: TouchLogs::everything(),
            local: BTreeMap::new(),
            next_local: LOCAL_ANIM_ID_BASE,
            local_deletes: Vec::new(),
        }
    }

    /// The id for a new local anim.
    pub(crate) fn allocate_local_id(&mut self) -> AnimId {
        let id = self.next_local;
        self.next_local += 1;
        id
    }

    /// The local anims, in the order they were made (the local list's).
    #[cfg(test)]
    pub(crate) fn local_ids(&self) -> Vec<AnimId> {
        self.local.keys().copied().collect()
    }

    /// The id the next local anim will take: a walk that stops before it
    /// leaves out every local anim made while it runs.
    pub(crate) fn local_id_end(&self) -> AnimId {
        self.next_local
    }

    /// The first local anim after `after` (from the first with `None`) and
    /// before `end`, in the local list's order: a cursor over the list that
    /// skips an anim removed since the walk began. Local ids are never given
    /// out twice, so the walk meets the anims a copy of the list would hold.
    pub(crate) fn next_local_id(&self, after: Option<AnimId>, end: AnimId) -> Option<AnimId> {
        let from = after.map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded);
        self.local
            .range((from, std::ops::Bound::Excluded(end)))
            .next()
            .map(|(&id, _)| id)
    }

    /// The local anims and their ids, in the local list's order.
    pub(crate) fn local_iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.local.iter()
    }

    /// Take presentation anim `id` out of its layer, as Display's remove
    /// takes an ordinary anim out of its vector.
    pub(crate) fn conceal_presentation(&mut self, id: AnimId) {
        if let Some(anim) = self.local.get_mut(&id)
            && anim.presentation.is_some()
        {
            anim.presentation = Some(None);
        }
    }

    /// The presentation anims, in the order they were made.
    pub(crate) fn presentation_iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.local.iter().filter(|(_, anim)| anim.is_presentation())
    }

    /// Retire the local anim `id` for the end-of-frame drain.
    pub(crate) fn queue_local_delete(&mut self, id: AnimId) {
        if !self.local_deletes.contains(&id) {
            self.local_deletes.push(id);
        }
    }

    /// Whether Destroy has retired the local anim `id`.
    pub(crate) fn local_delete_queued(&self, id: AnimId) -> bool {
        self.local_deletes.contains(&id)
    }

    /// Remove the retired local anims. Their Destroy already released the
    /// sound and left Display; the destructor's `0x00422A69` takes each out
    /// of the local list.
    pub(crate) fn drain_local_deletes(&mut self) -> Vec<AnimId> {
        let drained = std::mem::take(&mut self.local_deletes);
        for &id in &drained {
            self.remove(id);
        }
        drained
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AnimId, &AnimObject)> {
        self.anims.iter()
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut AnimObject> {
        self.touched.note_all();
        self.looks.note_all();
        self.anims.values_mut()
    }

    pub fn contains_key(&self, id: AnimId) -> bool {
        if is_local_anim_id(id) {
            return self.local.contains_key(&id);
        }
        self.anims.contains_key(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.anims.len()
    }

    pub(crate) fn key_at(&self, index: usize) -> Option<AnimId> {
        self.anims.keys().nth(index).copied()
    }

    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: AnimTouchReader) -> Touched {
        self.touched.take(reader as usize)
    }

    /// Take the look log, leaving it empty.
    pub(crate) fn take_looks(&mut self) -> Touched {
        self.looks.take(0)
    }

    /// Note that an in-place write gave `id` a new type.
    pub(crate) fn note_look(&mut self, id: AnimId) {
        self.looks.note(id, self.anims.len());
    }
}

impl Default for AnimStore {
    fn default() -> Self {
        Self::from_anims(BTreeMap::new())
    }
}

impl Clone for AnimStore {
    /// A clone starts with an everything-touched log: whatever was derived
    /// from the original says nothing certain about the copy's future. The
    /// local anims come along, so the presentation copy draws them.
    fn clone(&self) -> Self {
        Self {
            local: self.local.clone(),
            next_local: self.next_local,
            local_deletes: self.local_deletes.clone(),
            ..Self::from_anims(self.anims.clone())
        }
    }
}

/// The local anims are left out, as they are from saves and the hash.
impl PartialEq for AnimStore {
    fn eq(&self, other: &Self) -> bool {
        self.anims == other.anims
    }
}

impl Eq for AnimStore {}

impl std::fmt::Debug for AnimStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AnimStore").field(&self.anims).finish()
    }
}

/// The saved form is the former newtype `AnimStore(map)`; the log and the
/// local anims are not saved.
impl Serialize for AnimStore {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("AnimStore", &self.anims)
    }
}

impl<'de> Deserialize<'de> for AnimStore {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename = "AnimStore")]
        struct Saved(BTreeMap<AnimId, AnimObject>);
        let Saved(anims) = Saved::deserialize(deserializer)?;
        Ok(Self::from_anims(anims))
    }
}

/// Count one more stored anim naming `owner`.
fn count_owner(counts: &mut LookupMap<u64, u32>, owner: Option<u64>) {
    if let Some(owner) = owner {
        *counts.entry(owner).or_default() += 1;
    }
}

/// Count one fewer stored anim naming `owner`, forgetting an owner none names.
fn uncount_owner(counts: &mut LookupMap<u64, u32>, owner: Option<u64>) {
    if let Some(owner) = owner
        && let std::collections::hash_map::Entry::Occupied(mut count) = counts.entry(owner)
    {
        *count.get_mut() -= 1;
        if *count.get() == 0 {
            count.remove();
        }
    }
}

impl Simulation {
    /// Run the effect anims that change nothing but their picture as
    /// presentation anims from now on ([`AnimConstructorInput::Presentation`]).
    /// Simulations start without, in native order, so a comparison with
    /// gamemd's unique ids, Logic order and anim rows holds; real matches,
    /// replays of them and sim-bench turn it on.
    pub fn present_effect_anims(&mut self) {
        self.effect_anims.presentation = true;
    }

    /// Whether picture-only effect anims run as presentation anims
    /// ([`Self::present_effect_anims`]).
    pub(crate) fn effect_anims_presented(&self) -> bool {
        self.effect_anims.presentation
    }
}
