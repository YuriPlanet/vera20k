//! Shared object storage, registration order and spatial occupation authority.
//!
//! ObjectSubstrate owns the entity, animation, voxel-animation and particle-system
//! stores, independent Logic/display vectors, counters and occupation planes.
//! Terrain, projectile and wave stores remain in their mechanism owners and
//! participate in the same lifecycle dispatch and global object-ID namespace.
//!
//! Lifecycle operations live on Simulation because Reveal/Conceal/UnInit also
//! coordinate houses, terrain and other mechanism state. Storage order, active
//! Logic order and per-cell object order are distinct contracts. Field comments
//! identify serialized authority versus caches rebuilt after snapshot loading.
//!
//! Dependency rules: part of sim/; no presentation, audio or network dependency.

use serde::{Deserialize, Serialize};
use std::hash::Hash;

use super::LogicVector;
use crate::sim::anim_class::AnimStore;
use crate::sim::cell_rect::CellReservationGrid;
use crate::sim::entity_store::EntityStore;
use crate::sim::occupancy::{
    AirSlotGrid, HiddenOccupationGrid, OccupancyGrid, RawCellOccupationGrid,
};
use crate::sim::particles::ParticleSystemStore;
use crate::sim::voxel_anim::VoxelAnimStore;

/// One retained identity in original AbstractClass deferred queue B0F698.
/// Tag/Trigger ordinals belong to their own registries; Object/Overlay stable
/// IDs share the substrate allocator but retain distinct destructor owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DeferredDeleteTicket {
    Object(u64),
    Overlay(u64),
    Tag(usize),
    Trigger(usize),
    DiskLaser(u64),
}

/// Serialized, ordered authority for every725C70 enqueue. Registry membership
/// and readiness belong to the corresponding object owners; this queue owns
/// admission order and duplicate removal, including destructor-added entries.
#[derive(Debug, Clone, Default)]
pub struct DeferredDeleteQueue {
    entries: Vec<Option<DeferredDeleteTicket>>,
    /// Derived identity→storage positions; updated at every append/removal.
    positions: std::collections::BTreeMap<DeferredDeleteTicket, Vec<usize>>,
    /// Derived Fenwick prefix counts select a native logical cursor in O(log N).
    /// Storage holes never affect queue order, saves or hashes. Rebuilt after
    /// compaction/load; no family-specific queue or queue shadow exists.
    prefix_counts: Vec<usize>,
    live_count: usize,
    #[cfg(any(test, feature = "test-support"))]
    fail_next_overlay_queue: bool,
}

impl DeferredDeleteQueue {
    pub(crate) fn queue_object(&mut self, id: u64) {
        self.append(DeferredDeleteTicket::Object(id));
    }
    pub(crate) fn queue_tag(&mut self, id: usize) {
        self.append(DeferredDeleteTicket::Tag(id));
    }
    pub(crate) fn queue_trigger(&mut self, id: usize) {
        self.append(DeferredDeleteTicket::Trigger(id));
    }
    pub(super) fn queue_disk_laser(&mut self, id: u64) {
        self.append(DeferredDeleteTicket::DiskLaser(id));
    }
    pub(crate) fn queue_overlay(&mut self, id: u64) -> Result<(), ()> {
        #[cfg(any(test, feature = "test-support"))]
        if std::mem::take(&mut self.fail_next_overlay_queue) {
            return Err(());
        }
        self.entries.try_reserve(1).map_err(|_| ())?;
        self.append(DeferredDeleteTicket::Overlay(id));
        Ok(())
    }
    pub fn contains_object(&self, id: &u64) -> bool {
        self.positions
            .contains_key(&DeferredDeleteTicket::Object(*id))
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn contains_overlay(&self, id: u64) -> bool {
        self.positions
            .contains_key(&DeferredDeleteTicket::Overlay(id))
    }
    pub(crate) fn remove_object(&mut self, id: u64) {
        self.remove(DeferredDeleteTicket::Object(id));
    }
    pub(super) fn remove(&mut self, ticket: DeferredDeleteTicket) {
        if let Some(positions) = self.positions.remove(&ticket) {
            for position in positions {
                self.entries[position] = None;
                self.live_count -= 1;
                self.change_prefix(position, false);
            }
        }
    }
    pub(super) fn ticket(&self, index: usize) -> Option<DeferredDeleteTicket> {
        let position = self.storage_position(index)?;
        self.entries[position]
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn len(&self) -> usize {
        self.live_count
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn is_empty(&self) -> bool {
        self.live_count == 0
    }
    pub(crate) fn retained_tickets(&self) -> impl Iterator<Item = DeferredDeleteTicket> + '_ {
        self.entries.iter().flatten().copied()
    }
    pub fn object_tickets(&self) -> impl Iterator<Item = u64> + '_ {
        self.retained_tickets().filter_map(|ticket| match ticket {
            DeferredDeleteTicket::Object(id) => Some(id),
            _ => None,
        })
    }
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        self.live_count.hash(hasher);
        for ticket in self.entries.iter().flatten() {
            // Preserve the established object-only fold. Non-Object tickets
            // carry an explicit family marker before their retained identity.
            match ticket {
                DeferredDeleteTicket::Object(id) => id.hash(hasher),
                _ => {
                    "AbstractDeferredTicket".hash(hasher);
                    ticket.hash(hasher);
                }
            }
        }
    }
    fn append(&mut self, ticket: DeferredDeleteTicket) {
        let position = self.entries.len();
        self.entries.push(Some(ticket));
        self.positions.entry(ticket).or_default().push(position);
        self.live_count += 1;
        if position + 1 >= self.prefix_counts.len() {
            self.rebuild_prefix();
        } else {
            self.change_prefix(position, true);
        }
    }
    fn change_prefix(&mut self, position: usize, add: bool) {
        let mut index = position + 1;
        while index < self.prefix_counts.len() {
            if add {
                self.prefix_counts[index] += 1;
            } else {
                self.prefix_counts[index] -= 1;
            }
            index += index.isolate_lowest_one();
        }
    }
    fn rebuild_prefix(&mut self) {
        let capacity = self.entries.len().max(8).next_power_of_two();
        self.prefix_counts = vec![0; capacity + 1];
        for (index, entry) in self.entries.iter().enumerate() {
            self.prefix_counts[index + 1] = usize::from(entry.is_some());
        }
        for index in 1..=capacity {
            let parent = index + index.isolate_lowest_one();
            if parent <= capacity {
                self.prefix_counts[parent] += self.prefix_counts[index];
            }
        }
    }
    fn storage_position(&self, logical_index: usize) -> Option<usize> {
        if logical_index >= self.live_count {
            return None;
        }
        let capacity = self.prefix_counts.len() - 1;
        let mut target = logical_index + 1;
        let mut position = 0;
        let mut step = capacity;
        while step != 0 {
            let next = position + step;
            if next <= capacity && self.prefix_counts[next] < target {
                target -= self.prefix_counts[next];
                position = next;
            }
            step /= 2;
        }
        Some(position)
    }
    /// Finish a native scan with one stable compaction. Destructors and their
    /// callbacks have already observed the same live logical queue throughout.
    pub(super) fn finish_drain(&mut self) {
        if self.entries.len() != self.live_count {
            self.entries.retain(Option::is_some);
            self.rebuild_indexes();
        }
    }
    fn rebuild_indexes(&mut self) {
        self.positions.clear();
        self.live_count = 0;
        for (position, ticket) in self.entries.iter().enumerate() {
            if let Some(ticket) = ticket {
                self.positions.entry(*ticket).or_default().push(position);
                self.live_count += 1;
            }
        }
        self.rebuild_prefix();
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn tickets(&self) -> Vec<DeferredDeleteTicket> {
        self.entries.iter().flatten().copied().collect()
    }
    /// Check every derived index against the sole serialized order source.
    /// Native history tests call this before and after every saved boundary.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn assert_consistent_for_test(&self) {
        let mut rebuilt = Self {
            entries: self.entries.clone(),
            ..Self::default()
        };
        rebuilt.rebuild_indexes();
        assert_eq!(self.positions, rebuilt.positions);
        if self.prefix_counts.is_empty() {
            assert!(self.entries.is_empty());
        } else {
            assert_eq!(self.prefix_counts, rebuilt.prefix_counts);
        }
        assert_eq!(self.live_count, rebuilt.live_count);
        for (index, ticket) in self.retained_tickets().enumerate() {
            assert_eq!(self.ticket(index), Some(ticket));
        }
        assert_eq!(self.ticket(self.live_count), None);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn object_ids(&self) -> Vec<u64> {
        self.object_tickets().collect()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn overlay_count(&self) -> usize {
        self.entries
            .iter()
            .flatten()
            .filter(|ticket| matches!(ticket, DeferredDeleteTicket::Overlay(_)))
            .count()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn clear_for_test(&mut self) {
        *self = Self::default();
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn remove_ticket_for_test(&mut self, ticket: DeferredDeleteTicket) {
        self.remove(ticket);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn fail_next_overlay_queue_for_test(&mut self) {
        self.fail_next_overlay_queue = true;
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn swap_for_test(&mut self, a: usize, b: usize) {
        let a = self.storage_position(a).unwrap();
        let b = self.storage_position(b).unwrap();
        self.entries.swap(a, b);
        self.rebuild_indexes();
    }
}

impl PartialEq for DeferredDeleteQueue {
    fn eq(&self, other: &Self) -> bool {
        self.entries
            .iter()
            .flatten()
            .eq(other.entries.iter().flatten())
    }
}
impl Eq for DeferredDeleteQueue {}

impl Serialize for DeferredDeleteQueue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.live_count))?;
        for ticket in self.entries.iter().flatten() {
            seq.serialize_element(ticket)?;
        }
        seq.end()
    }
}
impl<'de> Deserialize<'de> for DeferredDeleteQueue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tickets = Vec::<DeferredDeleteTicket>::deserialize(deserializer)?;
        let mut queue = Self {
            entries: tickets.into_iter().map(Some).collect(),
            ..Self::default()
        };
        queue.rebuild_indexes();
        Ok(queue)
    }
}

/// Monotonic source for the AirTracker's registration order. Each airborne
/// entity stores the value assigned when it entered its bucket; this counter
/// hands out the next one. The sole mutator is `next()` — callers cannot mis-increment
/// or skip the saturating semantics. Serialized + hashed at its `ObjectSubstrate` field
/// (a `#[serde(transparent)]` + derived-`Hash` newtype is byte- and hash-identical to the
/// bare `u64` it replaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnterOrderCounter(u64);

impl EnterOrderCounter {
    /// Fresh counter. Starts at 1; 0 is the reserved sentinel.
    pub(crate) const fn new() -> Self {
        Self(1)
    }

    /// Return the current order value and advance. Saturating — never wraps,
    /// matching the pre-consolidation `saturating_add(1)` at every assign-site.
    pub(crate) fn next(&mut self) -> u64 {
        let order = self.0;
        self.0 = self.0.saturating_add(1);
        order
    }

    /// Next value that will be handed out.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn current(self) -> u64 {
        self.0
    }
}

/// The object kinds the LogicVector registration/removal dispatch
/// distinguishes (F13). Classification probes the stores in this fixed order:
/// anims → voxel anims → particle systems → terrain objects → projectiles →
/// waves → entities — the pre-consolidation dispatch order with the
/// `VoxelAnimClass` registry inserted next to the other ObjectClass registry it
/// shares a death with. Object IDs are unique across stores (one monotonic
/// `next_stable_object_id` namespace), so at most one store can match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectKind {
    Anim,
    VoxelAnim,
    ParticleSystem,
    Terrain,
    Projectile,
    Wave,
    Smudge,
    Entity,
}

/// Owns the active-object order and the substrate's monotonic counters. Field
/// paths are `Simulation.substrate.*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectSubstrate {
    /// Monotonic per-instance id source (never reused). Each spawned entity
    /// draws the next value; a stale reference degrades to `None` rather than
    /// aliasing a reused slot.
    pub next_stable_object_id: u64,
    /// Monotonic source for the independently ordered AirTracker. Cell lists
    /// keep their own insertion order. See `EnterOrderCounter`.
    pub(crate) next_air_tracker_order: EnterOrderCounter,
    /// LogicClass active-object vector — the authority on AI visitation order.
    /// Tail-append on reveal, compacting-remove on conceal. Serialized verbatim.
    #[serde(default)]
    pub(crate) logic: LogicVector,
    /// Independent persistent DisplayClass layers, including the Ground vector
    /// read by crate effects. Never reconstructed from Logic or storage order.
    pub(crate) display: super::display_layers::DisplayLayers,
    /// Actual CellClass-style memberships/order, preserved across save/restore.
    /// Cell483C10/4839F0 saves and swizzles its ground/deck heads; rebuilding
    /// from current locomotor phase or terrain cannot recover those histories.
    pub occupancy: OccupancyGrid,
    /// Authoritative raw CellClass occupation bytes. These destructive
    /// OR/AND-not bytes are serialized verbatim and are never rebuilt from
    /// entity lists.
    #[serde(default)]
    pub(crate) raw_cell_occupation: RawCellOccupationGrid,
    /// Separate authoritative building hidden-object counters. Serialized
    /// verbatim because RemoveOccupy's enter-only cancellation is not
    /// reconstructible from the currently placed entity set.
    #[serde(default)]
    pub(crate) hidden_occupation: HiddenOccupationGrid,
    /// Authoritative CellClass `+0xE0` AltObject slots — the one airborne
    /// object each cell holds. Serialized verbatim: a hovering Jumpjet's claim
    /// is not reconstructible from its position or locomotor phase.
    #[serde(default)]
    pub air_slots: AirSlotGrid,
    /// Authoritative CellClass `+0xDC` per-house Building base reservations,
    /// including the single shared dummy CellClass mask.
    #[serde(default)]
    pub base_reservations: CellReservationGrid,
    /// Plain-struct entity storage (dense, in stable_id order, + infantry registry).
    /// The authoritative object store — serialized verbatim (NOT skipped).
    pub entities: EntityStore,
    /// Separate AnimClass registry sharing the global object ID namespace and
    /// LogicVector with entities.
    #[serde(default)]
    pub anims: AnimStore,
    /// `VoxelAnimClass` registry — the flying VXL debris a death throws. Shares
    /// the global object-ID namespace and the LogicVector with entities and
    /// anims, exactly as native's `VoxelAnimClass::Constructor` does through
    /// `AbstractClass::Create_ID` and `ObjectClass::Unlimbo`.
    #[serde(default)]
    pub(crate) voxel_anims: VoxelAnimStore,
    /// ParticleSystemClass registry. Systems share the global object-ID
    /// namespace and LogicVector; individual particles remain container-owned.
    #[serde(default)]
    pub(crate) particle_systems: ParticleSystemStore,
    /// Deferred-delete queue (the native `PendingDeleteList`). Ordered tickets may
    /// survive a Rust snapshot boundary: between enqueue and the ordinary late
    /// drain an entity remains resolvable in storage while its independent
    /// lifecycle facts describe dead/limbo/cell/logic state. Serialized verbatim;
    /// the state hash folds queue identities in insertion order.
    #[serde(default)]
    pub pending_delete: DeferredDeleteQueue,
}

impl ObjectSubstrate {
    /// Sole stable-ID allocator, also used while a constructor borrows the
    /// map and other disjoint substrate fields. Native numeric IDs have their
    /// separate Scenario cursor; every Rust object shares this namespace.
    pub(crate) fn allocate_stable_id(counter: &mut u64, entities: &EntityStore) -> u64 {
        let id = *counter;
        // Fixtures may insert entities directly without advancing the cursor.
        #[cfg(any(test, feature = "test-support"))]
        let id = {
            let mut id = id;
            while entities.contains(id) {
                id += 1;
            }
            id
        };
        #[cfg(not(any(test, feature = "test-support")))]
        debug_assert!(!entities.contains(id), "stable id {id} is already live");
        *counter = id.saturating_add(1);
        id
    }

    /// Fresh substrate for a new world. Counters start at 1 (0 is a reserved
    /// sentinel), matching the pre-consolidation `Simulation::new` initializers.
    pub(crate) fn new() -> Self {
        Self {
            next_stable_object_id: 1,
            next_air_tracker_order: EnterOrderCounter::new(),
            logic: LogicVector::new(),
            display: Default::default(),
            occupancy: OccupancyGrid::new(),
            raw_cell_occupation: RawCellOccupationGrid::new(),
            hidden_occupation: HiddenOccupationGrid::new(),
            air_slots: AirSlotGrid::default(),
            base_reservations: CellReservationGrid::new(),
            entities: EntityStore::new(),
            anims: AnimStore::default(),
            voxel_anims: VoxelAnimStore::default(),
            particle_systems: ParticleSystemStore::default(),
            pending_delete: DeferredDeleteQueue::default(),
        }
    }
}

impl ObjectSubstrate {
    /// Restore-time validation of the substrate's monotonic counters and the
    /// deserialized LogicVector (F13; moved verbatim from
    /// `restore_after_snapshot_load`, which remains the ordered coordinator).
    /// Returns the validated logic-member set so the coordinator's store
    /// cross-checks read exactly what was admitted here.
    pub(crate) fn validate_restored_counters_and_logic(
        &self,
        highest_id: u64,
        identities: &std::collections::BTreeMap<u64, &'static str>,
    ) -> Result<std::collections::BTreeSet<u64>, crate::sim::snapshot::SnapshotRestoreError> {
        use crate::sim::snapshot::SnapshotRestoreError;
        if self.next_stable_object_id <= highest_id {
            return Err(SnapshotRestoreError::ObjectIdCounterBehind {
                next_id: self.next_stable_object_id,
                highest_id,
            });
        }

        let mut seen_logic = std::collections::BTreeSet::new();
        for &object_id in self.display.ordered_ids() {
            if !identities.contains_key(&object_id) {
                return Err(SnapshotRestoreError::MissingDisplayIdentity { object_id });
            }
        }
        for &object_id in self.logic.as_slice() {
            if !seen_logic.insert(object_id) {
                return Err(SnapshotRestoreError::DuplicateLogicIdentity { object_id });
            }
            if !identities.contains_key(&object_id) {
                return Err(SnapshotRestoreError::MissingLogicIdentity { object_id });
            }
        }
        Ok(seen_logic)
    }

    // State-hash folds over substrate-owned occupation state (F13).
    // Called from `state_hash` at fixed positions; the fold
    // order and byte layout are part of the hash contract.

    /// Fold the authoritative sparse raw occupation bytes without conflating
    /// coordinates or the ground/deck planes. Empty raw state contributes no
    /// bytes, preserving established hashes while every modeled zero remains
    /// represented canonically by the absence of a sparse entry.
    pub(crate) fn fold_raw_cell_occupation(&self, hasher: &mut impl std::hash::Hasher) {
        if let Some(dummy) = self.raw_cell_occupation.dummy_for_hash() {
            b"raw-dummy-occupation-v1".hash(hasher);
            dummy.hash(hasher);
        }
        let entry_count = self.raw_cell_occupation.entry_count();
        if entry_count == 0 {
            return;
        }

        b"raw-cell-occupation-v2".hash(hasher);
        entry_count.hash(hasher);
        for (rx, ry, ground, deck, ground_owner, deck_owner) in self.raw_cell_occupation.entries() {
            0xC1u8.hash(hasher); // entry delimiter
            rx.hash(hasher);
            ry.hash(hasher);
            0u8.hash(hasher); // ground-plane tag
            ground.hash(hasher);
            1u8.hash(hasher); // deck-plane tag
            deck.hash(hasher);
            ground_owner.hash(hasher);
            deck_owner.hash(hasher);
        }
    }

    /// Fold the cell AltObject slots. Empty stays silent so a save with no
    /// hovering Jumpjet hashes exactly as it did before the slots existed.
    pub(crate) fn fold_air_slots(&self, hasher: &mut impl std::hash::Hasher) {
        let entry_count = self.air_slots.entry_count();
        if entry_count == 0 {
            return;
        }

        b"cell-air-slots-v1".hash(hasher);
        entry_count.hash(hasher);
        for (rx, ry, owner) in self.air_slots.entries() {
            rx.hash(hasher);
            ry.hash(hasher);
            owner.hash(hasher);
        }
    }

    pub(crate) fn fold_hidden_occupation(&self, hasher: &mut impl std::hash::Hasher) {
        let entry_count = self.hidden_occupation.entry_count();
        if entry_count == 0 {
            return;
        }

        b"hidden-cell-occupation-v1".hash(hasher);
        entry_count.hash(hasher);
        for (rx, ry, count) in self.hidden_occupation.entries() {
            rx.hash(hasher);
            ry.hash(hasher);
            count.hash(hasher);
        }
    }

    pub(crate) fn fold_base_reservations(&self, hasher: &mut impl std::hash::Hasher) {
        b"building-base-reservation-v1".hash(hasher);
        let entry_count = self.base_reservations.entries().count();
        entry_count.hash(hasher);
        for (rx, ry, mask) in self.base_reservations.entries() {
            rx.hash(hasher);
            ry.hash(hasher);
            mask.hash(hasher);
        }
        self.base_reservations.dummy_mask().hash(hasher);
    }
}

impl Default for ObjectSubstrate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_order_counter_next_returns_pre_increment_then_advances() {
        let mut c = EnterOrderCounter::new();
        assert_eq!(c.next(), 1);
        assert_eq!(c.next(), 2);
        assert_eq!(c.next(), 3);
    }

    #[test]
    fn enter_order_counter_saturates_at_max() {
        let mut c = EnterOrderCounter(u64::MAX);
        // Returns MAX, then stays MAX (saturating, never wraps to 0).
        assert_eq!(c.next(), u64::MAX);
        assert_eq!(c.next(), u64::MAX);
    }
}
