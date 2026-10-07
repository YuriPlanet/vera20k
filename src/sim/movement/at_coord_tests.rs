use super::*;
use serde_json::Value;
use std::collections::BTreeSet;

#[test]
fn active_walk_and_hover_queries_retain_committed_xyz_after_other_coordinate_writes() {
    for family in [LocomotorKind::Walk, LocomotorKind::Hover] {
        let mut e = crate::sim::game_entity::GameEntity::test_default(1, "E1", "Americans", 5, 5);
        let mut loco = crate::sim::movement::locomotor::LocomotorState::for_test_kind(family);
        let head = DriveCoord {
            x: 6 * 256 + 128,
            y: 5 * 256 + 128,
            z: 416,
        };
        set_head(&mut loco, Some(head));
        e.locomotor = Some(loco);
        e.position.exact_z_leptons = Some(0);
        assert!(AtCoordQuery::from_entity(&e).unwrap().matches(head));
        assert!(
            !AtCoordQuery::from_entity(&e)
                .unwrap()
                .matches(DriveCoord { z: 0, ..head })
        );
        e.position.exact_z_leptons = Some(832);
        e.movement_target = None;
        assert!(
            AtCoordQuery::from_entity(&e).unwrap().matches(head),
            "Stop/path retirement must not reconstruct Head_To"
        );
        set_head(e.locomotor.as_mut().unwrap(), None);
        assert_eq!(AtCoordQuery::from_entity(&e).unwrap().head.z, 832);
    }
}

/// Walk's head, or the Hover object's (whose writer is its ProcessMovement).
fn set_head(loco: &mut crate::sim::movement::locomotor::LocomotorState, head: Option<DriveCoord>) {
    match loco.hover_runtime_mut() {
        Some(hover) => hover.set_head(head),
        None => loco.set_step_head(head),
    }
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

fn kind(value: &Value) -> LocomotorKind {
    match value.as_str().unwrap() {
        "drive" => LocomotorKind::Drive,
        "ship" => LocomotorKind::Ship,
        "walk" => LocomotorKind::Walk,
        "hover" => LocomotorKind::Hover,
        "fly" => LocomotorKind::Fly,
        "jumpjet" => LocomotorKind::Jumpjet,
        "rocket" => LocomotorKind::Rocket,
        "teleport" => LocomotorKind::Teleport,
        other => panic!("unexpected native family {other}"),
    }
}

#[test]
fn coordinate_queries_match_original_heads_transforms_and_native_probes() {
    let data: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/locomotor_at_coord.json",
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 339);
    let mut drive_turns = BTreeSet::new();
    let mut ship_turns = BTreeSet::new();
    let mut probes = 0;
    for case in cases {
        let input = &case["input"];
        let output = &case["output"];
        let family = kind(&input["family"]);
        let track = AtCoordTrack {
            turn_index: input["turn_index"].as_i64().unwrap_or(-1) as i32,
            cursor: input["cursor"].as_i64().unwrap_or(0) as i32,
            reversed: input["reversed"].as_bool().unwrap_or(false),
        };
        let query = AtCoordQuery::from_state(
            family,
            coord(&input["current"]),
            Some(coord(&input["stored_head"])),
            track,
        )
        .unwrap();
        assert_eq!(query.head, coord(&output["head"]), "{}", input["name"]);
        if !input["table"].is_null() {
            let table = &input["table"];
            let turn = turn_track_at(track.turn_index as usize).unwrap();
            assert_eq!(
                i64::from(turn.normal_track),
                table["normal"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(turn.short_track),
                table["short"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(turn.target_facing),
                table["facing"].as_i64().unwrap()
            );
            assert_eq!(i64::from(turn.flags), table["flags"].as_i64().unwrap());
            let raw = raw_track_meta(turn.normal_track).unwrap();
            assert_eq!(
                i64::from(raw.occupation_handoff_point_index),
                table["handoff"].as_i64().unwrap()
            );
            if !table["point"].is_null() {
                let point = raw_track_points(turn.normal_track)
                    [raw.occupation_handoff_point_index as usize];
                assert_eq!(
                    [
                        i64::from(point.x),
                        i64::from(point.y),
                        i64::from(point.facing)
                    ],
                    [
                        table["point"][0].as_i64().unwrap(),
                        table["point"][1].as_i64().unwrap(),
                        table["point"][2].as_i64().unwrap()
                    ]
                );
            }
            match family {
                LocomotorKind::Drive => {
                    drive_turns.insert(track.turn_index);
                }
                LocomotorKind::Ship => {
                    ship_turns.insert(track.turn_index);
                }
                _ => unreachable!(),
            }
        }
        for original in output["queries"].as_array().unwrap() {
            probes += 1;
            let native_xy = original["track_xy"].as_array().map(|xy| {
                [
                    xy[0].as_i64().unwrap() as i32,
                    xy[1].as_i64().unwrap() as i32,
                ]
            });
            assert_eq!(
                query.handoff.map(|point| [point.x, point.y]),
                native_xy,
                "{} {} handoff",
                input["family"],
                input["name"]
            );
            assert_eq!(
                query.matches(coord(&original["probe"])),
                original["at"].as_bool().unwrap(),
                "{} {} probe {}",
                input["family"],
                input["name"],
                original["probe"]
            );
        }
    }
    assert_eq!(probes, 4197);
    assert_eq!(drive_turns, (0..72).collect());
    assert_eq!(ship_turns, (0..64).collect());
    let false_queries = data["false_queries"].as_array().unwrap();
    assert_eq!(false_queries.len(), 8);
    for original in false_queries {
        let query = AtCoordQuery::from_state(
            kind(&original["family"]),
            NULL_COORD,
            Some(NULL_COORD),
            AtCoordTrack::default(),
        );
        assert_eq!(
            query.is_some_and(|q| q.matches(coord(&original["probe"]))),
            original["at"].as_bool().unwrap()
        );
    }
}
