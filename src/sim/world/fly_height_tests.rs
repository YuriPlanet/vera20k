//! Native Fly height vectors exercised through the production cell transaction.

use super::lifecycle_tests::{insert_entity, install_common_raw_terrain};
use super::{PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::snapshot::GameSnapshot;
use crate::util::fixed_math::SimFixed;

fn vectors() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fly_height.json"
    ))
    .unwrap()
}

fn fixture(row: &serde_json::Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let integer = |name: &str, default: i32| input[name].as_i64().map_or(default, |v| v as i32);
    let flag = |name: &str| input[name].as_bool().unwrap_or(false);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nFlightLevel=1500\n[AircraftTypes]\n0=TEST\n\
         [TEST]\nStrength=100\nSpeed=10\nLandable=yes\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         FlightLevel={}\nIsDropship={}\n",
        integer("flight_level", -1),
        if flag("dropship") { "yes" } else { "no" },
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(0);
    install_common_raw_terrain(
        &mut sim,
        22,
        22,
        integer("level", 0) as u8,
        flag("bridge").then_some((10, 10)),
    );
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(10, 10)
        .unwrap()
        .slope_type = integer("slope", 0) as u8;
    assert_eq!(sim.allocate_stable_id(), 1);
    let category = if input["kind"] == "unit" {
        EntityCategory::Unit
    } else {
        EntityCategory::Aircraft
    };
    insert_entity(&mut sim, 1, category);
    sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(LocomotorState::from_object_type(
        rules.object("TEST").unwrap(),
        0,
    ));
    assert!(matches!(
        sim.try_reveal_entity(
            1,
            RevealRequest {
                position: RevealPosition {
                    rx: 10,
                    ry: 10,
                    z: integer("level", 0) as u8,
                    sub_x: SimFixed::from_num(128),
                    sub_y: SimFixed::from_num(128)
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            }
        ),
        RevealOutcome::Revealed { .. }
    ));
    sim.remove_entity_occupancy(1);
    if flag("loaded") {
        assert_eq!(sim.allocate_stable_id(), 2);
        insert_entity(&mut sim, 2, EntityCategory::Infantry);
        sim.substrate.entities.get_mut(2).unwrap().passenger_role =
            PassengerRole::Inside { transport_id: 1 };
        let mut cargo = PassengerCargo::new(1, 0);
        cargo.board_forced(2, 1);
        sim.substrate.entities.get_mut(1).unwrap().passenger_role =
            PassengerRole::Transport { cargo };
    }
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.exact_z_leptons = Some(integer("z", 0));
    entity.on_bridge = flag("on_bridge");
    entity.health.current = integer("health", 100);
    let loco = entity.locomotor.as_mut().unwrap();
    // A deliberately stale cache must not override the physical coordinate.
    loco.altitude = SimFixed::from_num(123);
    if flag("landing") {
        loco.begin_fly_landing();
    }
    loco.set_fly_target_height(integer("target", 0));
    sim.add_entity_occupancy(1);
    (sim, rules)
}

#[test]
fn healthy_native_height_vectors_reach_production_coordinates() {
    let mut count = 0;
    for row in vectors() {
        // Health<=0 belongs to the preceding fall/crash controller, not the
        // healthy production transaction compared here. The kernel test also
        // checks those original bounded-range cases.
        if row["input"]["health"].as_i64().is_some_and(|h| h <= 0) {
            continue;
        }
        let (mut sim, rules) = fixture(&row);
        let before_rng = sim.scenario_rng.logical_state();
        sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        let entity = sim.substrate.entities.get(1).unwrap();
        let expected_z = row["z"].as_i64().unwrap() as i32;
        let expected_height = row["height"].as_i64().unwrap() as i32;
        assert_eq!(
            entity.position.exact_z_leptons,
            Some(expected_z),
            "{}",
            row["input"]
        );
        assert_eq!(
            entity.on_bridge,
            row["on_bridge"].as_bool().unwrap(),
            "{}",
            row["input"]
        );
        assert_eq!(
            entity.locomotor.as_ref().unwrap().altitude,
            SimFixed::saturating_from_num(expected_height),
            "{}",
            row["input"]
        );
        assert_eq!(
            sim.foot_navigation_coordinate(1).unwrap().z,
            expected_z,
            "{}",
            row["input"]
        );
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        count += 1;
    }
    assert_eq!(count, 132);
}

#[test]
fn fly_integer_target_flags_and_cargo_survive_save_and_continuation() {
    for name in [
        "wide_60000_65536",
        "dropship_True_200_0",
        "ordinary_True_1480",
    ] {
        let row = vectors()
            .into_iter()
            .find(|r| r["input"]["name"] == name)
            .unwrap();
        let (mut sim, rules) = fixture(&row);
        let terrain = sim.resolved_terrain.clone();
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "Fly native height continuation", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.resolved_terrain = terrain;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), sim.state_hash(), "{name}");
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .fly_runtime(),
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .fly_runtime()
        );
        for frame in 1..=8 {
            for instance in [&mut sim, &mut restored] {
                instance.session.tick = frame;
                instance.session.binary_frame = frame as u32;
                instance.tick_air_movement_with_cell_lists_one(1, Some(&rules));
            }
            assert_eq!(
                restored.state_hash(),
                sim.state_hash(),
                "{name} frame {frame}"
            );
            if frame == 1 {
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(1)
                        .unwrap()
                        .position
                        .exact_z_leptons,
                    Some(row["z"].as_i64().unwrap() as i32)
                );
            }
        }
    }
}

#[test]
fn fly_hash_distinguishes_targets_and_native_flags() {
    let row = vectors()
        .into_iter()
        .find(|r| r["input"]["name"] == "ordinary_land_100")
        .unwrap();
    let (mut sim, _) = fixture(&row);
    let baseline = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .set_fly_target_height(65536);
    let target = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .begin_fly_takeoff(0);
    let takeoff = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .begin_fly_landing();
    let landing = sim.state_hash();
    let unique = [baseline, target, takeoff, landing]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        4,
        "target and each native flag are hash-visible"
    );
}

#[test]
fn repeated_aircraft_attack_visits_do_not_divide_the_fly_target() {
    use crate::sim::aircraft::{AircraftMission, tick_aircraft_missions};
    let row = vectors()
        .into_iter()
        .find(|r| r["input"]["name"] == "ordinary_False_1500")
        .unwrap();
    let (mut sim, rules) = fixture(&row);
    for sub_state in [3, 4, 3, 4] {
        sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
            Some(AircraftMission::Attack {
                sub_state,
                has_fired: false,
                is_strafe: false,
            });
        tick_aircraft_missions(&mut sim, &rules, None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .fly_target_height(),
            1500
        );
    }
}
