use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::components::{DriveCoord, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::drive_track;
use crate::sim::movement::track_host::TrackWorldEvent;
use crate::sim::movement::track_process::{TrackFamily, TrackInvocation};
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

/// WalkLocomotionClass::ProcessMovement `0x0075BD33..0x0075BD70` executed on
/// each row's owner and head (`tools/spatial_oracle/sqrt_approx_distances.py`):
/// a head the owner stands less than 17 leptons from completes.
#[test]
fn walk_head_completion_matches_original_rows() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/sqrt_approx_distances.json",
    ))
    .unwrap();
    let rows = corpus["walk"].as_array().unwrap();
    for row in rows {
        let [owner, head] = [&row["owner"], &row["head"]]
            .map(|v| serde_json::from_value::<[i32; 3]>(v.clone()).unwrap());
        let mut entity = GameEntity::test_default(1, "E1", "Americans", 0, 0);
        entity.position.put_world_xy([owner[0], owner[1]]);
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Walk);
        let [x, y, z] = head;
        locomotor.set_step_head(Some(DriveCoord { x, y, z }));
        assert_eq!(
            completed_walk_head(&entity.position, &Some(locomotor)).is_some(),
            row["expected"].as_i64().unwrap() < 17,
            "{}",
            row["name"]
        );
    }
    assert_eq!(rows.len(), 5);
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
        let target = u16::from(to) << 8;
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        entity.body_facing = crate::sim::movement::FacingClass::new(u16::from(from) << 8, rot);
        crate::sim::movement::drive_do_turn(&mut entity, target, 0);
        for frame in 0..1000u32 {
            match handle_vehicle_rotation(&entity.body_facing, frame) {
                RotationResult::ReadyToMove => {
                    assert_eq!(
                        entity.body_facing.current(frame),
                        target,
                        "rotation must land exactly on the target"
                    );
                    return frame;
                }
                RotationResult::StillRotating => {}
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
    entity.set_category_for_test(EntityCategory::Unit);
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked_for_test(true);
    let head = DriveCoord::cell(10, 9, 0);
    let track = TrackProgress::for_test(0, 0, false, 0);
    if kind == LocomotorKind::Drive {
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(
                    DriveLocomotionRuntime::default()
                        .with_head_to_for_test(Some(head))
                        .with_track_for_test(track)
                        .with_track_valid_for_test(true)
                ))
        );
    } else {
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .install_ship_state_for_test(Some(
                    ShipLocomotionRuntime::default()
                        .with_head_to_for_test(Some(head))
                        .with_track_for_test(track)
                        .with_track_valid_for_test(true)
                ))
        );
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
            active_gate: false,
            retry: false,
        },
        budget,
    )
}

fn retained_track(entity: &GameEntity, kind: LocomotorKind) -> TrackProgress {
    if kind == LocomotorKind::Drive {
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track()
    } else {
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track()
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
        entity.position.set_sub_x_for_test(SimFixed::from_num(85));
        entity.position.set_sub_y_for_test(SimFixed::from_num(154));
        entity.position.set_exact_z_leptons_for_test(Some(104));
        let track = TrackProgress::for_test(
            0, // The zero-XY sentinel is paid separately after all real points.
            drive_track::raw_track_points(1).len() as i32,
            false,
            8,
        );
        if kind == LocomotorKind::Drive {
            let state = entity.locomotor.as_mut().unwrap();
            assert!(state.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                Some(head)
            ));
            assert!(state.store_track_progress(
                crate::sim::movement::track_process::TrackFamily::Drive,
                track
            ));
        } else {
            let state = entity.locomotor.as_mut().unwrap();
            assert!(state.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Ship,
                Some(head)
            ));
            assert!(state.store_track_progress(
                crate::sim::movement::track_process::TrackFamily::Ship,
                track
            ));
        }
        assert_eq!(sim.run_track_points(invocation, budget, None, None), 1);
        let entity = sim.substrate.entities.get(1).unwrap();
        // The terminal SetHeight(0) grounds the retained head XY. The fixture
        // has no map, so that ground is the Dummy cell's level 0.
        assert_eq!(
            super::super::ground_pose::position_world_coord(&entity.position),
            DriveCoord { z: 0, ..head }
        );
        assert_eq!(retained_track(entity, kind).turn_index(), -1);
        assert_eq!(retained_track(entity, kind).cursor(), 0);
        assert_eq!(super::super::track_head::committed_track_head(entity), None);
        assert_eq!(
            entity
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .and_then(|s| s.head_to()),
            None
        );
        assert_eq!(
            entity
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_ship_runtime())
                .and_then(|r| r.retained())
                .and_then(|s| s.head_to()),
            None
        );
    }
}

#[test]
fn gsi_04_05_paid_track_point_clears_current_before_same_cell_coordinate_commit() {
    let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, 8);
    sim.substrate.raw_cell_occupation.mark_ground(10, 10, 0x20);
    sim.substrate.raw_cell_occupation.mark_ground(10, 9, 0x20);
    let mut paid_commits = 0;
    sim.run_track_points_observed(invocation, budget, None, None, &mut |sim, id, event| {
        if event == TrackWorldEvent::SetCoords {
            paid_commits += 1;
            let entity = sim.substrate.entities.get(id).unwrap();
            assert_eq!((entity.position.rx(), entity.position.ry()), (10, 10));
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 10) & 0x20,
                0
            );
            assert_eq!(
                sim.substrate.raw_cell_occupation.ground_bits(10, 9) & 0x20,
                0x20
            );
            assert!(!entity.foot_occupation_enabled());
        }
    });
    assert!(paid_commits >= 1, "observe the actual coordinate receiver");
}

#[test]
fn drive_track_each_call_consumes_fresh_native_frame_budget() {
    let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, 11);
    sim.run_track_points(invocation, budget, None, None);
    let first = retained_track(sim.substrate.entities.get(1).unwrap(), LocomotorKind::Drive);
    assert_eq!((first.cursor(), first.residual()), (1, 4));
    sim.run_track_points(invocation, budget, None, None);
    let second = retained_track(sim.substrate.entities.get(1).unwrap(), LocomotorKind::Drive);
    assert_eq!((second.cursor(), second.residual()), (3, 1));
}

#[test]
fn fresh_track_pays_point_zero_and_terminal_through_the_production_host() {
    for (budget, expected_cursor, expected_residual, expected_steps) in
        [(168, 23, 7, 23), (169, 0, 6, 24)]
    {
        let (mut sim, invocation, budget) = native_track_fixture(LocomotorKind::Drive, budget);
        assert_eq!(
            sim.run_track_points(invocation, budget, None, None),
            expected_steps
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        let progress = retained_track(entity, LocomotorKind::Drive);
        assert_eq!(progress.cursor(), expected_cursor);
        assert_eq!(progress.residual(), expected_residual);
        if budget == 168 {
            assert_eq!(
                progress.turn_index(),
                0,
                "exactly seven cannot pay the sentinel"
            );
        } else {
            assert_eq!(progress.turn_index(), -1);
            assert_eq!(
                super::super::ground_pose::position_world_xy(&entity.position),
                [10 * 256 + 128, 9 * 256 + 128]
            );
            assert_eq!(budget - progress.residual(), 163);
        }
    }
}
