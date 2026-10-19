//! Boris' aircraft creation: `Create_Air_Reinforcement @ 0x0065E850`,
//! called by `AirstrikeClass::StartStrike @ 0x0041D926` with Attack, a
//! target cell, no destination and no passengers.
//!
//! Original instructions pick one house edge before constructing any plane,
//! prepend each plane to the manager's list, run FNPC per plane, then queue,
//! target, Unlimbo and Commence. The edge and constructor/unlimbo owners keep
//! their Scenario draws; FNPC draws nothing. Commence writes the mission timer.
//! A failed Unlimbo deletes that plane and returns the prior count minus one.

use super::{FrameEffects, PlacementEvidence, Simulation};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose;

#[derive(Default)]
pub(crate) struct AirstrikeAircraftSpawn {
    /// Native head-first list, the reverse of construction/Logic order.
    pub(crate) aircraft: Vec<u64>,
    /// Native return, including its unusual failed-Unlimbo decrement.
    pub(crate) count: i32,
}

impl Simulation {
    pub(crate) fn create_airstrike_aircraft(
        &mut self,
        owner: u64,
        target: u64,
        type_id: &str,
        count: i32,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> AirstrikeAircraftSpawn {
        let mut result = AirstrikeAircraftSpawn::default();
        let Some(boris) = self.substrate.entities.get(owner) else {
            return result;
        };
        let house_id = boris.owner();
        let owner_name = self.interner.resolve(house_id).to_owned();
        let Some(target) = self.substrate.entities.get(target) else {
            return result;
        };
        //41D8C9 calls GetCoords (+48), not TargetCoords (+A4), and divides
        //towards zero before narrowing to the CellStruct words.
        let coord = ground_pose::object_get_coords(target, self.resolved_terrain.as_ref());
        let target = TargetKind::Cell(
            crate::util::lepton::lepton_to_cell_packed(coord.x) as u16,
            crate::util::lepton::lepton_to_cell_packed(coord.y) as u16,
        );
        let (authored, waypoint) = self.houses.get(&house_id).map_or((-1, 0), |house| {
            (house.authored_edge(), house.waypoint_edge)
        });
        let edge = super::edge_cell::Edge::authored_or_waypoint(authored, waypoint);
        let Some(seed) = self.aircraft_edge_cell(edge) else {
            return result;
        };
        for _ in 0..count {
            let plane = self.with_object_placement_scope(|sim| {
                sim.construct_object_limbo_at_height(
                    type_id,
                    &owner_name,
                    0,
                    0,
                    0,
                    0,
                    rules,
                    effects,
                )
            });
            let Some(plane) = plane else {
                break;
            };
            self.link_airstrike_aircraft(owner, plane);
            self.substrate
                .entities
                .get_mut(plane)
                .unwrap()
                .mission_only
                .mark();
            //65E91C..65E94F: Winged, zone -1, Normal, bridge-zone false,
            //1x1, no height/occupation/overlay gate, bridges allowed.
            let cell = self
                .nearby_passable_cell(
                    (i32::from(seed.0 as i16), i32::from(seed.1 as i16)),
                    NearbyArgs::plain(
                        PassabilityArgs {
                            speed_type: SpeedType::Winged,
                            required_zone_id: None,
                            movement_zone: MovementZone::Normal,
                            bridge_aware_zone: false,
                        },
                        NearbyFootprint::SINGLE,
                    ),
                )
                .unwrap_or((0, 0));
            crate::sim::mission::authority::queue_entity_mission_deferred(
                self.substrate.entities.get_mut(plane).unwrap(),
                MissionId::from_known(MissionType::Attack),
            );
            self.assign_target_represented(plane, Some(target), Some(rules), None, effects)
                .expect("constructed aircraft target setter");
            let placed = self.with_object_placement_scope(|sim| {
                sim.reveal_constructed_object_at_height(
                    plane,
                    cell.0,
                    cell.1,
                    0,
                    0,
                    PlacementEvidence::MarkSucceeded,
                    rules,
                    effects,
                )
            });
            if placed.is_none() {
                let _ = self.discard_constructed_limbo(plane, Some(rules), effects);
                result.count -= 1;
                break;
            }
            crate::sim::mission::authority::commence_entity_mission(
                self.substrate.entities.get_mut(plane).unwrap(),
                self.session.binary_frame,
            );
            result.aircraft.push(plane);
            result.count += 1;
        }
        result.aircraft.reverse();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::playfield::PlayfieldBounds;
    use crate::sim::world::edge_cell::Edge;

    fn world() -> Option<(RuleSet, Simulation, u64, u64)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let (rules, mut sim, _) =
            crate::sim::superweapon::chronosphere_tests::world_with(rules, 64, &[]);
        sim.playfield_bounds = Some(PlayfieldBounds::from_raw_local_size(32, 32, [2, 4, 28, 22]));
        sim.playfield_size_height = Some(32);
        let boris = sim
            .spawn_object("BORIS", "Russians", 38, 38, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("GAPOWR", "Americans", 40, 40, 0, &rules)
            .unwrap();
        Some((rules, sim, boris, target))
    }

    #[test]
    fn retail_sortie_uses_one_seed_cell_and_reverse_construction_membership() {
        let Some((rules, mut sim, boris, target)) = world() else {
            return;
        };
        let owner = sim.substrate.entities.get(boris).unwrap().owner();
        let at = ground_pose::object_get_coords(
            sim.substrate.entities.get(target).unwrap(),
            sim.resolved_terrain.as_ref(),
        );
        let sortie = sim.create_airstrike_aircraft(
            boris,
            target,
            "BPLN",
            3,
            &rules,
            FrameEffects::default(),
        );
        assert_eq!(sortie.count, 3);
        assert_eq!(sortie.aircraft.len(), 3);
        assert!(sortie.aircraft.windows(2).all(|pair| pair[0] > pair[1]));
        let first = sim.substrate.entities.get(sortie.aircraft[0]).unwrap();
        let position = (first.position.rx(), first.position.ry());
        for id in sortie.aircraft {
            let plane = sim.substrate.entities.get(id).unwrap();
            assert_eq!(plane.owner(), owner);
            assert_eq!(plane.airstrike_link(), Some(boris));
            assert!(plane.spawn_owner_id.owner().is_none());
            assert!(plane.is_mission_only());
            assert_eq!(
                plane.attack_target.target(),
                Some(TargetKind::Cell((at.x / 256) as u16, (at.y / 256) as u16))
            );
            // TechnoUnlimbo6F6E2A calls AircraftEnterIdle4176F0. Its
            // airborne loaner arm (417802..417A33), with ammo and no Team,
            // replaces the previously queued Attack with Hunt. Target stays
            // the designated Cell; CreateAir65EA5C's final Commence retains it.
            assert_eq!(plane.mission.current().known(), Some(MissionType::Hunt));
            assert_eq!((plane.position.rx(), plane.position.ry()), position);
            assert!(!plane.lifecycle.in_limbo());
        }
    }

    #[test]
    fn zero_count_still_spends_the_single_edge_pick_before_the_count_gate() {
        let Some((rules, mut sim, boris, target)) = world() else {
            return;
        };
        let owner = sim.substrate.entities.get(boris).unwrap().owner();
        let house = sim.houses.get(&owner).unwrap();
        let edge = Edge::authored_or_waypoint(house.authored_edge(), house.waypoint_edge);
        let mut expected = sim.scenario_rng.clone();
        super::super::edge_cell::find_paradrop_edge_cell(
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
            edge,
            &mut expected,
        );
        let sortie = sim.create_airstrike_aircraft(
            boris,
            target,
            "BPLN",
            0,
            &rules,
            FrameEffects::default(),
        );
        assert_eq!(sortie.count, 0);
        assert!(sortie.aircraft.is_empty());
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn spawned_aircraft_retarget_uses_type_flags_even_without_spawnowner() {
        let Some((rules, mut sim, _, _)) = world() else {
            return;
        };
        for name in ["BPLN", "HORNET", "V3ROCKET", "ORCA"] {
            for target in [None, Some(TargetKind::Cell(42, 42))] {
                let id = sim
                    .spawn_object(name, "Russians", 30, 30, 0, &rules)
                    .unwrap();
                let plane = sim.substrate.entities.get_mut(id).unwrap();
                assert!(plane.spawn_owner_id.owner().is_none());
                plane
                    .attack_target
                    .set_for_test(Some(TargetKind::Cell(40, 40)));
                plane
                    .mission
                    .set_current_for_test(MissionId::from_known(MissionType::Attack), 5);
                plane.ammo_mut().unwrap().set_current_for_test(2);
                sim.assign_target_represented(
                    id,
                    target,
                    Some(&rules),
                    None,
                    FrameEffects::default(),
                )
                .unwrap();
                let plane = sim.substrate.entities.get(id).unwrap();
                // A NULL request jumps to the store (0x006FCDF3), keeping
                // the ammo; only a new target leaves the run.
                let should_clear = matches!(name, "BPLN" | "HORNET") && target.is_some();
                assert_eq!(
                    plane.ammo().unwrap().current(),
                    if should_clear { 0 } else { 2 },
                    "{name}, {target:?}"
                );
                assert_eq!(
                    plane.attack_target.target(),
                    if should_clear { None } else { target },
                    "{name}, {target:?}"
                );
            }
        }
    }
}
