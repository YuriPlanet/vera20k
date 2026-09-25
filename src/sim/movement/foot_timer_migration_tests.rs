//! Production timer ownership regressions. Native references are the bounded
//! path_delay_rules and track_blocked_timers corpora, not full fresh movement.
//! In particular Ship's current fresh admission adapter does not prove code2.

use super::locomotor::{LocomotorState, MovementLayer};
use super::{DestinationTiming, MovementConfig, issue_direct_move, issue_move_command};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, FootPathQueue, FootPathRuntime, Health, MovementTarget,
    ShipLocomotionRuntime,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
use serde_json::Value;

fn rules(path_delay: &str, blockage: &str, walk: bool) -> RuleSet {
    let registry = if walk {
        "InfantryTypes"
    } else {
        "VehicleTypes"
    };
    let speed = if walk { 4 } else { 0 };
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[AI]\nPathDelay={path_delay}\nBlockagePathDelay={blockage}\n\
         [{registry}]\n0=TIMER\n[TIMER]\nStrength=100\nSpeed={speed}\nAccelerates=no\n",
    )))
    .unwrap()
}

fn fixture(kind: LocomotorKind) -> Simulation {
    let mut sim = Simulation::with_seed(41);
    let owner = sim.intern("Americans");
    let type_ref = sim.intern("TIMER");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        8,
        8,
        0,
        0,
        owner,
        Health { current: 100 },
        type_ref,
        if kind == LocomotorKind::Walk {
            EntityCategory::Infantry
        } else {
            EntityCategory::Unit
        },
        0,
        5,
        false,
    );
    entity.position.sub_x = SimFixed::from_num(128);
    entity.position.sub_y = SimFixed::from_num(128);
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity.drive_accelerates = false;
    match kind {
        LocomotorKind::Drive => entity.drive_locomotion = Some(DriveLocomotionRuntime::default()),
        LocomotorKind::Ship => entity.ship_locomotion = Some(ShipLocomotionRuntime::default()),
        LocomotorKind::Walk => {}
        _ => unreachable!(),
    }
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    sim
}

fn timer_pair(entity: &GameEntity) -> (CdTimer, CdTimer, u32) {
    let path = &entity.navigation.path_runtime;
    (path.movement_timer, path.blocked_timer, path.retries_left)
}

fn json_i32(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

#[test]
fn accepted_direct_and_regular_orders_keep_signed_rules_delays_and_retry_word() {
    let native: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/path_delay_rules.json"
    ))
    .unwrap();
    let rows = native["blockage_path_delay"].as_array().unwrap();
    let grid = PathGrid::new(20, 20);
    for raw in ["0", "-1", "65536", "2147483648", "4294967295"] {
        let expected = rows
            .iter()
            .find(|row| row["initial"] == 60 && row["raw"] == raw)
            .map(|row| json_i32(&row["output"]))
            .unwrap();
        for kind in [
            LocomotorKind::Drive,
            LocomotorKind::Ship,
            LocomotorKind::Walk,
        ] {
            let rules = rules("0.01", raw, kind == LocomotorKind::Walk);
            for frame in [123, u32::MAX, i32::MAX as u32] {
                for command in 0..3 {
                    let mut sim = fixture(kind);
                    let entity = sim.substrate.entities.get_mut(1).unwrap();
                    entity.navigation.path_runtime = FootPathRuntime {
                        movement_timer: CdTimer::from_raw(17, -9),
                        blocked_timer: CdTimer::from_raw(19, 42),
                        path_blocked: true,
                        retries_left: u32::MAX,
                    };
                    let timing = DestinationTiming::from_rules(frame, Some(&rules));
                    match command {
                        0 => timing.accept(sim.substrate.entities.get_mut(1).unwrap()),
                        1 => assert!(issue_direct_move(
                            &mut sim.substrate.entities,
                            1,
                            (10, 8),
                            SimFixed::from_num(165),
                            timing,
                        )),
                        2 => assert!(issue_move_command(
                            &mut sim.substrate.entities,
                            &grid,
                            1,
                            (10, 8),
                            SimFixed::from_num(165),
                            false,
                            None,
                            None,
                            None,
                            timing,
                        )),
                        _ => unreachable!(),
                    }
                    let entity = sim.substrate.entities.get(1).unwrap();
                    assert_eq!(
                        timer_pair(entity),
                        (
                            CdTimer::started(frame as i32, 0),
                            CdTimer::started(frame as i32, expected),
                            u32::MAX,
                        ),
                        "{kind:?} command={command} frame={frame} BlockagePathDelay={raw}"
                    );
                    assert!(!entity.navigation.path_runtime.path_blocked);
                }
            }
        }
    }
}

fn install_stationary_path_request(sim: &mut Simulation) {
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.navigation.path_replay = FootPathQueue {
        directions: vec![0, 0],
        cursor: 0,
        reference_cell: Some((8, 8)),
    };
    entity.movement_target = Some(MovementTarget {
        path: vec![(8, 8), (8, 7), (8, 6)],
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        final_goal: Some((8, 6)),
        speed: SIM_ZERO,
        current_speed: SIM_ZERO,
        ..Default::default()
    });
}

#[test]
fn ordinary_drive_and_ship_process_do_not_age_native_waiting_timer_fields() {
    let native: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_blocked_timers.json"
    ))
    .unwrap();
    let rules = rules("0.01", "65536", false);
    let grid = PathGrid::new(20, 20);
    let mut checked = [0; 2];
    for row in native["rows"].as_array().unwrap() {
        let input = &row["input"];
        if input["null_head"] != false
            || input["path_result"] != 1
            || input["zone_result"] != 1
            || input["retire"] != false
            || input["timer_middle_word"] != 0
            || !matches!(
                input["name"].as_str(),
                Some("paused_movement" | "signed_wrap" | "negative_elapsed" | "negative_duration")
            )
        {
            continue;
        }
        let (kind, family) = match input["family"].as_str().unwrap() {
            "drive" => (LocomotorKind::Drive, 0),
            "ship" => (LocomotorKind::Ship, 1),
            _ => unreachable!(),
        };
        let mut sim = fixture(kind);
        install_stationary_path_request(&mut sim);
        for call in row["calls"].as_array().unwrap() {
            if call["boundary"] != "waiting_common_tail" {
                break;
            }
            let frame = json_i32(&call["frame"]);
            let expected_movement = CdTimer::from_raw(
                json_i32(&call["after"]["movement"][0]),
                json_i32(&call["after"]["movement"][2]),
            );
            let expected_blocked = CdTimer::from_raw(
                json_i32(&call["after"]["blocked_timer"][0]),
                json_i32(&call["after"]["blocked_timer"][2]),
            );
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.navigation.path_runtime = FootPathRuntime {
                movement_timer: expected_movement,
                blocked_timer: expected_blocked,
                path_blocked: true,
                retries_left: 0x8000_0001,
            };
            assert!(!expected_movement.expired(frame), "native wait branch");
            sim.session.binary_frame = frame as u32;
            sim.session.tick = u64::from(frame as u32);
            // Speed0 isolates timer ownership from paid coordinates. The live
            // Process corridor still sees an ordinary path request. This does
            // not claim execution of the corpus's supplied code2 callbacks.
            for _ in 0..2 {
                let stats = sim
                    .process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
                    .unwrap();
                assert!(stats.movers_total > 0, "ordinary {kind:?} mover visited");
                assert_eq!(
                    timer_pair(sim.substrate.entities.get(1).unwrap()),
                    (expected_movement, expected_blocked, 0x8000_0001),
                    "{kind:?} {} frame={frame}",
                    input["name"]
                );
                checked[family] += 1;
            }
        }
    }
    assert!(checked.into_iter().all(|count| count >= 10));
}

#[test]
fn rules_driven_sentinel_timers_survive_snapshot_and_same_frame_process() {
    let native: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/path_delay_rules.json"
    ))
    .unwrap();
    let expected_path = native["path_delay"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["raw"] == "0.01")
        .map(|row| json_i32(&row["ticks"]))
        .unwrap();
    let rules = rules("0.01", "65536", false);
    let grid = PathGrid::new(20, 20);
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut sim = fixture(kind);
        install_stationary_path_request(&mut sim);
        sim.session.binary_frame = u32::MAX;
        sim.session.tick = u64::from(u32::MAX);
        let timing = MovementConfig::from_rules(u32::MAX, Some(&rules));
        assert_eq!(timing.path_delay_ticks, expected_path);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        DestinationTiming::from_rules(u32::MAX, Some(&rules)).accept(entity);
        entity
            .navigation
            .path_runtime
            .start_movement(u32::MAX, timing.path_delay_ticks);
        entity.navigation.path_runtime.retries_left = 0x8000_0001;
        let expected = timer_pair(entity);
        assert_eq!(expected.0, CdTimer::from_raw(-1, expected_path));
        assert_eq!(expected.1, CdTimer::from_raw(-1, 65536));
        // Native in-scenario load resets Scenario RNG to Seed0. Compare timer
        // persistence with both simulations on that same lifecycle boundary.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 1, rules.simulation_config_hash(), "Foot timers", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(restored.state_hash(), sim.state_hash());
        for frame in [u32::MAX, u32::MAX, 0, 0] {
            for instance in [&mut sim, &mut restored] {
                instance.session.binary_frame = frame;
                instance
                    .process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
                    .unwrap();
                let entity = instance.substrate.entities.get(1).unwrap();
                assert_eq!(timer_pair(entity), expected);
                assert_eq!(
                    entity
                        .navigation
                        .path_runtime
                        .movement_timer
                        .remaining(frame as i32),
                    expected_path
                );
                assert_eq!(
                    entity
                        .navigation
                        .path_runtime
                        .blocked_timer
                        .remaining(frame as i32),
                    65536
                );
            }
            assert_eq!(restored.state_hash(), sim.state_hash());
        }
    }
}

#[test]
fn paid_walk_progress_clears_only_blocked_latch_and_retains_timer_words() {
    let rules = rules("0.01", "65536", true);
    let grid = PathGrid::new(20, 20);
    let mut sim = fixture(LocomotorKind::Walk);
    sim.session.binary_frame = 107;
    sim.session.tick = 107;
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.facing = 64;
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .set_step_head(Some(DriveCoord {
            x: 8 * 256 + 192,
            y: 8 * 256 + 128,
            z: 0,
        }));
    entity.movement_target = Some(MovementTarget {
        path: vec![(8, 8), (9, 8)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        final_goal: Some((9, 8)),
        speed: SimFixed::from_num(165),
        current_speed: SimFixed::from_num(165),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });
    entity
        .navigation
        .path_runtime
        .start_blocked(101, rules.general.blockage_path_delay_ticks);
    entity.navigation.path_runtime.path_blocked = true;
    entity.navigation.path_runtime.retries_left = 0x8000_0001;
    let expected_blocked = entity.navigation.path_runtime.blocked_timer;
    let before = super::ground_pose::position_world_xy(&entity.position);
    sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
        .unwrap();
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_ne!(
        super::ground_pose::position_world_xy(&entity.position),
        before,
        "paid Walk step reached"
    );
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (8, 8),
        "no boundary receiver needed"
    );
    assert!(!entity.navigation.path_runtime.path_blocked);
    assert_eq!(
        entity.navigation.path_runtime.blocked_timer,
        expected_blocked
    );
    assert_eq!(entity.navigation.path_runtime.retries_left, 0x8000_0001);
}
