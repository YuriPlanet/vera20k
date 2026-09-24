use super::*;
use crate::map::entities::EntityCategory;
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::sim::components::{DriveLocomotionRuntime, NavTargetRef, ShipLocomotionRuntime};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
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
    e.category = if kind == LocomotorKind::Walk {
        EntityCategory::Infantry
    } else {
        EntityCategory::Unit
    };
    e.position.rx = (current.x / 256).clamp(0, i32::from(u16::MAX)) as u16;
    e.position.ry = (current.y / 256).clamp(0, i32::from(u16::MAX)) as u16;
    e.position.sub_x = SimFixed::from_num(current.x - i32::from(e.position.rx) * 256);
    e.position.sub_y = SimFixed::from_num(current.y - i32::from(e.position.ry) * 256);
    e.position.exact_z_leptons = Some(current.z);
    e.locomotor = Some(LocomotorState::for_test_kind(kind));
    match kind {
        LocomotorKind::Drive => {
            e.drive_locomotion = Some(DriveLocomotionRuntime {
                head_to: Some(head),
                ..Default::default()
            })
        }
        LocomotorKind::Ship => {
            e.ship_locomotion = Some(ShipLocomotionRuntime {
                head_to: Some(head),
                ..Default::default()
            })
        }
        LocomotorKind::Walk | LocomotorKind::Hover => {
            e.locomotor.as_mut().unwrap().set_step_head(Some(head))
        }
        _ => {}
    }
    e
}

#[test]
fn world_queries_match_all_original_foot_coordinate_rows_and_snapshot() {
    let rows: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/foot_navigation_coordinate.json"
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
        let track = e
            .drive_locomotion
            .as_mut()
            .map(|s| &mut s.track)
            .or_else(|| e.ship_locomotion.as_mut().map(|s| &mut s.track));
        if let Some(track) = track {
            track.turn_index = input["turn_index"].as_i64().unwrap() as i32;
            track.cursor = input["cursor"].as_i64().unwrap() as i32;
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
            e.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
                tube_id: TubeId(0),
                cursor: 0,
                target: NULL_COORD,
            });
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
                Vec::new(),
                Vec::new(),
            );
        }
        assert_eq!(
            restored.foot_navigation_coordinate(1).unwrap(),
            coord(&row["coordinate"]),
            "restored {input}"
        );
        if input["tube"].is_array() {
            let e = sim.substrate.entities.get_mut(1).unwrap();
            e.lifecycle.object_alive = false;
            e.lifecycle.in_limbo = true;
            e.locomotor = None;
            assert_eq!(
                sim.foot_navigation_coordinate(1).unwrap(),
                coord(&row["coordinate"])
            );
        }
    }
}

#[test]
fn hover_projects_altitude_once_and_at_coord_ignores_tube_exit() {
    let raw = DriveCoord::cell(5, 5, 37);
    let mut e = entity(1, LocomotorKind::Hover, raw, NULL_COORD);
    e.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(125);
    assert_eq!(navigation_coordinate(&e, None).unwrap().z, 37);
    // Exact Object Z already includes height. Only a legacy coarse position
    // needs the independent controller displacement projected into it.
    e.position.exact_z_leptons = None;
    e.position.z = 1;
    assert_eq!(navigation_coordinate(&e, None).unwrap().z, 229);
    e.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
        tube_id: TubeId(0),
        cursor: 0,
        target: NULL_COORD,
    });
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
    assert_eq!(at.head_z(), 229);
    assert!(at.matches(DriveCoord { z: 229, ..raw }));
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
    mover.navigation.nav_com = Some(NavTargetRef::Entity { id: 2 });
    mover.drive_locomotion.as_mut().unwrap().destination = Some(DriveCoord::cell(7, 4, 0));
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
            .drive_locomotion
            .as_ref()
            .unwrap()
            .destination
    };
    // Drive 0x4B05D0 is reached only after a track end in the same Process;
    // an ordinary visit leaves +34 alone.
    sim.process_ground_locomotor_for_test(1, None, None, None)
        .unwrap();
    assert_eq!(destination(&sim), Some(DriveCoord::cell(7, 4, 0)));
    assert!(
        sim.begin_track_end_continuation(
            1,
            crate::sim::movement::track_process::TrackFamily::Drive,
            None
        )
        .unwrap()
    );
    assert_eq!(destination(&sim), Some(target_head));
}

#[test]
fn missing_foot_receiver_is_an_error_and_tube_does_not_hide_missing_descriptor() {
    let mut e = entity(1, LocomotorKind::Drive, NULL_COORD, NULL_COORD);
    e.drive_locomotion = None;
    assert!(navigation_coordinate(&e, None).is_err());
    e.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
        tube_id: TubeId(0),
        cursor: 0,
        target: NULL_COORD,
    });
    e.locomotor = None;
    assert!(
        navigation_coordinate(&e, None)
            .unwrap_err()
            .contains("TubeClass")
    );
}

#[test]
fn flight_queries_follow_live_altitude_producers_and_keep_jumpjet_exact_z() {
    use crate::sim::movement::{air_movement, rocket_movement};
    let mut sim = Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    let base = DriveCoord::cell(8, 6, 104);
    // Fly reads GetHeight against actual terrain. Rocket still owns its
    // independent displacement controller and receives the same starting Z.
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
    let loco = fly.locomotor.as_mut().unwrap();

    loco.set_fly_target_height(1000);
    sim.substrate.entities.insert(fly);
    sim.substrate
        .entities
        .insert(entity(2, LocomotorKind::Rocket, base, NULL_COORD));
    assert!(rocket_movement::attach_rocket_state(
        &mut sim.substrate.entities,
        2,
        (8, 6),
        (18, 6),
        SimFixed::from_num(250)
    ));
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .rocket_state
        .as_mut()
        .unwrap()
        .phase = rocket_movement::RocketPhase::Ascent;
    air_movement::tick_air_movement(
        &mut sim.substrate.entities,
        &[1],
        1,
        1,
        Some(&terrain),
        None,
    );
    rocket_movement::tick_rocket_movement(&mut sim.substrate.entities, &[2], 1);
    for id in [1, 2] {
        let e = sim.substrate.entities.get(id).unwrap();
        let altitude = if id == 1 {
            e.locomotor.as_ref().unwrap().altitude
        } else {
            e.rocket_state.as_ref().unwrap().altitude
        };
        assert!(altitude > SimFixed::from_num(0));
        assert_eq!(
            sim.foot_navigation_coordinate(id).unwrap().z,
            104 + altitude.to_num::<i32>()
        );
    }
    // A retained payload image cannot override the active Rocket producer.
    if let crate::sim::movement::locomotion::LocomotorRuntimePayload::Rocket(Some(copy)) = &mut sim
        .substrate
        .entities
        .get_mut(2)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .runtime_payload
    {
        copy.altitude = SimFixed::from_num(2000);
    }
    let live = sim
        .substrate
        .entities
        .get(2)
        .unwrap()
        .rocket_state
        .as_ref()
        .unwrap()
        .altitude
        .to_num::<i32>();
    assert_eq!(sim.foot_navigation_coordinate(2).unwrap().z, 104 + live);
    let mut jumpjet = entity(
        3,
        LocomotorKind::Jumpjet,
        DriveCoord { z: 604, ..base },
        NULL_COORD,
    );
    jumpjet.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(500);
    assert_eq!(navigation_coordinate(&jumpjet, None).unwrap().z, 604);
}

#[test]
fn teleport_transfer_matches_original_coordinate_rows() {
    use crate::sim::movement::{locomotor_owner, teleport_movement};
    let rows: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/teleport_coordinate_switch.json"
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 24);
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let kind = match input["family"].as_str().unwrap() {
            "fly" => LocomotorKind::Fly,
            "hover" => LocomotorKind::Hover,
            "rocket" => LocomotorKind::Rocket,
            _ => unreachable!(),
        };
        let physical = coord(&input["coordinate"]);
        // Keep wide native XYZ in exact Z; the native XY extreme rows exercise
        // the transfer invariant rather than the map's bounded cell adapter.
        let mut e = entity(
            1,
            kind,
            DriveCoord {
                x: 1408,
                y: 1664,
                ..physical
            },
            NULL_COORD,
        );
        e.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(123);
        let occupied = input["occupied"].as_bool().unwrap();
        if occupied {
            let runtime = crate::sim::movement::locomotion::piggyback::StashedLocomotor::capture(
                e.locomotor.as_ref().unwrap(),
            );
            e.locomotor.as_mut().unwrap().piggyback = Some(runtime);
        }
        let before = current_coordinate(&e);
        let before_runtime = bincode::serialize(&e.locomotor).unwrap();
        let mut entities = crate::sim::entity_store::EntityStore::new();
        entities.insert(e);
        let accepted = teleport_movement::issue_teleport_command(
            &mut entities,
            1,
            (8, 9),
            &Default::default(),
            false,
            37,
        );
        assert_eq!(accepted, row["begin"].as_u64().unwrap() == 0);
        let e = entities.get_mut(1).unwrap();
        assert_eq!(current_coordinate(e), before);
        assert_eq!(current_coordinate(e).z, coord(&row["query"]).z);
        if occupied {
            assert_eq!(bincode::serialize(&e.locomotor).unwrap(), before_runtime);
            assert!(e.teleport_state.is_none());
        } else {
            e.teleport_state = None;
            assert!(locomotor_owner::restore_admitted_primary(e));
            assert_eq!(current_coordinate(e), before);
            assert_eq!(current_coordinate(e).z, coord(&row["after_end"]).z);
            assert_eq!(e.locomotor.as_ref().unwrap().active_kind(), kind);
        }
    }
}

#[test]
fn configured_teleporter_commands_keep_physical_z_through_save_relocate_and_restore() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::command::Command;
    use crate::sim::movement::{rocket_movement, teleport_movement};
    for kind in [
        LocomotorKind::Fly,
        LocomotorKind::Hover,
        LocomotorKind::Rocket,
    ] {
        for attack_move in [false, true] {
            for exact in [None, Some(-37)] {
                let rules = RuleSet::from_ini(&IniFile::from_str(
                    "[VehicleTypes]\n0=E1\n[E1]\nStrength=100\nSpeed=4\nTeleporter=yes\n\
                     [General]\nChronoTrigger=no\nChronoMinimumDelay=2\n",
                ))
                .unwrap();
                let mut sim = Simulation::new();
                let id = sim.allocate_stable_id();
                assert_eq!(id, 1);
                let e = entity(id, kind, DriveCoord::cell(5, 5, 0), NULL_COORD);
                // The entity helper interns its names; copy that table only
                // after construction so production command resolution sees them.
                sim.interner = crate::sim::intern::test_interner();
                // Commands require a revealed actor; inserting a constructed
                // object directly leaves its native InLimbo byte set.
                let (_, outcome) = sim.unlimbo(e);
                assert!(matches!(
                    outcome,
                    crate::sim::world::RevealOutcome::Revealed { .. }
                ));
                let e = sim.substrate.entities.get_mut(1).unwrap();
                e.position.z = 2;
                e.position.exact_z_leptons = exact;
                e.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(125);
                if kind == LocomotorKind::Rocket {
                    assert!(rocket_movement::attach_rocket_state(
                        &mut sim.substrate.entities,
                        1,
                        (5, 5),
                        (20, 5),
                        SimFixed::from_num(250),
                    ));
                    let e = sim.substrate.entities.get_mut(1).unwrap();
                    let rocket = e.rocket_state.as_mut().unwrap();
                    rocket.altitude = SimFixed::from_num(125);
                    rocket.phase = rocket_movement::RocketPhase::Ascent;
                    e.locomotor.as_mut().unwrap().runtime_payload =
                        crate::sim::movement::locomotion::LocomotorRuntimePayload::Rocket(Some(
                            rocket.clone(),
                        ));
                }
                let before = current_coordinate(sim.substrate.entities.get(1).unwrap());
                let command = if attack_move {
                    Command::AttackMove {
                        entity_id: 1,
                        target_rx: 8,
                        target_ry: 9,
                        queue: false,
                    }
                } else {
                    Command::Move {
                        entity_id: 1,
                        target_rx: 8,
                        target_ry: 9,
                        queue: false,
                        group_id: None,
                    }
                };
                assert!(
                    sim.apply_command(
                        "Americans",
                        &command,
                        Some(&rules),
                        None,
                        &Default::default()
                    ),
                    "{kind:?} attack_move={attack_move} exact={exact:?}"
                );
                assert_eq!(
                    sim.foot_navigation_coordinate(1).unwrap(),
                    before,
                    "{kind:?}"
                );
                for stage in 0..4 {
                    let bytes = crate::sim::snapshot::GameSnapshot::save(
                        &sim,
                        1,
                        0,
                        "Teleport coordinate",
                        0,
                    );
                    sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
                        .unwrap()
                        .sim;
                    sim.restore_after_snapshot_load().unwrap();
                    let expected_z = if stage == 0 { before.z } else { 312 };
                    assert_eq!(
                        sim.foot_navigation_coordinate(1).unwrap().z,
                        expected_z,
                        "{kind:?} stage {stage}"
                    );
                    if stage == 3 {
                        break;
                    }
                    let terrain = ResolvedTerrainGrid::from_cells(1, 1, Vec::new());
                    terrain.test_set_dummy_cell_level_slope(3, 0);
                    teleport_movement::tick_teleport_movement(
                        &mut sim.substrate.entities,
                        &mut sim.substrate.occupancy,
                        &[1],
                        stage,
                        Some(&terrain),
                        None,
                    );
                    // The Rocket pass exists later in the real object turn; it
                    // may not overwrite an active Teleport image during delay.
                    if kind == LocomotorKind::Rocket && stage < 2 {
                        let e = sim.substrate.entities.get(1).unwrap();
                        let before = e.rocket_state.clone();
                        let image = e.locomotor.as_ref().unwrap().runtime_payload.clone();
                        assert!(
                            rocket_movement::tick_rocket_movement(
                                &mut sim.substrate.entities,
                                &[1],
                                stage
                            )
                            .is_empty()
                        );
                        let e = sim.substrate.entities.get(1).unwrap();
                        assert_eq!(e.rocket_state, before);
                        assert_eq!(e.locomotor.as_ref().unwrap().runtime_payload, image);
                    }
                }
                let e = sim.substrate.entities.get(1).unwrap();
                assert!(e.teleport_state.is_none());
                assert_eq!(e.locomotor.as_ref().unwrap().active_kind(), kind);
                if kind == LocomotorKind::Rocket {
                    let before = current_coordinate(e).z;
                    rocket_movement::tick_rocket_movement(&mut sim.substrate.entities, &[1], 4);
                    let e = sim.substrate.entities.get(1).unwrap();
                    assert_eq!(
                        current_coordinate(e).z,
                        before + e.rocket_state.as_ref().unwrap().altitude.to_num::<i32>()
                    );
                } else {
                    assert_eq!(
                        e.locomotor.as_ref().unwrap().altitude,
                        SimFixed::from_num(0)
                    );
                }
            }
        }
    }
}
