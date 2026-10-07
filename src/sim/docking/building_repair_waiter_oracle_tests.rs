//! Original GADEPT Mark/MTNK waiter boundary comparisons at levels0/4.
//!
//! building_repair.depot_waiters.{json,md} declares admitted objects, map
//! geometry and the one supplied FindPath route. Mark, Unit entry, parking,
//! RNG/timers, NULL destination and radio cleanup use production owners.
//! This bounded comparison does not certify native crowded travel/Logic AI.

use super::*;
use crate::sim::docking::building_dock::try_pending_entry;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;

fn waiter_corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_repair.depot_waiters.json",
    ))
    .unwrap()
}

fn waiter_scene(golden: &Value, case: &Value, control: &Value) -> Scene {
    scene(
        golden,
        &json!({
            "input": case["input"], "unit": "MTNK", "building": "GADEPT",
            "before": control["before"], "rng_before": control["rng_before"],
            "unit_dispatch_before": control["unit_dispatch_before"],
            "foot_before": control["foot_before"]
        }),
    )
}

fn compare_foot(scene: &Scene, expected: &Value) {
    let actor = scene.sim.substrate.entities.get(scene.tank).unwrap();
    let path = &actor.navigation.path_runtime;
    let drive = actor
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|runtime| runtime.retained())
        .unwrap();
    let head = drive.head_to().unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
    assert_eq!(actor.navigation.path_replay.cursor, 0);
    let words: Vec<i32> = actor
        .navigation
        .path_replay
        .directions
        .iter()
        .map(|&word| if word == u8::MAX { -1 } else { i32::from(word) })
        .chain(std::iter::repeat(-1))
        .take(24)
        .collect();
    assert_eq!(
        json!({
            "movement_timer": [path.movement_timer.start_frame(), path.movement_timer.duration()],
            "blocked_timer": [path.blocked_timer.start_frame(), path.blocked_timer.duration()],
            "retries_left": path.retries_left, "path_blocked": path.path_blocked,
            "nav_queue_count": actor.navigation.nav_queue.len(), "path": words,
            "head": [head.x, head.y, head.z], "selector": drive.track().turn_index,
            "valid": u8::from(drive.track_valid()), "in_playfield": actor.in_playfield,
            "mission_only": actor.is_mission_only()
        }),
        *expected,
        "original Foot/Drive state projection"
    );
}

#[test]
fn native_marked_foundation_entry_parking_and_retry_match_shared_owners() {
    let golden = waiter_corpus();
    assert_eq!(golden["cases"].as_array().unwrap().len(), 2);
    for case in golden["cases"].as_array().unwrap() {
        let mark = json!({"before": case["marked"]["after"], "rng_before": case["mark_rng"]});
        let mut marked = waiter_scene(&golden, case, &mark);
        marked.compare(&mark["before"], "native marked prestate");
        assert_eq!(
            marked.rules.general.close_enough,
            int(&golden["inputs"]["after"]["close_enough"])
        );
        for cell in case["mark_after"].as_array().unwrap() {
            let at = &cell["cell"];
            assert_eq!(
                u32::from(
                    marked
                        .sim
                        .substrate
                        .raw_cell_occupation
                        .ground_bits(int(&at[0]) as u16, int(&at[1]) as u16,)
                ),
                cell["ground"].as_u64().unwrap() as u32,
                "production Building Mark's complete foundation"
            );
        }
        for entry in case["entry_cells"].as_array().unwrap() {
            let at = &entry["cell"];
            let terrain = marked.sim.resolved_terrain.as_ref().unwrap();
            let result = marked
                .sim
                .foot_can_enter(
                    marked.tank,
                    terrain.native_cell_identity((int(&at[0]) as i16, int(&at[1]) as i16)),
                    InfantryEntryArgs {
                        direction: int(&entry["args"][0]),
                        height: int(&entry["args"][1]),
                        previous_cell: None,
                    },
                    &marked.rules,
                    None,
                )
                .unwrap();
            assert_eq!(
                u64::from(result),
                entry["result"].as_u64().unwrap(),
                "native Unit73F0A0 {at}"
            );
        }
        marked.sim.remove_entity_occupancy(marked.depot);
        for cell in case["removal"]["after"].as_array().unwrap() {
            let at = &cell["cell"];
            assert_eq!(
                u32::from(
                    marked
                        .sim
                        .substrate
                        .raw_cell_occupation
                        .ground_bits(int(&at[0]) as u16, int(&at[1]) as u16)
                ),
                cell["ground"].as_u64().unwrap() as u32,
                "production Building Mark0 cleanup"
            );
        }
        for control in case["controls"].as_array().unwrap() {
            let name = control["name"].as_str().unwrap();
            if name == "shared_goal_destination" {
                // Its stationary second MTNK precondition belongs to the
                // composed Drive comparison below, not this single waiter.
                continue;
            }
            let mut scene = waiter_scene(&golden, case, control);
            scene.compare(&control["before"], name);
            match name {
                "queue_enter" => {
                    scene
                        .sim
                        .mission_queue_exact(
                            scene.tank,
                            MissionId::from_known(MissionType::Enter),
                            1,
                            scene.sim.session.binary_frame,
                            &LiveReadyInputProvider {
                                rules: &scene.rules,
                            },
                        )
                        .unwrap();
                }
                "busy_destination" => {
                    scene.sim.set_unit_destination(
                        scene.tank,
                        NavTargetRef::Building { id: scene.depot },
                        &scene.rules,
                        true,
                    );
                }
                "park_dispatch" => {
                    crate::sim::world::dispatch_foot_mission(
                        &mut scene.sim,
                        scene.tank,
                        &scene.rules,
                        ObjectAiCtx::default(),
                    );
                }
                "busy_retry"
                | "free_outside_retry"
                | "free_foundation_retry"
                | "blocked_goal_foundation_retry" => {
                    try_pending_entry(&mut scene.sim, &scene.rules, scene.tank);
                }
                "release_by_break" | "blocked_goal_release_by_break" => {
                    radio::transmit(
                        &mut scene.sim,
                        scene.other.unwrap(),
                        scene.depot,
                        RadioMessage::Break,
                        RadioPayload::default(),
                        Some(&scene.rules),
                    );
                }
                "null_destination_on_foundation" => {
                    scene
                        .sim
                        .set_unit_null_destination(scene.tank, Some(&scene.rules), None);
                }
                other => panic!("unrepresented original waiter control {other}"),
            }
            scene.compare(&control["after"], name);
            if control["unit_dispatch_after"].is_array() {
                let timer = scene
                    .sim
                    .substrate
                    .entities
                    .get(scene.tank)
                    .unwrap()
                    .mission
                    .dispatch_timer();
                assert_eq!(
                    json!([timer.start_frame(), timer.delay()]),
                    control["unit_dispatch_after"],
                    "{name} native dispatcher"
                );
            }
        }
    }
}

#[test]
fn native_actual_ally_entry_near_stop_preserves_pending_until_foundation_refusal() {
    use crate::sim::components::FootPathQueue;
    use crate::sim::movement::{
        ProcessMovementArgs, fresh_oracle_seam, track_process::TrackFamily,
    };

    let golden = waiter_corpus();
    let case = &golden["cases"][1];
    let native = &case["blocked_goal"];
    let mut scene = waiter_scene(&golden, case, native);
    let peer = scene
        .sim
        .spawn_object("MTNK", "Russians", 8, 12, 0, &scene.rules)
        .unwrap();
    // Native supplies an admitted peer plus its original Drive constructor,
    // excluding complete Unit construction. Bind the actual pre-Process RNG
    // after Rust constructs that same prepared occupant; no spawn-RNG claim.
    scene.sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(native["rng_before"]["scenario"].as_str().unwrap());
    assert!(
        scene
            .sim
            .substrate
            .entities
            .get(peer)
            .unwrap()
            .navigation
            .nav_com
            .is_none()
    );
    let terrain = scene.sim.resolved_terrain.as_ref().unwrap();
    let entry = scene
        .sim
        .foot_can_enter(
            scene.tank,
            terrain.native_cell_identity((8, 12)),
            InfantryEntryArgs {
                direction: 4,
                height: 4,
                previous_cell: None,
            },
            &scene.rules,
            None,
        )
        .unwrap();
    assert_eq!(
        u64::from(entry),
        native["stationary_ally_entry"].as_u64().unwrap()
    );
    {
        // Exact pre-Process native route/facing priors; no prior travel claim.
        let actor = scene.sim.substrate.entities.get_mut(scene.tank).unwrap();
        actor.navigation.path_replay = FootPathQueue {
            directions: native["foot_before"]["path"]
                .as_array()
                .unwrap()
                .iter()
                .map_while(|v| (int(v) != -1).then(|| int(v) as u8))
                .collect(),
            ..Default::default()
        };
        actor.body_facing = crate::sim::movement::FacingClass::new(0x8000, 0);
    }
    compare_foot(&scene, &native["foot_before"]);
    scene.compare(&native["before"], "blocked-goal prepared prestate");
    let distance = native["trace"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["caller"] == "0x4b313b")
        .unwrap();
    assert_eq!(
        crate::sim::cell_kernel::native_xyz_distance(0, -256, 0),
        int(&distance["returned_eax"])
    );
    fresh_oracle_seam::install_path_only(vec![fresh_oracle_seam::SuppliedPath::Found(vec![
        4, 4, 4,
    ])]);
    let result = scene.sim.run_track_process_movement(
        scene.tank,
        TrackFamily::Drive,
        ProcessMovementArgs::OUTER,
        None,
        &scene.rules,
        None,
    );
    let (calls, unused) = fresh_oracle_seam::finish();
    result.unwrap();
    assert_eq!(unused, 0);
    assert_eq!(
        calls
            .iter()
            .filter(|r| matches!(r, fresh_oracle_seam::FreshCallRecord::FindPath { .. }))
            .count(),
        native["requests"].as_array().unwrap().len()
    );
    scene.compare(&native["after"], "original Drive blocked-goal response");
    compare_foot(&scene, &native["foot_after"]);
    radio::transmit(
        &mut scene.sim,
        scene.other.unwrap(),
        scene.depot,
        RadioMessage::Break,
        RadioPayload::default(),
        Some(&scene.rules),
    );
    try_pending_entry(&mut scene.sim, &scene.rules, scene.tank);
    let refused = case["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "blocked_goal_foundation_retry")
        .unwrap();
    scene.compare(&refused["after"], "original retry after blocked-goal stop");
}
