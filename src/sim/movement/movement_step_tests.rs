use super::*;
use crate::sim::components::{FootPathQueue, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::track_host::TrackWorldEvent;
use crate::sim::movement::track_process::{TrackFamily, TrackInvocation};
use crate::sim::world::Simulation;

#[test]
fn exhausted_foot_queue_cannot_bypass_drive_or_ship_track_admission() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for directions in [vec![], vec![2, 2]] {
            let queue = crate::sim::components::FootPathQueue {
                cursor: directions.len() as u16,
                directions,
                reference_cell: Some((0, 0)),
            };
            let mut target = MovementTarget {
                path: vec![(0, 0), (1, 0)],
                next_index: 1,
                move_dir_x: SimFixed::from_num(256),
                move_dir_y: SIM_ZERO,
                move_dir_len: SimFixed::from_num(256),
                current_speed: SimFixed::from_num(255),
                ..Default::default()
            };
            let mut position = Position {
                rx: 0,
                ry: 0,
                z: 0,
                exact_z_leptons: None,
                sub_x: CELL_CENTER_LEPTON,
                sub_y: CELL_CENTER_LEPTON,
            };
            let before = position.clone();
            let result = advance_lepton_position(
                &mut target,
                &mut position,
                &mut Some(LocomotorState::for_test_kind(kind)),
                EntityCategory::Unit,
                SimFixed::from_num(255),
                SimFixed::from_num(1) / SimFixed::from_num(15),
                1,
            );
            assert!(
                matches!(result, AdvanceResult::DriveTrackActive),
                "{kind:?}"
            );
            assert_eq!(
                (
                    position.rx,
                    position.ry,
                    position.sub_x,
                    position.sub_y,
                    position.z,
                    position.exact_z_leptons
                ),
                (
                    before.rx,
                    before.ry,
                    before.sub_x,
                    before.sub_y,
                    before.z,
                    before.exact_z_leptons
                ),
                "{kind:?}: no admitted track, no coordinate step"
            );
            assert_eq!(target.next_index, 1);
            assert!(queue.remaining_directions().is_empty());
        }
    }
}

/// Body/hull in-place turn duration = abs(delta_8bit) / ROT native frames
/// (gamemd DriveLocomotionClass::Do_Turn on the hull FacingClass at the
/// unit's rules ROT). Verified in
/// docs/research/BODY_FACING_DRIVE_LOCOMOTOR_ROT_GHIDRA_REPORT.md: for ROT=5
/// a 90° (0x40) turn is 12 frames and a 180° (0x80) turn is 25 frames — the
/// values gamemd produces, and the whole point of the frame-based model
/// (the old ms-integrated path was tick-rate-dependent and ~2× too fast).
#[test]
fn test_body_rotation_matches_native_frame_duration() {
    // Drive the in-place rotation frame by frame, returning the native-frame
    // count at which it completes (ReadyToMove with the exact target reached).
    fn frames_to_turn(from: u8, to: u8, rot: i32) -> u32 {
        let mut facing = from;
        let mut facing_target = Some(to);
        let mut body_facing = None;
        let mut position = Position {
            rx: 5,
            ry: 5,
            z: 0,
            exact_z_leptons: None,
            sub_x: crate::util::lepton::CELL_CENTER_LEPTON,
            sub_y: crate::util::lepton::CELL_CENTER_LEPTON,
        };
        let mut locomotor = None;
        for frame in 0..1000u32 {
            match handle_vehicle_rotation(
                &mut facing,
                &mut facing_target,
                &mut body_facing,
                &mut position,
                &mut locomotor,
                rot,
                frame,
                0,
            ) {
                RotationResult::ReadyToMove => {
                    assert_eq!(facing, to, "rotation must land exactly on the target");
                    assert!(body_facing.is_none(), "interpolator cleared on completion");
                    return frame;
                }
                RotationResult::StillRotating { .. } => {}
            }
        }
        panic!("rotation did not complete within 1000 frames");
    }

    // ROT=5 (MTNK/AMCV/HTNK/…): 0x40 = 16384/1280 = 12 frames; 0x80 = 25.
    assert_eq!(
        frames_to_turn(0x00, 0x40, 5),
        12,
        "90° at ROT=5 = 12 frames"
    );
    assert_eq!(
        frames_to_turn(0x00, 0x80, 5),
        25,
        "180° at ROT=5 = 25 frames"
    );
    // Counter-clockwise 90° (shortest arc) is the same duration.
    assert_eq!(
        frames_to_turn(0x40, 0x00, 5),
        12,
        "CCW 90° at ROT=5 = 12 frames"
    );
    // ROT=0 snaps instantly (no gradual rotation).
    assert_eq!(frames_to_turn(0x00, 0x40, 0), 0, "ROT=0 turns instantly");
    // Native signed ROT preserves the low byte: -255 is +0x0100,
    // while -1 is the non-positive rate0xFF00.
    assert_eq!(frames_to_turn(0x00, 0x40, -255), 64);
    assert_eq!(frames_to_turn(0x00, 0x40, -1), 0);
}

fn native_track_fixture(kind: LocomotorKind, budget: i32) -> (Simulation, TrackInvocation, i32) {
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
    entity.category = EntityCategory::Unit;
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    let head = DriveCoord::cell(10, 9, 0);
    let track = TrackProgress {
        turn_index: 0,
        cursor: 0,
        reversed: false,
        residual: 0,
    };
    if kind == LocomotorKind::Drive {
        entity.drive_locomotion = Some(DriveLocomotionRuntime {
            head_to: Some(head),
            track,
            track_valid: true,
            ..Default::default()
        });
    } else {
        entity.ship_locomotion = Some(ShipLocomotionRuntime {
            head_to: Some(head),
            track,
            track_valid: true,
            ..Default::default()
        });
    }
    sim.interner = crate::sim::intern::test_interner();
    sim.substrate.entities.insert(entity);
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    (
        sim,
        TrackInvocation {
            entity_id: 1,
            family: if kind == LocomotorKind::Drive {
                TrackFamily::Drive
            } else {
                TrackFamily::Ship
            },
            apply_fresh_occupation: false,
        },
        budget,
    )
}

fn retained_track(entity: &GameEntity, kind: LocomotorKind) -> TrackProgress {
    if kind == LocomotorKind::Drive {
        entity.drive_locomotion.as_ref().unwrap().track
    } else {
        entity.ship_locomotion.as_ref().unwrap().track
    }
}

#[test]
fn fresh_retry_terminal_retains_raw_head_for_both_track_families() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let (mut sim, invocation, budget) = native_track_fixture(kind, 0);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let head = DriveCoord {
            x: 10 * 256 + 85,
            y: 10 * 256 + 153,
            z: 731,
        };
        entity.position.sub_x = SimFixed::from_num(85);
        entity.position.sub_y = SimFixed::from_num(154);
        entity.position.exact_z_leptons = Some(104);
        let track = TrackProgress {
            turn_index: 0,
            // The zero-XY sentinel is paid separately after all real points.
            cursor: drive_track::raw_track_points(1).len() as i32,
            reversed: false,
            residual: 8,
        };
        if kind == LocomotorKind::Drive {
            let state = entity.drive_locomotion.as_mut().unwrap();
            state.head_to = Some(head);
            state.track = track;
        } else {
            let state = entity.ship_locomotion.as_mut().unwrap();
            state.head_to = Some(head);
            state.track = track;
        }
        assert_eq!(
            sim.run_track_points(invocation, budget, None, None, None),
            1
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            super::super::ground_pose::position_world_coord(&entity.position),
            head
        );
        assert_eq!(retained_track(entity, kind).turn_index, -1);
        assert_eq!(retained_track(entity, kind).cursor, 0);
        assert_eq!(super::super::track_head::committed_track_head(entity), None);
        assert_eq!(
            entity.drive_locomotion.as_ref().and_then(|s| s.head_to),
            None
        );
        assert_eq!(
            entity.ship_locomotion.as_ref().and_then(|s| s.head_to),
            None
        );
    }
}

#[test]
fn drive_track_completion_preserves_residual_through_fresh_acceptance() {
    let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, 0);
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let current = super::super::ground_pose::position_world_coord(&entity.position);
    let drive = entity.drive_locomotion.as_mut().unwrap();
    // Terminal budget uses the actual current/head distance. Distance 11
    // refunds zero after the paid sentinel (Drive4B1F48..4B1FDC).
    drive.head_to = Some(DriveCoord {
        y: current.y - 11,
        ..current
    });
    drive.track.cursor = drive_track::raw_track_points(1).len() as i32;
    drive.track.residual = 8;
    entity.navigation.nav_com = Some(crate::sim::components::NavTargetRef::Cell { rx: 11, ry: 10 });
    entity.navigation.path_replay = FootPathQueue {
        directions: vec![2],
        cursor: 0,
        reference_cell: Some((10, 10)),
    };
    entity.movement_target = Some(MovementTarget {
        path: vec![(10, 10), (11, 10)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        final_goal: Some((11, 10)),
        ..Default::default()
    });
    assert_eq!(
        sim.run_track_points(invocation, budget, None, None, None),
        1
    );
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    assert_eq!(entity.drive_locomotion.as_ref().unwrap().track.residual, 1);
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().track.turn_index,
        -1
    );
    let mut target = entity.movement_target.take().unwrap();
    let mut occupation = CellOccupationGrid::new();
    // Actual combat hull ownership can retain all16 bits with no movement
    // facing target. The earlier rotation sample must not destroy the one-bit
    // mismatch proved by drive_fresh_turn.json (initial0x4001/direction2).
    entity.facing = 0x40;
    entity.facing_target = None;
    entity.body_facing = Some(crate::sim::movement::FacingClass::new(0x4001, 5));
    assert!(matches!(
        handle_vehicle_rotation(
            &mut entity.facing,
            &mut entity.facing_target,
            &mut entity.body_facing,
            &mut entity.position,
            &mut entity.locomotor,
            5,
            2,
            3,
        ),
        RotationResult::ReadyToMove
    ));
    let live_facing = entity.body_facing.as_ref().unwrap().current(2);
    assert_eq!(live_facing, 0x4001);
    let prepare = |facing,
                   entity: &mut GameEntity,
                   target: &mut MovementTarget,
                   occupation: &mut CellOccupationGrid| {
        prepare_native_track(
            &mut entity.foot_occupation_enabled,
            &mut entity.navigation.path_replay,
            target,
            &entity.position,
            facing,
            &mut entity.facing_target,
            &mut entity.drive_locomotion,
            &mut entity.ship_locomotion,
            &entity.locomotor,
            entity.category,
            1,
            occupation,
            DriveCellAdmission::default(),
            MovementLayer::Ground,
        )
    };
    assert!(matches!(
        prepare(live_facing, entity, &mut target, &mut occupation),
        Some(NativeTrackPreparation::TurnFirst(_))
    ));
    assert_eq!(entity.facing_target, Some(64));
    assert_eq!(entity.drive_locomotion.as_ref().unwrap().track.residual, 1);
    let Some(NativeTrackPreparation::Invoke(next)) =
        prepare(0x4000, entity, &mut target, &mut occupation)
    else {
        panic!("aligned head must accept the fresh native segment");
    };
    let track = entity.drive_locomotion.as_ref().unwrap().track;
    assert_eq!(track.turn_index, 18);
    assert_eq!(track.cursor, 0);
    assert_eq!(track.residual, 1);
    entity.movement_target = Some(target);
    assert_eq!(sim.run_track_points(next, 0, None, None, None), 0);
    let track = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .drive_locomotion
        .as_ref()
        .unwrap()
        .track;
    assert_eq!(track.cursor, 0, "residual alone cannot buy a point");
    assert_eq!(track.residual, 1);
}

#[test]
fn drive_track_first_native_frame_uses_native_frame_budget() {
    let current_speed = SimFixed::from_num(255) * SimFixed::lit("0.7");
    let budget =
        crate::sim::movement::foot_speed::owner_current_speed_from_fraction(current_speed, SIM_ONE);
    assert_eq!(budget, 11);
    let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, budget);
    assert_eq!(
        sim.run_track_points(invocation, budget, None, None, None),
        1
    );
    let track = retained_track(sim.substrate.entities.get(1).unwrap(), LocomotorKind::Drive);
    assert_eq!(track.cursor, 1);
    assert_eq!(track.residual, 4);
}

#[test]
fn gsi_04_05_paid_track_point_clears_current_before_same_cell_coordinate_commit() {
    let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, 8);
    sim.substrate
        .cell_occupation
        .mark_vehicle_on_layer(10, 10, 1, MovementLayer::Ground);
    sim.substrate
        .cell_occupation
        .mark_vehicle_on_layer(10, 9, 1, MovementLayer::Ground);
    let mut paid_commits = 0;
    sim.run_track_points_observed(
        invocation,
        budget,
        None,
        None,
        None,
        &mut |sim, id, event| {
            if event == TrackWorldEvent::SetCoords {
                paid_commits += 1;
                let entity = sim.substrate.entities.get(id).unwrap();
                assert_eq!((entity.position.rx, entity.position.ry), (10, 10));
                assert_eq!(
                    sim.substrate
                        .cell_occupation
                        .vehicle_bits(10, 10, MovementLayer::Ground),
                    0
                );
                assert_eq!(
                    sim.substrate
                        .cell_occupation
                        .vehicle_bits(10, 9, MovementLayer::Ground),
                    0x20
                );
                assert!(!entity.foot_occupation_enabled);
            }
        },
    );
    assert!(paid_commits >= 1, "observe the actual coordinate receiver");
}

#[test]
fn drive_track_each_call_consumes_fresh_native_frame_budget() {
    let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, 11);
    sim.run_track_points(invocation, budget, None, None, None);
    let first = retained_track(sim.substrate.entities.get(1).unwrap(), LocomotorKind::Drive);
    assert_eq!((first.cursor, first.residual), (1, 4));
    sim.run_track_points(invocation, budget, None, None, None);
    let second = retained_track(sim.substrate.entities.get(1).unwrap(), LocomotorKind::Drive);
    assert_eq!((second.cursor, second.residual), (3, 1));
}

#[test]
fn fresh_track_pays_point_zero_and_terminal_through_the_production_host() {
    for (budget, expected_cursor, expected_residual, expected_steps) in
        [(168, 23, 7, 23), (169, 0, 6, 24)]
    {
        let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, budget);
        assert_eq!(
            sim.run_track_points(invocation, budget, None, None, None),
            expected_steps
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        let progress = retained_track(entity, LocomotorKind::Drive);
        assert_eq!(progress.cursor, expected_cursor);
        assert_eq!(progress.residual, expected_residual);
        if budget == 168 {
            assert_eq!(
                progress.turn_index, 0,
                "exactly seven cannot pay the sentinel"
            );
        } else {
            assert_eq!(progress.turn_index, -1);
            assert_eq!(
                super::super::ground_pose::position_world_xy(&entity.position),
                [10 * 256 + 128, 9 * 256 + 128]
            );
            assert_eq!(budget - progress.residual, 163);
        }
    }
}
