//! Bounded comparisons with original gamemd entry and turn-latch execution.
//! The corpus metadata records executable SHA and callback substitutions.
//! The 120 sampler rows establish the latch transition, including the state
//! visible before reason0. Supplied callback mutations are not Rust world
//! parity: this production corridor has no callback-observation seam yet.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, ShipLocomotionRuntime, TrackProgress,
};
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::facing_class::FacingClass;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::snapshot::GameSnapshot;
use serde_json::{Value, json};

fn entry_cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_process_entry.json"
    ))
    .expect("original Drive/Ship TrackProcess entry corpus")
}

fn turn_cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_turn_latch.json"
    ))
    .expect("original Drive/Ship Process turn corpus")
}

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("signed corpus integer")).unwrap()
}

fn bit(value: &Value) -> bool {
    match integer(value) {
        0 => false,
        1 => true,
        other => panic!("non-boolean native byte {other}"),
    }
}

fn family(input: &Value) -> LocomotorKind {
    match input["family"].as_str().unwrap() {
        "drive" => LocomotorKind::Drive,
        "ship" => LocomotorKind::Ship,
        other => panic!("unexpected native family {other}"),
    }
}

fn retained(entity: &GameEntity, kind: LocomotorKind) -> (bool, bool, TrackProgress) {
    match kind {
        LocomotorKind::Drive => {
            let state = entity.drive_locomotion.as_ref().unwrap();
            (state.track_valid, state.turn_latched, state.track)
        }
        LocomotorKind::Ship => {
            let state = entity.ship_locomotion.as_ref().unwrap();
            (state.track_valid, state.turn_latched, state.track)
        }
        _ => unreachable!(),
    }
}

fn set_retained(
    entity: &mut GameEntity,
    kind: LocomotorKind,
    valid: bool,
    latch: bool,
    track: TrackProgress,
) {
    match kind {
        LocomotorKind::Drive => {
            let state = entity.drive_locomotion.as_mut().unwrap();
            state.track_valid = valid;
            state.turn_latched = latch;
            state.track = track;
        }
        LocomotorKind::Ship => {
            let state = entity.ship_locomotion.as_mut().unwrap();
            state.track_valid = valid;
            state.turn_latched = latch;
            state.track = track;
        }
        _ => unreachable!(),
    }
}

fn fixture(kind: LocomotorKind) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    let mut entity = GameEntity::test_default(1, "TURN", "Americans", 8, 8);
    entity.type_ref = sim.intern("TURN");
    entity.owner = sim.intern("Americans");
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    match kind {
        LocomotorKind::Drive => entity.drive_locomotion = Some(DriveLocomotionRuntime::default()),
        LocomotorKind::Ship => entity.ship_locomotion = Some(ShipLocomotionRuntime::default()),
        _ => unreachable!(),
    }
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy =
        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
    sim
}

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TURN\n[TURN]\nTurret=yes\nSpeed=0\nAccelerates=no\n",
    ))
    .unwrap()
}

fn event<'a>(case: &'a Value, name: &str) -> Option<&'a Value> {
    case["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["event"] == name)
}

#[test]
fn track_entry_matches_all_144_native_rows_and_only_changes_rejected_residual() {
    let cases = entry_cases();
    assert_eq!(cases.len(), 144);
    let mut admitted = 0;
    for (index, case) in cases.iter().enumerate() {
        let input = &case["input"];
        let kind = family(input);
        let mut sim = fixture(kind);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let track = TrackProgress {
            turn_index: integer(&input["selector"]),
            cursor: 19,
            reversed: true,
            residual: integer(&case["residual_before"]),
        };
        set_retained(
            entity,
            kind,
            bit(&input["track_valid"]),
            bit(&input["class_flag_62"]),
            track,
        );
        // A consumed prefix must not become the native live queue head.
        entity.navigation.path_replay.directions = vec![8, integer(&input["queue_head"]) as u8, 6];
        entity.navigation.path_replay.cursor = 1;
        let mut expected = serde_json::to_value(&*entity).unwrap();
        let field = if kind == LocomotorKind::Drive {
            "drive_locomotion"
        } else {
            "ship_locomotion"
        };
        expected[field]["track"]["residual"] = case["residual_after"].clone();
        let actual = admit_track_entry(entity, bit(&input["turret"]));
        assert_eq!(
            actual,
            case["prefix_eligible"].as_bool().unwrap(),
            "entry {index}: {input}"
        );
        assert_eq!(
            serde_json::to_value(&*entity).unwrap(),
            expected,
            "entry {index}: {input}"
        );
        admitted += usize::from(actual);
    }
    assert_eq!(admitted, 60);
}

#[test]
fn retained_sampler_matches_all_120_native_sampler_rows_before_external_callback() {
    let cases = turn_cases();
    assert_eq!(cases.len(), 136);
    let mut sampled = 0;
    let mut callbacks = 0;
    let mut bypassed = 0;
    for (index, case) in cases.iter().enumerate() {
        let Some(native_sample) = event(case, "sample_return") else {
            assert_eq!(case["boundary"], "active_track_dispatch", "row {index}");
            bypassed += 1;
            continue;
        };
        sampled += 1;
        let mut latch = bit(&case["before"]["latch"]);
        let completed = sample(&mut latch, bit(&native_sample["value"]));
        let callback = event(case, "completion_callback");
        assert_eq!(
            completed,
            callback.is_some(),
            "sampler {index}: {}",
            case["input"]
        );
        if let Some(callback) = callback {
            callbacks += 1;
            assert_eq!(integer(&callback["reason"]), 0, "row {index}");
            assert_eq!(
                latch,
                bit(&callback["state"]["latch"]),
                "pre-callback row {index}"
            );
            assert!(!latch, "native clears +62 before reason0 callback");
        } else {
            assert_eq!(latch, bit(&case["after"]["latch"]), "row {index}");
        }
    }
    assert_eq!((sampled, callbacks, bypassed), (120, 50, 16));
}

#[test]
fn live_facing_sampler_matches_108_representable_native_rows() {
    let mut compared = 0;
    let mut unsupported_profiles = 0;
    for (index, case) in turn_cases().iter().enumerate() {
        let Some(native_sample) = event(case, "sample_return") else {
            continue;
        };
        let profile = &case["profile"];
        // Current FacingClass uses an unsigned rate and None means no timer.
        // Native signed negative rate and paused start=-1 need a separate
        // primitive migration; do not reinterpret these profiles to make parity.
        if integer(&profile["rate"]) < 0 || integer(&profile["start"]) == -1 {
            unsupported_profiles += 1;
            continue;
        }
        let facing: FacingClass = serde_json::from_value(json!({
            "current": 0,
            "prev": profile.get("desired").and_then(Value::as_u64).unwrap_or(8192),
            "start_frame": profile["start"],
            "duration_frames": profile["duration"],
            "rot_per_frame": profile["rate"],
        }))
        .unwrap();
        let frame = u32::try_from(profile["frame"].as_u64().unwrap()).unwrap();
        assert_eq!(
            facing.is_rotating(frame),
            bit(&native_sample["value"]),
            "native sampler row {index}"
        );
        compared += 1;
    }
    assert_eq!((compared, unsupported_profiles), (108, 12));
}

#[test]
fn active_dispatch_matches_all_48_native_gate_rows_without_requiring_head() {
    let mut compared = 0;
    let mut bypassed = 0;
    for case in turn_cases() {
        let input = &case["input"];
        if input["stage"] != "active_gate" {
            continue;
        }
        let kind = family(input);
        let mut sim = fixture(kind);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        set_retained(
            entity,
            kind,
            bit(&input["track_valid"]),
            bit(&input["latch"]),
            TrackProgress {
                turn_index: integer(&input["selector"]),
                ..Default::default()
            },
        );
        let active = super::super::track_head::active_track_family(entity).is_some();
        assert_eq!(
            active,
            case["boundary"] == "active_track_dispatch",
            "{input}"
        );
        assert!(super::super::track_head::committed_track_head(entity).is_none());
        compared += 1;
        bypassed += usize::from(active);
    }
    assert_eq!((compared, bypassed), (48, 16));
}

fn setup_turn(sim: &mut Simulation, kind: LocomotorKind, live: bool, latch: bool, active: bool) {
    sim.session.binary_frame = if live { 101 } else { 132 };
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let mut facing = FacingClass::new(0, 1);
    facing.set(0x2000, 100);
    assert_eq!(facing.is_rotating(sim.session.binary_frame), live);
    entity.body_facing = Some(facing);
    set_retained(
        entity,
        kind,
        active,
        latch,
        TrackProgress {
            turn_index: if active { 0 } else { -1 },
            cursor: 0,
            reversed: false,
            residual: if active { 0 } else { 4 },
        },
    );
    if active {
        let head = Some(DriveCoord {
            x: 8 * 256 + 128,
            y: 7 * 256 + 128,
            z: 0,
        });
        match kind {
            LocomotorKind::Drive => entity.drive_locomotion.as_mut().unwrap().head_to = head,
            LocomotorKind::Ship => entity.ship_locomotion.as_mut().unwrap().head_to = head,
            _ => unreachable!(),
        }
    }
}

#[test]
fn actual_ground_process_samples_disagreement_and_active_tracks_bypass_it() {
    let rules = rules();
    let grid = PathGrid::new(20, 20);
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for live in [false, true] {
            for active in [false, true] {
                let mut sim = fixture(kind);
                setup_turn(&mut sim, kind, live, !live, active);
                let position = super::super::ground_pose::position_world_coord(
                    &sim.substrate.entities.get(1).unwrap().position,
                );
                let stats = sim
                    .process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
                    .unwrap();
                let entity = sim.substrate.entities.get(1).unwrap();
                let (valid, latch, track) = retained(entity, kind);
                assert_eq!(
                    latch,
                    if active { !live } else { live },
                    "{kind:?} live={live} active={active}"
                );
                assert_eq!(valid, active);
                assert_eq!(
                    super::super::ground_pose::position_world_coord(&entity.position),
                    position
                );
                assert_eq!(track.residual, if active { 0 } else { 4 });
                assert_eq!(track.cursor, 0);
                assert_eq!(stats.movers_total, u32::from(active));
            }
        }
    }
}

#[test]
fn actual_turn_completion_reason_zero_does_not_promote_queued_mission() {
    let rules = rules();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut sim = fixture(kind);
        setup_turn(&mut sim, kind, false, true, false);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .mission
            .apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(MissionType::Move),
                suspended: MissionId::NONE,
                queued: MissionId::from_known(MissionType::Unload),
                movement_bypass_latch: 0,
                handler_state: 4,
                mission_start_frame: 3,
                ai_counter: 11,
                dispatch_timer: MissionDispatchTimer::from_raw(3, 90),
            });
        let before = sim.substrate.entities.get(1).unwrap().mission;
        sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
            .unwrap();
        assert!(!retained(sim.substrate.entities.get(1).unwrap(), kind).1);
        assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);
        // Positive control: the same receiver with arrival reason2 must
        // promote this fixture, without dispatching the new mission handler.
        sim.unit_track_per_cell(1, PerCellReason::Arrival, Some(&rules), None);
        let mission = sim.substrate.entities.get(1).unwrap().mission;
        assert_eq!(mission.current().known(), Some(MissionType::Unload));
        assert_eq!(mission.queued(), MissionId::NONE);
        assert_eq!(mission.handler_state(), 0);
        assert_eq!(mission.ai_counter(), 0);
    }
}

#[test]
fn actual_process_exact_destination_bypasses_sampler_for_guard_but_not_move() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for mission in [MissionType::Guard, MissionType::Move] {
            let mut sim = fixture(kind);
            setup_turn(&mut sim, kind, true, false, false);
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            let destination = Some(super::super::ground_pose::position_world_coord(
                &entity.position,
            ));
            match kind {
                LocomotorKind::Drive => {
                    entity.drive_locomotion.as_mut().unwrap().destination = destination
                }
                LocomotorKind::Ship => {
                    entity.ship_locomotion.as_mut().unwrap().destination = destination
                }
                _ => unreachable!(),
            }
            entity.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_known(mission),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(0, 0),
            });
            sim.process_ground_locomotor_for_test(1, None, None, None)
                .unwrap();
            assert_eq!(
                retained(sim.substrate.entities.get(1).unwrap(), kind).1,
                mission == MissionType::Move,
                "{kind:?} {mission:?}"
            );
        }
    }
}

#[test]
fn both_family_latches_survive_serde_snapshot_and_change_current_hash() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut sim = fixture(kind);
        setup_turn(&mut sim, kind, false, false, false);
        let unlatched_hash = sim.state_hash();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let (valid, _, track) = retained(entity, kind);
        set_retained(entity, kind, valid, true, track);
        match kind {
            LocomotorKind::Drive => {
                let state = entity.drive_locomotion.as_ref().unwrap();
                let restored: DriveLocomotionRuntime =
                    bincode::deserialize(&bincode::serialize(state).unwrap()).unwrap();
                assert_eq!(&restored, state);
            }
            LocomotorKind::Ship => {
                let state = entity.ship_locomotion.as_ref().unwrap();
                let restored: ShipLocomotionRuntime =
                    bincode::deserialize(&bincode::serialize(state).unwrap()).unwrap();
                assert_eq!(&restored, state);
            }
            _ => unreachable!(),
        }
        let latched_hash = sim.state_hash();
        assert_ne!(
            latched_hash, unlatched_hash,
            "{kind:?} latch is deterministic state"
        );
        let bytes = GameSnapshot::save(&sim, 1, 2, "turn latch", 3);
        let restored = GameSnapshot::load(&bytes).unwrap().sim;
        let before = sim.substrate.entities.get(1).unwrap();
        let after = restored.substrate.entities.get(1).unwrap();
        assert_eq!(after.drive_locomotion, before.drive_locomotion);
        assert_eq!(after.ship_locomotion, before.ship_locomotion);
        assert_eq!(after.body_facing, before.body_facing);
        assert_eq!(restored.state_hash(), latched_hash);
    }
}

#[test]
fn actual_entry_turn_gate_precedes_speed_and_points_for_both_families() {
    use crate::util::fixed_math::SimFixed;
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for turret in [false, true] {
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[VehicleTypes]\n0=TURN\n[TURN]\nTurret={}\nSpeed=6\nAccelerates=no\n",
                if turret { "yes" } else { "no" }
            )))
            .unwrap();
            let mut sim = fixture(kind);
            setup_turn(&mut sim, kind, false, true, true);
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.drive_accelerates = false;
            entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
            entity.foot_speed.cached_current_speed = 123;
            let (valid, latch, mut track) = retained(entity, kind);
            track.residual = 17;
            set_retained(entity, kind, valid, latch, track);
            match kind {
                LocomotorKind::Drive => {
                    entity
                        .drive_locomotion
                        .as_mut()
                        .unwrap()
                        .target_speed_fraction = SimFixed::ONE
                }
                LocomotorKind::Ship => {
                    entity
                        .ship_locomotion
                        .as_mut()
                        .unwrap()
                        .target_speed_fraction = SimFixed::ONE
                }
                _ => unreachable!(),
            }
            let before = super::super::ground_pose::position_world_coord(&entity.position);
            sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
                .unwrap();
            let entity = sim.substrate.entities.get(1).unwrap();
            let (_, latch, track) = retained(entity, kind);
            assert!(
                latch,
                "active Process must not sample the expired live timer"
            );
            if turret {
                assert_eq!(entity.foot_speed.applied_fraction, SimFixed::ONE);
                assert!(track.cursor > 0, "admitted prefix must reach paid points");
            } else {
                assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.25"));
                assert_eq!(entity.foot_speed.cached_current_speed, 123);
                assert_eq!((track.cursor, track.residual), (0, 0));
                assert_eq!(
                    super::super::ground_pose::position_world_coord(&entity.position),
                    before
                );
            }
        }
    }
}

#[test]
fn ordinary_fresh_turn_and_drive_refusal_reach_entry_without_running_speed() {
    use crate::sim::components::{FootPathQueue, MovementTarget};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::util::fixed_math::SimFixed;

    let native: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_outer_entry_continuation.json"
    ))
    .unwrap();

    // The composed native outer/entry witness separates this reached fresh
    // return from the live-rotation Process return tested below. AL is ignored;
    // invalid descriptor/ordinary queue clears residual before the speed prefix.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TURN\n[TURN]\nStrength=100\nSpeed=6\nAccelerates=no\n",
    ))
    .unwrap();
    let grid = PathGrid::new(20, 20);
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for refused in [false, true] {
            // The old fresh Ship adapter still lacks CanEnter dispatch. Its
            // migration has separate acceptance; do not invent a Ship refusal.
            if refused && kind == LocomotorKind::Ship {
                continue;
            }
            for residual in [14, -2] {
                let mut sim = fixture(kind);
                let entity = sim.substrate.entities.get_mut(1).unwrap();
                entity.category = crate::map::entities::EntityCategory::Unit;
                entity.facing = if refused { 64 } else { 0 };
                entity.body_facing = None;
                entity.facing_target = None;
                entity.drive_accelerates = false;
                entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
                entity.foot_speed.cached_current_speed = 123;
                let speed_before = entity.foot_speed.clone();
                entity.navigation.nav_com = Some(NavTargetRef::cell(10, 8));
                entity.navigation.path_replay = FootPathQueue {
                    directions: vec![2, 2],
                    cursor: 0,
                    reference_cell: Some((8, 8)),
                };
                entity.movement_target = Some(MovementTarget {
                    path: vec![(8, 8), (9, 8), (10, 8)],
                    path_layers: vec![MovementLayer::Ground; 3],
                    next_index: 1,
                    speed: SimFixed::from_num(330),
                    current_speed: SimFixed::from_num(330),
                    final_goal: Some((10, 8)),
                    ..Default::default()
                });
                set_retained(
                    entity,
                    kind,
                    false,
                    false,
                    TrackProgress {
                        residual,
                        ..Default::default()
                    },
                );
                let target_fraction = SimFixed::lit("0.375");
                match kind {
                    LocomotorKind::Drive => {
                        entity
                            .drive_locomotion
                            .as_mut()
                            .unwrap()
                            .target_speed_fraction = target_fraction;
                    }
                    LocomotorKind::Ship => {
                        entity
                            .ship_locomotion
                            .as_mut()
                            .unwrap()
                            .target_speed_fraction = target_fraction;
                    }
                    _ => unreachable!(),
                }
                let before = super::super::ground_pose::position_world_coord(&entity.position);
                if refused {
                    sim.substrate.cell_occupation.mark_vehicle_on_layer(
                        9,
                        8,
                        99,
                        MovementLayer::Ground,
                    );
                }
                let stats = sim
                    .process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
                    .unwrap();
                let entity = sim.substrate.entities.get(1).unwrap();
                let (valid, _, track) = retained(entity, kind);
                assert_eq!(stats.moved_steps, 0, "{kind:?} refused={refused}");
                assert_eq!(stats.selection_admission_refusals, u32::from(refused));
                assert!(!valid);
                assert_eq!(track.turn_index, -1);
                let native_rows: Vec<_> = native["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| {
                        let input = &row["input"];
                        family(input) == kind
                            && integer(&input["residual"]) == residual
                            && integer(&input["valid"]) == 0
                            && integer(&input["selector"]) == -1
                            && integer(&input["queue_head"]) == 2
                            && integer(&input["latch"]) == 0
                            && integer(&input["turret"]) == 0
                            && integer(&input["alive"]) == 1
                            && integer(&input["output"]) == 0
                    })
                    .collect();
                assert_eq!(native_rows.len(), 2, "both native fresh AL returns");
                for native in native_rows {
                    assert_eq!(native["track_called"], true);
                    assert_eq!(native["prefix_eligible"], false);
                    assert_eq!(
                        track.residual,
                        integer(&native["after"]["residual"]),
                        "{kind:?} refused={refused} before={residual}"
                    );
                }
                assert_eq!(entity.foot_speed, speed_before);
                let retained_fraction = match kind {
                    LocomotorKind::Drive => {
                        entity
                            .drive_locomotion
                            .as_ref()
                            .unwrap()
                            .target_speed_fraction
                    }
                    LocomotorKind::Ship => {
                        entity
                            .ship_locomotion
                            .as_ref()
                            .unwrap()
                            .target_speed_fraction
                    }
                    _ => unreachable!(),
                };
                assert_eq!(retained_fraction, target_fraction);
                assert_eq!(
                    super::super::ground_pose::position_world_coord(&entity.position),
                    before,
                );
                assert_eq!(
                    sim.substrate.raw_cell_occupation.ground_bits(9, 8) & 0x20,
                    0
                );
                if !refused {
                    assert_eq!(entity.facing_target, Some(64));
                    assert_eq!(
                        entity.navigation.path_replay.remaining_directions(),
                        &[2, 2]
                    );
                }
            }
        }
    }
}

#[test]
fn live_rotation_returns_before_fresh_selection_with_a_queued_path_and_no_display_target() {
    use crate::sim::components::{FootPathQueue, MovementTarget};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::util::fixed_math::SimFixed;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TURN\n[TURN]\nSpeed=6\nAccelerates=no\n",
    ))
    .unwrap();
    let grid = PathGrid::new(20, 20);
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut sim = fixture(kind);
        setup_turn(&mut sim, kind, true, false, false);
        let cells = (0..20)
            .flat_map(|ry| {
                (0..20).map(move |rx| {
                    let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
                    if (rx, ry) == (8, 8) {
                        cell.slope_type = 5;
                    }
                    cell
                })
            })
            .collect();
        sim.resolved_terrain =
            Some(crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(20, 20, cells));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(8, 8)
                .unwrap()
                .slope_type,
            5
        );
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .active_slope_transition_mut()
            .unwrap()
            .snap(2, 100);
        entity.drive_accelerates = false;
        entity.facing_target = None;
        entity.navigation.nav_com = Some(NavTargetRef::cell(8, 7));
        entity.navigation.path_replay = FootPathQueue {
            directions: vec![0, 0],
            cursor: 0,
            reference_cell: Some((8, 8)),
        };
        entity.movement_target = Some(MovementTarget {
            path: vec![(8, 8), (8, 7), (8, 6)],
            path_layers: vec![MovementLayer::Ground; 3],
            next_index: 1,
            speed: SimFixed::from_num(330),
            current_speed: SimFixed::from_num(330),
            final_goal: Some((8, 6)),
            ..Default::default()
        });
        let before_position = super::super::ground_pose::position_world_coord(&entity.position);
        let before_facing = serde_json::to_value(&entity.body_facing).unwrap();
        let before_queue = entity.navigation.path_replay.clone();
        let before_track = retained(entity, kind).2;
        let stats = sim
            .process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
            .unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            stats.movers_total, 0,
            "{kind:?}: no movement preparation during live turn"
        );
        assert_eq!(
            super::super::ground_pose::position_world_coord(&entity.position),
            before_position
        );
        assert_eq!(
            serde_json::to_value(&entity.body_facing).unwrap(),
            before_facing
        );
        assert_eq!(entity.navigation.path_replay, before_queue);
        assert_eq!(retained(entity, kind), (false, true, before_track));
        assert!(entity.facing_target.is_none());
        assert_eq!(
            super::super::slope_transition::state_for_entity(entity)
                .unwrap()
                .hash_fields(),
            (2, 5, 101, 3),
            "{kind:?}: slope prelude must run before the live-turn return"
        );
        sim.session.binary_frame = 102;
        sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
            .unwrap();
        let slope = super::super::slope_transition::state_for_entity(
            sim.substrate.entities.get(1).unwrap(),
        )
        .unwrap();
        assert_eq!(slope.hash_fields(), (2, 5, 101, 3));
        assert_eq!(
            slope.remaining(102),
            2,
            "equal slope must not restart its timer"
        );
    }
}

#[test]
fn actual_turn_and_arrival_crush_use_binary_frame_for_both_shield_kinds() {
    use crate::map::entities::EntityCategory;
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::util::fixed_math::SimFixed;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TURN\n[TURN]\nSpeed=6\nAccelerates=no\n[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nCrushable=yes\n",
    )).unwrap();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for reason in [PerCellReason::TurnComplete, PerCellReason::Arrival] {
            for shield in [InvulnKind::IronCurtain, InvulnKind::ForceShield] {
                for (tick, frame) in [(0, 100), (100, 130)] {
                    let mut sim = fixture(kind);
                    sim.session.tick = tick;
                    sim.session.binary_frame = frame;
                    let crusher = sim.substrate.entities.get_mut(1).unwrap();
                    crusher.regular_crusher = true;
                    crusher.drive_accelerates = false;
                    let position = crusher.position.clone();
                    let current = super::super::ground_pose::position_world_coord(&position);
                    let arrival = reason == PerCellReason::Arrival;
                    set_retained(
                        crusher,
                        kind,
                        arrival,
                        !arrival,
                        TrackProgress {
                            turn_index: if arrival { 0 } else { -1 },
                            cursor: if arrival {
                                super::super::drive_track::raw_track_points(1).len() as i32
                            } else {
                                0
                            },
                            ..Default::default()
                        },
                    );
                    if arrival {
                        match kind {
                            LocomotorKind::Drive => {
                                let state = crusher.drive_locomotion.as_mut().unwrap();
                                state.head_to = Some(current);
                                state.target_speed_fraction = SimFixed::ONE;
                            }
                            LocomotorKind::Ship => {
                                let state = crusher.ship_locomotion.as_mut().unwrap();
                                state.head_to = Some(current);
                                state.target_speed_fraction = SimFixed::ONE;
                            }
                            _ => unreachable!(),
                        }
                    }
                    for (id, age) in [(2, 10), (3, 40)] {
                        let mut victim = GameEntity::test_default(id, "E1", "Soviets", 8, 8);
                        victim.type_ref = sim.intern("E1");
                        victim.owner = sim.intern("Soviets");
                        victim.category = EntityCategory::Infantry;
                        victim.mission_leaf =
                            crate::sim::mission::leaf::MissionLeafState::for_entity_category(
                                EntityCategory::Infantry,
                            );
                        victim.is_voxel = false;
                        victim.sub_cell = Some(0);
                        victim.position = position.clone();
                        victim.crushable = true;
                        victim.lifecycle.object_alive = true;
                        victim.lifecycle.in_limbo = false;
                        victim.lifecycle.cell_marked = true;
                        victim.invulnerability = Some(InvulnerabilityState {
                            start_frame: frame - age,
                            duration_frames: 30,
                            kind: shield,
                        });
                        sim.substrate.entities.insert(victim);
                    }
                    sim.substrate.occupancy =
                        crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
                    sim.process_ground_locomotor_for_test(1, Some(&rules), None, None)
                        .unwrap();
                    let context =
                        format!("{kind:?} {reason:?} {shield:?} tick={tick} frame={frame}");
                    assert!(
                        sim.substrate
                            .entities
                            .get(2)
                            .unwrap()
                            .lifecycle
                            .object_alive,
                        "protected: {context}"
                    );
                    assert!(
                        !sim.substrate
                            .entities
                            .get(3)
                            .unwrap()
                            .lifecycle
                            .object_alive,
                        "expired: {context}"
                    );
                    assert!(!sim.substrate.pending_delete.contains(&2), "{context}");
                    assert!(sim.substrate.pending_delete.contains(&3), "{context}");
                }
            }
        }
    }
}
