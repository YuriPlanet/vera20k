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
    AirSlotGrid, CellOccupationGrid, HiddenOccupationGrid, OccupancyGrid, RawCellOccupationGrid,
};
use crate::sim::particles::ParticleSystemStore;
use crate::sim::voxel_anim::VoxelAnimStore;

const FIRST_MULTIPLAYER_FEEDBACK_ANIM_ID: u64 = 1 << 63;

const fn first_multiplayer_feedback_anim_id() -> u64 {
    FIRST_MULTIPLAYER_FEEDBACK_ANIM_ID
}

/// Monotonic source for rebuilt CellClass-style object-list (enter) order. Each
/// entity stores the last value assigned when it entered a cell list; this counter
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

    /// Next value that will be handed out. Snapshot restoration uses this to
    /// reject a counter that could reuse an already-restored cell-entry order.
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
    Entity,
}

/// Owns the active-object order and the substrate's monotonic counters. Field
/// paths are `Simulation.substrate.*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ObjectSubstrate {
    /// Monotonic per-instance id source (never reused). Each spawned entity
    /// draws the next value; a stale reference degrades to `None` rather than
    /// aliasing a reused slot.
    pub(crate) next_stable_object_id: u64,
    /// Monotonic source for CellClass-style object-list (enter) order and the
    /// independently ordered AirTracker. See `EnterOrderCounter`.
    pub(crate) next_occupancy_enter_order: EnterOrderCounter,
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
    pub(crate) occupancy: OccupancyGrid,
    /// Independent ground/deck vehicle-occupation bit planes. Rebuilt from
    /// entity lifecycle and serialized Drive footprint state after load.
    #[serde(skip)]
    pub(crate) cell_occupation: CellOccupationGrid,
    /// Authoritative raw CellClass occupation bytes. Unlike the owner-aware
    /// Drive compatibility cache above, these destructive OR/AND-not bytes are
    /// serialized verbatim and are never rebuilt from entity lists.
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
    pub(crate) air_slots: AirSlotGrid,
    /// Authoritative CellClass `+0xDC` per-house Building base reservations,
    /// including the single shared dummy CellClass mask.
    #[serde(default)]
    pub(crate) base_reservations: CellReservationGrid,
    /// Plain-struct entity storage (`BTreeMap<u64, GameEntity>` + by_owner index).
    /// The authoritative object store — serialized verbatim (NOT skipped).
    pub(crate) entities: EntityStore,
    /// Separate AnimClass registry sharing the global object ID namespace and
    /// LogicVector with entities.
    #[serde(default)]
    pub(crate) anims: AnimStore,
    /// `VoxelAnimClass` registry — the flying VXL debris a death throws. Shares
    /// the global object-ID namespace and the LogicVector with entities and
    /// anims, exactly as native's `VoxelAnimClass::Constructor` does through
    /// `AbstractClass::AssignUniqueID` and `ObjectClass::Unlimbo`.
    #[serde(default)]
    pub(crate) voxel_anims: VoxelAnimStore,
    /// Multiplayer click-feedback animations use a separate, sync-exempt
    /// registry and never enter the ordinary LogicVector.
    #[serde(skip)]
    pub(crate) multiplayer_feedback_anims: AnimStore,
    // Reserved for the verified sync-exempt feedback spawn path, which is not wired yet.
    #[serde(skip, default = "first_multiplayer_feedback_anim_id")]
    pub(crate) next_multiplayer_feedback_anim_id: u64,
    #[serde(skip)]
    pub(crate) multiplayer_feedback_pending_delete: Vec<u64>,
    /// ParticleSystemClass registry. Systems share the global object-ID
    /// namespace and LogicVector; individual particles remain container-owned.
    #[serde(default)]
    pub(crate) particle_systems: ParticleSystemStore,
    /// Deferred-delete queue (the native `PendingDeleteList`). Ordered IDs may
    /// survive a Rust snapshot boundary: between enqueue and the ordinary late
    /// drain an entity remains resolvable in storage while its independent
    /// lifecycle facts describe dead/limbo/cell/logic state. Serialized verbatim;
    /// the state hash folds the queue length followed by IDs in insertion order.
    #[serde(default)]
    pub(crate) pending_delete: Vec<u64>,
}

impl ObjectSubstrate {
    /// Fresh substrate for a new world. Counters start at 1 (0 is a reserved
    /// sentinel), matching the pre-consolidation `Simulation::new` initializers.
    pub(crate) fn new() -> Self {
        Self {
            next_stable_object_id: 1,
            next_occupancy_enter_order: EnterOrderCounter::new(),
            logic: LogicVector::new(),
            display: Default::default(),
            occupancy: OccupancyGrid::new(),
            cell_occupation: CellOccupationGrid::new(),
            raw_cell_occupation: RawCellOccupationGrid::new(),
            hidden_occupation: HiddenOccupationGrid::new(),
            air_slots: AirSlotGrid::default(),
            base_reservations: CellReservationGrid::new(),
            entities: EntityStore::new(),
            anims: AnimStore::default(),
            voxel_anims: VoxelAnimStore::default(),
            multiplayer_feedback_anims: AnimStore::default(),
            next_multiplayer_feedback_anim_id: FIRST_MULTIPLAYER_FEEDBACK_ANIM_ID,
            multiplayer_feedback_pending_delete: Vec::new(),
            particle_systems: ParticleSystemStore::default(),
            pending_delete: Vec::new(),
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

        let highest_order = self
            .entities
            .values()
            .filter(|entity| entity.lifecycle.cell_marked)
            .map(|entity| entity.occupancy_enter_order)
            .max()
            .unwrap_or(0);
        let next_order = self.next_occupancy_enter_order.current();
        if next_order <= highest_order {
            return Err(SnapshotRestoreError::OccupancyOrderCounterBehind {
                next_order,
                highest_order,
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
    // Called from `state_hash_with_schema` at fixed positions; the fold
    // order and byte layout are part of the hash contract.

    /// Fold the authoritative sparse raw occupation bytes without conflating
    /// coordinates or the ground/deck planes. Empty raw state contributes no
    /// bytes, preserving established hashes while every modeled zero remains
    /// represented canonically by the absence of a sparse entry.
    pub(crate) fn fold_raw_cell_occupation(
        &self,
        hasher: &mut impl std::hash::Hasher,
        include_process_dummy: bool,
        include_infantry_owners: bool,
    ) {
        if include_process_dummy && let Some(dummy) = self.raw_cell_occupation.dummy_for_hash() {
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
            if include_infantry_owners {
                ground_owner.hash(hasher);
                deck_owner.hash(hasher);
            }
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
    fn enter_order_counter_new_starts_at_one() {
        let mut c = EnterOrderCounter::new();
        // First handout is 1 (0 is the reserved sentinel).
        assert_eq!(c.next(), 1);
    }

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
