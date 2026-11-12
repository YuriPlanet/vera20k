//! Dense entity storage with deterministic sorted iteration.
//!
//! `EntityStore` holds every game entity, boxed, in ascending stable_id order
//! in one vector, with an id -> slot index for O(1) lookup. Stable ids are
//! monotonic and never reused, so a new entity normally appends; a removed one
//! leaves an empty slot until the next compaction, which keeps the order.
//! Iteration is always in ascending stable_id order, which lockstep relies on.
//!
//! ## Borrow patterns
//! - Single entity mutation: `store.get_mut(id)` borrows only that entry
//! - Cross-entity reads during mutation: read target first (clone needed data),
//!   then get_mut on the other entity
//! - Batch iteration with mutation: collect `keys_sorted()`, loop with
//!   `get_mut_if()` so an entity the walk leaves unchanged is not handed out
//!   (every hand-out enters the touch logs; see `touched`)
//!
//! ## Dependency rules
//! - Part of sim/ — stores sim/game_entity; the shared AirTracker query reads
//!   existing Cell geometry and bucket kernels, never app/presentation state.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::util::lookup_hash::LookupMap;

/// Only this module can authorize a live indexed-owner write.
/// Payload consumers can read owner identity but cannot construct this capability.
pub(crate) struct OwnerChangeAuthority(());

/// Only this module can write an entity's AircraftTracker membership, so the
/// airborne set ([`EntityStore::airborne`]) changes with it.
pub(crate) struct AirMembershipAuthority(());

/// Only this module can store an Anim in a field [`GameEntity::held_anims`]
/// names, so the anim-holder index notes the holder
/// ([`EntityStore::write_held_anim`]).
pub(crate) struct AnimHolderAuthority(());

/// Container for all game entities, keyed by stable_id.
///
/// All iteration methods return entities in ascending stable_id order, which
/// is critical for lockstep multiplayer.
///
/// A House's buildings in native order are its House+0x68 list
/// (`HouseBaseState::buildings`); its per-type counts are its
/// `HouseTracking`.
#[derive(Debug)]
pub struct EntityStore {
    /// The stored ids in ascending order, one per slot. A removed entity's
    /// slot keeps its id, with no entity, until [`Self::compact`].
    ids: Vec<u64>,
    /// The entity of each slot. Boxed: an entity is about 2.2 KB, so insert,
    /// remove and compaction move pointers, not entities. serde writes a
    /// `Box<T>` as its `T`.
    entities: Vec<Option<Box<GameEntity>>>,
    /// stable_id -> slot, for every slot that holds an entity. Only looked
    /// up, never iterated, so its order cannot reach a result.
    slot_of: LookupMap<u64, u32>,
    /// VERA-only: the id last given to [`Self::focus`] and its slot (`None`:
    /// not stored), so an object's turn, which looks its own id up about a
    /// hundred times, hashes it once ([`Self::slot`]). `slot_of` is the
    /// source of truth: every function that writes it re-reads the focused
    /// id's slot ([`Self::refocus`]), so a lookup answers the same with or
    /// without it. Never saved or hashed.
    focus: (u64, Option<u32>),
    /// Lookups by id so far, and those of them the id index answered (test
    /// builds only), for checks of what a turn looks up.
    #[cfg(any(test, feature = "test-support"))]
    lookups: [std::sync::atomic::AtomicU64; 2],
    /// VERA-only: each slot's owner, beside the boxed entities for walks that
    /// ask only whose an object is ([`Self::owner_of`]). The entity's own
    /// owner is the source of truth: [`Self::insert`] copies it and
    /// [`Self::change_owner`], its one live writer, writes both. A removed
    /// slot keeps a stale owner until [`Self::compact`] drops the slot.
    /// Rebuilt on load, never saved or hashed.
    owners: Vec<InternedId>,
    /// VERA-only: each slot's category, beside the boxed entities for walks
    /// that ask only what class an object is ([`Self::category_of`]). A
    /// stored entity's category never changes (a replacement comes through
    /// [`Self::insert`], which copies it). Kept and rebuilt as `owners` is.
    categories: Vec<EntityCategory>,
    /// VERA-only: each slot's sleep mark and quiet flag ([`SleepCell`]),
    /// beside the boxed entities so the live object pass passes a sleeping
    /// object without loading it, and a quiet one's turn skips steps without
    /// reading their state. Every route to a `&mut GameEntity` settles and
    /// clears both first ([`Self::wake_slot`]), so a sleeping or quiet entity
    /// is one nothing has written since its last turn. Kept through
    /// compaction as `owners` is; a load starts every object awake and not
    /// quiet. Never saved or hashed: the save form and the state hash read a
    /// sleeping object as settled.
    sleep: Vec<SleepCell>,
    /// VERA-only: the stored ids of each owner's objects of each category,
    /// ascending ([`Self::owned_ids`]), for walks that want one house's
    /// objects of one class without loading the others. The entities' owners
    /// and categories are the source of truth: [`Self::insert`] and
    /// [`Self::remove`] keep it, [`Self::change_owner`], the owners' one live
    /// writer, moves an id between lists, and compaction changes no id.
    /// Rebuilt on load ([`Self::rebuild_class_registries`]), never saved or
    /// hashed, and only looked up, never iterated as a map.
    owned: LookupMap<(InternedId, EntityCategory), Vec<u64>>,
    /// Derived InfantryClass registry, in monotonic construction-ID order.
    /// Uninit retains an entry; remove compacts it. Class/category is immutable
    /// while stored, like indexed identity (replace through remove/insert).
    infantry_registry: Vec<u64>,
    /// FootClass array8B3DC4, including Aircraft and stored limbo objects.
    /// Derived from the same monotonic constructor IDs as Techno below;
    /// parent Foot registration precedes its constructor-manager children.
    /// UnInit retains rows; removal/replacement/restore update this cache.
    foot_registry: Vec<u64>,
    /// Derived TechnoClass registration index from monotonic constructor IDs.
    /// Production construction stores each parent before manager children;
    /// see construction33/55,world_spawn::store_with_constructor_managers,
    /// authored_health::unlimbo and reinforcement construction. Stable ID is
    /// authoritative, this cache is rebuilt on restore and never serialized.
    /// UnInit keeps a row; physical remove compacts the registry independently
    /// of the dense storage's empty slots and compaction schedule.
    /// RMG PreconsumedGenerated registration parity is not established here.
    techno_registry: Vec<u64>,
    /// BuildingClass::Array A8EB40, derived from immutable class and the
    /// same constructor IDs. Insert/remove/replace/restore maintain it like
    /// the other class registries; UnInit alone retains its row.
    building_registry: Vec<u64>,
    /// VERA-only: `(bucket, enter order, stable_id)` of every stored entity
    /// the AircraftTracker holds (an air bucket): each bucket's entries in
    /// the order [`Self::airborne_in_query_order`] copies them. The entity's
    /// tracker membership is the source of truth: [`Self::insert`] and
    /// [`Self::remove`] keep this set, and [`Self::set_air_membership`], the
    /// membership's one live writer, writes both. Rebuilt on load, never
    /// saved or hashed.
    airborne: BTreeSet<(u16, u64, u64)>,
    /// VERA-only: `(Anim, holder)` for each Anim a stored entity may hold in
    /// a field [`GameEntity::held_anims`] names, so an Anim's expiry
    /// (`Simulation::notify_anim_pointer_expired`, through
    /// [`Self::take_anim_holders`]) visits those entities instead of
    /// re-reading every entity handed out since the last expiry. A superset;
    /// the entities' fields are the truth. [`Self::insert`] and
    /// [`Self::write_held_anim`], the only route to storing an Anim in one of
    /// those fields, add pairs; the expiry takes an Anim's pairs, its
    /// callbacks clearing every field that names it; a removal drops the
    /// removed entity's. A field cleared otherwise leaves its pair until
    /// then. Rebuilt on load, never saved or hashed.
    anim_holders: BTreeSet<(u64, u64)>,
    /// Which entities may have changed since each reader last took its log
    /// ([`Self::take_touched`]). Every route to a `&mut GameEntity` goes
    /// through the store, so an entity missing from a log has not changed,
    /// but for the mission AI counter and a building's tint sample, which
    /// change every frame and no reader derives from
    /// ([`Self::count_mission_ai_visit`], [`Self::capture_building_tint_sample`]).
    touched: TouchLogs<{ TouchReader::COUNT }>,
    /// The empty stand-in of a presentation copy of the world
    /// ([`Self::detached`]), whose objects are in the presentation frame
    /// instead. Debug builds refuse reads of it.
    detached: bool,
}

/// One slot's sleep mark (sim-scale; results unchanged). An idle object's
/// turn changes nothing but its per-visit counters until one of its timers
/// comes due, so the live object pass may count the visit as owed instead of
/// running it (`Simulation::advance_live_object_pass`).
#[derive(Debug, Clone, Copy, Default)]
struct SleepCell {
    /// The first frame whose turn must run; 0 while awake.
    wake_at: u32,
    /// Visits the pass counted instead of running.
    owed: u32,
    /// What the sleeper's owner looked like when it fell asleep, for the
    /// pass to compare (`world::idle_sleep`).
    note: u8,
    /// The frame after the last turn that left the object fit to sleep,
    /// while awake; 0 for none ([`EntityStore::fit_to_sleep`]).
    fit_until: u32,
    /// The object is quiet (`world::quiet_units`): its last turn left every
    /// state its quiet turn skips off, and nothing has been handed it for
    /// writing since. Every mutable hand-out clears it, asleep or awake.
    quiet: bool,
}

/// A product derived from the entities that re-derives from the touch log.
/// Each reads its own log, so one reader's take leaves the others' intact.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TouchReader {
    /// The movement pass's kept blocker plane.
    BlockerPlane = 0,
    /// The kept Ground display sort keys.
    GroundKeys = 1,
    /// The pointer-expiry broadcast's listener index.
    ExpiryListeners = 2,
}

impl TouchReader {
    const COUNT: usize = 3;
}

impl Clone for EntityStore {
    /// A clone starts with an everything-touched log: whatever was derived
    /// from the original says nothing certain about the copy's future.
    fn clone(&self) -> Self {
        Self {
            ids: self.ids.clone(),
            entities: self.entities.clone(),
            slot_of: self.slot_of.clone(),
            focus: self.focus,
            #[cfg(any(test, feature = "test-support"))]
            lookups: Default::default(),
            owners: self.owners.clone(),
            categories: self.categories.clone(),
            sleep: self.sleep.clone(),
            owned: self.owned.clone(),
            infantry_registry: self.infantry_registry.clone(),
            foot_registry: self.foot_registry.clone(),
            techno_registry: self.techno_registry.clone(),
            building_registry: self.building_registry.clone(),
            airborne: self.airborne.clone(),
            anim_holders: self.anim_holders.clone(),
            touched: TouchLogs::everything(),
            detached: self.detached,
        }
    }
}

impl EntityStore {
    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: TouchReader) -> Touched {
        self.touched.take(reader as usize)
    }

    /// Mark `stable_id` asleep until frame `wake_at` (above 0): the pass may
    /// count its visits before that frame as owed. Any mutable hand-out wakes
    /// it first.
    pub(crate) fn sleep_until(&mut self, stable_id: u64, wake_at: u32, note: u8) {
        debug_assert!(wake_at != 0, "wake frame 0 means awake");
        if let Some(slot) = self.slot(stable_id) {
            let cell = &mut self.sleep[slot as usize];
            cell.wake_at = wake_at;
            cell.note = note;
        }
    }

    /// `stable_id`'s turn at `frame` left it fit to sleep. Answers whether
    /// its turn the frame before did too: only then did this whole turn run
    /// on a resting object, so its samplers hold what a next turn would
    /// sample. Waking forgets it.
    pub(crate) fn fit_to_sleep(&mut self, stable_id: u64, frame: u32) -> bool {
        let Some(slot) = self.slot(stable_id) else {
            return false;
        };
        let cell = &mut self.sleep[slot as usize];
        let rested = cell.fit_until == frame;
        cell.fit_until = frame.wrapping_add(1);
        rested
    }

    /// `stable_id`'s turn at `frame` left something in progress.
    pub(crate) fn unfit_to_sleep(&mut self, stable_id: u64) {
        if let Some(slot) = self.slot(stable_id) {
            self.sleep[slot as usize].fit_until = 0;
        }
    }

    /// The frame `stable_id` sleeps until and its note, if it is asleep.
    #[inline]
    pub(crate) fn sleeps_until(&self, stable_id: u64) -> Option<(u32, u8)> {
        let cell = self.sleep[self.slot(stable_id)? as usize];
        (cell.wake_at != 0).then_some((cell.wake_at, cell.note))
    }

    /// Count the visit the pass makes to sleeping `stable_id` instead of
    /// running its turn.
    pub(crate) fn owe_visit(&mut self, stable_id: u64) {
        if let Some(slot) = self.slot(stable_id) {
            let cell = &mut self.sleep[slot as usize];
            cell.owed = cell.owed.wrapping_add(1);
        }
    }

    /// Drop the visits `stable_id` is owed without settling them: the sleep
    /// check's, which counts one only to hash the skipped turn
    /// (`world::idle_sleep`).
    pub(crate) fn forget_owed(&mut self, stable_id: u64) {
        if let Some(slot) = self.slot(stable_id) {
            self.sleep[slot as usize].owed = 0;
        }
    }

    /// The visits sleeping `stable_id` is owed, which its mission counter
    /// does not hold yet ([`GameEntity::settle_sleep`]).
    pub(crate) fn owed_visits(&self, stable_id: u64) -> u32 {
        self.slot(stable_id)
            .map_or(0, |slot| self.sleep[slot as usize].owed)
    }

    /// The visits each stored entity is owed, in [`Self::values`] order.
    pub(crate) fn owed_in_value_order(&self) -> Vec<u32> {
        self.entities
            .iter()
            .zip(&self.sleep)
            .filter(|(entity, _)| entity.is_some())
            .map(|(_, cell)| cell.owed)
            .collect()
    }

    /// Wake `stable_id`, settling its owed visits.
    pub(crate) fn wake(&mut self, stable_id: u64) {
        if let Some(slot) = self.slot(stable_id) {
            self.wake_slot(slot as usize);
        }
    }

    /// Wake every object: for a change that sleeping objects read without
    /// being written (a house's power, a cell's hidden state).
    pub(crate) fn wake_all(&mut self) {
        for slot in 0..self.sleep.len() {
            self.wake_slot(slot);
        }
    }

    /// Whether `stable_id` is quiet: its last turn left it so, and nothing
    /// has been handed it for writing since (`world::quiet_units`).
    #[inline]
    pub(crate) fn is_quiet(&self, stable_id: u64) -> bool {
        self.slot(stable_id)
            .is_some_and(|slot| self.sleep[slot as usize].quiet)
    }

    /// `stable_id`'s entity and whether it is quiet, in one lookup.
    pub(crate) fn get_and_quiet(&self, stable_id: u64) -> Option<(&GameEntity, bool)> {
        let slot = self.slot(stable_id)? as usize;
        Some((self.entities[slot].as_deref()?, self.sleep[slot].quiet))
    }

    /// Mark `stable_id` quiet until its next mutable hand-out.
    pub(crate) fn set_quiet(&mut self, stable_id: u64) {
        if let Some(slot) = self.slot(stable_id) {
            self.sleep[slot as usize].quiet = true;
        }
    }

    /// How many objects are asleep.
    pub(crate) fn sleeping_count(&self) -> usize {
        self.sleep.iter().filter(|cell| cell.wake_at != 0).count()
    }

    /// Clear `slot`'s sleep mark and quiet flag and give its entity the
    /// visits it is owed, before anything writes it.
    #[inline]
    fn wake_slot(&mut self, slot: usize) {
        let cell = self.sleep[slot];
        if cell.wake_at == 0 {
            if cell.quiet {
                self.sleep[slot].quiet = false;
            }
            return;
        }
        self.sleep[slot] = SleepCell::default();
        if cell.owed == 0 {
            return;
        }
        // Through a shared reference, as the per-visit step does: waking is
        // not a hand-out.
        if let Some(entity) = self.entities[slot].as_deref() {
            entity.settle_sleep(cell.owed);
        }
    }

    /// How many mutable hand-outs the store has made (test builds only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hand_outs(&self) -> u64 {
        self.touched.hand_outs()
    }

    /// How many lookups by id the store has answered, and how many of them
    /// through the id index rather than the focused slot (test builds only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn lookups(&self) -> (u64, u64) {
        let count = |counter: &std::sync::atomic::AtomicU64| {
            counter.load(std::sync::atomic::Ordering::Relaxed)
        };
        (count(&self.lookups[0]), count(&self.lookups[1]))
    }

    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            entities: Vec::new(),
            slot_of: HashMap::default(),
            // An empty store holds no id.
            focus: (0, None),
            #[cfg(any(test, feature = "test-support"))]
            lookups: Default::default(),
            owners: Vec::new(),
            categories: Vec::new(),
            sleep: Vec::new(),
            owned: LookupMap::default(),
            infantry_registry: Vec::new(),
            foot_registry: Vec::new(),
            techno_registry: Vec::new(),
            building_registry: Vec::new(),
            airborne: BTreeSet::new(),
            anim_holders: BTreeSet::new(),
            touched: TouchLogs::everything(),
            detached: false,
        }
    }

    /// The empty store a presentation copy of the world holds in place of
    /// the objects (`Simulation::presentation_copy`): drawing reads them
    /// from its presentation frame. Debug builds panic on a read of it, so a
    /// reader still reaching for the world's objects shows up in any draw.
    pub(crate) fn detached() -> Self {
        Self {
            detached: true,
            ..Self::new()
        }
    }

    /// Debug builds: refuse a read of a [`Self::detached`] store.
    #[inline]
    fn check_attached(&self) {
        debug_assert!(
            !self.detached,
            "read of the objects of a presentation copy of the world: read the presentation frame's records instead"
        );
    }

    /// Insert an entity, replacing one of the same id (rare — stable_ids are
    /// monotonic). Returns its stable_id.
    pub fn insert(&mut self, entity: GameEntity) -> u64 {
        let id = entity.stable_id();
        self.touched.note(id, self.len());
        let category = entity.category();
        let infantry = category == EntityCategory::Infantry;
        let owner = entity.owner();
        let foot = category.is_foot();
        // A replaced entity leaves its owner's list and the airborne set;
        // this one joins its owner's list, and the airborne set if the
        // tracker holds it.
        if let Some((key, entry)) = self.get(id).map(|replaced| {
            (
                (replaced.owner(), replaced.category()),
                airborne_entry(replaced),
            )
        }) {
            self.unlist_owned(id, key);
            if let Some(entry) = entry {
                self.airborne.remove(&entry);
            }
        }
        self.list_owned(id, (owner, category));
        if let Some(entry) = airborne_entry(&entity) {
            self.airborne.insert(entry);
        }
        self.anim_holders
            .extend(entity.held_anims().map(|anim| (anim, id)));
        let entity = Some(Box::new(entity));
        if let Some(&slot) = self.slot_of.get(&id) {
            self.entities[slot as usize] = entity;
            self.owners[slot as usize] = owner;
            self.categories[slot as usize] = category;
            self.sleep[slot as usize] = SleepCell::default();
        } else if self.ids.last().is_none_or(|&last| last < id) {
            self.slot_of.insert(id, self.ids.len() as u32);
            self.ids.push(id);
            self.entities.push(entity);
            self.owners.push(owner);
            self.categories.push(category);
            self.sleep.push(SleepCell::default());
        } else {
            // An id below the newest: a fixture's, or a removed id stored again.
            let slot = self.ids.partition_point(|&stored| stored < id);
            if self.ids[slot] == id {
                self.entities[slot] = entity;
                self.owners[slot] = owner;
                self.categories[slot] = category;
                self.sleep[slot] = SleepCell::default();
                self.slot_of.insert(id, slot as u32);
            } else {
                self.ids.insert(slot, id);
                self.entities.insert(slot, entity);
                self.owners.insert(slot, owner);
                self.categories.insert(slot, category);
                self.sleep.insert(slot, SleepCell::default());
                self.reindex_from(slot);
            }
        }
        self.refocus();
        if let Err(index) = self.techno_registry.binary_search(&id) {
            self.techno_registry.insert(index, id);
        }
        if let Ok(index) = self.building_registry.binary_search(&id) {
            self.building_registry.remove(index);
        }
        if category == EntityCategory::Structure {
            let index = self
                .building_registry
                .partition_point(|&existing| existing < id);
            self.building_registry.insert(index, id);
        }
        self.remove_infantry_index(id);
        if infantry {
            let index = self
                .infantry_registry
                .partition_point(|&existing| existing < id);
            self.infantry_registry.insert(index, id);
        }
        if let Ok(index) = self.foot_registry.binary_search(&id) {
            self.foot_registry.remove(index);
        }
        if foot {
            let index = self
                .foot_registry
                .partition_point(|&existing| existing < id);
            self.foot_registry.insert(index, id);
        }
        id
    }

    /// Remove an entity by stable_id. Returns the removed entity if it existed.
    pub fn remove(&mut self, stable_id: u64) -> Option<GameEntity> {
        let slot = *self.slot_of.get(&stable_id)? as usize;
        self.wake_slot(slot);
        self.slot_of.remove(&stable_id);
        self.refocus();
        let removed = self.entities[slot].take().map(|entity| *entity);
        if let Some(entity) = &removed {
            self.unlist_owned(stable_id, (entity.owner(), entity.category()));
            if let Some(entry) = airborne_entry(entity) {
                self.airborne.remove(&entry);
            }
            for anim in entity.held_anims() {
                self.anim_holders.remove(&(anim, stable_id));
            }
        }
        self.touched.note(stable_id, self.len());
        self.remove_infantry_index(stable_id);
        if let Ok(index) = self.foot_registry.binary_search(&stable_id) {
            self.foot_registry.remove(index);
        }
        if let Ok(index) = self.techno_registry.binary_search(&stable_id) {
            self.techno_registry.remove(index);
        }
        if let Ok(index) = self.building_registry.binary_search(&stable_id) {
            self.building_registry.remove(index);
        }
        // Compact once a quarter of the slots are empty, so a walk skips few
        // and compaction's cost spreads over many removals.
        let empty = self.ids.len() - self.slot_of.len();
        if empty >= 64 && empty * 4 >= self.ids.len() {
            self.compact();
        }
        removed
    }

    /// Drop the empty slots, keeping the order, and re-index.
    fn compact(&mut self) {
        let mut kept = 0;
        for slot in 0..self.ids.len() {
            if self.entities[slot].is_some() {
                self.ids.swap(kept, slot);
                self.entities.swap(kept, slot);
                self.owners.swap(kept, slot);
                self.categories.swap(kept, slot);
                self.sleep.swap(kept, slot);
                kept += 1;
            }
        }
        self.ids.truncate(kept);
        self.entities.truncate(kept);
        self.owners.truncate(kept);
        self.categories.truncate(kept);
        self.sleep.truncate(kept);
        self.reindex_from(0);
    }

    /// Re-point the index at every stored slot from `first` on.
    fn reindex_from(&mut self, first: usize) {
        for slot in first..self.ids.len() {
            if self.entities[slot].is_some() {
                self.slot_of.insert(self.ids[slot], slot as u32);
            }
        }
        self.refocus();
    }

    /// Remember `stable_id`'s slot, so lookups of it skip the id index until
    /// another id is focused. For the object whose turn it is; an id the
    /// store does not hold is remembered as absent. No lookup's answer
    /// changes.
    pub(crate) fn focus(&mut self, stable_id: u64) {
        // Every writer of `slot_of` re-reads the focused slot, so focusing
        // the same id again has nothing to look up.
        if self.focus.0 == stable_id {
            return;
        }
        self.focus.0 = stable_id;
        self.refocus();
    }

    /// Re-read the focused id's slot after a write to `slot_of`.
    fn refocus(&mut self) {
        self.focus.1 = self.slot_of.get(&self.focus.0).copied();
    }

    /// `stable_id`'s slot, if it is stored: the focused id's without the id
    /// index. Debug builds check the focused slot against the index (the
    /// live-read check).
    #[inline]
    fn slot(&self, stable_id: u64) -> Option<u32> {
        self.check_attached();
        #[cfg(any(test, feature = "test-support"))]
        self.lookups[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.focus.0 != stable_id {
            #[cfg(any(test, feature = "test-support"))]
            self.lookups[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return self.slot_of.get(&stable_id).copied();
        }
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.focus.1 == self.slot_of.get(&stable_id).copied(),
            "the focused slot disagrees with the id index for entity {stable_id}"
        );
        self.focus.1
    }

    /// The stored entities in ascending stable_id order, with their ids.
    fn stored(&self) -> Stored<'_> {
        self.check_attached();
        Stored {
            ids: self.ids.iter(),
            entities: self.entities.iter(),
            remaining: self.len(),
        }
    }

    /// Clear all RadioClass-style live contacts involving `stable_id`.
    ///
    /// Idempotent. Safe if `stable_id` is absent.
    pub fn clear_radio_contacts_for(&mut self, stable_id: u64) {
        // The walk below writes every entity.
        self.wake_all();
        let stored = self.len();
        self.touched.note(stable_id, stored);
        for entity in self.entities.iter_mut().filter_map(Option::as_deref_mut) {
            if entity.has_live_contact_with(stable_id) || entity.tether.partner() == Some(stable_id)
            {
                self.touched.note(entity.stable_id(), stored);
            }
            entity.clear_live_contact_with(stable_id);
            // Drop a dangling tether to the departing entity (the BREAK
            // cascade a limbo'd dock partner would otherwise miss).
            entity.tether.forget(stable_id);
            if entity.stable_id() == stable_id {
                entity.radio_contacts.clear_all();
                entity.tether.release();
            }
        }
    }

    /// Look up an entity by stable_id (immutable).
    pub fn get(&self, stable_id: u64) -> Option<&GameEntity> {
        let slot = self.slot(stable_id)?;
        self.entities[slot as usize].as_deref()
    }

    /// `stable_id`'s owner, read from the owner column without touching the
    /// entity. Debug builds check it against the entity (the live-read check).
    pub(crate) fn owner_of(&self, stable_id: u64) -> Option<InternedId> {
        let slot = self.slot(stable_id)? as usize;
        let owner = self.owners[slot];
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.entities[slot].as_deref().map(GameEntity::owner) == Some(owner),
            "the owner column disagrees with entity {stable_id}"
        );
        Some(owner)
    }

    /// `stable_id`'s category, read from the category column without touching
    /// the entity. Debug builds check it against the entity (the live-read
    /// check).
    pub(crate) fn category_of(&self, stable_id: u64) -> Option<EntityCategory> {
        let slot = self.slot(stable_id)? as usize;
        let category = self.categories[slot];
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.entities[slot].as_deref().map(GameEntity::category) == Some(category),
            "the category column disagrees with entity {stable_id}"
        );
        Some(category)
    }

    /// `stable_id`'s entity and the slot it is stored in. Slots stay put
    /// while the store is borrowed, so tables built under one borrow may
    /// index by them; every slot is below [`Self::slot_count`].
    pub(crate) fn get_in_slot(&self, stable_id: u64) -> Option<(usize, &GameEntity)> {
        let slot = self.slot(stable_id)? as usize;
        Some((slot, self.entities[slot].as_deref()?))
    }

    /// One more than the highest slot.
    pub(crate) fn slot_count(&self) -> usize {
        self.entities.len()
    }

    /// Mutate ordinary entity payload. Indexed identity is read through accessors;
    /// live ownership changes use the world lifecycle and `change_owner` below.
    /// Replacing a stored entity wholesale is unsupported: remove/insert through
    /// the owning lifecycle instead so its indexes and registrations are updated.
    pub fn get_mut(&mut self, stable_id: u64) -> Option<&mut GameEntity> {
        let slot = self.slot(stable_id)?;
        self.touched.note(stable_id, self.slot_of.len());
        self.wake_slot(slot as usize);
        self.entities[slot as usize].as_deref_mut()
    }

    /// `get_mut` for an entity `admit` accepts, which it reads first: one it
    /// refuses is not handed out, so it stays out of the touch logs. For walks
    /// that visit every entity and change few.
    pub(crate) fn get_mut_if(
        &mut self,
        stable_id: u64,
        admit: impl FnOnce(&GameEntity) -> bool,
    ) -> Option<&mut GameEntity> {
        let slot = self.slot(stable_id)? as usize;
        // Read through a shared reference: only an admitted entity is
        // handed out and noted in the touch logs.
        if !admit(self.entities[slot].as_deref()?) {
            return None;
        }
        self.touched.note(stable_id, self.slot_of.len());
        self.wake_slot(slot);
        self.entities[slot].as_deref_mut()
    }

    /// Step `stable_id`'s mission AI counter (`+0xC4`) for one AI visit
    /// (`0x006FA646..0x006FA64F`) without a note in the touch logs: every
    /// techno steps it every frame, and no product derived from the logs
    /// reads it. No-op if the entity is absent.
    pub(crate) fn count_mission_ai_visit(&mut self, stable_id: u64) {
        // Through a shared reference: the counter steps without a mutable
        // hand-out, which would wake the object and note it in the logs.
        if let Some(entity) = self.get(stable_id) {
            entity.mission.count_ai_visit();
        }
    }

    /// Keep a building's relight light at `frame` for the draw
    /// ([`crate::sim::building_art::BuildingArt::capture_tint_sample`]),
    /// unless the kept one is current
    /// ([`crate::sim::building_art::BuildingArt::tint_sample_is_current`]),
    /// without a note in the touch logs: every building relights every
    /// frame, and no product derived from the logs reads the sample. No-op
    /// for an absent entity or one that is not a building.
    pub(crate) fn capture_building_tint_sample(
        &mut self,
        stable_id: u64,
        frame: u32,
        airstrike_active: bool,
    ) {
        // Read first: a building whose sample already says what the draw
        // would read is not handed out.
        if self.get(stable_id).is_none_or(|entity| {
            entity.category() != EntityCategory::Structure
                || entity.building_art.tint_sample_is_current(
                    entity.flashing.flashed(),
                    entity.invulnerability.as_ref(),
                    &entity.airstrike_tint,
                    airstrike_active,
                )
        }) {
            return;
        }
        if let Some(entity) = self.unlogged_mut(stable_id) {
            entity.building_art.capture_tint_sample(
                frame,
                entity.flashing.flashed(),
                entity.invulnerability.as_ref(),
                &entity.airstrike_tint,
                airstrike_active,
            );
        }
    }

    /// `stable_id`'s entity without a note: only for the writes the touch
    /// log's contract excepts.
    fn unlogged_mut(&mut self, stable_id: u64) -> Option<&mut GameEntity> {
        let slot = *self.slot_of.get(&stable_id)?;
        self.wake_slot(slot as usize);
        self.entities[slot as usize].as_deref_mut()
    }

    /// Check if an entity exists.
    pub fn contains(&self, stable_id: u64) -> bool {
        self.slot(stable_id).is_some()
    }

    /// Number of entities in the store.
    pub fn len(&self) -> usize {
        self.check_attached();
        self.slot_of.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.check_attached();
        self.slot_of.is_empty()
    }

    /// Get sorted keys for deterministic iteration.
    ///
    /// Callers typically iterate with `get()` or `get_mut_if()`:
    /// ```ignore
    /// let keys = store.keys_sorted();
    /// for &id in &keys {
    ///     if let Some(entity) = store.get_mut_if(id, |entity| entity.rocking.is_some()) { ... }
    /// }
    /// ```
    pub fn keys_sorted(&self) -> Vec<u64> {
        self.stored().map(|(id, _)| id).collect()
    }

    /// The stored ids of the entities whose category `keep` selects, in
    /// ascending stable_id order, chosen by the category column without
    /// loading an entity.
    pub(crate) fn ids_of_categories(&self, keep: impl Fn(EntityCategory) -> bool) -> Vec<u64> {
        self.check_attached();
        self.ids
            .iter()
            .zip(&self.categories)
            .zip(&self.entities)
            .filter(|&((_, &category), entity)| entity.is_some() && keep(category))
            .map(|((&id, _), _)| id)
            .collect()
    }

    /// The ids of the stored entities `keep` selects, in ascending stable_id
    /// order. `keep` runs on worker threads ([`crate::sim::parallel`]), so it
    /// only reads.
    pub(crate) fn par_select(&self, keep: impl Fn(u64, &GameEntity) -> bool + Sync) -> Vec<u64> {
        crate::sim::parallel::select_indices(self.ids.len(), |slot| {
            self.entities[slot]
                .as_deref()
                .is_some_and(|entity| keep(self.ids[slot], entity))
        })
        .into_iter()
        .map(|slot| self.ids[slot])
        .collect()
    }

    /// `pick`'s answers for the stored entities it answers, in ascending
    /// stable_id order. `pick` runs on worker threads
    /// ([`crate::sim::parallel`]), so it only reads.
    pub fn par_filter_map<'a, R: Send>(
        &'a self,
        pick: impl Fn(u64, &'a GameEntity) -> Option<R> + Sync,
    ) -> Vec<R> {
        crate::sim::parallel::filter_map_indices(self.ids.len(), |slot| {
            self.entities[slot]
                .as_deref()
                .and_then(|entity| pick(self.ids[slot], entity))
        })
    }

    /// Action14's full Techno array, independent of Logic and Display order.
    /// Read length/slot afresh after callbacks; append/removal is visible.
    pub(crate) fn techno_registry_at(&self, index: usize) -> Option<u64> {
        self.techno_registry.get(index).copied()
    }
    pub(crate) fn techno_registry_len(&self) -> usize {
        self.techno_registry.len()
    }

    /// UnitClass::Array8B410C is the Unit subset of constructor registration
    /// order. Derive this one-shot reader view from the existing registry;
    /// there is no second class list to save or keep in sync.
    pub(crate) fn unit_registry_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.techno_registry
            .iter()
            .copied()
            .filter(|&id| self.category_of(id) == Some(EntityCategory::Unit))
    }

    pub(crate) fn building_registry_at(&self, index: usize) -> Option<u64> {
        self.building_registry.get(index).copied()
    }
    pub(crate) fn building_registry_len(&self) -> usize {
        self.building_registry.len()
    }

    pub(crate) fn foot_registry_at(&self, index: usize) -> Option<u64> {
        self.foot_registry.get(index).copied()
    }

    pub(crate) fn foot_registry_len(&self) -> usize {
        self.foot_registry.len()
    }

    /// Native InfantryClass array index. Read afresh after a callback: removing
    /// a row shifts successors left, while a newly constructed row is visible.
    /// This is distinct from the live Logic vector and includes dead/limbo rows.
    pub(crate) fn infantry_registry_at(&self, index: usize) -> Option<u64> {
        self.infantry_registry.get(index).copied()
    }

    /// The native InfantryClass array's length.
    pub(crate) fn infantry_registry_len(&self) -> usize {
        self.infantry_registry.len()
    }

    fn remove_infantry_index(&mut self, id: u64) {
        if let Ok(index) = self.infantry_registry.binary_search(&id) {
            self.infantry_registry.remove(index);
        }
    }

    /// Iterate all entities in deterministic stable_id order (immutable).
    pub fn iter_sorted(
        &self,
    ) -> impl DoubleEndedIterator<Item = (u64, &GameEntity)> + ExactSizeIterator {
        self.stored()
    }

    /// Iterate all entity values in deterministic stable_id order (immutable).
    pub fn values_sorted(&self) -> impl ExactSizeIterator<Item = &GameEntity> {
        self.stored().map(|(_, entity)| entity)
    }

    /// Iterate all entities in stable_id order (immutable).
    pub fn values(&self) -> impl ExactSizeIterator<Item = &GameEntity> {
        self.stored().map(|(_, entity)| entity)
    }

    /// The entities of `category` in stable_id order, chosen by the category
    /// column without loading the others.
    pub fn values_of_category(
        &self,
        category: EntityCategory,
    ) -> impl Iterator<Item = &GameEntity> {
        self.check_attached();
        self.categories
            .iter()
            .zip(&self.entities)
            .filter(move |&(&stored, _)| stored == category)
            .filter_map(|(_, entity)| entity.as_deref())
    }

    /// The entities `keep` selects by id and owner, in stable_id order,
    /// chosen from the id and owner columns without loading the others.
    /// Debug builds check each owner against its entity (the live-read
    /// check).
    pub(crate) fn values_of_owners(
        &self,
        keep: impl Fn(u64, InternedId) -> bool,
    ) -> impl Iterator<Item = &GameEntity> {
        self.ids
            .iter()
            .zip(&self.owners)
            .zip(&self.entities)
            .filter_map(move |((&id, &owner), entity)| {
                let entity = entity.as_deref()?;
                debug_assert!(
                    !crate::sim::touch_log::live_read_check_enabled() || entity.owner() == owner,
                    "the owner column disagrees with entity {id}"
                );
                keep(id, owner).then_some(entity)
            })
    }

    /// Iterate all entities mutably in stable_id order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut GameEntity> {
        self.touched.note_all();
        self.wake_all();
        self.entities.iter_mut().filter_map(Option::as_deref_mut)
    }

    /// Store an Anim in one of `holder`'s fields [`GameEntity::held_anims`]
    /// names through `write`, that field's own write, then note every Anim
    /// the holder holds for [`Self::take_anim_holders`]. The only route to
    /// such a store: those writes take an [`AnimHolderAuthority`], which
    /// only this module makes. Clearing a field needs no note. `None` when
    /// `holder` is not stored.
    pub(crate) fn write_held_anim<R>(
        &mut self,
        holder: u64,
        write: impl FnOnce(&mut GameEntity, AnimHolderAuthority) -> R,
    ) -> Option<R> {
        let slot = self.slot(holder)?;
        self.touched.note(holder, self.slot_of.len());
        self.wake_slot(slot as usize);
        let entity = self.entities[slot as usize].as_deref_mut()?;
        let written = write(entity, AnimHolderAuthority(()));
        self.anim_holders
            .extend(entity.held_anims().map(|anim| (anim, holder)));
        Some(written)
    }

    /// The stored entities that may hold `anim` in a field
    /// [`GameEntity::held_anims`] names, ascending, leaving none noted for
    /// it: the caller, `Simulation::notify_anim_pointer_expired`, clears
    /// every such field naming it. Debug builds check that every stored
    /// entity holding it is among them (the live-read check).
    pub(crate) fn take_anim_holders(&mut self, anim: u64) -> Vec<u64> {
        let noted: Vec<u64> = self
            .anim_holders
            .range((anim, 0)..=(anim, u64::MAX))
            .map(|&(_, holder)| holder)
            .collect();
        for &holder in &noted {
            self.anim_holders.remove(&(anim, holder));
        }
        let holders: Vec<u64> = noted
            .into_iter()
            .filter(|&holder| self.contains(holder))
            .collect();
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self
                    .par_select(|_, entity| entity.held_anims().any(|held| held == anim))
                    .iter()
                    .all(|holder| holders.binary_search(holder).is_ok()),
            "an entity holds Anim {anim} without a noted pair"
        );
        holders
    }

    /// `(Anim, holder)` for every Anim a stored entity holds.
    fn walk_anim_holders(&self) -> BTreeSet<(u64, u64)> {
        self.stored()
            .flat_map(|(id, entity)| entity.held_anims().map(move |anim| (anim, id)))
            .collect()
    }

    /// Move an entity to a new owner. It does NOT touch the houses' tracking
    /// or lists; `Simulation::change_owner`, the only production caller,
    /// moves them.
    /// No-op if the entity is absent or already owned by `new_owner`.
    pub fn change_owner(&mut self, stable_id: u64, new_owner: crate::sim::intern::InternedId) {
        self.touched.note(stable_id, self.len());
        if let Some(slot) = self.slot(stable_id) {
            self.wake_slot(slot as usize);
        }
        if let Some(slot) = self.slot(stable_id)
            && let Some(e) = self.entities[slot as usize].as_deref_mut()
            && e.owner() != new_owner
        {
            let (old_owner, category) = (e.owner(), e.category());
            e.set_owner_from_store(new_owner, OwnerChangeAuthority(()));
            self.owners[slot as usize] = new_owner;
            self.unlist_owned(stable_id, (old_owner, category));
            self.list_owned(stable_id, (new_owner, category));
        }
    }

    /// The stored ids of `owner`'s objects of `category`, ascending: the ids
    /// a walk of every entity in stable_id order keeps for that owner and
    /// category, without loading the others. Debug builds check them against
    /// that walk (the live-read check).
    pub(crate) fn owned_ids(&self, owner: InternedId, category: EntityCategory) -> &[u64] {
        let ids = self
            .owned
            .get(&(owner, category))
            .map_or(&[][..], Vec::as_slice);
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || ids.iter().copied().eq(self
                    .stored()
                    .filter(|(_, entity)| entity.owner() == owner && entity.category() == category)
                    .map(|(id, _)| id)),
            "the owned index disagrees with the entities of {owner:?}'s {category:?}"
        );
        ids
    }

    /// Put `id` on the `key` owner's list, in order. Ids mostly arrive
    /// ascending, so this mostly appends.
    fn list_owned(&mut self, id: u64, key: (InternedId, EntityCategory)) {
        let ids = self.owned.entry(key).or_default();
        if let Err(index) = ids.binary_search(&id) {
            ids.insert(index, id);
        }
    }

    /// Take `id` off the `key` owner's list.
    fn unlist_owned(&mut self, id: u64, key: (InternedId, EntityCategory)) {
        if let Some(ids) = self.owned.get_mut(&key)
            && let Ok(index) = ids.binary_search(&id)
        {
            ids.remove(index);
        }
    }

    /// Write `stable_id`'s AircraftTracker membership, its bucket (`None`:
    /// not tracked) and enter order, and the airborne set with it. The
    /// tracker's Add, Remove and Update (`Simulation::aircraft_tracker_add`
    /// and its siblings) are the only callers. No-op if the entity is absent.
    pub(crate) fn set_air_membership(
        &mut self,
        stable_id: u64,
        bucket: Option<u16>,
        enter_order: u64,
    ) {
        let Some(slot) = self.slot(stable_id) else {
            return;
        };
        self.touched.note(stable_id, self.slot_of.len());
        self.wake_slot(slot as usize);
        let Some(entity) = self.entities[slot as usize].as_deref_mut() else {
            return;
        };
        if let Some(entry) = airborne_entry(entity) {
            self.airborne.remove(&entry);
        }
        entity.set_air_membership_from_store(bucket, enter_order, AirMembershipAuthority(()));
        if let Some(bucket) = bucket {
            self.airborne.insert((bucket, enter_order, stable_id));
        }
    }

    /// The ids the AircraftTracker holds, in no order a caller may rely on.
    pub(crate) fn airborne_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.airborne.iter().map(|&(_, _, id)| id)
    }

    /// The stored entities the AircraftTracker holds, by enter order and,
    /// for the ties only fixtures make, stable_id.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn airborne(&self) -> impl Iterator<Item = &GameEntity> {
        let mut entries: Vec<(u64, u64)> = self
            .airborne
            .iter()
            .map(|&(_, order, id)| (order, id))
            .collect();
        entries.sort_unstable();
        entries.into_iter().filter_map(|(_, id)| self.get(id))
    }

    /// Materialize AircraftTracker412B40/4137A0 in retained per-bucket entry
    /// order. Shared by GreatestThreat6F91D3 and area damage4892D8. Current
    /// object coordinates do not relocate entries; FootAir owns that update.
    /// Repeated query buckets deliberately copy their vectors again. Each
    /// bucket's entries come from the airborne set, which debug builds check
    /// against a walk of every entity on every call (the live-read check).
    pub(crate) fn airborne_in_query_order(
        &self,
        center_cell: &crate::sim::cell_rect::CellRef<'_>,
        radius_cells: i32,
        map_extent: (u16, u16),
    ) -> Vec<u64> {
        //412B6E calls Cell vt+48;413A30 truncates the returned XYZ's
        // X/Y toward zero and packs16. Read the resolved live Cell rather
        // than its lookup argument: aliases and the shared Dummy can differ.
        let point = center_cell
            .center_coordinate()
            .expect("admitted Cell ground coordinate");
        let center = (
            crate::util::lepton::lepton_to_cell_packed(point.x) as u16,
            crate::util::lepton::lepton_to_cell_packed(point.y) as u16,
        );
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.airborne == self.walk_airborne(),
            "the airborne set disagrees with the entities"
        );
        let mut ordered = Vec::new();
        for bucket in crate::sim::occupancy::air_spatial_query_bucket_order(
            center.0,
            center.1,
            radius_cells,
            map_extent.0,
            map_extent.1,
        ) {
            ordered.extend(
                self.airborne
                    .range((bucket, 0, 0)..=(bucket, u64::MAX, u64::MAX))
                    .map(|&(_, _, id)| id),
            );
        }
        ordered
    }

    /// `(bucket, enter order, stable_id)` of every stored entity the tracker
    /// holds.
    fn walk_airborne(&self) -> BTreeSet<(u16, u64, u64)> {
        self.values().filter_map(airborne_entry).collect()
    }

    /// Rebuild class registries, and the owners' lists, from primary storage.
    /// Owned by Deserialize; synthetic fixtures may also rebuild after raw
    /// setup.
    pub(crate) fn rebuild_class_registries(&mut self) {
        let mut owned: LookupMap<(InternedId, EntityCategory), Vec<u64>> = LookupMap::default();
        // Storage iterates in ascending stable_id order, so each list comes
        // out sorted.
        for (id, entity) in self.stored() {
            owned
                .entry((entity.owner(), entity.category()))
                .or_default()
                .push(id);
        }
        self.owned = owned;
        let infantry: Vec<u64> = self
            .stored()
            .filter(|(_, entity)| entity.category() == EntityCategory::Infantry)
            .map(|(id, _)| id)
            .collect();
        // Storage iterates in ascending stable_id order, so the infantry
        // registry comes out sorted.
        self.infantry_registry = infantry;
        self.foot_registry = self
            .stored()
            .filter(|(_, entity)| entity.category().is_foot())
            .map(|(id, _)| id)
            .collect();
        self.techno_registry = self.stored().map(|(id, _)| id).collect();
        self.building_registry = self
            .stored()
            .filter(|(_, entity)| entity.category() == EntityCategory::Structure)
            .map(|(id, _)| id)
            .collect();
    }
}

/// [`EntityStore`]'s stored entities in ascending stable_id order, skipping
/// empty slots. Its length is exact, so a collect allocates once.
struct Stored<'a> {
    ids: std::slice::Iter<'a, u64>,
    entities: std::slice::Iter<'a, Option<Box<GameEntity>>>,
    /// Stored entities not yet yielded from either end.
    remaining: usize,
}

impl<'a> Iterator for Stored<'a> {
    type Item = (u64, &'a GameEntity);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = *self.ids.next()?;
            if let Some(entity) = self.entities.next()?.as_deref() {
                self.remaining -= 1;
                return Some((id, entity));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl DoubleEndedIterator for Stored<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let id = *self.ids.next_back()?;
            if let Some(entity) = self.entities.next_back()?.as_deref() {
                self.remaining -= 1;
                return Some((id, entity));
            }
        }
    }
}

impl ExactSizeIterator for Stored<'_> {}

impl serde::Serialize for EntityStore {
    /// The stored entities as a map from stable_id, in ascending order: the
    /// form the former `BTreeMap` storage wrote.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // The save codec needs the length up front, which a filtering iterator
        // cannot give `collect_map`.
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (id, entity) in self.stored() {
            // A sleeping entity is saved as its next turn would find it.
            match self.owed_visits(id) {
                0 => map.serialize_entry(&id, entity)?,
                owed => {
                    let settled = entity.clone();
                    settled.settle_sleep(owed);
                    map.serialize_entry(&id, &settled)?;
                }
            }
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for EntityStore {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entities = BTreeMap::<u64, Box<GameEntity>>::deserialize(deserializer)?;
        let mut store = Self::new();
        store.ids.reserve(entities.len());
        store.entities.reserve(entities.len());
        for (id, entity) in entities {
            store.slot_of.insert(id, store.ids.len() as u32);
            store.ids.push(id);
            store.owners.push(entity.owner());
            store.categories.push(entity.category());
            store.sleep.push(SleepCell::default());
            store.entities.push(Some(entity));
        }
        store.refocus();
        store.rebuild_class_registries();
        store.airborne = store.walk_airborne();
        store.anim_holders = store.walk_anim_holders();
        Ok(store)
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

/// `entity`'s entry in the airborne set, if the AircraftTracker holds it.
fn airborne_entry(entity: &GameEntity) -> Option<(u16, u64, u64)> {
    entity
        .air_spatial_bucket()
        .map(|bucket| (bucket, entity.air_spatial_enter_order(), entity.stable_id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;

    /// An AI visit steps the counter without handing the entity out, and a
    /// refused `get_mut_if` hands nothing out either.
    #[test]
    fn an_ai_visit_hands_nothing_out() {
        let mut store = EntityStore::new();
        store.insert(GameEntity::test_default(7, "MTNK", "Americans", 5, 5));
        let handed_out = store.hand_outs();
        store.count_mission_ai_visit(7);
        store.count_mission_ai_visit(7);
        assert!(store.get_mut_if(7, |_| false).is_none());
        assert_eq!(store.hand_outs(), handed_out);
        assert_eq!(store.get(7).unwrap().mission.ai_counter(), 2);
    }

    /// Populated original412B40 ->4137A0 controls. Buckets hold opaque
    /// tokens; the last numeric row deliberately repeats a token in two
    /// buckets, which is not a legal EntityStore membership. Its bucket walk
    /// is still compared, without fabricating two actors with the same ID.
    #[test]
    fn shared_air_query_bucket_and_entity_order_match_original() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/distributed_fire.json",
        ))
        .unwrap();
        let rows = corpus["air_query"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        let mut materialized = 0;
        for row in rows {
            let input = &row["input"];
            let point = |name: &str| {
                (
                    input[name][0].as_u64().unwrap() as u16,
                    input[name][1].as_u64().unwrap() as u16,
                )
            };
            let center = point("center_cell");
            let extent = point("map_extent");
            let radius = input["radius_cells"].as_i64().unwrap() as i32;
            let actual = crate::sim::occupancy::air_spatial_query_bucket_order(
                center.0, center.1, radius, extent.0, extent.1,
            );
            let expected: Vec<_> = row["copied_bucket_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect();
            assert_eq!(actual, expected, "{}", row["name"]);
            if row["name"] == "retained_order_duplicate_tokens" {
                continue;
            }
            let mut store = EntityStore::new();
            let mut order = 0;
            for bucket in input["buckets"].as_array().unwrap() {
                for token in bucket["ids"].as_array().unwrap() {
                    order += 1;
                    let id = token.as_u64().unwrap();
                    store.insert(make_entity(id).with_air_spatial_membership_for_test(
                        Some(bucket["bucket"].as_u64().unwrap() as u16),
                        order,
                    ));
                }
            }
            let expected: Vec<_> = row["copied_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            let cell = crate::map::resolved_terrain::test_flat_cell(center.0, center.1);
            assert_eq!(
                store.airborne_in_query_order(
                    &crate::sim::cell_rect::CellRef::Real(&cell),
                    radius,
                    extent
                ),
                expected,
                "{}",
                row["name"]
            );
            assert_eq!(row["copied_ids"], row["drained_ids"]);
            assert_eq!(row["remaining_count"], 0);
            materialized += 1;
        }
        assert_eq!(materialized, 6);
    }

    fn make_entity(id: u64) -> GameEntity {
        GameEntity::test_default(id, "HTNK", "Americans", 10, 10)
    }

    /// Each reader takes its own log, so one reader's take leaves the other's
    /// notes in place; a clone reports everything to both.
    #[test]
    fn each_touch_reader_takes_its_own_log() {
        let mut store = EntityStore::new();
        for id in [1, 2, 3] {
            store.insert(make_entity(id));
        }
        // Nothing has been derived from a fresh store yet.
        assert_eq!(store.take_touched(TouchReader::BlockerPlane), Touched::All);
        assert_eq!(store.take_touched(TouchReader::GroundKeys), Touched::All);
        {
            let position = &mut store.get_mut(2).unwrap().position;
            position.set_rx_for_test(position.rx() + 1);
        }
        store.get_mut(3);
        assert_eq!(
            store.take_touched(TouchReader::BlockerPlane),
            Touched::Ids(vec![2, 3])
        );
        store.get_mut(1);
        assert_eq!(
            store.take_touched(TouchReader::GroundKeys),
            Touched::Ids(vec![2, 3, 1])
        );
        assert_eq!(
            store.take_touched(TouchReader::BlockerPlane),
            Touched::Ids(vec![1])
        );
        let mut copy = store.clone();
        assert_eq!(copy.take_touched(TouchReader::GroundKeys), Touched::All);
        assert_eq!(copy.take_touched(TouchReader::BlockerPlane), Touched::All);
        let _ = store.values_mut().count();
        assert_eq!(store.take_touched(TouchReader::GroundKeys), Touched::All);
        assert_eq!(store.take_touched(TouchReader::BlockerPlane), Touched::All);
    }

    /// Insert, remove and lookup answer as the former `BTreeMap` storage did,
    /// across compactions, re-stored ids and ids below the newest; the owner
    /// column follows every entity's owner through them and through
    /// `change_owner`, the category column every entity's category through
    /// them (and walks of one category keep the stable_id order), each
    /// owner's list of its objects of a category both, in stable_id order,
    /// and the airborne set every entity's tracker membership through them
    /// and through `set_air_membership`.
    #[test]
    fn storage_answers_as_an_ordered_map_through_compactions() {
        let mut store = EntityStore::new();
        let mut model: BTreeMap<u64, u16> = BTreeMap::new();
        let houses = ["Americans", "Russians", "YuriCountry"].map(crate::sim::intern::test_intern);
        let mut owners: BTreeMap<u64, InternedId> = BTreeMap::new();
        let classes = [
            EntityCategory::Unit,
            EntityCategory::Infantry,
            EntityCategory::Structure,
            EntityCategory::Aircraft,
        ];
        let mut categories: BTreeMap<u64, EntityCategory> = BTreeMap::new();
        // Tracked entities' (bucket, enter order). Orders repeat across ids,
        // as fixtures' do, so ties fall to stable_id.
        let mut tracked: BTreeMap<u64, (u16, u64)> = BTreeMap::new();
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % bound
        };
        let mut newest = 0;
        let mut compactions = 0;
        let mut largest_compaction = 0;
        for step in 0..20_000 {
            let slots = store.ids.len();
            // 4,000 steps of growth alternate with 2,000 of heavy removal, so
            // the store passes 4,000 slots and compaction runs repeatedly, at
            // small and large sizes.
            let removing = step % 6_000 >= 4_000;
            match next(10) {
                // Mostly appends, as monotonic ids do.
                0..=4 if !removing => {
                    newest += 1 + next(3);
                    let mut entity = make_entity(newest);
                    entity.position.set_rx_for_test(step as u16);
                    entity.set_owner_for_test(houses[step % 3]);
                    entity.set_category_for_test(classes[step % 4]);
                    if next(4) == 0 {
                        let membership = (next(400) as u16, next(1_000));
                        entity = entity
                            .with_air_spatial_membership_for_test(Some(membership.0), membership.1);
                        tracked.insert(newest, membership);
                    }
                    store.insert(entity);
                    model.insert(newest, step as u16);
                    owners.insert(newest, houses[step % 3]);
                    categories.insert(newest, classes[step % 4]);
                }
                // A fixture's id below the newest, or a removed id stored again.
                5 => {
                    let id = next(newest + 1);
                    let mut entity = make_entity(id);
                    entity.position.set_rx_for_test(step as u16);
                    entity.set_owner_for_test(houses[step % 3]);
                    entity.set_category_for_test(classes[step % 4]);
                    // A replacement starts untracked or tracked afresh.
                    tracked.remove(&id);
                    if next(2) == 0 {
                        let membership = (next(400) as u16, next(1_000));
                        entity = entity
                            .with_air_spatial_membership_for_test(Some(membership.0), membership.1);
                        tracked.insert(id, membership);
                    }
                    store.insert(entity);
                    model.insert(id, step as u16);
                    owners.insert(id, houses[step % 3]);
                    categories.insert(id, classes[step % 4]);
                }
                0..=8 => {
                    // While removing, pick a stored id most of the time.
                    let id = if removing && !model.is_empty() && next(4) > 0 {
                        *model.keys().nth(next(model.len() as u64) as usize).unwrap()
                    } else {
                        next(newest + 1)
                    };
                    assert_eq!(store.remove(id).map(|e| e.position.rx()), model.remove(&id));
                    owners.remove(&id);
                    categories.remove(&id);
                    tracked.remove(&id);
                }
                _ => {
                    let id = next(newest + 1);
                    if let Some(entity) = store.get_mut(id) {
                        entity.position.set_rx_for_test(step as u16);
                    }
                    if let Some(rx) = model.get_mut(&id) {
                        *rx = step as u16;
                    }
                    store.change_owner(id, houses[step % 3]);
                    if let Some(owner) = owners.get_mut(&id) {
                        *owner = houses[step % 3];
                    }
                    // The tracker's Add or Update, or its Remove.
                    let membership = (next(3) > 0).then(|| (next(400) as u16, next(1_000)));
                    store.set_air_membership(
                        id,
                        membership.map(|(bucket, _)| bucket),
                        membership.map_or(0, |(_, order)| order),
                    );
                    if model.contains_key(&id) {
                        match membership {
                            Some(membership) => tracked.insert(id, membership),
                            None => tracked.remove(&id),
                        };
                    }
                }
            }
            if store.ids.len() < slots {
                compactions += 1;
                largest_compaction = largest_compaction.max(slots);
            }
            let probe = next(newest + 1);
            assert_eq!(store.contains(probe), model.contains_key(&probe));
            assert_eq!(
                store.get(probe).map(|e| e.position.rx()),
                model.get(&probe).copied()
            );
            assert_eq!(store.owner_of(probe), owners.get(&probe).copied());
            assert_eq!(store.category_of(probe), categories.get(&probe).copied());
            if step % 97 == 0 {
                assert!(
                    owners
                        .iter()
                        .all(|(&id, &owner)| store.owner_of(id) == Some(owner)
                            && store.get(id).map(GameEntity::owner) == Some(owner))
                );
                assert!(
                    categories
                        .iter()
                        .all(|(&id, &category)| store.category_of(id) == Some(category))
                );
                for class in classes {
                    assert_eq!(
                        store
                            .values_of_category(class)
                            .map(GameEntity::stable_id)
                            .collect::<Vec<_>>(),
                        categories
                            .iter()
                            .filter(|&(_, &category)| category == class)
                            .map(|(&id, _)| id)
                            .collect::<Vec<_>>()
                    );
                    for house in houses {
                        assert_eq!(
                            store.owned_ids(house, class),
                            owners
                                .iter()
                                .filter(|&(id, &owner)| owner == house && categories[id] == class)
                                .map(|(&id, _)| id)
                                .collect::<Vec<_>>()
                        );
                    }
                }
                assert_eq!(
                    store.building_registry,
                    categories
                        .iter()
                        .filter(|(_, category)| **category == EntityCategory::Structure)
                        .map(|(&id, _)| id)
                        .collect::<Vec<_>>(),
                    "Building registry derives from the model's immutable class"
                );
                let mut expected_airborne: Vec<(u64, u64, u16)> = tracked
                    .iter()
                    .map(|(&id, &(bucket, order))| (order, id, bucket))
                    .collect();
                expected_airborne.sort_unstable();
                let airborne: Vec<(u64, u64, u16)> = store
                    .airborne()
                    .map(|e| {
                        (
                            e.air_spatial_enter_order(),
                            e.stable_id(),
                            e.air_spatial_bucket().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(airborne, expected_airborne);
                assert_eq!(store.len(), model.len());
                assert_eq!(
                    store.keys_sorted(),
                    model.keys().copied().collect::<Vec<_>>()
                );
                assert_eq!(
                    store.techno_registry,
                    model.keys().copied().collect::<Vec<_>>(),
                    "native registration rows must not follow dense empty slots"
                );
                let stored: Vec<(u64, u16)> = store
                    .iter_sorted()
                    .rev()
                    .map(|(id, e)| (id, e.position.rx()))
                    .collect();
                let expected: Vec<(u64, u16)> = model.iter().rev().map(|(&k, &v)| (k, v)).collect();
                assert_eq!(stored, expected);
                assert!(
                    store.ids.len() - store.len() < 64
                        || (store.ids.len() - store.len()) * 4 < store.ids.len()
                );
                assert_eq!(store.iter_sorted().len(), model.len());
            }
        }
        assert!(
            compactions >= 5 && largest_compaction >= 2_000,
            "{compactions} compactions, the largest at {largest_compaction} slots"
        );
    }

    /// A save writes the store as the former `BTreeMap` storage did, so the
    /// snapshot format is unchanged, and a restore reads it back in order.
    #[test]
    fn saved_form_is_the_ordered_map_form() {
        let mut store = EntityStore::new();
        for id in [4, 9, 2, 7] {
            store.insert(make_entity(id));
        }
        store.remove(9);
        store.change_owner(7, crate::sim::intern::test_intern("Russians"));
        store.set_air_membership(4, Some(12), 30);
        store.set_air_membership(2, Some(12), 31);
        let map: BTreeMap<u64, Box<GameEntity>> = [2, 4, 7]
            .into_iter()
            .map(|id| (id, Box::new(store.get(id).unwrap().clone())))
            .collect();
        let saved = crate::util::save_codec::serialize(&store).unwrap();
        assert_eq!(saved, crate::util::save_codec::serialize(&map).unwrap());
        let restored: EntityStore = crate::util::save_codec::deserialize(&saved).unwrap();
        assert_eq!(restored.keys_sorted(), [2, 4, 7]);
        assert_eq!(restored.techno_registry, [2, 4, 7]);
        for id in [2, 4, 7] {
            assert_eq!(restored.owner_of(id), Some(store.get(id).unwrap().owner()));
        }
        let (americans, russians) = (
            crate::sim::intern::test_intern("Americans"),
            crate::sim::intern::test_intern("Russians"),
        );
        for copy in [&restored, &store.clone()] {
            assert_eq!(copy.owned_ids(americans, EntityCategory::Unit), [2, 4]);
            assert_eq!(copy.owned_ids(russians, EntityCategory::Unit), [7]);
        }
        let airborne: Vec<u64> = restored.airborne().map(GameEntity::stable_id).collect();
        assert_eq!(
            airborne,
            [4, 2],
            "tracked entities come back in enter order"
        );
        assert_eq!(
            crate::util::save_codec::serialize(&restored).unwrap(),
            saved
        );
    }

    #[test]
    fn building_registry_tracks_class_replacement_removal_clone_and_restore() {
        let mut store = EntityStore::new();
        let mut building = make_entity(7);
        building.set_category_for_test(EntityCategory::Structure);
        store.insert(building);
        store.insert(make_entity(2));
        let mut earlier = make_entity(3);
        earlier.set_category_for_test(EntityCategory::Structure);
        store.insert(earlier);
        store
            .get_mut(3)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(false);
        assert_eq!(
            store.building_registry,
            [3, 7],
            "UnInit does not remove a class row"
        );
        store.insert(make_entity(3));
        assert_eq!(store.building_registry, [7]);
        let mut replacement = make_entity(2);
        replacement.set_category_for_test(EntityCategory::Structure);
        store.insert(replacement);
        assert_eq!(store.building_registry_at(0), Some(2));
        store.remove(2);
        assert_eq!(store.building_registry_at(0), Some(7));
        assert_eq!(store.building_registry_at(1), None);
        let restored: EntityStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&store).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.building_registry, store.building_registry);
        assert_eq!(store.clone().building_registry, store.building_registry);
    }

    #[test]
    fn infantry_registry_retains_uninit_compacts_and_rebuilds_after_restore() {
        use crate::map::entities::EntityCategory;
        let mut store = EntityStore::new();
        for id in [1, 2, 3] {
            let mut e = make_entity(id);
            e.set_category_for_test(EntityCategory::Infantry);
            store.insert(e);
        }
        store
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(false);
        store
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        assert_eq!(store.infantry_registry_at(0), Some(1));
        store.remove(1);
        // A caller that just visited index0 next reads index1, skipping2.
        assert_eq!(store.infantry_registry_at(1), Some(3));
        let mut appended = make_entity(4);
        appended.set_category_for_test(EntityCategory::Infantry);
        store.insert(appended);
        assert_eq!(store.infantry_registry_at(2), Some(4));
        // Replacement through the store updates category membership too.
        store.insert(make_entity(3));
        assert_eq!(store.infantry_registry, [2, 4]);
        let restored: EntityStore =
            serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        assert_eq!(restored.infantry_registry, [2, 4]);
        assert_eq!(restored.infantry_registry_at(2), None);
        assert_eq!(store.foot_registry, [2, 3, 4]);
        assert_eq!(restored.foot_registry, store.foot_registry);
        let mut building = make_entity(3);
        building.set_category_for_test(EntityCategory::Structure);
        store.insert(building);
        assert_eq!(store.foot_registry, [2, 4]);
    }

    /// An Anim's expiry reaches each stored entity holding the Anim, once
    /// and ascending, whether a held-Anim write stored it, the entity was
    /// stored holding it or a load restored it, and no removed holder.
    #[test]
    fn anim_holders_reach_every_holder_through_writes_inserts_and_loads() {
        let hold = |entity: &mut GameEntity, authority: AnimHolderAuthority| {
            entity.hold_parachute_animation(50, authority)
        };
        let mut store = EntityStore::new();
        let mut stored_holding = make_entity(3);
        hold(&mut stored_holding, AnimHolderAuthority(()));
        store.insert(stored_holding);
        for id in [1, 2, 4] {
            store.insert(make_entity(id));
        }
        for id in [1, 4] {
            store.write_held_anim(id, hold).unwrap();
        }
        store.remove(4);
        let loaded: EntityStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&store).unwrap(),
        )
        .unwrap();
        for mut copy in [store.clone(), loaded] {
            assert_eq!(copy.take_anim_holders(50), [1, 3]);
        }
    }

    /// A focused id answers as the id index does while its slot moves under
    /// it (an id stored below it, a compaction), when it is removed and
    /// stored again, and when the store never held it; a load answers for
    /// its own ids.
    #[test]
    fn a_focused_id_answers_as_the_id_index_does_while_its_slot_moves() {
        let mut store = EntityStore::new();
        for id in (10..=400).step_by(2) {
            store.insert(make_entity(id));
        }
        let held = |store: &mut EntityStore, id: u64| {
            let found = [
                store.get(id).map(GameEntity::stable_id),
                store.get_mut(id).map(|entity| entity.stable_id()),
                store.get_in_slot(id).map(|(_, entity)| entity.stable_id()),
            ];
            assert_eq!(found, [store.contains(id).then_some(id); 3]);
            assert_eq!(store.owner_of(id).is_some(), store.contains(id));
            assert_eq!(store.category_of(id).is_some(), store.contains(id));
            store.contains(id)
        };
        store.focus(300);
        assert!(held(&mut store, 300));
        // An id stored below it shifts its slot by one.
        store.insert(make_entity(11));
        assert!(held(&mut store, 300));
        // The removal that compacts the store moves it again.
        let slots = store.ids.len();
        let mut below = (10..200).step_by(2);
        while store.ids.len() == slots {
            store.remove(below.next().expect("a removal compacts the store"));
        }
        assert!(held(&mut store, 300));
        store.remove(300);
        assert!(!held(&mut store, 300));
        store.insert(make_entity(300));
        assert!(held(&mut store, 300));
        // An id the store never held is absent until it is stored.
        store.focus(7);
        assert!(!held(&mut store, 7));
        store.insert(make_entity(7));
        assert!(held(&mut store, 7) && held(&mut store, 300) && !held(&mut store, 10));
        let mut loaded: EntityStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&store).unwrap(),
        )
        .unwrap();
        let mut copy = store.clone();
        for other in [&mut loaded, &mut copy] {
            assert!(held(other, 7) && held(other, 300) && !held(other, 0));
        }
    }

    #[test]
    fn clear_radio_contacts_removes_reciprocal_contacts() {
        let mut store = EntityStore::new();
        let mut first = make_entity(1);
        let mut second = make_entity(2);
        first.mark_live_contact_with(2);
        second.mark_live_contact_with(1);
        store.insert(first);
        store.insert(second);

        store.clear_radio_contacts_for(1);

        assert!(store.get(1).unwrap().radio_contacts.is_empty());
        assert!(store.get(2).unwrap().radio_contacts.is_empty());
    }

    #[test]
    fn clear_radio_contacts_preserves_remaining_order() {
        let mut store = EntityStore::new();
        let mut entity = make_entity(1);
        entity.radio_contacts.set_capacity(4); // hold more than one contact
        entity.mark_live_contact_with(2);
        entity.mark_live_contact_with(3);
        entity.mark_live_contact_with(4);
        store.insert(entity);
        store.insert(make_entity(3));

        store.clear_radio_contacts_for(3);

        // Removal nulls slot 1 in place (no compaction): 2 and 4 keep their slots.
        let contacts = &store.get(1).unwrap().radio_contacts;
        assert_eq!(contacts.len(), 2);
        assert!(contacts.contains(2) && contacts.contains(4) && !contacts.contains(3));
        assert_eq!(contacts.find_slot(2), Some(0));
        assert_eq!(contacts.find_slot(4), Some(2));
    }
}
