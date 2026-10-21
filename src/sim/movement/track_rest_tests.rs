//! Connected Drive rest-speed regressions, native Process4B0828 and Stop4AFE00.
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::world::FrameEffects;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

#[test]
fn drive_stop_retains_speed_until_an_eligible_process_rest_tail() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1097);
    sim.session.set_binary_frame_for_test(100);
    let id = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let before = SimFixed::lit("0.75");
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state()
    );
    entity.foot_speed.set_speed_fraction(before);
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .store_track_target_fraction(TrackFamily::Drive, SimFixed::lit("0.9"));
    assert!(entity.navigation.nav_com().is_none());
    assert!(
        entity
            .navigation
            .path_replay()
            .remaining_directions()
            .is_empty()
    );
    let rng = rng_streams(&sim);
    sim.locomotor_stop_moving(id, &rules, None, FrameEffects::default())
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .foot_speed
            .applied_fraction(),
        before,
        "native Stop changes class target/destination; Foot speed waits for Process"
    );
    sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .foot_speed
            .applied_fraction(),
        SIM_ZERO,
        "the admitted Drive rest tail clears speed before the next order"
    );
    assert_eq!(rng_streams(&sim), rng);
}

fn fixed_bits(value: &serde_json::Value) -> SimFixed {
    SimFixed::from_num(f64::from_bits(
        u64::from_str_radix(value.as_str().unwrap(), 16).unwrap(),
    ))
}

fn coordinate(value: &serde_json::Value) -> crate::sim::components::DriveCoord {
    crate::sim::components::DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

fn scene_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=300\nSpeed=6\nROT=5\n\
         SpeedType=Track\nTurret=no\nAccelerates=yes\nAccelerationFactor=0.03\n\
         DeaccelerationFactor=0.002\nSlowdownDistance=500\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .unwrap()
}

fn scene(rules: &crate::rules::ruleset::RuleSet) -> (crate::sim::world::Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1097);
    sim.session.set_binary_frame_for_test(100);
    let id = sim
        .spawn_object("TANK", "Americans", 10, 10, 0, rules)
        .unwrap();
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state()
    );
    entity.foot_speed.set_speed_fraction(SimFixed::lit("0.75"));
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .store_track_target_fraction(TrackFamily::Drive, SimFixed::lit("0.9"));
    (sim, id)
}

#[test]
fn drive_rest_admission_matches_original_whole_process_controls() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::MissionId;
    let rules = scene_rules();
    let corpus = crate::test_fixture::json("tools/spatial_oracle/track_rest_speed.json");
    for row in corpus["process"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, id) = scene(&rules);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        let initial = fixed_bits(&row["before"]["speed_bits"]);
        if initial < SIM_ZERO {
            // An explicit raw native boundary control, outside the ordinary
            // setter's clamped domain. Seed through the saved component reader.
            let mut raw = serde_json::to_value(&entity.foot_speed).unwrap();
            raw["applied_fraction"] = serde_json::to_value(initial).unwrap();
            entity.foot_speed = serde_json::from_value(raw).unwrap();
        } else {
            entity.foot_speed.set_speed_fraction(initial);
        }
        entity.mission.set_current_for_test(
            MissionId::from_raw(input["mission"].as_i64().unwrap_or(2) as i32),
            0,
        );
        let loco = entity.locomotor.as_mut().unwrap();
        loco.store_track_destination(TrackFamily::Drive, input.get("destination").map(coordinate));
        loco.store_track_head(TrackFamily::Drive, input.get("head").map(coordinate));
        if input["nav"] == true {
            entity
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::cell(10, 10)));
        }
        if let Some(word) = input["queue"].as_i64() {
            entity
                .navigation
                .path_replay_mut()
                .set_directions_for_test(vec![word as u8]);
        }
        if input["turn"] == true {
            entity.body_facing = serde_json::from_value(serde_json::json!({
                "current":0, "prev":8192, "start_frame":100,
                "duration_frames":4, "rot_per_frame":16,
            }))
            .unwrap();
            sim.session.set_binary_frame_for_test(101);
        }
        let rng = rng_streams(&sim);
        sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
            .unwrap();
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.foot_speed.applied_fraction(),
            fixed_bits(&row["after"]["speed_bits"]),
            "{input}"
        );
        let actual = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        assert_eq!(actual, coordinate(&row["after"]["coords"]), "{input}");
        assert_eq!(rng_streams(&sim), rng, "{input}");
    }
}

/// Full original Drive visits, including fresh head and paid points; native
/// AStar supplies one east step. Rust uses its ordinary flat-map path search.
#[test]
fn drive_stop_rest_and_immediate_reorder_match_native_departure_frames() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::movement::facing_class::FacingClass;
    let rules = scene_rules();
    let corpus = crate::test_fixture::json("tools/spatial_oracle/track_rest_speed.json");
    for row in corpus["paid"].as_array().unwrap() {
        let (mut sim, id) = scene(&rules);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.body_facing = FacingClass::new(0x4000, 5);
        entity.locomotor.as_mut().unwrap().store_track_destination(
            TrackFamily::Drive,
            Some(crate::sim::components::DriveCoord::cell(11, 10, 0)),
        );
        let rng = rng_streams(&sim);
        sim.locomotor_stop_moving(id, &rules, None, FrameEffects::default())
            .unwrap();
        if row["rest"] == true {
            sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
                .unwrap();
        }
        sim.assign_destination_represented(
            id,
            Some(NavTargetRef::cell(11, 10)),
            &rules,
            None,
            FrameEffects::default(),
        )
        .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .mission
            .set_current_for_test(MissionId::from_known(MissionType::Move), 0);
        for state in row["states"].as_array().unwrap() {
            sim.session
                .set_binary_frame_for_test(state["frame"].as_u64().unwrap() as u32);
            sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
                .unwrap();
            let entity = sim.substrate.entities.get(id).unwrap();
            let actual = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let expected = coordinate(&state["coords"]);
            // Existing SimFixed speed ramp deliberately quantizes native
            // deceleration; one crossed integer budget can move two raw points.
            assert!(
                (actual.x - expected.x).abs() <= 2
                    && actual.y == expected.y
                    && actual.z == expected.z,
                "rest={} frame={} actual={actual:?} native={expected:?}",
                row["rest"],
                state["frame"]
            );
            assert!(
                (entity.foot_speed.applied_fraction() - fixed_bits(&state["speed_bits"])).abs()
                    <= SimFixed::lit("0.001")
            );
        }
        assert_eq!(rng_streams(&sim), rng);
    }
}

fn rng_streams(sim: &crate::sim::world::Simulation) -> [String; 3] {
    [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

#[test]
fn native_stop_and_null_destination_histories_preserve_frame_order() {
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::mission::{MissionId, MissionType};
    let rules = scene_rules();
    let corpus = crate::test_fixture::json("tools/spatial_oracle/track_rest_speed.json");
    for row in corpus["histories"].as_array().unwrap() {
        let (mut sim, id) = scene(&rules);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_destination(TrackFamily::Drive, Some(DriveCoord::cell(11, 10, 0)));
        let rng = rng_streams(&sim);
        if row["input"]["stop"] == "locomotor" {
            sim.locomotor_stop_moving(id, &rules, None, FrameEffects::default())
                .unwrap();
        } else {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::cell(11, 10)));
            sim.assign_destination_represented(id, None, &rules, None, FrameEffects::default())
                .unwrap();
            sim.complete_pending_order(id, &rules, None, FrameEffects::default());
        }
        for state in row["states"].as_array().unwrap().iter().skip(1) {
            match state["stage"].as_str().unwrap() {
                "rest" => {
                    sim.session.set_binary_frame_for_test(101);
                    sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
                        .unwrap();
                }
                "new_order" => {
                    sim.assign_destination_represented(
                        id,
                        Some(NavTargetRef::cell(11, 10)),
                        &rules,
                        None,
                        FrameEffects::default(),
                    )
                    .unwrap();
                    sim.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .mission
                        .set_current_for_test(MissionId::from_known(MissionType::Move), 0);
                }
                // This native boundary precedes FindPath; the separate paid
                // comparison checks its subsequent six complete Process calls.
                "first_path_request" | "stop" => {}
                other => panic!("unexpected native history stage {other}"),
            }
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .foot_speed
                    .applied_fraction(),
                fixed_bits(&state["speed_bits"]),
                "{} {}",
                row["input"],
                state["stage"]
            );
        }
        assert_eq!(rng_streams(&sim), rng);
    }
}

#[test]
fn a_callback_destination_is_published_before_the_drive_rest_decision() {
    use crate::sim::components::NavTargetRef;
    let rules = scene_rules();
    let (mut sim, id) = scene(&rules);
    // Restore_Mission owes its already-restored NavCom's class setter.
    // Native has run it before returning from PerCell to the common tail.
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::cell(11, 10)));
    entity.navigation.owe_destination_setter();
    sim.drive_process_rest_tail(id, &rules, None, FrameEffects::default());
    let entity = sim.substrate.entities.get(id).unwrap();
    assert!(
        entity
            .locomotor
            .as_ref()
            .unwrap()
            .track_destination(TrackFamily::Drive)
            .is_some()
    );
    assert_eq!(entity.foot_speed.applied_fraction(), SimFixed::lit("0.75"));
}

#[test]
fn drive_stop_rest_and_departure_survive_production_snapshot_restore() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::movement::facing_class::FacingClass;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::SnapshotProcessState;
    let rules = scene_rules();
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
    for wait in [false, true] {
        let (mut original, id) = scene(&rules);
        original.substrate.entities.get_mut(id).unwrap().body_facing = FacingClass::new(0x4000, 5);
        original.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
        original
            .restore_map_authority_after_snapshot_load(&rules, &overlays, None)
            .unwrap();
        // Native load resets Scenario from Seed0; compare that documented cursor.
        original.scenario_rng = crate::sim::rng::SimRng::new(0);
        original
            .locomotor_stop_moving(id, &rules, None, FrameEffects::default())
            .unwrap();
        let bytes = GameSnapshot::save(&original, 0, 0, "drive-rest", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_process_state_for_load(SnapshotProcessState::Live(&original));
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(
            original.resolved_terrain.as_ref().unwrap().clone(),
            original.terrain_speed_config().clone(),
            &rules,
        );
        restored
            .restore_map_authority_after_snapshot_load(&rules, &overlays, None)
            .unwrap();
        restored.resolve_type_handles(&rules);
        assert_eq!(original.state_hash(), restored.state_hash());
        for sim in [&mut original, &mut restored] {
            if wait {
                sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
                    .unwrap();
            }
            sim.assign_destination_represented(
                id,
                Some(NavTargetRef::cell(11, 10)),
                &rules,
                None,
                FrameEffects::default(),
            )
            .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .mission
                .set_current_for_test(MissionId::from_known(MissionType::Move), 0);
        }
        for frame in 101..107 {
            for sim in [&mut original, &mut restored] {
                sim.session.set_binary_frame_for_test(frame);
                sim.process_ground_locomotor_stats_for_test(id, Some(&rules), None)
                    .unwrap();
            }
            assert_eq!(
                original.state_hash(),
                restored.state_hash(),
                "wait={wait} frame={frame}"
            );
            assert_eq!(rng_streams(&original), rng_streams(&restored));
        }
    }
}
