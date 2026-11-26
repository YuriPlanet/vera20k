//! The Unit's Mech locomotor uses the shared Foot route, a cell-centred
//! head and a turning hull. UnitClass owns any transit after direction 8.

use super::ground_pose;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::world::{FrameAdvanceError, FrameEffects, Simulation};
use crate::util::fixed_math::SIM_ONE;

impl Simulation {
    /// Mech 0x005B17B0 (fresh step and arrival).
    /// It unmarks the old head, asks the crate at the requested coordinate,
    /// then marks the admitted head or the unit's own location.
    pub(crate) fn mech_set_head(
        &mut self,
        id: u64,
        requested: Option<DriveCoord>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Mech head owner")?;
        let mut coord = requested.filter(|c| *c != super::foot_coordinate::NULL_COORD);
        if let Some(c) = coord.as_mut() {
            let terrain = self
                .resolved_terrain
                .as_ref()
                .ok_or("Mech head requires map cells")?;
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let ground = ground_pose::ground_surface_z_at(
                [c.x, c.y],
                false,
                Some(terrain),
                self.path_grid.as_deref(),
            )
            .unwrap_or(0);
            let cell = cells.lookup_world(c.x, c.y);
            let height = ground_pose::position_world_coord(&actor.position).z;
            c.z = if cells.flags(cell) & 0x100 != 0
                && height
                    > ground.wrapping_add(3 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
            {
                ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS)
            } else {
                ground
            };
        }
        if let Some(old) = actor.locomotor.as_ref().and_then(|l| l.step_head()) {
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                old,
                false,
                rules.ares_form(),
            );
        }
        let cell =
            super::foot_path::coord_cell(coord.unwrap_or(super::foot_coordinate::NULL_COORD));
        let accepted = self.pickup_crate_at(id, cell, rules, registry, effects);
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return Ok(false);
        };
        let head = coord.filter(|_| accepted && !actor.lifecycle.in_limbo());
        if let Some(loco) = actor.locomotor.as_mut() {
            loco.set_step_head(head);
        }
        if (!accepted || actor.lifecycle.in_limbo()) && !actor.lifecycle.object_alive() {
            return Ok(false);
        }
        let mark = head.unwrap_or_else(|| ground_pose::position_world_coord(&actor.position));
        Self::object_raw_receiver_at(
            &mut self.substrate,
            &self.path_grid,
            &self.resolved_terrain,
            id,
            mark,
            true,
            rules.ares_form(),
        );
        Ok(head.is_some())
    }

    /// 0x005B0FBF..0x005B108A (fresh head), 0x005B1396..0x005B1472
    /// (after arrival). Only the wall overlay at index zero arms the slowdown.
    pub(super) fn mech_crush_ahead(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(&direction) = actor
            .navigation
            .path_replay()
            .remaining_directions()
            .first()
        else {
            return;
        };
        let Some(delta) = crate::util::direction::direction_delta(direction) else {
            return;
        };
        let cell = (
            actor.position.rx().wrapping_add_signed(delta.0 as i16),
            actor.position.ry().wrapping_add_signed(delta.1 as i16),
        );
        let Some(terrain) = &*self.resolved_terrain else {
            return;
        };
        let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain)
            .lookup((cell.0 as i16, cell.1 as i16));
        if self.cell_overlay(cell).overlay_id != Some(0) {
            return;
        }
        let Some(object) = self.interner.object_type(actor.type_ref(), rules) else {
            return;
        };
        if !crate::sim::combat::veterancy::is_crusher(actor.veterancy.rank_kind(), object) {
            return;
        }
        let tilt = object.tilts_when_crushes;
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.foot_speed.arm_crush_slowdown();
            if tilt {
                actor.rocking_mut().set_vel_forwards_native(
                    crate::util::native_x87::NativeF32Bits::from_bits(0xbc23_d70a),
                );
            }
        }
    }

    /// Mech ProcessMovement 0x005B01C0 waits for a turning hull before even
    /// requesting a path. With a head, 0x005B118B..0x005B178E pays one step.
    /// With no head it falls through to the shared fresh/retry path.
    pub(crate) fn process_mech_head(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Result<Option<bool>, FrameAdvanceError> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let Some(loco) = actor
            .locomotor
            .as_ref()
            .filter(|l| l.active_kind() == LocomotorKind::Mech)
        else {
            return Ok(None);
        };
        let frame = self.session.binary_frame();
        if actor.body_facing.is_rotating(frame) {
            return Ok(Some(false));
        }
        let Some(head) = loco.step_head() else {
            return Ok(None);
        };
        if super::movement_step::completed_walk_head(&actor.position, &actor.locomotor).is_some() {
            return self
                .run_completed_walk_step(id, head, rules, registry, effects)
                .map(Some);
        }
        let adjusted = super::foot_speed::adjusted_speed(
            actor,
            self.interner.object_type(actor.type_ref(), rules),
            rules.general.veteran_speed,
            &self.houses,
        );
        let current = ground_pose::position_world_coord(&actor.position);
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return Ok(Some(false));
        };
        actor.foot_speed.set_speed_fraction(SIM_ONE);
        let speed = super::foot_speed::owner_current_speed_from_native_fraction(
            adjusted,
            actor.foot_speed.native_applied_fraction(),
            actor.effect_speed_multiplier(),
        );
        let facing = crate::util::direction_tables::facing16_from_delta(
            head.x.wrapping_sub(current.x),
            head.y.wrapping_sub(current.y),
        );
        super::drive_do_turn(actor, facing, frame);
        if actor.body_facing.is_rotating(frame) {
            return Ok(Some(false));
        }
        if actor.foot_occupation_enabled() {
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                current,
                false,
                rules.ares_form(),
            );
            if let Some(actor) = self.substrate.entities.get_mut(id) {
                actor.foot_occupation_enabled.suspend();
                actor.navigation.path_runtime_mut().clear_blockage();
            }
        }
        let [x, y] =
            crate::util::native_trig::facing_step_world_xy([current.x, current.y], facing, speed);
        let step = DriveCoord { x, y, z: current.z };
        if super::foot_path::coord_cell(current) != super::foot_path::coord_cell(step) {
            self.run_walk_boundary(id, step, rules, registry, effects);
        } else {
            let marked = self.substrate.replace_foot_marked_byte(id, false);
            ground_pose::foot_set_location(
                &mut self.substrate.entities,
                id,
                step,
                rules,
                &self.interner,
            );
            if let Some(actor) = self.substrate.entities.get_mut(id) {
                actor.position.set_height(
                    actor.on_bridge(),
                    0,
                    self.resolved_terrain.as_ref(),
                    self.path_grid.as_deref(),
                );
                actor.navigation.path_runtime_mut().clear_scold_latch();
            }
            self.substrate.replace_foot_marked_byte(id, marked);
        }
        Ok(Some(false))
    }
}

#[cfg(all(test, tests_sim_movement))]
mod tests {
    use super::*;
    use crate::sim::occupancy::VEHICLE_OCCUPATION_BIT;

    const MECH_RULES: &str = "[VehicleTypes]\n0=MECH\n[MECH]\nStrength=400\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nLocomotor={55D141B8-DB94-11D1-AC98-006008055BB5}\n";

    /// 5B17B0 reserves the fresh head; +78 (5B01A0) subsequently stores
    /// exact XYZ with no crate or raw occupation callback.
    #[test]
    fn a_forced_mech_head_preserves_height_and_the_fresh_step_reservation() {
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(MECH_RULES);
        let id = sim
            .spawn_object("MECH", "Americans", 15, 15, 64, &rules)
            .unwrap();
        let mut effects = FrameEffects::default();
        let initial = DriveCoord::cell(16, 15, 0);
        sim.mech_set_head(id, Some(initial), &rules, Some(&registry), &mut effects)
            .unwrap();
        let mut forced = DriveCoord::cell(17, 15, 570);
        forced.x += 37;
        sim.foot_force_immediate_destination(
            id,
            Some(forced),
            &rules,
            Some(&registry),
            &mut effects,
        )
        .unwrap();
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.locomotor.as_ref().unwrap().step_head(), Some(forced));
        assert_ne!(
            sim.substrate.raw_cell_occupation.ground_bits(16, 15) & VEHICLE_OCCUPATION_BIT,
            0
        );
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(17, 15) & VEHICLE_OCCUPATION_BIT,
            0
        );
    }
    /// Foot Limbo 4DB324 dispatches Mech Mark_All_Occupation_Bits 5B1A50.
    #[test]
    fn removing_a_mech_releases_the_reserved_cell_ahead() {
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules(MECH_RULES);
        let id = sim
            .spawn_object("MECH", "Americans", 15, 15, 0, &rules)
            .unwrap();
        let mut effects = FrameEffects::default();
        assert!(sim.set_unit_destination(
            id,
            crate::sim::components::NavTargetRef::cell(17, 15),
            &rules,
            true,
            &mut effects
        ));
        sim.mech_set_head(
            id,
            Some(DriveCoord::cell(16, 15, 0)),
            &rules,
            Some(&registry),
            &mut effects,
        )
        .unwrap();
        assert_ne!(
            sim.substrate.raw_cell_occupation.ground_bits(16, 15) & VEHICLE_OCCUPATION_BIT,
            0
        );
        sim.techno_limbo_with_rules(id, &rules);
        assert_eq!(
            sim.substrate.raw_cell_occupation.ground_bits(16, 15) & VEHICLE_OCCUPATION_BIT,
            0
        );
    }
}
