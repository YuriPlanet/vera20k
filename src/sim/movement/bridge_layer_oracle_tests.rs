//! Original Foot4DDC40→Object5F6A70 receipts through the shared Rust owner.
//! Supplied scalar/zone fixtures are inputs, not map-loader or flood-fill goldens.

use super::ground_pose;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

pub(crate) fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_bridge_layer.json",
    ))
    .unwrap()
}

pub(crate) fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

fn pair(value: &Value) -> (u16, u16) {
    (
        value[0].as_i64().unwrap() as u16,
        value[1].as_i64().unwrap() as u16,
    )
}

pub(crate) fn candidate(input: &Value, id: u64, type_name: &str) -> GameEntity {
    let current = coord(&input["current"]);
    let head = coord(&input["stored_head"]);
    let kind = match input["family"].as_str().unwrap() {
        "drive" => LocomotorKind::Drive,
        "ship" => LocomotorKind::Ship,
        "walk" => LocomotorKind::Walk,
        "hover" => LocomotorKind::Hover,
        _ => unreachable!(),
    };
    let mut entity = GameEntity::test_default(id, type_name, "Victim", 0, 0);
    entity.category = if kind == LocomotorKind::Walk {
        EntityCategory::Infantry
    } else {
        EntityCategory::Unit
    };
    entity.mission_leaf =
        crate::sim::mission::MissionLeafState::for_entity_category(entity.category);
    entity.position.rx = (current.x / 256).clamp(0, i32::from(u16::MAX)) as u16;
    entity.position.ry = (current.y / 256).clamp(0, i32::from(u16::MAX)) as u16;
    entity.position.sub_x = SimFixed::from_num(current.x - i32::from(entity.position.rx) * 256);
    entity.position.sub_y = SimFixed::from_num(current.y - i32::from(entity.position.ry) * 256);
    entity.position.exact_z_leptons = Some(current.z);
    entity.on_bridge = input["on_bridge"].as_bool().unwrap();
    if input["locomotor_present"].as_bool().unwrap() {
        entity.locomotor = Some(LocomotorState::for_test_kind(kind));
        let track = crate::sim::components::TrackProgress {
            turn_index: input["turn_index"].as_i64().unwrap_or(-1) as i32,
            cursor: input["cursor"].as_i64().unwrap_or(0) as i32,
            ..Default::default()
        };
        match kind {
            LocomotorKind::Drive => {
                assert!(
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_drive_state_for_test(Some(
                            DriveLocomotionRuntime::default()
                                .with_head_to_for_test(Some(head))
                                .with_track_for_test(track)
                        ))
                )
            }
            LocomotorKind::Ship => {
                assert!(
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_ship_state_for_test(Some(
                            ShipLocomotionRuntime::default()
                                .with_head_to_for_test(Some(head))
                                .with_track_for_test(track)
                        ))
                )
            }
            LocomotorKind::Hover => entity
                .locomotor
                .as_mut()
                .and_then(|loco| loco.hover_runtime_mut())
                .unwrap()
                .set_head(Some(head)),
            _ => entity.locomotor.as_mut().unwrap().set_step_head(Some(head)),
        }
    }
    if input["tube"].is_array() {
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 0,
            target: DriveCoord { x: 0, y: 0, z: 0 },
        });
    }
    if input["nav_com_coord"].is_array() {
        let nav = coord(&input["nav_com_coord"]);
        entity.navigation.nav_com = Some(NavTargetRef::Cell {
            rx: (nav.x / 256) as u16,
            ry: (nav.y / 256) as u16,
        });
    }
    entity
}

pub(crate) fn terrain(input: &Value) -> ResolvedTerrainGrid {
    let supplied = input["cells"].as_array().unwrap();
    let width = supplied
        .iter()
        .map(|cell| pair(&cell["xy"]).0 + 1)
        .max()
        .unwrap_or(16)
        .max(16);
    let height = supplied
        .iter()
        .map(|cell| pair(&cell["xy"]).1 + 1)
        .max()
        .unwrap_or(16)
        .max(16);
    let mut cells: Vec<_> = (0..height)
        .flat_map(|y| (0..width).map(move |x| crate::map::resolved_terrain::test_clear_cell(x, y)))
        .collect();
    for scalar in supplied {
        let (x, y) = pair(&scalar["xy"]);
        let cell = &mut cells[usize::from(y) * usize::from(width) + usize::from(x)];
        cell.level = scalar["level"].as_i64().unwrap() as u8;
        cell.slope_type = scalar["slope"].as_u64().unwrap() as u8;
        cell.bridge_facts.raw_flags = scalar["flags"].as_u64().unwrap() as u32;
    }
    let tubes = input["tube"]
        .as_array()
        .map(|tube| {
            vec![TubeFact::explicit(
                (0, 0),
                pair(&Value::Array(tube.clone())),
                0,
                vec![0],
            )]
        })
        .unwrap_or_default();
    let mut terrain = ResolvedTerrainGrid::from_cells_with_tubes(width, height, cells, tubes);
    terrain.test_set_native_allocated_cells(
        &supplied
            .iter()
            .map(|cell| pair(&cell["xy"]))
            .collect::<Vec<_>>(),
    );
    reset_dummy(&terrain, input);
    terrain
}

pub(crate) fn reset_dummy(terrain: &ResolvedTerrainGrid, input: &Value) {
    let dummy = terrain.shared_cell_dummy();
    let raw = &input["dummy"];
    dummy.set_level_slope(
        raw["level"].as_i64().unwrap() as i8,
        raw["slope"].as_u64().unwrap() as u8,
    );
    dummy.write_raw_flags(raw["flags"].as_u64().unwrap() as u32);
    dummy.stamp_coord(
        raw["xy"][0].as_i64().unwrap() as i32,
        raw["xy"][1].as_i64().unwrap() as i32,
    );
}

pub(crate) fn zones(terrain: &ResolvedTerrainGrid, row: &Value) -> ZoneGrid {
    let supplied = &row["zones"];
    let size = pair(&supplied["size"]);
    let records: Vec<_> = supplied["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| BridgeEndpointRecord {
            endpoint_a: pair(&record["a"]),
            endpoint_b: pair(&record["b"]),
            active: record["active"].as_bool().unwrap(),
            bridge_kind: BridgeRecordKind::High,
        })
        .collect();
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &PathGrid::from_resolved_terrain(terrain),
        terrain,
        &records,
        terrain.width(),
        terrain.height(),
        Some((i32::from(size.0), i32::from(size.1))),
    );
    let base = zones.base_topology_mut();
    base.zone_ids.fill(0);
    for cell in supplied["group_cells"].as_array().unwrap() {
        let (x, y) = pair(&cell["xy"]);
        base.zone_ids[usize::from(y) * usize::from(terrain.width()) + usize::from(x)] =
            cell["group"].as_u64().unwrap() as u16;
    }
    let labels: Vec<_> = supplied["raw_labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect();
    for raw in &mut base.raw_zone_ids_by_row {
        *raw = labels.clone();
    }
    reset_dummy(terrain, &row["input"]);
    zones
}

pub(crate) fn assert_dummy(terrain: &ResolvedTerrainGrid, expected: &Value, context: &str) {
    let dummy = terrain.shared_cell_dummy();
    let snap = dummy.snapshot();
    assert_eq!(
        snap.coord,
        (
            expected["xy"][0].as_i64().unwrap() as i32,
            expected["xy"][1].as_i64().unwrap() as i32
        ),
        "{context}: Dummy coordinate"
    );
    assert_eq!(
        (snap.level, snap.slope_type, dummy.raw_flags()),
        (
            expected["level"].as_i64().unwrap() as i8,
            expected["slope"].as_u64().unwrap() as u8,
            expected["flags"].as_u64().unwrap() as u32
        ),
        "{context}: retained Dummy fields"
    );
}

#[test]
fn native_bridge_fresh_drive_ship_queries_survive_snapshot() {
    let corpus = corpus();
    let rows = corpus["constructor_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap();
        let terrain = terrain(input);
        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        sim.resolved_terrain = Some(terrain.clone());
        let mut entity = candidate(input, 1, "DEFENDER");
        // Drive4AF540/Ship69EC50, including base55A6C0, initialize a null
        // retained head. The Rust owner stores that fresh payload lazily.
        if let Some(loco) = entity.locomotor.as_mut() {
            let _ = loco.install_drive_state_for_test(None);
        };
        if let Some(loco) = entity.locomotor.as_mut() {
            let _ = loco.install_ship_state_for_test(None);
        };
        sim.substrate.entities.insert(entity);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 0, context, 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            &crate::sim::runtime::SimResources::empty().rules,
        );
        for state in [&sim, &restored] {
            let terrain = state.resolved_terrain.as_ref().unwrap();
            reset_dummy(terrain, input);
            let navigation = state.foot_navigation_coordinate(1).unwrap();
            assert_eq!(navigation, coord(&row["output"]["coordinate"]), "{context}");
            assert_eq!(
                ground_pose::navigation_should_be_on_bridge(
                    &NativeCellQuery::canonical(terrain),
                    navigation,
                    coord(&input["current"]),
                    input["on_bridge"].as_bool().unwrap(),
                    false,
                )
                .unwrap(),
                row["output"]["should_be_on_bridge"].as_bool().unwrap(),
                "{context}: source layer"
            );
            assert_dummy(terrain, &row["output"]["final_dummy"], context);
        }
    }
}

#[test]
fn native_bridge_layers_match_all_416_original_rows() {
    let corpus = corpus();
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 416);
    for row in rows {
        let input = &row["input"];
        let context = format!("{}:{}", input["family"], input["name"]);
        let terrain = terrain(input);
        let cells = NativeCellQuery::canonical(&terrain);
        for prefix in input["prefix_queries"].as_array().unwrap() {
            let xyz = coord(prefix);
            let _ = cells.lookup_world(xyz.x, xyz.y);
        }
        let current = coord(&input["current"]);
        // The two i32 extreme physical inputs are scalar arithmetic witnesses;
        // Position's cell/subcell storage cannot represent those coordinates.
        let navigation = if input["name"]
            .as_str()
            .unwrap()
            .starts_with("signed_extreme_dummy")
        {
            super::foot_coordinate::head_or_current(Some(coord(&input["stored_head"])), current)
        } else {
            let mut sim = Simulation::new();
            sim.resolved_terrain = Some(terrain.clone());
            sim.substrate
                .entities
                .insert(candidate(input, 1, "DEFENDER"));
            sim.foot_navigation_coordinate(1).unwrap()
        };
        assert_eq!(
            navigation,
            coord(&row["output"]["coordinate"]),
            "{context}: Foot coordinate"
        );
        assert_eq!(
            ground_pose::navigation_should_be_on_bridge(
                &cells,
                navigation,
                current,
                input["on_bridge"].as_bool().unwrap(),
                input["tube"].is_array()
            )
            .unwrap(),
            row["output"]["should_be_on_bridge"].as_bool().unwrap(),
            "{context}: source layer"
        );
        assert_dummy(&terrain, &row["output"]["final_dummy"], &context);
    }
}
