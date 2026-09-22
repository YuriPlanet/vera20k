//! Original MoveTo/Foot caller comparisons. Unit preprocessing rows remain
//! evidence for its pending complete setter; these tests do not claim that port.
use super::*;
use crate::sim::components::{FootPathQueue, FootPathRuntime};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

const ZERO: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

fn coord(v: &Value) -> DriveCoord {
    DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    }
}

fn corpus() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_destination.json"
    ))
    .unwrap()
}

fn terrain(bridge: bool) -> ResolvedTerrainGrid {
    ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| {
                    crate::sim::world::common_raw_test_terrain_cell(
                        x,
                        y,
                        0,
                        bridge && (x, y) == (11, 10),
                    )
                })
            })
            .collect(),
    )
}

fn actor(input: &Value) -> GameEntity {
    let mut e = GameEntity::test_default(1, "MOVER", "Americans", 10, 10);
    e.category = crate::map::entities::EntityCategory::Unit;
    e.position.sub_x = SimFixed::from_num(128);
    e.position.sub_y = SimFixed::from_num(128);
    let kind = if input["family"] == "drive" {
        LocomotorKind::Drive
    } else {
        LocomotorKind::Ship
    };
    e.locomotor = Some(LocomotorState::for_test_kind(kind));
    e.locomotor.as_mut().unwrap().powered = input["power_off"] != true;
    let head = input.get("head").map_or(
        DriveCoord {
            x: 2816,
            y: 2688,
            z: 123,
        },
        coord,
    );
    let head = (head != ZERO).then_some(head);
    let destination = Some(DriveCoord {
        x: 700,
        y: 800,
        z: 900,
    });
    if kind == LocomotorKind::Drive {
        e.drive_locomotion = Some(DriveLocomotionRuntime {
            destination,
            head_to: head,
            ..Default::default()
        });
    } else {
        e.ship_locomotion = Some(ShipLocomotionRuntime {
            destination,
            head_to: head,
            ..Default::default()
        });
    }
    e.navigation.nav_com_aux = Some(NavTargetRef::cell(0, 0));
    e.navigation.path_replay = FootPathQueue {
        directions: vec![2, 3, 4, 5],
        reference_cell: Some((9, 8)),
        ..Default::default()
    };
    e.navigation.path_runtime = FootPathRuntime {
        movement_timer: CdTimer::started(50, 5),
        blocked_timer: CdTimer::started(40, 6),
        path_blocked: true,
        retries_left: 7,
    };
    if input["warp_out"] == true || input["warp_in"] == true {
        e.teleport_state = Some(TeleportState {
            phase: if input["warp_out"] == true {
                TeleportPhase::Relocate
            } else {
                TeleportPhase::ChronoDelay
            },
            target_rx: 11,
            target_ry: 10,
            being_warped_ticks: 3,
        });
    }
    e
}

fn compare(e: &GameEntity, row: &Value) {
    let (destination, head) = if row["input"]["family"] == "drive" {
        let d = e.drive_locomotion.as_ref().unwrap();
        (d.destination, d.head_to)
    } else {
        let d = e.ship_locomotion.as_ref().unwrap();
        (d.destination, d.head_to)
    };
    assert_eq!(
        destination,
        (coord(&row["destination"]) != ZERO).then(|| coord(&row["destination"])),
        "{row}"
    );
    assert_eq!(head.unwrap_or(ZERO), coord(&row["head"]), "{row}");
    assert_eq!(
        u8::from(e.locomotor.as_ref().unwrap().powered),
        row["power"].as_u64().unwrap() as u8,
        "{row}"
    );
    if row["input"]["family"] == "drive" {
        assert_eq!(
            super::super::drive_locomotor_is_moving(e),
            row["moving"].as_bool().unwrap(),
            "{row}"
        );
    }
    let p = &e.navigation.path_runtime;
    let nav = e.navigation.nav_com.map(|n| match n {
        NavTargetRef::Cell { rx, ry } => [rx, ry],
        _ => panic!("cell fixture"),
    });
    for (key, actual) in [
        ("nav", json!(nav)),
        ("aux", json!(e.navigation.nav_com_aux.is_some())),
        (
            "path",
            json!(e.navigation.path_replay.remaining_directions()),
        ),
        ("reference", json!(e.navigation.path_replay.reference_cell)),
        (
            "movement_timer",
            json!([p.movement_timer.start_frame(), p.movement_timer.duration()]),
        ),
        (
            "blocked_timer",
            json!([p.blocked_timer.start_frame(), p.blocked_timer.duration()]),
        ),
        ("blocked", json!(u8::from(p.path_blocked))),
        ("retries", json!(p.retries_left)),
    ] {
        assert_eq!(actual, row[key], "{key}: {row}");
    }
}

#[test]
fn track_move_to_and_accepted_foot_calls_match_original_warp_and_zero_semantics() {
    let rows = corpus();
    assert_eq!(rows.len(), 126);
    let mut counts = [0, 0, 0];
    for row in rows {
        let input = &row["input"];
        // These30 original full Unit calls preserve the still-required class
        // queue/force/one-shot latch evidence; no Rust whole-Unit claim here.
        if input["entry"] == "unit" {
            counts[2] += 1;
            continue;
        }
        let mut e = actor(input);
        let terrain = terrain(input["bridge"] == true);
        if input["entry"] == "move" {
            counts[0] += 1;
            if input["family"] == "drive" {
                drive_set_destination(&mut e, coord(&input["request"]), Some(&terrain));
            } else {
                ship_set_destination(&mut e, coord(&input["request"]), Some(&terrain));
            }
        } else {
            counts[1] += 1;
            set_destination_internal_cell(&mut e, (11, 10), Some(&terrain));
            super::super::DestinationTiming::new(100, 22).accept(&mut e);
        }
        compare(&e, &row);
        let restored: GameEntity =
            serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        compare(&restored, &row);
    }
    assert_eq!(counts, [72, 24, 30]);
}

#[test]
fn refused_track_request_does_not_allocate_payload_or_stamp_dummy() {
    for family in ["drive", "ship"] {
        let mut e = actor(&json!({"family":family,"warp_out":true}));
        e.drive_locomotion = None;
        e.ship_locomotion = None;
        let terrain = ResolvedTerrainGrid::from_cells(1, 1, Vec::new());
        terrain.stamp_dummy_cell_requested_coord(7, 8);
        let before = terrain.shared_cell_dummy().snapshot();
        let coord = DriveCoord {
            x: -4096,
            y: 800000,
            z: 17,
        };
        if family == "drive" {
            assert!(!drive_set_destination(&mut e, coord, Some(&terrain)));
        } else {
            ship_set_destination(&mut e, coord, Some(&terrain));
        }
        assert!(e.drive_locomotion.is_none() && e.ship_locomotion.is_none());
        assert_eq!(terrain.shared_cell_dummy().snapshot(), before);
    }
}

#[test]
fn live_drive_target_refresh_resumes_after_owner_warp_ends() {
    let mut e = actor(&json!({"family":"drive","warp_in":true}));
    let before = e.drive_locomotion.clone();
    let terrain = terrain(false);
    let destination = DriveCoord {
        x: 2944,
        y: 2688,
        z: -123,
    };
    assert!(!refresh_drive_destination_coord(
        &mut e,
        destination,
        Some(&terrain)
    ));
    assert_eq!(e.drive_locomotion, before);
    // Existing teleport owner clears the arrival byte when its timer expires.
    e.teleport_state.as_mut().unwrap().being_warped_ticks = 0;
    assert!(refresh_drive_destination_coord(
        &mut e,
        destination,
        Some(&terrain)
    ));
    assert_eq!(
        e.drive_locomotion.as_ref().unwrap().destination,
        Some(destination)
    );
    assert_eq!(
        e.drive_locomotion.as_ref().unwrap().head_to,
        before.unwrap().head_to
    );
}
