use super::*;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::pathfinding::PathGrid;
use crate::util::fixed_math::SimFixed;

fn corpus() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_head_coordinates.json"
    ))
    .unwrap()
}

fn coord(value: &serde_json::Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

#[test]
fn original_head_coordinate_expressions_match_all_288_saved_cases() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 288);
    for case in cases {
        let input = &case["input"];
        let source = if input["operation"] == "fresh" {
            &input["current"]
        } else {
            &input["base"]
        };
        assert_eq!(
            offset_head(coord(source), input["direction"].as_u64().unwrap() as u8),
            coord(&case["output"]),
            "{input}"
        );
    }
}

#[test]
fn first_process_publishes_raw_head_and_matching_progress_for_drive_and_ship() {
    let corpus = corpus();
    for (kind, family) in [
        (LocomotorKind::Drive, "drive"),
        (LocomotorKind::Ship, "ship"),
    ] {
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| {
                let input = &case["input"];
                input["family"] == family
                    && input["operation"] == "fresh"
                    && input["name"] == "noncentered_retained_z"
                    && input["direction"] == 2
            })
            .unwrap();
        let initial = coord(&case["input"]["current"]);
        let expected = coord(&case["output"]);
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 8, 8);
        entity.locomotor = Some(LocomotorState::for_test_kind(kind));
        entity.position.sub_x = SimFixed::from_num(initial.x % 256);
        entity.position.sub_y = SimFixed::from_num(initial.y % 256);
        entity.position.z = 1; // Deliberately differs from the retained raw Z.
        entity.position.exact_z_leptons = Some(initial.z);
        entity.facing = 64;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        let mut entities = EntityStore::new();
        entities.insert(entity);
        let grid = PathGrid::new(20, 20);
        assert!(crate::sim::movement::issue_move_command(
            &mut entities,
            &grid,
            1,
            (11, 8),
            SimFixed::from_num(0),
            false,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        ));
        assert!(committed_track_head(entities.get(1).unwrap()).is_none());
        let mut sim = crate::sim::world::Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        sim.substrate.entities = entities;
        sim.substrate.occupancy =
            crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
        sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
            .unwrap();
        let entity = sim.substrate.entities.get(1).unwrap();
        let head = match kind {
            LocomotorKind::Drive => entity.drive_locomotion.as_ref().unwrap().head_to,
            LocomotorKind::Ship => entity.ship_locomotion.as_ref().unwrap().head_to,
            _ => unreachable!(),
        };
        assert_eq!(head, Some(expected), "{kind:?}");
        assert_eq!(committed_track_head(entity), Some(expected));
        let progress = match kind {
            LocomotorKind::Drive => {
                let state = entity.drive_locomotion.as_ref().unwrap();
                state.track
            }
            LocomotorKind::Ship => {
                let state = entity.ship_locomotion.as_ref().unwrap();
                state.track
            }
            _ => unreachable!(),
        };
        assert_eq!(progress.turn_index, 18);
        assert_eq!(progress.cursor, 0);
        assert!(!progress.reversed);
    }
}

#[test]
fn destination_change_preserves_the_committed_head() {
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 8, 8);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    let head = DriveCoord {
        x: 2389,
        y: 2201,
        z: 731,
    };
    entity.drive_locomotion = Some(crate::sim::components::DriveLocomotionRuntime {
        head_to: Some(head),
        ..Default::default()
    });
    crate::sim::movement::navcom::set_destination_internal_cell(&mut entity, (13, 8), None);
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().head_to,
        Some(head)
    );
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().destination,
        Some(DriveCoord::cell(13, 8, 0))
    );
    crate::sim::movement::navcom::refresh_drive_destination_coord(
        &mut entity,
        DriveCoord {
            x: 3109,
            y: 2201,
            z: -347,
        },
        None,
    );
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().head_to,
        Some(head)
    );
}

#[test]
fn chained_queue_consumption_preserves_the_native_replay_reference() {
    let mut queue = crate::sim::components::FootPathQueue {
        directions: vec![2, 3, 4],
        cursor: 2,
        reference_cell: Some((10, 9)),
        ..Default::default()
    };
    crate::sim::movement::path_markers::consume_path_replay(&mut queue, 1);
    assert_eq!(queue.cursor, 3);
    assert_eq!(queue.reference_cell, Some((10, 9)));
}

#[test]
fn committed_head_requires_active_family_head_and_selector_independently_of_drive_valid() {
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 8, 8);
    let head = DriveCoord {
        x: 2389,
        y: 2201,
        z: 731,
    };
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        head_to: Some(head),
        track: crate::sim::components::TrackProgress {
            turn_index: 18,
            ..Default::default()
        },
        track_valid: false,
        ..Default::default()
    });
    assert_eq!(committed_track_head(&entity), Some(head));
    entity.drive_locomotion.as_mut().unwrap().head_to = None;
    assert_eq!(committed_track_head(&entity), None);
    entity.drive_locomotion.as_mut().unwrap().head_to = Some(head);
    entity.drive_locomotion.as_mut().unwrap().track.turn_index = -1;
    assert_eq!(committed_track_head(&entity), None);
    entity.drive_locomotion.as_mut().unwrap().track.turn_index = 18;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
    assert_eq!(
        committed_track_head(&entity),
        None,
        "inactive Drive cannot supply a Ship head"
    );
    entity.ship_locomotion = Some(ShipLocomotionRuntime {
        head_to: Some(head),
        track: crate::sim::components::TrackProgress {
            turn_index: 18,
            ..Default::default()
        },
        ..Default::default()
    });
    assert_eq!(committed_track_head(&entity), Some(head));
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    assert_eq!(committed_track_head(&entity), None);
}

#[test]
fn production_process_admission_uses_valid_selector_independently_of_head() {
    use crate::sim::components::{DriveLocomotionRuntime, ShipLocomotionRuntime, TrackProgress};
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for valid in [false, true] {
            for selector in [-1, 0] {
                for nonnull_head in [false, true] {
                    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 8, 8);
                    entity.lifecycle.in_limbo = false;
                    entity.lifecycle.cell_marked = true;
                    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
                    entity.drive_accelerates = false;
                    entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
                    let track = TrackProgress {
                        turn_index: selector,
                        cursor: 0,
                        ..Default::default()
                    };
                    let head_to = nonnull_head.then_some(DriveCoord::cell(8, 7, 0));
                    if kind == LocomotorKind::Drive {
                        entity.drive_locomotion = Some(DriveLocomotionRuntime {
                            track,
                            head_to,
                            track_valid: valid,
                            target_speed_fraction: SimFixed::lit("0.75"),
                            ..Default::default()
                        });
                    } else {
                        entity.ship_locomotion = Some(ShipLocomotionRuntime {
                            track,
                            head_to,
                            track_valid: valid,
                            target_speed_fraction: SimFixed::lit("0.75"),
                            ..Default::default()
                        });
                    }
                    let mut sim = crate::sim::world::Simulation::new();
                    sim.interner = crate::sim::intern::test_interner();
                    sim.substrate.entities.insert(entity);
                    let grid = PathGrid::new(20, 20);
                    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
                        .unwrap();
                    let entity = sim.substrate.entities.get(1).unwrap();
                    assert_eq!(
                        entity.foot_speed.applied_fraction,
                        SimFixed::lit(if valid && selector != -1 {
                            "0.75"
                        } else {
                            "0.25"
                        }),
                        "{kind:?} valid={valid} selector={selector} head={nonnull_head}"
                    );
                }
            }
        }
    }
}

#[test]
fn retained_admission_matches_native_entry_cases_without_tube_or_turn_latch() {
    use crate::sim::components::{DriveLocomotionRuntime, ShipLocomotionRuntime, TrackProgress};
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_process_entry.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 144);
    let mut checked = 0;
    for row in rows {
        let input = &row["input"];
        // This production predicate owns the retained-track branch. The
        // separate tube8 and Process-latched +62 gate are not represented by it.
        if input["class_flag_62"] != 0 || input["queue_head"] == 8 {
            continue;
        }
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 8, 8);
        let valid = input["track_valid"].as_u64().unwrap() != 0;
        let track = TrackProgress {
            turn_index: input["selector"].as_i64().unwrap() as i32,
            ..Default::default()
        };
        if input["family"] == "drive" {
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            entity.drive_locomotion = Some(DriveLocomotionRuntime {
                track,
                track_valid: valid,
                ..Default::default()
            });
        } else {
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
            entity.ship_locomotion = Some(ShipLocomotionRuntime {
                track,
                track_valid: valid,
                ..Default::default()
            });
        }
        assert_eq!(
            active_track_family(&entity).is_some(),
            row["prefix_eligible"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(row["original_code_and_vtable_unchanged"], true);
        checked += 1;
    }
    assert_eq!(checked, 48);
}
