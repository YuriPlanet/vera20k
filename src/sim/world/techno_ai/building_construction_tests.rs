//! Construction comparisons through the existing per-object mission owner.
//! The saved original routes supply controls and per-frame state. Full stock
//! Grand/Anim/RNG joins are covered separately; these inherited routes substitute
//! Grand_Opening and do not certify its downstream effects.

use super::*;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::BuildingUp;
use crate::sim::world::PlacementEvidence;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_construction.json",
    ))
    .unwrap()
}

fn constructed(control: [i32; 3], origin: i32) -> (Simulation, RuleSet, u64) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=B\n[B]\nStrength=100\n",
    ))
    .unwrap();
    rules.set_buildup_control_for_test("B", control);
    let mut sim = Simulation::new();
    sim.session.binary_frame = origin as u32;
    sim.session.game_options.game_speed = 0;
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(16));
    let id = sim
        .spawn_object_limbo_at_height("B", "Americans", 5, 5, 0, 0, &rules)
        .unwrap();
    assert!(
        sim.reveal_constructed_object_at_height(
            id,
            5,
            5,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            &rules,
        )
        .is_some()
    );
    (sim, rules, id)
}

/// This failed before migration: computer frame0 still had mission-1 because
/// the old construction owner ran at the frame tail, after every object's visit.
#[test]
fn construction_completes_within_its_native_object_visit() {
    let corpus = corpus();
    let mut compared = 0;
    for row in corpus["route"].as_array().unwrap() {
        let input = &row["input"];
        let route = input["route"].as_str().unwrap();
        if route == "sale" {
            continue;
        }
        let control = serde_json::from_value(input["control"].clone()).unwrap();
        let (mut sim, rules, id) = constructed(control, 0);
        let entry = match route {
            "player" => BuildingUp::placed_by_player(control, 0),
            "computer" => BuildingUp::placed_by_computer(control, 0),
            "deploy" => BuildingUp::deployed(control, 0),
            other => panic!("unknown native route {other}"),
        };
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .install_building_up(entry, 0);
        if route == "player" {
            // Original4FB4A6 ->43CD01. This inherited primitive row supplies
            // the already selected yard; the production PLACE test exercises
            // its actual selection, HELLO, C and first-contact teardown.
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .begin_building_body(crate::sim::building_construction::BuildingBodyMode::Idle, 0);
        }
        let mut completed_at = None;
        for frame in row["frames"].as_array().unwrap() {
            let now = frame["frame"].as_u64().unwrap() as u32;
            sim.session.binary_frame = now;
            sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());
            let entity = sim.substrate.entities.get(id).unwrap();
            let context = format!("{} frame{now}", input["name"]);
            assert_eq!(
                i64::from(entity.mission.current().raw()),
                frame["mission"].as_i64().unwrap(),
                "{context}: mission"
            );
            assert_eq!(
                i64::from(entity.mission.queued().raw()),
                frame["queue"].as_i64().unwrap(),
                "{context}: queue"
            );
            assert_eq!(
                u64::from(entity.mission.handler_state()),
                frame["status"].as_u64().unwrap(),
                "{context}: status"
            );
            assert_eq!(
                entity.building_body_state().map(i64::from),
                frame["bstate"].as_i64(),
                "{context}: BState"
            );
            assert_eq!(
                entity.queued_building_body_state().map(i64::from),
                frame["queued_bstate"].as_i64(),
                "{context}: queued BState"
            );
            assert_eq!(
                entity.building_actually_placed,
                frame["grand_opening"].as_bool().unwrap(),
                "{context}: opening is visible to the next object"
            );
            assert_eq!(
                u64::from(entity.building_ready_latch()),
                frame["done"].as_u64().unwrap(),
                "{context}: sole ready byte"
            );
            assert_eq!(
                i64::from(entity.construction_stage_value()),
                frame["stage"].as_i64().unwrap(),
                "{context}: stage"
            );
            if entity.building_actually_placed {
                completed_at = Some(now as i32);
                break;
            }
        }
        if route == "player" {
            assert_eq!(
                BuildingUp::player_placement_frames_to_complete(control, &sim.session.game_options),
                completed_at,
                "{}: presentation estimate",
                input["name"]
            );
        }
        compared += 1;
    }
    assert_eq!(compared, 18);
}

/// Five original Mission_Construction slices end before Update's post-ready
/// Commence and queued-body application. Execute those same production owners,
/// observing the intermediate state rather than maintaining another mission.
#[test]
fn construction_matches_the_original_mission_visits() {
    let corpus = corpus();
    let mut compared = 0;
    for row in corpus["mission"].as_array().unwrap() {
        let input = &row["input"];
        let frames = row["frames"].as_array().unwrap();
        let origin = frames[0]["timer"][0].as_i64().unwrap() as i32 - 1;
        let control = serde_json::from_value(input["control"].clone()).unwrap();
        let (mut sim, rules, id) = constructed(control, origin);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .install_building_up(BuildingUp::placed_by_computer(control, origin), origin);
        for frame in frames {
            let now = origin + frame["frame"].as_i64().unwrap() as i32;
            sim.session.binary_frame = now as u32;
            update_animation(&mut sim, id, Some(&rules));
            dispatch(&mut sim, id, Some(&rules), ObjectAiCtx::default());
            let entity = sim.substrate.entities.get(id).unwrap();
            let context = format!("{} frame{now}", input["name"]);
            assert_eq!(
                i64::from(entity.mission.current().raw()),
                frame["mission"].as_i64().unwrap(),
                "{context}: mission"
            );
            assert_eq!(
                i64::from(entity.mission.queued().raw()),
                frame["queued"].as_i64().unwrap(),
                "{context}: queue"
            );
            assert_eq!(
                u64::from(entity.mission.handler_state()),
                frame["status"].as_u64().unwrap(),
                "{context}: status"
            );
            assert_eq!(
                entity.building_body_state().map(i64::from),
                frame["bstate"].as_i64(),
                "{context}: BState"
            );
            assert_eq!(
                u64::from(entity.building_ready_latch()),
                frame["done"].as_u64().unwrap(),
                "{context}: ready"
            );
            let stage = entity.native_stage();
            assert_eq!(
                i64::from(stage.value()),
                frame["stage"].as_i64().unwrap(),
                "{context}: stage"
            );
            assert_eq!(
                i64::from(stage.rate()),
                frame["rate"].as_i64().unwrap(),
                "{context}: rate"
            );
            assert_eq!(
                [
                    i64::from(stage.timer().start_frame()),
                    i64::from(stage.timer().duration())
                ],
                [
                    frame["timer"][0].as_i64().unwrap(),
                    frame["timer"][1].as_i64().unwrap()
                ],
                "{context}: clock"
            );
            assert_eq!(
                i64::from(entity.mission.dispatch_timer().delay()),
                frame["delay"].as_i64().unwrap(),
                "{context}: dispatch"
            );
            let opening = frame["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| call[0] == "grand_opening");
            assert_eq!(
                entity.building_actually_placed, opening,
                "{context}: opening"
            );
        }
        compared += 1;
    }
    assert_eq!(compared, 5);
}
