//! Shared Foot air tracking and Cell AltObject notification authority.
//!
//! AircraftTracker Add4134A0/Remove4135D0/Update4138C0 retain Foot+560,
//! independently of Cell487D70's concrete Foot4E00B0 notification at+564.
//! Original executable controls: tools/spatial_oracle/jumpjet_states.py
//! (`stock_taken_state1_arrival`, `slot_claim_replacement_release`).

use super::GameEntity;
use crate::map::entities::EntityCategory;
use crate::sim::occupancy::{air_spatial_bucket_index, air_spatial_tracks_entity};
use crate::sim::world::Simulation;

const NATIVE_NULL_CELL: (i16, i16) = (0, 0);

/// Saved native Foot Cells and the ordered airborne-vector projection.
/// Bucket and order retain their serialized names and constructor values.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct FootAirState {
    air_spatial_bucket: Option<u16>,
    air_spatial_enter_order: u64,
    air_tracker_cell: (i16, i16),
    air_slot_cell: (i16, i16),
}

impl FootAirState {
    pub(super) fn new(stable_id: u64) -> Self {
        Self {
            air_spatial_enter_order: stable_id,
            ..Self::default()
        }
    }
}

impl GameEntity {
    /// Native Foot+560, written by the tracker callback41C160.
    pub(crate) fn air_tracker_cell(&self) -> (i16, i16) {
        self.foot_air.air_tracker_cell
    }

    /// Native Foot+564, retained by the concrete Cell-slot notification4E00B0.
    pub(crate) fn air_slot_cell(&self) -> (i16, i16) {
        self.foot_air.air_slot_cell
    }

    pub(crate) fn air_spatial_bucket(&self) -> Option<u16> {
        self.foot_air.air_spatial_bucket
    }

    pub(crate) fn air_spatial_enter_order(&self) -> u64 {
        self.foot_air.air_spatial_enter_order
    }

    /// Owner virtual+2F8 receiver41C160. Add, Remove and Update share this
    /// one retained+560 write; slot notification4E00B0 owns a different Cell.
    fn retain_air_tracker_cell(&mut self, cell: (i16, i16)) {
        self.foot_air.air_tracker_cell = cell;
    }

    #[cfg(test)]
    pub(crate) fn with_air_spatial_membership_for_test(
        mut self,
        bucket: Option<u16>,
        enter_order: u64,
    ) -> Self {
        self.foot_air.air_spatial_bucket = bucket;
        self.foot_air.air_spatial_enter_order = enter_order;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_air_tracker_cell_for_test(mut self, cell: (i16, i16)) -> Self {
        self.foot_air.air_tracker_cell = cell;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_air_slot_cell_for_test(mut self, cell: (i16, i16)) -> Self {
        self.foot_air.air_slot_cell = cell;
        self
    }
}

impl Simulation {
    /// AircraftTracker::Add4134A0 appends the current Cell's bucket and invokes
    /// Foot's virtual+2F8 callback41C160 at4134CA, retaining that Cell in+560.
    /// BeginTakeoff4CF9B9 and Jumpjet State0 call this for an untracked Foot.
    pub(crate) fn aircraft_tracker_add(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.foot_air.air_spatial_bucket.is_some() {
            return;
        }
        let cell = (entity.position.rx as i16, entity.position.ry as i16);
        let bucket = air_spatial_bucket_index(
            entity.position.rx,
            entity.position.ry,
            self.session.map_width,
            self.session.map_height,
        );
        let order = self.substrate.next_air_tracker_order.next();
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity.retain_air_tracker_cell(cell);
        entity.foot_air.air_spatial_bucket = Some(bucket);
        entity.foot_air.air_spatial_enter_order = order;
    }

    /// AircraftTracker::Remove4135D0 removes membership and invokes41C160 at
    /// 413601 with NativeNull8B3D88. Slot notification+564 is independent.
    pub(crate) fn aircraft_tracker_remove(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.retain_air_tracker_cell(NATIVE_NULL_CELL);
            entity.foot_air.air_spatial_bucket = None;
            entity.foot_air.air_spatial_enter_order = 0;
        }
    }

    /// AircraftTracker::Update4138C0 invokes41C160 at4138DA before its bucket
    /// comparison41399A. A within-bucket Cell crossing changes+560 while
    /// retaining the vector order; a bucket crossing appends at the new tail.
    pub(crate) fn aircraft_tracker_update_cell(&mut self, id: u64, new_cell: (i16, i16)) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.retain_air_tracker_cell(new_cell);
        let bucket = air_spatial_bucket_index(
            new_cell.0 as u16,
            new_cell.1 as u16,
            self.session.map_width,
            self.session.map_height,
        );
        if entity.foot_air.air_spatial_bucket == Some(bucket) {
            return;
        }
        let order = self.substrate.next_air_tracker_order.next();
        entity.foot_air.air_spatial_bucket = Some(bucket);
        entity.foot_air.air_spatial_enter_order = order;
    }

    /// Reconcile represented air-vector membership after a physical step.
    /// Fly, Jumpjet and Rocket enter/leave through their native callbacks;
    /// marking a grounded instance cannot create a new tracker registration.
    /// Other represented air categories retain their existing admission
    /// predicate.
    pub(crate) fn sync_air_spatial_membership(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let tracked = entity.foot_air.air_spatial_bucket.is_some();
        let explicit_membership = entity.locomotor.as_ref().is_some_and(|locomotor| {
            locomotor.fly_runtime().is_some()
                || locomotor.jumpjet_runtime().is_some()
                || locomotor.rocket_runtime().is_some()
        });
        let desired = entity.lifecycle.object_alive
            && !entity.lifecycle.in_limbo
            && entity.lifecycle.cell_marked
            && !entity.passenger_role.is_inside_transport()
            && if explicit_membership {
                tracked
            } else {
                air_spatial_tracks_entity(entity)
            };
        let cell = (entity.position.rx as i16, entity.position.ry as i16);
        match (tracked, desired) {
            (false, false) => {}
            (false, true) => self.aircraft_tracker_add(id),
            (true, false) => self.aircraft_tracker_remove(id),
            (true, true) => self.aircraft_tracker_update_cell(id, cell),
        }
    }

    /// CellClass::SetAltObject487D70. A foreign claim returns AL0 unchanged.
    /// A successful claim writes Cell+E0 at487DC8, then directly notifies the
    /// Foot at487DD9. Release notifies the prior Foot at487DA5 before clearing
    /// Cell+E0 at487DAA; an empty release performs no notification.
    pub(crate) fn set_cell_air_slot(&mut self, cell: (i16, i16), owner: Option<u64>) -> bool {
        let held = self
            .substrate
            .air_slots
            .holder(cell.0 as u16, cell.1 as u16);
        if let Some(owner) = owner {
            if held.is_some_and(|held| held != owner) {
                return false;
            }
            self.substrate.air_slots.set_holder(cell, Some(owner));
            self.notify_foot_air_slot(owner, cell);
        } else {
            if let Some(held) = held {
                self.notify_foot_air_slot(held, NATIVE_NULL_CELL);
            }
            self.substrate.air_slots.set_holder(cell, None);
        }
        true
    }

    /// Concrete Foot4E00B0, called only for the native Foot flag+14&4.
    /// Native old/new Cells are dwords, including NativeNull(0,0). For a
    /// non-null replacement4E00EC..4E0104 directly clears the old matching
    /// Cell+E0; it does not recurse through Cell487D70. Consequently a
    /// repeated same-owner claim clears its newly written slot and retains+564.
    fn notify_foot_air_slot(&mut self, id: u64, new_cell: (i16, i16)) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category == EntityCategory::Structure {
            return;
        }
        let old_cell = entity.foot_air.air_slot_cell;
        if old_cell != NATIVE_NULL_CELL
            && new_cell != NATIVE_NULL_CELL
            && self
                .substrate
                .air_slots
                .holder(old_cell.0 as u16, old_cell.1 as u16)
                == Some(id)
        {
            self.substrate.air_slots.set_holder(old_cell, None);
        }
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .foot_air
            .air_slot_cell = new_cell;
    }

    /// Foot destructor's selected cache block4D3632..4D366E, with EBX0 from
    /// original XOR4D3595. If the remembered non-null Cell still holds this
    /// Foot,4D3668 directly clears its+E0. Neither487D70/4E00B0 nor a+564
    /// write occurs. UnInit/Limbo retain this slot until physical destruction.
    /// Native corpus: jumpjet_states.json/unit_uninit_live_slot; this block's
    /// execution does not establish the whole destructor or deferred drain.
    pub(crate) fn clear_foot_air_slot_at_destruction(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category == EntityCategory::Structure {
            return;
        }
        let cell = entity.foot_air.air_slot_cell;
        if cell != NATIVE_NULL_CELL
            && self
                .substrate
                .air_slots
                .holder(cell.0 as u16, cell.1 as u16)
                == Some(id)
        {
            self.substrate.air_slots.set_holder(cell, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::LocomotorState;

    fn native_control_world() -> Simulation {
        let mut sim = Simulation::new();
        // Composed native controls initialize the tracker with extent40x40.
        sim.session.map_width = 40;
        sim.session.map_height = 40;
        for (id, rx) in [(1, 10), (2, 12)] {
            sim.substrate.entities.insert(GameEntity::test_default(
                id,
                "SHAD",
                "Americans",
                rx,
                10,
            ));
            sim.add_entity_occupancy(id);
        }
        // The native fixture starts with the foreign Foot already tracked and
        // holding Cell12,10; both independent caches are retained at12,10.
        sim.aircraft_tracker_add(2);
        assert!(sim.set_cell_air_slot((12, 10), Some(2)));
        sim
    }

    /// Executed native r8: stock_taken_state1_arrival, process36/45/55/66/109.
    /// The cache changes within a bucket, a crossing appends behind the foreign
    /// object, and touchdown removes only the actor. No arithmetic golden is
    /// inferred from Rust; Cells and bucket identities are native observations.
    #[test]
    fn native_tracker_crossings_preserve_or_append_vector_order() {
        let mut sim = native_control_world();
        sim.aircraft_tracker_add(1);
        let actor_order = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .air_spatial_enter_order();
        let foreign_order = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .air_spatial_enter_order();
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_spatial_bucket(),
            Some(105)
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().air_spatial_bucket(),
            Some(106)
        );

        sim.aircraft_tracker_update_cell(1, (11, 10));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_tracker_cell(), (11, 10));
        assert_eq!(actor.air_spatial_enter_order(), actor_order);
        sim.aircraft_tracker_update_cell(1, (12, 10));
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_spatial_bucket(), Some(106));
        assert!(actor.air_spatial_enter_order() > foreign_order);
        let crossed_order = actor.air_spatial_enter_order();
        for cell in [(13, 10), (13, 11)] {
            sim.aircraft_tracker_update_cell(1, cell);
            let actor = sim.substrate.entities.get(1).unwrap();
            assert_eq!(actor.air_tracker_cell(), cell);
            assert_eq!(actor.air_spatial_enter_order(), crossed_order);
        }
        sim.aircraft_tracker_remove(1);
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_tracker_cell(), NATIVE_NULL_CELL);
        assert_eq!(actor.air_spatial_bucket(), None);
        assert_eq!(actor.air_spatial_enter_order(), 0);
        let foreign = sim.substrate.entities.get(2).unwrap();
        assert_eq!(foreign.air_spatial_bucket(), Some(106));
        assert_eq!(foreign.air_spatial_enter_order(), foreign_order);
    }

    /// Executed native r8: slot_claim_replacement_release. The same-owner
    /// write/notification clears its own raw slot, leaving a non-null+564;
    /// empty release does not manufacture a notification to clear that cache.
    #[test]
    fn native_slot_claim_refusal_replacement_same_owner_and_release() {
        let mut sim = native_control_world();
        sim.aircraft_tracker_add(1);
        assert!(!sim.set_cell_air_slot((12, 10), Some(1)));
        assert_eq!(sim.substrate.air_slots.holder(12, 10), Some(2));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_slot_cell(),
            NATIVE_NULL_CELL
        );
        assert!(sim.set_cell_air_slot((10, 10), Some(1)));
        assert!(sim.set_cell_air_slot((11, 10), Some(1)));
        assert_eq!(sim.substrate.air_slots.holder(10, 10), None);
        assert_eq!(sim.substrate.air_slots.holder(11, 10), Some(1));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_slot_cell(),
            (11, 10)
        );
        assert!(sim.set_cell_air_slot((11, 10), Some(1)));
        assert_eq!(sim.substrate.air_slots.holder(11, 10), None);
        assert!(sim.set_cell_air_slot((11, 10), None));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_slot_cell(),
            (11, 10)
        );
        assert!(sim.set_cell_air_slot((12, 10), None));
        assert!(sim.set_cell_air_slot((12, 10), Some(1)));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_slot_cell(),
            (12, 10)
        );
        assert!(sim.set_cell_air_slot((12, 10), None));
        assert_eq!(sim.substrate.air_slots.holder(12, 10), None);
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_slot_cell(), NATIVE_NULL_CELL);
        assert_eq!(actor.air_tracker_cell(), (10, 10));
        assert_eq!(actor.air_spatial_bucket(), Some(105));
    }

    #[test]
    fn a_marked_grounded_jumpjet_requires_explicit_tracker_entry() {
        let mut sim = native_control_world();
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.lifecycle.in_limbo = false;
        actor.lifecycle.cell_marked = true;
        actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Jumpjet));
        sim.sync_air_spatial_membership(1);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_spatial_bucket(),
            None
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_tracker_cell(),
            NATIVE_NULL_CELL
        );
        sim.aircraft_tracker_add(1);
        let order = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .air_spatial_enter_order();
        sim.substrate.entities.get_mut(1).unwrap().position.rx = 11;
        sim.sync_air_spatial_membership(1);
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_tracker_cell(), (11, 10));
        assert_eq!(actor.air_spatial_enter_order(), order);
        sim.aircraft_tracker_remove(1);
        sim.sync_air_spatial_membership(1);
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_spatial_bucket(),
            None
        );
    }

    /// Actual active-game UnInit followed by the selected original destructor
    /// cache block, recorded in schema2 rather than a Rust-derived golden.
    #[test]
    fn native_uninit_retains_slot_until_destructor_cached_cell_clear() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/jumpjet_states.json",
        ))
        .unwrap();
        let control = native["composed_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|control| control["name"] == "unit_uninit_live_slot")
            .unwrap();
        let output = &control["output"];
        let after = |label| {
            let step = output["steps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|step| step["label"] == label)
                .unwrap();
            assert_eq!(step["draw_start"], step["draw_end"]);
            &output["boundaries"][step["after"].as_u64().unwrap() as usize]["state"]
        };
        let cell = |value: &serde_json::Value| {
            (
                value[0].as_i64().unwrap() as i16,
                value[1].as_i64().unwrap() as i16,
            )
        };
        let slot = |state: &serde_json::Value, xy: [i16; 2]| {
            state["cells"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["cell"] == serde_json::json!(xy))
                .unwrap()["slot"]
                .as_str()
                .unwrap()
                .to_owned()
        };

        let mut sim = native_control_world();
        sim.aircraft_tracker_add(1);
        assert!(sim.set_cell_air_slot((10, 10), Some(1)));
        sim.uninit(1);
        let retired = after("unit_uninit");
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_tracker_cell(), cell(&retired["tracker_cell"]));
        assert_eq!(actor.air_slot_cell(), cell(&retired["slot_cell"]));
        assert_eq!(actor.lifecycle.object_alive, retired["alive"] == 1);
        assert_eq!(actor.lifecycle.cell_marked, retired["marked"] == 1);
        assert!(actor.lifecycle.in_limbo);
        assert_eq!(sim.substrate.pending_delete, vec![1]);
        assert_eq!(slot(retired, [10, 10]), "0x20000000");
        assert_eq!(sim.substrate.air_slots.holder(10, 10), Some(1));

        sim.clear_foot_air_slot_at_destruction(1);
        let destroyed = after("foot_destructor_slot_block");
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(actor.air_tracker_cell(), cell(&destroyed["tracker_cell"]));
        assert_eq!(actor.air_slot_cell(), cell(&destroyed["slot_cell"]));
        assert_eq!(slot(destroyed, [10, 10]), "0x0");
        assert_eq!(sim.substrate.air_slots.holder(10, 10), None);
        assert_eq!(slot(destroyed, [12, 10]), "0x28060000");
        assert_eq!(sim.substrate.air_slots.holder(12, 10), Some(2));
        assert_eq!(sim.substrate.pending_delete, vec![1]);
    }

    #[test]
    fn destructor_cached_cell_preserves_a_foreign_holder_and_stale_cache() {
        let mut sim = native_control_world();
        assert!(sim.set_cell_air_slot((10, 10), Some(1)));
        // Executed same-owner claim leaves the old+564 Cell with an empty E0;
        // another Foot can now claim it while the first keeps that stale Cell.
        assert!(sim.set_cell_air_slot((10, 10), Some(1)));
        assert!(sim.set_cell_air_slot((10, 10), Some(2)));
        sim.uninit(1);
        let foreign_before = sim.substrate.entities.get(2).unwrap().air_slot_cell();
        sim.clear_foot_air_slot_at_destruction(1);
        assert_eq!(sim.substrate.air_slots.holder(10, 10), Some(2));
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().air_slot_cell(),
            (10, 10)
        );
        assert_eq!(
            sim.substrate.entities.get(2).unwrap().air_slot_cell(),
            foreign_before
        );
    }
}
