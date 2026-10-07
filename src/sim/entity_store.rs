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
//! - Batch iteration with mutation: collect `keys_sorted()`, loop with
//!   `get_mut_if()` so an entity the walk leaves unchanged is not handed out
//!   (every hand-out enters the touch logs; see `touched`)
//! - One entity held out of the store for its turn: `store.take_turn(id)`
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
    /// The entity whose turn it is.
    pub(crate) fn entity(&mut self) -> &mut GameEntity {
        self.entity.as_mut().expect("held until drop")
    }
}

impl Drop for EntityTurn<'_> {
    fn drop(&mut self) {
        if let Some(entity) = self.entity.take() {
            self.store.entities.insert(entity.stable_id(), entity);
        }
    }
}

/// Container for all game entities, keyed by stable_id.
///
/// Uses `BTreeMap<u64, GameEntity>` for deterministic sorted iteration
/// and O(log n) lookup. All iteration methods return entities in
/// ascending stable_id order, which is critical for lockstep multiplayer.
///
/// A House's buildings in native order are its House+0x68 list
/// (`HouseBaseState::buildings`); its per-type counts are its
/// `HouseTracking`.
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
struct TouchLogs {
    logs: [TouchLog; TouchReader::COUNT],
    /// Every hand-out so far, for test checks that a pass handed nothing out
    /// ([`EntityStore::hand_outs`]).
    #[cfg(test)]
    hand_outs: u64,
}

impl TouchLogs {
    fn everything() -> Self {
        Self {
            logs: std::array::from_fn(|_| TouchLog::everything()),
            #[cfg(test)]
            hand_outs: 0,
        }
    }

    fn note(&mut self, id: u64, stored: usize) {
        #[cfg(test)]
        {
            self.hand_outs += 1;
        }
        for log in &mut self.logs {
            log.note(id, stored);
        }
    }

    fn note_all(&mut self) {
        #[cfg(test)]
        {
            self.hand_outs += 1;
        }
        for log in &mut self.logs {
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
            touched: TouchLogs::everything(),
        }
    }
}

impl EntityStore {
    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: TouchReader) -> Touched {
        self.touched.logs[reader as usize].take()
    }

    /// How many mutable hand-outs the store has made (test builds only).
    #[cfg(test)]
    pub(crate) fn hand_outs(&self) -> u64 {
        self.touched.hand_outs
    }

    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            infantry_registry: Vec::new(),
            touched: TouchLogs::everything(),
        }
    }

    /// Insert an entity, replacing one of the same id (rare — stable_ids are
    /// monotonic). Returns its stable_id.
    pub fn insert(&mut self, entity: GameEntity) -> u64 {
        let id = entity.stable_id();
        self.touched.note(id, self.entities.len());
        let infantry = entity.category == crate::map::entities::EntityCategory::Infantry;
        self.entities.insert(id, Box::new(entity));
        self.remove_infantry_index(id);
        if infantry {
            let index = self
                .infantry_registry
                .partition_point(|&existing| existing < id);
            self.infantry_registry.insert(index, id);
        }
        id
    }

    /// Remove an entity by stable_id. Returns the removed entity if it existed.
    pub fn remove(&mut self, stable_id: u64) -> Option<GameEntity> {
        let removed = self.entities.remove(&stable_id).map(|entity| *entity);
        if removed.is_some() {
            self.touched.note(stable_id, self.entities.len());
        }
        if removed.is_some() {
            self.remove_infantry_index(stable_id);
        }
        removed
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

    /// `get_mut` for an entity `admit` accepts, which it reads first: one it
    /// refuses is not handed out, so it stays out of the touch logs. For walks
    /// that visit every entity and change few.
    pub(crate) fn get_mut_if(
        &mut self,
        stable_id: u64,
        admit: impl FnOnce(&GameEntity) -> bool,
    ) -> Option<&mut GameEntity> {
        let stored = self.entities.len();
        let entity = self.entities.get_mut(&stable_id)?;
        if !admit(entity) {
            return None;
        }
        self.touched.note(stable_id, stored);
        Some(entity.as_mut())
    }

    /// Lift one entity out of the store for its own turn. The entity returns
    /// to the map when the guard drops, on every exit path. Its indexed identity (owner,
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
    /// Callers typically iterate with `get()` or `get_mut_if()`:
    /// ```ignore
    /// let keys = store.keys_sorted();
    /// for &id in &keys {
    ///     if let Some(entity) = store.get_mut_if(id, |entity| entity.rocking.is_some()) { ... }
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
    pub fn iter_sorted(&self) -> impl DoubleEndedIterator<Item = (u64, &GameEntity)> {
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

    /// Move an entity to a new owner. It does NOT touch the houses' tracking
    /// or lists; `Simulation::change_owner`, the only production caller,
    /// moves them.
    /// No-op if the entity is absent or already owned by `new_owner`.
    pub fn change_owner(&mut self, stable_id: u64, new_owner: crate::sim::intern::InternedId) {
        self.touched.note(stable_id, self.entities.len());
        if let Some(e) = self.entities.get_mut(&stable_id)
            && e.owner() != new_owner
        {
            e.set_owner_from_store(new_owner, OwnerChangeAuthority(()));
        }
    }

    /// Rebuild the infantry registry from primary storage. Owned by
    /// Deserialize; synthetic fixtures may also rebuild after raw setup.
    pub(crate) fn rebuild_infantry_registry(&mut self) {
        self.infantry_registry.clear();
        for (&id, entity) in &self.entities {
            if entity.category == crate::map::entities::EntityCategory::Infantry {
                self.infantry_registry.push(id);
            }
        }
        // The entities BTreeMap iterates in ascending stable_id order, so the
        // infantry registry comes out sorted.
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
            touched: TouchLogs::everything(),
        };
        store.rebuild_infantry_registry();
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
        {
            let mut turn = store.take_turn(2).expect("entity 2 is stored");
            turn.entity().position.rx = 17;
        }
        assert_eq!(store.get(2).unwrap().position.rx, 17);
        assert_eq!(store.len(), 3);

        fn leaves_early(store: &mut EntityStore) -> Option<()> {
            let mut turn = store.take_turn(3)?;
            turn.entity().position.ry = 99;
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
        let target_pos = store.get(2).map(|e| e.position);
        // Then mutate attacker (no conflict).
        if let (Some(attacker), Some(pos)) = (store.get_mut(1), target_pos) {
            // In real code: compute firing direction, apply cooldown, etc.
            assert_eq!(pos.rx, 30);
            attacker.body_facing.snap(0x8000, 0); // face toward target
        }

        assert_eq!(
            store
                .get(1)
                .expect("should exist")
                .body_facing
                .destination(),
            0x8000
        );
    }
}
