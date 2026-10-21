use super::*;
use crate::map::entities::EntityCategory;
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::sim::components::NavTargetRef;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

fn coord(v: &Value) -> DriveCoord {
    DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    }
}

fn entity(id: u64, kind: LocomotorKind, current: DriveCoord, head: DriveCoord) -> GameEntity {
    let mut e = GameEntity::test_default(id, "E1", "Americans", 0, 0);
    e.set_category_for_test(if kind == LocomotorKind::Walk {
        EntityCategory::Infantry
    } else {
        EntityCategory::Unit
    });
    e.position
        .set_rx_for_test((current.x / 256).clamp(0, i32::from(u16::MAX)) as u16);
    e.position
        .set_ry_for_test((current.y / 256).clamp(0, i32::from(u16::MAX)) as u16);
    e.position.set_sub_x_for_test(SimFixed::from_num(
        current.x - i32::from(e.position.rx()) * 256,
    ));
    e.position.set_sub_y_for_test(SimFixed::from_num(
        current.y - i32::from(e.position.ry()) * 256,
    ));
    e.position.set_exact_z_leptons_for_test(Some(current.z));
    e.locomotor = Some(LocomotorState::for_test_kind(kind));
    match kind {
        LocomotorKind::Drive => {
            assert!(
                e.locomotor
                    .as_mut()
                    .unwrap()
                    .install_drive_state_for_test(Some(
                        DriveLocomotionRuntime::default().with_head_to_for_test(Some(head))
                    ))
            )
        }
        LocomotorKind::Ship => {
            assert!(
                e.locomotor
                    .as_mut()
                    .unwrap()
                    .install_ship_state_for_test(Some(
                        ShipLocomotionRuntime::default().with_head_to_for_test(Some(head))
                    ))
            )
        }
        LocomotorKind::Walk => e.locomotor.as_mut().unwrap().set_step_head(Some(head)),
        LocomotorKind::Hover => e
            .locomotor
            .as_mut()
            .and_then(|loco| loco.hover_runtime_mut())
            .unwrap()
            .set_head(Some(head)),
        _ => {}
    }
    e
}

#[test]
fn lazy_track_constructor_projects_original_null_head_coordinates() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_navigation_coordinate.json",
    ))
    .unwrap();
    let mut checked = 0;
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let kind = match input["family"].as_str().unwrap() {
            "drive" => LocomotorKind::Drive,
            "ship" => LocomotorKind::Ship,
            _ => continue,
        };
        if input["stored_head"] != serde_json::json!([0, 0, 0]) || input["tube"].is_array() {
            continue;
        }
        let mut actor = entity(1, kind, coord(&input["current"]), NULL_COORD);
        if let Some(loco) = actor.locomotor.as_mut() {
            let _ = loco.install_drive_state_for_test(None);
        };
        if let Some(loco) = actor.locomotor.as_mut() {
            let _ = loco.install_ship_state_for_test(None);
        };
        assert_eq!(
            navigation_coordinate(&actor, None).unwrap(),
            coord(&row["coordinate"])
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn world_queries_match_all_original_foot_coordinate_rows_and_snapshot() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_navigation_coordinate.json",
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 44);
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let kind = match input["family"].as_str().unwrap() {
            "drive" => LocomotorKind::Drive,
            "ship" => LocomotorKind::Ship,
            "walk" => LocomotorKind::Walk,
            "hover" => LocomotorKind::Hover,
            _ => unreachable!(),
        };
        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        let mut e = entity(
            1,
            kind,
            coord(&input["current"]),
            coord(&input["stored_head"]),
        );
        let family = match kind {
            LocomotorKind::Drive => Some(crate::sim::movement::track_process::TrackFamily::Drive),
            LocomotorKind::Ship => Some(crate::sim::movement::track_process::TrackFamily::Ship),
            _ => None,
        };
        if let Some(family) = family {
            let loco = e.locomotor.as_mut().unwrap();
            if let Some(mut track) = loco.track_progress(family) {
                track.set_turn_index(input["turn_index"].as_i64().unwrap() as i32);
                track.set_cursor(input["cursor"].as_i64().unwrap() as i32);
                assert!(loco.store_track_progress(family, track));
            }
        }
        if let Some(tube) = input["tube"].as_array() {
            let exit = (
                tube[0].as_i64().unwrap() as u16,
                tube[1].as_i64().unwrap() as u16,
            );
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells_with_tubes(
                1,
                1,
                vec![],
                vec![TubeFact::explicit((0, 0), exit, 0, vec![0])],
            ));
            e.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
                TubeId(0),
                0,
                NULL_COORD,
            ));
        }
        sim.substrate.entities.insert(e);
        assert_eq!(
            sim.foot_navigation_coordinate(1).unwrap(),
            coord(&row["coordinate"]),
            "{input}"
        );
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 0, "Foot coordinate", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        if let Some(terrain) = sim.resolved_terrain.as_ref() {
            // Tube descriptors belong to the immutable map, rehydrated through
            // the production cache rebuild. Only the Foot transit state is saved.
            restored.rebuild_caches_after_load(
                terrain.clone(),
                crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
                &crate::sim::runtime::SimResources::empty().rules,
            );
        }
        assert_eq!(
            restored.foot_navigation_coordinate(1).unwrap(),
            coord(&row["coordinate"]),
            "restored {input}"
        );
        if input["tube"].is_array() {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.lifecycle.set_object_alive_for_test(false);
            e.lifecycle.set_in_limbo_for_test(true);
            e.locomotor = None;
            assert_eq!(
                sim.foot_navigation_coordinate(1).unwrap(),
                coord(&row["coordinate"])
            );
        }
    }
}

#[test]
fn hover_reads_its_location_z_and_at_coord_ignores_tube_exit() {
    let raw = DriveCoord::cell(5, 5, 37);
    let mut e = entity(1, LocomotorKind::Hover, raw, NULL_COORD);
    assert_eq!(navigation_coordinate(&e, None).unwrap().z, 37);
    // Exact Object Z already includes height; an object without one stands
    // on its coarse level.
    e.position.set_exact_z_leptons_for_test(None);
    e.position.set_z_for_test(1);
    assert_eq!(navigation_coordinate(&e, None).unwrap().z, 104);
    e.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
        TubeId(0),
        0,
        NULL_COORD,
    ));
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        1,
        1,
        vec![],
        vec![TubeFact::explicit((0, 0), (9, 10), 0, vec![0])],
    );
    assert_eq!(
        navigation_coordinate(&e, Some(&terrain)).unwrap(),
        DriveCoord::cell(9, 10, 0)
    );
    let at = crate::sim::movement::at_coord::AtCoordQuery::from_entity(&e).unwrap();
    assert_eq!(at.head_z(), 104);
    assert!(at.matches(DriveCoord { z: 104, ..raw }));
}

#[test]
fn raw_copy_classes_have_foot_coordinates_but_no_at_coord_match() {
    for kind in [
        LocomotorKind::Fly,
        LocomotorKind::Rocket,
        LocomotorKind::Teleport,
    ] {
        let current = DriveCoord::cell(8, 6, 917);
        let e = entity(1, kind, current, NULL_COORD);
        assert_eq!(navigation_coordinate(&e, None).unwrap(), current);
        assert!(crate::sim::movement::at_coord::AtCoordQuery::from_entity(&e).is_none());
    }
}

#[test]
fn production_drive_reaim_reads_retained_target_head() {
    let mut sim = Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    let mut mover = entity(
        1,
        LocomotorKind::Drive,
        DriveCoord::cell(3, 4, 0),
        NULL_COORD,
    );
    mover
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::Object { id: 2 }));
    assert!(mover.locomotor.as_mut().unwrap().store_track_destination(
        crate::sim::movement::track_process::TrackFamily::Drive,
        Some(DriveCoord::cell(7, 4, 0))
    ));
    let target_head = DriveCoord::cell(8, 4, 416);
    sim.substrate.entities.insert(mover);
    sim.substrate.entities.insert(entity(
        2,
        LocomotorKind::Walk,
        DriveCoord::cell(7, 4, 0),
        target_head,
    ));
    let destination = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .destination()
    };
    // Drive 0x4B05D0, after a track end in the same Process, re-aims +34 at
    // an Infantry NavCom's live coordinate.
    assert_eq!(destination(&sim), Some(DriveCoord::cell(7, 4, 0)));
    assert!(
        sim.begin_track_end_continuation(
            1,
            crate::sim::movement::track_process::TrackFamily::Drive,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    assert_eq!(destination(&sim), Some(target_head));
}

#[test]
fn missing_foot_receiver_is_an_error_and_tube_does_not_hide_missing_descriptor() {
    let mut e = entity(1, LocomotorKind::Drive, NULL_COORD, NULL_COORD);
    e.locomotor = None;
    assert!(navigation_coordinate(&e, None).is_err());
    e.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
        TubeId(0),
        0,
        NULL_COORD,
    ));
    e.locomotor = None;
    assert!(
        navigation_coordinate(&e, None)
            .unwrap_err()
            .contains("TubeClass")
    );
}

#[test]
fn flight_queries_follow_live_altitude_producers_and_keep_jumpjet_exact_z() {
    let mut sim = Simulation::new();
    let base = DriveCoord::cell(8, 6, 104);
    // Fly reads GetHeight against actual terrain.
    let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        16,
        16,
        (0..16)
            .flat_map(|ry| {
                (0..16).map(move |rx| {
                    crate::sim::world::common_raw_test_terrain_cell(rx, ry, 1, false)
                })
            })
            .collect(),
    );
    assert_eq!(
        super::super::ground_pose::ground_surface_z_at(
            [base.x, base.y],
            false,
            Some(&terrain),
            None,
        ),
        Some(104)
    );
    let mut fly = entity(1, LocomotorKind::Fly, base, NULL_COORD);
    fly.lifecycle.set_object_alive_for_test(true);
    fly.lifecycle.set_in_limbo_for_test(false);
    let loco = fly.locomotor.as_mut().unwrap();

    loco.set_fly_target_height(1000);
    // Process steps the height of a moving Fly (`0x004CDA0B`).
    loco.fly_runtime_mut()
        .unwrap()
        .retain_destination(DriveCoord::cell(12, 6, 0), None, || 0);
    sim.substrate.entities.insert(fly);
    sim.interner = crate::sim::intern::test_interner();
    sim.resolved_terrain = Some(terrain);
    sim.tick_air_movement_with_cell_lists_one(
        1,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let e = sim.substrate.entities.get(1).unwrap();
    let height =
        crate::sim::movement::air_movement::current_fly_height(e, sim.resolved_terrain.as_ref());
    assert!(height > 0);
    assert_eq!(sim.foot_navigation_coordinate(1).unwrap().z, 104 + height);
    let jumpjet = entity(
        3,
        LocomotorKind::Jumpjet,
        DriveCoord { z: 604, ..base },
        NULL_COORD,
    );
    assert_eq!(navigation_coordinate(&jumpjet, None).unwrap().z, 604);
}
