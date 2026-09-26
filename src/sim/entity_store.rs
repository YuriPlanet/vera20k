//! BTreeMap-backed entity storage with deterministic sorted iteration.
//!
//! `EntityStore` replaces `hecs::World` as the container for all game entities.
//! Entities are keyed by their stable_id (u64) for O(log n) lookup. BTreeMap
//! provides deterministic sorted iteration natively — no manual cache needed.
//!
//! ## Borrow patterns
//! - Single entity mutation: `store.get_mut(id)` borrows only that entry
//! - Cross-entity reads during mutation: read target first (clone needed data),
//!   then get_mut on the other entity
//! - Batch iteration with mutation: collect `keys_sorted()`, loop with `get_mut()`
//! - One entity mutated while it reads the others live: `store.take_turn(id)`
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on sim/game_entity.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeMap;

use crate::sim::game_entity::GameEntity;
use crate::sim::touch_log::{TouchLog, Touched};

/// Only this module can authorize a live indexed-owner write.
/// Payload consumers can read owner identity but cannot construct this capability.
pub(crate) struct OwnerChangeAuthority(());

/// One entity held out of the store for its turn; see [`EntityStore::take_turn`].
pub(crate) struct EntityTurn<'a> {
    store: &'a mut EntityStore,
    entity: Option<Box<GameEntity>>,
}

impl EntityTurn<'_> {
    /// The entity whose turn it is, and everyone else.
    pub(crate) fn split(&mut self) -> (&mut GameEntity, OtherEntities<'_>) {
        let entity = self.entity.as_mut().expect("held until drop");
        (
            entity,
            OtherEntities {
                store: &*self.store,
            },
        )
    }
}

impl Drop for EntityTurn<'_> {
    fn drop(&mut self) {
        if let Some(entity) = self.entity.take() {
            self.store.entities.insert(entity.stable_id(), entity);
        }
    }
}

/// Read access to the entities other than the one taking its turn. Lookup by
/// id only: the owner and type indexes still list the absent entity, so they
/// are not offered here.
#[derive(Clone, Copy)]
pub(crate) struct OtherEntities<'a> {
    store: &'a EntityStore,
}

impl<'a> OtherEntities<'a> {
    /// A store nobody is lifted out of. The reader must not expect to find the
    /// acting entity's own turn-start facts here; callers supply those apart.
    pub(crate) fn whole(store: &'a EntityStore) -> Self {
        Self { store }
    }

    pub(crate) fn get(&self, stable_id: u64) -> Option<&'a GameEntity> {
        self.store.entities.get(&stable_id).map(Box::as_ref)
    }
}

/// Container for all game entities, keyed by stable_id.
///
/// Uses `BTreeMap<u64, GameEntity>` for deterministic sorted iteration
/// and O(log n) lookup. All iteration methods return entities in
/// ascending stable_id order, which is critical for lockstep multiplayer.
///
/// Maintains a secondary per-owner index (`by_owner`) so that queries like
/// "all buildings owned by house X" are O(that house's entities) instead of
/// O(total entities). The index is maintained **incrementally**: `insert`,
/// `remove`, and `change_owner` keep it in sync, so `ids_for_owner()` is always
/// current with no rebuild needed. `rebuild_owner_index()` exists only for the
/// deserialize finalizer (the primary map is bulk-loaded, bypassing `insert`).
#[derive(Debug)]
pub struct EntityStore {
    /// Primary storage: stable_id -> GameEntity. Boxed: an entity is about 3 KB,
    /// and map nodes that hold pointers keep insert, remove and `take_turn` from
    /// moving entities around. serde writes a `Box<T>` as its `T`, so the saved
    /// form is unchanged.
    entities: BTreeMap<u64, Box<GameEntity>>,
    /// Derived InfantryClass registry, in monotonic construction-ID order.
    /// Uninit retains an entry; remove compacts it. Class/category is immutable
    /// while stored, like indexed identity (replace through remove/insert).
    infantry_registry: Vec<u64>,
    /// Per-owner index: owner InternedId -> ascending-stable_id Vec of ids.
    /// Maintained incrementally by `insert`/`remove`/`change_owner`. Emptied
    /// owners are dropped from the map so a wiped-out house's `ids_for_owner`
    /// returns `&[]`, identical to a fresh rebuild. Deterministic iteration via
    /// BTreeMap key order + sorted Vecs.
    by_owner: BTreeMap<crate::sim::intern::InternedId, Vec<u64>>,
    /// Per-(owner, type) instance count over the stored entities — the O(1)
    /// answer `HouseClass::CountOwnedInstances @ 0x0049FAE0` gives from its
    /// per-house per-type counter array. Maintained incrementally next to
    /// `by_owner` (`insert`/`remove`/`change_owner`) and rebuilt with it.
    /// Counts every stored entity of the type, limbo and dying included; see
    /// [`Self::count_owned_of_type`] for the native-vs-Rust window.
    by_owner_type: BTreeMap<
        (
            crate::sim::intern::InternedId,
            crate::sim::intern::InternedId,
        ),
        u32,
    >,
    /// Which entities may have changed since each reader last took its log
    /// ([`Self::take_touched`]). Every route to a `&mut GameEntity` goes
    /// through the store, so an entity missing from a log has not changed.
    touched: TouchLogs,
}

/// A product derived from the entities that re-derives from the touch log.
/// Each reads its own log, so one reader's take leaves the others' intact.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TouchReader {
    /// The movement pass's owner block index, which passes on what it takes
    /// to the blocker plane.
    BlockIndex = 0,
    /// The kept Ground display sort keys.
    GroundKeys = 1,
}

impl TouchReader {
    const COUNT: usize = 2;
}

#[derive(Debug, Clone)]
struct TouchLogs([TouchLog; TouchReader::COUNT]);

impl TouchLogs {
    fn everything() -> Self {
        Self(std::array::from_fn(|_| TouchLog::everything()))
    }

    fn note(&mut self, id: u64, stored: usize) {
        for log in &mut self.0 {
            log.note(id, stored);
        }
    }

    fn note_all(&mut self) {
        for log in &mut self.0 {
            log.note_all();
        }
    }
}

impl Clone for EntityStore {
    /// A clone starts with an everything-touched log: whatever was derived
    /// from the original says nothing certain about the copy's future.
    fn clone(&self) -> Self {
        Self {
            entities: self.entities.clone(),
            infantry_registry: self.infantry_registry.clone(),
            by_owner: self.by_owner.clone(),
            by_owner_type: self.by_owner_type.clone(),
            touched: TouchLogs::everything(),
        }
    }
}

impl EntityStore {
    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: TouchReader) -> Touched {
        self.touched.0[reader as usize].take()
    }

    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            infantry_registry: Vec::new(),
            by_owner: BTreeMap::new(),
            by_owner_type: BTreeMap::new(),
            touched: TouchLogs::everything(),
        }
    }

    /// Insert an entity. Returns its stable_id. Maintains the `by_owner` index.
    /// If an entity with the same id already existed (rare — stable_ids are
    /// monotonic), its old owner entry is removed first.
    pub fn insert(&mut self, entity: GameEntity) -> u64 {
        let id = entity.stable_id();
        self.touched.note(id, self.entities.len());
        let owner = entity.owner();
        let type_ref = entity.type_ref();
        let infantry = entity.category == crate::map::entities::EntityCategory::Infantry;
        if let Some(old) = self.entities.insert(id, Box::new(entity)) {
            self.index_remove(old.owner(), id);
            self.type_count_remove(old.owner(), old.type_ref());
        }
        self.remove_infantry_index(id);
        if infantry {
            let index = self
                .infantry_registry
                .partition_point(|&existing| existing < id);
            self.infantry_registry.insert(index, id);
        }
        self.index_add(owner, id);
        self.type_count_add(owner, type_ref);
        id
    }

    /// Remove an entity by stable_id. Returns the removed entity if it existed.
    /// Maintains the `by_owner` index.
    pub fn remove(&mut self, stable_id: u64) -> Option<GameEntity> {
        let removed = self.entities.remove(&stable_id).map(|entity| *entity);
        if removed.is_some() {
            self.touched.note(stable_id, self.entities.len());
        }
        if let Some(ref e) = removed {
            self.index_remove(e.owner(), stable_id);
            self.type_count_remove(e.owner(), e.type_ref());
            self.remove_infantry_index(stable_id);
        }
        removed
    }

    /// Stored instances of `type_ref` owned by `owner` — O(1).
    ///
    /// Native `HouseClass::CountOwnedInstances @ 0x0049FAE0` reads the
    /// house's per-type counter array (`House+0x4`/`+0x8`, grown on demand
    /// and zero-filled), which counts an object from Unlimbo until Limbo.
    /// This count runs from `insert` until `remove` instead, so it differs
    /// only at the edges: a stored-but-in-limbo entity counts here (native
    /// excludes it; production structures sit in limbo only inside the spawn
    /// call that reveals or discards them, so no dispatch observes the gap),
    /// while a dying structure counts in both until it leaves the store /
    /// Limbos. Storage-order/type-index details of the native array are not
    /// modelled; only the `> 0` test and the count itself are consumed.
    pub fn count_owned_of_type(
        &self,
        owner: crate::sim::intern::InternedId,
        type_ref: crate::sim::intern::InternedId,
    ) -> u32 {
        self.by_owner_type
            .get(&(owner, type_ref))
            .copied()
            .unwrap_or(0)
    }

    /// Clear all RadioClass-style live contacts involving `stable_id`.
    ///
    /// Idempotent. Safe if `stable_id` is absent.
    pub fn clear_radio_contacts_for(&mut self, stable_id: u64) {
        let stored = self.entities.len();
        self.touched.note(stable_id, stored);
        for entity in self.entities.values_mut() {
            if entity.has_live_contact_with(stable_id)
                || entity.dock_entered_with == Some(stable_id)
            {
                self.touched.note(entity.stable_id(), stored);
            }
            entity.clear_live_contact_with(stable_id);
            // Drop a dangling dock-entered link pointing at the departing entity
            // (the BREAK cascade a limbo'd dock partner would otherwise miss).
            if entity.dock_entered_with == Some(stable_id) {
                entity.dock_entered_with = None;
            }
            if entity.stable_id() == stable_id {
                entity.radio_contacts.clear_all();
                entity.dock_entered_with = None;
            }
        }
    }

    /// Look up an entity by stable_id (immutable).
    pub fn get(&self, stable_id: u64) -> Option<&GameEntity> {
        self.entities.get(&stable_id).map(Box::as_ref)
    }

    /// Mutate ordinary entity payload. Indexed identity is read through accessors;
    /// live ownership changes use the world lifecycle and `change_owner` below.
    /// Replacing a stored entity wholesale is unsupported: remove/insert through
    /// the owning lifecycle instead so its indexes and registrations are updated.
    pub fn get_mut(&mut self, stable_id: u64) -> Option<&mut GameEntity> {
        let stored = self.entities.len();
        let entity = self.entities.get_mut(&stable_id)?;
        self.touched.note(stable_id, stored);
        Some(entity.as_mut())
    }

    /// Lift one entity out of the store for its own turn, so it can be mutated
    /// while every other entity stays readable. The entity returns to the map
    /// when the guard drops, on every exit path. Its indexed identity (owner,
    /// type, infantry registry) never leaves the indexes, which is sound because
    /// payload access cannot change it, exactly as with `get_mut`.
    pub(crate) fn take_turn(&mut self, stable_id: u64) -> Option<EntityTurn<'_>> {
        let entity = self.entities.remove(&stable_id)?;
        self.touched.note(stable_id, self.entities.len());
        Some(EntityTurn {
            store: self,
            entity: Some(entity),
        })
    }

    /// Check if an entity exists.
    pub fn contains(&self, stable_id: u64) -> bool {
        self.entities.contains_key(&stable_id)
    }

    /// Number of entities in the store.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Get sorted keys for deterministic iteration.
    ///
    /// Callers typically iterate with `get()` or `get_mut()`:
    /// ```ignore
    /// let keys = store.keys_sorted();
    /// for &id in &keys {
    ///     if let Some(entity) = store.get_mut(id) { ... }
    /// }
    /// ```
    pub fn keys_sorted(&self) -> Vec<u64> {
        self.entities.keys().copied().collect()
    }

    /// Native InfantryClass array index. Read afresh after a callback: removing
    /// a row shifts successors left, while a newly constructed row is visible.
    /// This is distinct from the live Logic vector and includes dead/limbo rows.
    pub(crate) fn infantry_registry_at(&self, index: usize) -> Option<u64> {
        self.infantry_registry.get(index).copied()
    }

    fn remove_infantry_index(&mut self, id: u64) {
        if let Ok(index) = self.infantry_registry.binary_search(&id) {
            self.infantry_registry.remove(index);
        }
    }

    /// Iterate all entities in deterministic stable_id order (immutable).
    pub fn iter_sorted(&self) -> impl Iterator<Item = (u64, &GameEntity)> {
        self.entities.iter().map(|(&k, v)| (k, v.as_ref()))
    }

    /// Iterate all entity values in deterministic stable_id order (immutable).
    pub fn values_sorted(&self) -> impl Iterator<Item = &GameEntity> {
        self.entities.values().map(Box::as_ref)
    }

    /// Iterate all entities in stable_id order (immutable).
    /// With BTreeMap, this is always deterministic.
    pub fn values(&self) -> impl Iterator<Item = &GameEntity> {
        self.entities.values().map(Box::as_ref)
    }

    /// Iterate all entities mutably in stable_id order.
    /// With BTreeMap, this is always deterministic.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut GameEntity> {
        self.touched.note_all();
        self.entities.values_mut().map(Box::as_mut)
    }

    /// Stable IDs owned by the given owner, in sorted order.
    /// Returns an empty slice if the owner has no entities.
    /// O(1) lookup + O(n) iteration where n = that owner's entity count.
    pub fn ids_for_owner(&self, owner: crate::sim::intern::InternedId) -> &[u64] {
        self.by_owner.get(&owner).map_or(&[], |ids| ids.as_slice())
    }

    /// Move an entity to a new owner: updates `entity.owner` AND the `by_owner`
    /// index together. Index only — does NOT touch the houses' tracking
    /// counts; `Simulation::change_owner`, the only production caller, moves
    /// them.
    /// No-op if the entity is absent or already owned by `new_owner`.
    pub fn change_owner(&mut self, stable_id: u64, new_owner: crate::sim::intern::InternedId) {
        self.touched.note(stable_id, self.entities.len());
        let (old_owner, type_ref) = match self.entities.get_mut(&stable_id) {
            Some(e) if e.owner() != new_owner => {
                let old = e.owner();
                e.set_owner_from_store(new_owner, OwnerChangeAuthority(()));
                (old, e.type_ref())
            }
            _ => return,
        };
        self.index_remove(old_owner, stable_id);
        self.index_add(new_owner, stable_id);
        self.type_count_remove(old_owner, type_ref);
        self.type_count_add(new_owner, type_ref);
    }

    fn type_count_add(
        &mut self,
        owner: crate::sim::intern::InternedId,
        type_ref: crate::sim::intern::InternedId,
    ) {
        *self.by_owner_type.entry((owner, type_ref)).or_insert(0) += 1;
    }

    /// Drop the entry when it reaches zero so the map matches a fresh rebuild.
    fn type_count_remove(
        &mut self,
        owner: crate::sim::intern::InternedId,
        type_ref: crate::sim::intern::InternedId,
    ) {
        if let Some(count) = self.by_owner_type.get_mut(&(owner, type_ref)) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.by_owner_type.remove(&(owner, type_ref));
            }
        }
    }

    /// Insert `id` into its owner bucket at the sorted (ascending) position.
    fn index_add(&mut self, owner: crate::sim::intern::InternedId, id: u64) {
        let v = self.by_owner.entry(owner).or_default();
        let pos = v.partition_point(|&x| x < id);
        v.insert(pos, id);
    }

    /// Remove `id` from its owner bucket; drop the bucket if it empties (so the
    /// map matches a fresh rebuild, which never stores empty owners).
    fn index_remove(&mut self, owner: crate::sim::intern::InternedId, id: u64) {
        if let Some(v) = self.by_owner.get_mut(&owner) {
            if let Ok(pos) = v.binary_search(&id) {
                v.remove(pos);
            }
            if v.is_empty() {
                self.by_owner.remove(&owner);
            }
        }
    }

    /// Rebuild the per-owner index from primary storage.
    /// Owned by Deserialize; synthetic fixtures may also rebuild after raw setup.
    pub(crate) fn rebuild_owner_index(&mut self) {
        self.by_owner.clear();
        self.by_owner_type.clear();
        self.infantry_registry.clear();
        for (&id, entity) in &self.entities {
            if entity.category == crate::map::entities::EntityCategory::Infantry {
                self.infantry_registry.push(id);
            }
            self.by_owner.entry(entity.owner()).or_default().push(id);
            *self
                .by_owner_type
                .entry((entity.owner(), entity.type_ref()))
                .or_insert(0) += 1;
        }
        // BTreeMap iteration is already sorted by key; Vecs are sorted because
        // entities BTreeMap iterates in ascending stable_id order.
    }
}

impl serde::Serialize for EntityStore {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.entities.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for EntityStore {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entities = BTreeMap::<u64, Box<GameEntity>>::deserialize(deserializer)?;
        let mut store = Self {
            entities,
            infantry_registry: Vec::new(),
            by_owner: BTreeMap::new(),
            by_owner_type: BTreeMap::new(),
            touched: TouchLogs::everything(),
        };
        store.rebuild_owner_index();
        Ok(store)
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;

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
        assert_eq!(store.take_touched(TouchReader::BlockIndex), Touched::All);
        assert_eq!(store.take_touched(TouchReader::GroundKeys), Touched::All);
        store.get_mut(2).unwrap().position.rx += 1;
        drop(store.take_turn(3));
        assert_eq!(
            store.take_touched(TouchReader::BlockIndex),
            Touched::Ids(vec![2, 3])
        );
        store.get_mut(1);
        assert_eq!(
            store.take_touched(TouchReader::GroundKeys),
            Touched::Ids(vec![2, 3, 1])
        );
        assert_eq!(
            store.take_touched(TouchReader::BlockIndex),
            Touched::Ids(vec![1])
        );
        let mut copy = store.clone();
        assert_eq!(copy.take_touched(TouchReader::GroundKeys), Touched::All);
        assert_eq!(copy.take_touched(TouchReader::BlockIndex), Touched::All);
        let _ = store.values_mut().count();
        assert_eq!(store.take_touched(TouchReader::GroundKeys), Touched::All);
        assert_eq!(store.take_touched(TouchReader::BlockIndex), Touched::All);
    }

    #[test]
    fn a_turn_lifts_one_entity_out_and_returns_it_on_every_exit() {
        let mut store = EntityStore::new();
        for id in [1, 2, 3] {
            store.insert(make_entity(id));
        }
        let owner = store.get(2).unwrap().owner();
        let type_ref = store.get(2).unwrap().type_ref();
        {
            let mut turn = store.take_turn(2).expect("entity 2 is stored");
            let (entity, others) = turn.split();
            // The others are readable while the mover is mutated; the mover
            // itself is not among them.
            entity.position.rx = others.get(1).unwrap().position.rx + 7;
            assert!(others.get(2).is_none());
            assert!(others.get(3).is_some());
        }
        assert_eq!(store.get(2).unwrap().position.rx, 17);
        assert_eq!(store.len(), 3);
        // Indexed identity never left the indexes.
        assert_eq!(store.ids_for_owner(owner), [1, 2, 3]);
        assert_eq!(store.count_owned_of_type(owner, type_ref), 3);

        fn leaves_early(store: &mut EntityStore) -> Option<()> {
            let mut turn = store.take_turn(3)?;
            let (entity, _) = turn.split();
            entity.position.ry = 99;
            None
        }
        assert!(leaves_early(&mut store).is_none());
        assert_eq!(store.get(3).unwrap().position.ry, 99);
        assert!(store.take_turn(42).is_none());
        assert_eq!(store.keys_sorted(), [1, 2, 3]);
    }

    #[test]
    fn infantry_registry_retains_uninit_compacts_and_rebuilds_after_restore() {
        use crate::map::entities::EntityCategory;
        let mut store = EntityStore::new();
        for id in [1, 2, 3] {
            let mut e = make_entity(id);
            e.category = EntityCategory::Infantry;
            store.insert(e);
        }
        store.get_mut(1).unwrap().lifecycle.object_alive = false;
        store.get_mut(1).unwrap().lifecycle.in_limbo = true;
        assert_eq!(store.infantry_registry_at(0), Some(1));
        store.remove(1);
        // A caller that just visited index0 next reads index1, skipping2.
        assert_eq!(store.infantry_registry_at(1), Some(3));
        let mut appended = make_entity(4);
        appended.category = EntityCategory::Infantry;
        store.insert(appended);
        assert_eq!(store.infantry_registry_at(2), Some(4));
        // Replacement through the store updates category membership too.
        store.insert(make_entity(3));
        assert_eq!(store.infantry_registry, [2, 4]);
        let restored: EntityStore =
            serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        assert_eq!(restored.infantry_registry, [2, 4]);
        assert_eq!(restored.infantry_registry_at(2), None);
    }

    #[test]
    fn test_insert_and_get() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        store.insert(make_entity(2));

        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
        assert!(store.contains(1));
        assert!(store.contains(2));
        assert!(!store.contains(3));

        let e = store.get(1).expect("entity 1 should exist");
        assert_eq!(e.stable_id, 1);
    }

    #[test]
    fn test_get_mut() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));

        let e = store.get_mut(1).expect("entity 1 should exist");
        e.health.current = 50;

        let e = store.get(1).expect("entity 1 should exist");
        assert_eq!(e.health.current, 50);
    }

    #[test]
    fn test_remove() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        store.insert(make_entity(2));

        let removed = store.remove(1);
        assert!(removed.is_some());
        assert_eq!(removed.expect("should be Some").stable_id, 1);
        assert_eq!(store.len(), 1);
        assert!(!store.contains(1));
        assert!(store.contains(2));

        // Removing non-existent ID returns None.
        assert!(store.remove(99).is_none());
    }

    #[test]
    fn clear_radio_contacts_removes_one_sided_peer_contact() {
        let mut store = EntityStore::new();
        let mut entity = make_entity(1);
        entity.mark_live_contact_with(2);
        store.insert(entity);
        store.insert(make_entity(2));

        store.clear_radio_contacts_for(2);

        assert!(store.get(1).unwrap().radio_contacts.is_empty());
        assert!(store.get(2).unwrap().radio_contacts.is_empty());
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
    fn clear_radio_contacts_missing_id_preserves_unrelated_contacts() {
        let mut store = EntityStore::new();
        let mut entity = make_entity(1);
        entity.radio_contacts.set_capacity(4); // hold more than one contact
        entity.mark_live_contact_with(2);
        entity.mark_live_contact_with(3);
        store.insert(entity);

        store.clear_radio_contacts_for(99);

        let contacts = &store.get(1).unwrap().radio_contacts;
        assert_eq!(contacts.len(), 2);
        assert!(contacts.contains(2) && contacts.contains(3));
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

    #[test]
    fn test_deterministic_iteration_order() {
        let mut store = EntityStore::new();
        // Insert in non-sorted order.
        store.insert(make_entity(5));
        store.insert(make_entity(1));
        store.insert(make_entity(3));
        store.insert(make_entity(2));
        store.insert(make_entity(4));

        let keys: Vec<u64> = store.keys_sorted();
        assert_eq!(keys, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_iter_sorted() {
        let mut store = EntityStore::new();
        store.insert(make_entity(3));
        store.insert(make_entity(1));
        store.insert(make_entity(2));

        let ids: Vec<u64> = store.iter_sorted().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_values_sorted() {
        let mut store = EntityStore::new();
        store.insert(make_entity(3));
        store.insert(make_entity(1));
        store.insert(make_entity(2));

        let ids: Vec<u64> = store.values_sorted().map(|e| e.stable_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_sorted_after_mutation() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        store.insert(make_entity(3));

        let keys: Vec<u64> = store.keys_sorted();
        assert_eq!(keys, vec![1, 3]);

        // Insert maintains order.
        store.insert(make_entity(2));
        let keys: Vec<u64> = store.keys_sorted();
        assert_eq!(keys, vec![1, 2, 3]);

        // Remove maintains order.
        store.remove(1);
        let keys: Vec<u64> = store.keys_sorted();
        assert_eq!(keys, vec![2, 3]);
    }

    #[test]
    fn test_empty_store() {
        let store = EntityStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert!(store.get(1).is_none());

        let keys: Vec<u64> = store.keys_sorted();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_mutable_iteration_pattern() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        store.insert(make_entity(2));
        store.insert(make_entity(3));

        // The canonical pattern for mutating during iteration:
        // collect keys, then get_mut each entity.
        let keys = store.keys_sorted();
        for &id in &keys {
            if let Some(entity) = store.get_mut(id) {
                entity.health.current = entity.health.current.saturating_sub(10);
            }
        }

        // Verify all were mutated.
        for &id in &[1u64, 2, 3] {
            let e = store.get(id).expect("should exist");
            assert_eq!(e.health.current, 90);
        }
    }

    #[test]
    fn test_cross_entity_read_pattern() {
        let mut store = EntityStore::new();
        let mut e1 = make_entity(1);
        e1.position.rx = 10;
        e1.position.ry = 20;
        store.insert(e1);

        let mut e2 = make_entity(2);
        e2.position.rx = 30;
        e2.position.ry = 40;
        store.insert(e2);

        // Read target position first (immutable borrow ends).
        let target_pos = store.get(2).map(|e| e.position.clone());
        // Then mutate attacker (no conflict).
        if let (Some(attacker), Some(pos)) = (store.get_mut(1), target_pos) {
            // In real code: compute firing direction, apply cooldown, etc.
            assert_eq!(pos.rx, 30);
            attacker.facing = 128; // face toward target
        }

        assert_eq!(store.get(1).expect("should exist").facing, 128);
    }

    #[test]
    fn test_per_owner_index() {
        use crate::sim::intern::StringInterner;

        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");
        let soviets = interner.intern("Russians");

        let mut store = EntityStore::new();

        let mut e1 = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        e1.owner = americans;
        let mut e2 = GameEntity::test_default(2, "MTNK", "Americans", 6, 6);
        e2.owner = americans;
        let mut e3 = GameEntity::test_default(3, "RHNO", "Russians", 10, 10);
        e3.owner = soviets;

        store.insert(e1);
        store.insert(e3);
        store.insert(e2);
        store.rebuild_owner_index();

        // Americans should have [1, 2] sorted.
        assert_eq!(store.ids_for_owner(americans), &[1, 2]);
        // Russians should have [3].
        assert_eq!(store.ids_for_owner(soviets), &[3]);

        // Remove one American entity, rebuild.
        store.remove(1);
        store.rebuild_owner_index();
        assert_eq!(store.ids_for_owner(americans), &[2]);
        assert_eq!(store.ids_for_owner(soviets), &[3]);

        // Remove all American entities, rebuild.
        store.remove(2);
        store.rebuild_owner_index();
        assert_eq!(store.ids_for_owner(americans), &[] as &[u64]);

        // Unknown owner returns empty slice.
        let unknown = interner.intern("Yuri");
        assert_eq!(store.ids_for_owner(unknown), &[] as &[u64]);
    }

    #[test]
    fn per_owner_type_count_tracks_insert_remove_change_owner_and_rebuild() {
        use crate::sim::intern::StringInterner;
        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");
        let soviets = interner.intern("Russians");
        let refinery = interner.intern("GAREFN");
        let tank = interner.intern("HTNK");
        let mut store = EntityStore::new();

        let mut e1 = GameEntity::test_default(1, "GAREFN", "Americans", 5, 5);
        e1.owner = americans;
        e1.type_ref = refinery;
        let mut e2 = GameEntity::test_default(2, "GAREFN", "Americans", 6, 6);
        e2.owner = americans;
        e2.type_ref = refinery;
        let mut e3 = GameEntity::test_default(3, "HTNK", "Americans", 7, 7);
        e3.owner = americans;
        e3.type_ref = tank;
        store.insert(e1);
        store.insert(e2);
        store.insert(e3);
        assert_eq!(store.count_owned_of_type(americans, refinery), 2);
        assert_eq!(store.count_owned_of_type(americans, tank), 1);
        assert_eq!(store.count_owned_of_type(soviets, refinery), 0);

        store.change_owner(2, soviets);
        assert_eq!(store.count_owned_of_type(americans, refinery), 1);
        assert_eq!(store.count_owned_of_type(soviets, refinery), 1);

        store.remove(1);
        assert_eq!(store.count_owned_of_type(americans, refinery), 0);
        assert!(!store.by_owner_type.contains_key(&(americans, refinery)));

        // Re-inserting an existing id retires the old entry's count first.
        let mut e2b = GameEntity::test_default(2, "HTNK", "Russians", 6, 6);
        e2b.owner = soviets;
        e2b.type_ref = tank;
        store.insert(e2b);
        assert_eq!(store.count_owned_of_type(soviets, refinery), 0);
        assert_eq!(store.count_owned_of_type(soviets, tank), 1);

        let before = store.by_owner_type.clone();
        store.rebuild_owner_index();
        assert_eq!(
            store.by_owner_type, before,
            "rebuild reproduces the incremental map"
        );
    }

    #[test]
    fn insert_indexes_immediately() {
        use crate::sim::intern::StringInterner;
        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");
        let mut store = EntityStore::new();
        let mut e = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        e.owner = americans;
        store.insert(e);
        // No rebuild: the index is current right after insert.
        assert_eq!(store.ids_for_owner(americans), &[1]);
    }

    #[test]
    fn remove_deindexes_immediately() {
        use crate::sim::intern::StringInterner;
        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");
        let mut store = EntityStore::new();
        let mut e = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        e.owner = americans;
        store.insert(e);
        store.remove(1);
        // Bucket emptied → owner dropped, identical to a fresh rebuild.
        assert_eq!(store.ids_for_owner(americans), &[] as &[u64]);
    }

    #[test]
    fn change_owner_moves_entry_immediately_and_is_idempotent() {
        use crate::sim::intern::StringInterner;
        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");
        let soviets = interner.intern("Russians");
        let mut store = EntityStore::new();
        let mut e = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        e.owner = americans;
        store.insert(e);

        store.change_owner(1, soviets);
        assert_eq!(store.ids_for_owner(americans), &[] as &[u64]);
        assert_eq!(store.ids_for_owner(soviets), &[1]);
        assert_eq!(store.get(1).unwrap().owner, soviets);

        // Same-owner call is a no-op (no duplicate in the bucket).
        store.change_owner(1, soviets);
        assert_eq!(store.ids_for_owner(soviets), &[1]);

        // Missing id is a no-op.
        store.change_owner(999, americans);
        assert_eq!(store.ids_for_owner(americans), &[] as &[u64]);
    }

    #[test]
    fn change_owner_preserves_sorted_order_in_both_buckets() {
        use crate::sim::intern::StringInterner;
        let mut interner = StringInterner::new();
        let a = interner.intern("Americans");
        let b = interner.intern("Russians");
        let mut store = EntityStore::new();
        for id in [10u64, 20, 30] {
            let mut e = GameEntity::test_default(id, "HTNK", "Americans", 5, 5);
            e.owner = a;
            store.insert(e);
        }
        let mut e = GameEntity::test_default(15, "RHNO", "Russians", 6, 6);
        e.owner = b;
        store.insert(e);
        // Move 20 from a→b; both buckets must stay ascending.
        store.change_owner(20, b);
        assert_eq!(store.ids_for_owner(a), &[10, 30]);
        assert_eq!(store.ids_for_owner(b), &[15, 20]);
    }

    /// Acceptance: a store built purely by incremental ops has a `by_owner`
    /// byte-identical to one produced by a full rebuild — proving
    /// deserialize-rebuild ≡ incremental.
    #[test]
    fn incremental_index_matches_rebuild() {
        use crate::sim::intern::StringInterner;
        let mut interner = StringInterner::new();
        let a = interner.intern("Americans");
        let b = interner.intern("Russians");
        let c = interner.intern("Yuri");
        let mut store = EntityStore::new();
        for (id, owner) in [(5u64, a), (1, b), (3, a), (2, c), (4, b)] {
            let mut e = GameEntity::test_default(id, "HTNK", "Americans", 5, 5);
            e.owner = owner;
            store.insert(e);
        }
        store.change_owner(3, b); // a→b
        store.change_owner(2, a); // c→a (empties c)
        store.remove(5); // drops from a
        let incremental = store.by_owner.clone();
        store.rebuild_owner_index();
        assert_eq!(incremental, store.by_owner);
    }

    #[test]
    fn test_rebuild_owner_index() {
        use crate::sim::intern::StringInterner;

        let mut interner = StringInterner::new();
        let americans = interner.intern("Americans");

        let mut store = EntityStore::new();
        let mut e1 = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        e1.owner = americans;
        let mut e2 = GameEntity::test_default(2, "MTNK", "Americans", 6, 6);
        e2.owner = americans;
        store.insert(e1);
        store.insert(e2);

        // Manually clear the index to simulate deserialization state.
        store.by_owner.clear();
        assert_eq!(store.ids_for_owner(americans), &[] as &[u64]);

        // Rebuild should restore the index.
        store.rebuild_owner_index();
        assert_eq!(store.ids_for_owner(americans), &[1, 2]);
    }
}
