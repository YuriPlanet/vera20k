use super::*;
use crate::sim::bridge_state::{BridgeRecordKind, BridgeRuntimeState};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_hierarchy::ZoneLevelGraph;
use serde_json::{Value, json};

fn coord(value: &Value) -> (u16, u16) {
    (
        value[0].as_i64().unwrap() as u16,
        value[1].as_i64().unwrap() as u16,
    )
}

fn record(value: &Value) -> BridgeEndpointRecord {
    BridgeEndpointRecord {
        endpoint_a: coord(&value[0]),
        endpoint_b: coord(&value[1]),
        active: value[2].as_bool().unwrap(),
        bridge_kind: if value[3] == 0 {
            BridgeRecordKind::High
        } else {
            BridgeRecordKind::Low
        },
    }
}

fn hierarchy(original: &Value) -> ZoneHierarchy {
    let width = original["width"].as_u64().unwrap() as u16;
    let mut levels = std::array::from_fn::<_, 3, _>(|level| {
        let ids = serde_json::from_value(original["zones"][level].clone()).unwrap();
        ZoneLevelGraph::new(31).with_cell_zone_ids(ids, width, width)
    });
    for graph in &mut levels {
        for zone in 0..32 {
            graph.push_edge(zone, ZoneEdgeRecord::new(zone, 1));
        }
    }
    let [a, b, c] = levels;
    ZoneHierarchy::new(a, b, c)
}

fn edges(hierarchy: &ZoneHierarchy) -> Value {
    json!(
        (0..3)
            .map(|level| (0..32)
                .map(|zone| hierarchy
                    .level(level)
                    .unwrap()
                    .edges(zone)
                    .iter()
                    .map(|edge| [u32::from(edge.neighbor), u32::from(edge.flag)])
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    )
}

#[test]
fn direct_repair_edges_match_original_all_theater_offsets_and_boundaries() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_repair_zones.json",
    ))
    .unwrap();
    let mut count = 0;
    for original in corpus["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["validate"] != true)
    {
        let input = &original["input"];
        let mut graph = hierarchy(original);
        let tile = input["tile"].as_u64().unwrap_or(106);
        let offset = (tile - if tile >= 200 { 200 } else { 100 }) as usize;
        let bridge = BridgeEndpointRecord {
            endpoint_a: coord(&input["a"]),
            endpoint_b: coord(&input["b"]),
            active: true,
            bridge_kind: BridgeRecordKind::High,
        };
        append_repaired_bridge_edges(
            &mut graph,
            &bridge,
            HIGH_BRIDGE_HIERARCHY_DIRECTIONS[offset] as u8,
            Some((8, 8)),
        )
        .unwrap();
        assert_eq!(edges(&graph), original["edges"], "{}", input["name"]);
        count += 1;
    }
    assert_eq!(count, 37);
}

#[test]
fn record_activation_matches_original_and_keeps_raw_connectivity_rows() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_repair_zones.json",
    ))
    .unwrap();
    let mut count = 0;
    for original in
        corpus["edges"].as_array().unwrap().iter().filter(|row| {
            row["input"]["validate"] == true && row["input"].get("recomputed").is_none()
        })
    {
        let input = &original["input"];
        let mut terrain = crate::sim::pathfinding::zone_map_tests::terrain_from_zone_classes(
            16, 16, &[0; 256], &[0; 256],
        );
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        terrain.cell_mut(5, 5).unwrap().final_tile_index = 106;
        let redirect_case = input["name"] == "source_fringe";
        if redirect_case {
            // Extra compatibility consumer regression. Native Validate's
            // bridge flags are false, so these structural flags do not alter
            // its captured result. A's fringe shortcut avoids a connectivity
            // rebuild despite distinct raw endpoint labels.
            for x in [7, 8] {
                terrain.cell_mut(x, 5).unwrap().bridge_facts.raw_flags = 0x100;
            }
            terrain.cell_mut(9, 5).unwrap().final_tile_index = 106;
        }
        let path = PathGrid::from_resolved_terrain(&terrain);
        let mut owner =
            BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 1, (8, 8));
        let records = input
            .get("records")
            .map(|v| v.as_array().unwrap().iter().map(record).collect())
            .unwrap_or_else(|| {
                vec![BridgeEndpointRecord {
                    endpoint_a: coord(&input["a"]),
                    endpoint_b: coord(&input["b"]),
                    active: false,
                    bridge_kind: BridgeRecordKind::High,
                }]
            });
        owner.test_set_endpoint_records(records);
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &path,
            &terrain,
            owner.endpoint_records(),
            16,
            16,
            Some((8, 8)),
        );
        zones.hierarchy = hierarchy(original);
        let base = &mut zones.base_topology;
        base.zone_ids.fill(1);
        let b = coord(&input["b"]);
        let native_index = (i32::from(b.1 as i16) * 17 + i32::from(b.0 as i16)).clamp(0, 288);
        base.zone_ids[(native_index / 17 * 16 + native_index % 17) as usize] = 2;
        let raw: Vec<u16> = input
            .get("raw")
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .unwrap_or(vec![2, 3]);
        base.raw_zone_ids_by_row[0] = vec![0, raw[0], raw[1]];
        let before = (base.zone_ids.clone(), base.raw_zone_ids_by_row.clone());
        if redirect_case {
            assert_eq!(
                zones.get_zone_id_native(&terrain, (7, 5), MovementZone::Normal, true),
                Some(3)
            );
        }
        let bounds: Vec<i32> = input
            .get("bounds")
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .unwrap_or(vec![0, 0, 8, 8]);
        let bounds = Some(PlayfieldBounds {
            base: 8,
            off_fc: bounds[0],
            off_100: bounds[1],
            off_104: bounds[2],
            off_108: bounds[3],
        });
        terrain.native_cell_identity((1234, -2345));
        let returned = owner
            .validate_repaired_zones(&terrain, (5, 5), |bridge| {
                zones.activate_repaired_bridge(&terrain, bridge, bounds)
            })
            .unwrap();
        zones.retain_repaired_bridge_records(&path, &terrain, owner.endpoint_records());
        assert_eq!(json!(returned), original["returned"], "{}", input["name"]);
        assert_eq!(
            edges(&zones.hierarchy),
            original["edges"],
            "{}",
            input["name"]
        );
        assert_eq!(
            json!(
                owner
                    .endpoint_records()
                    .iter()
                    .map(|r| r.active)
                    .collect::<Vec<_>>()
            ),
            original["active"]
        );
        let base = &zones.base_topology;
        assert_eq!(
            (base.zone_ids.clone(), base.raw_zone_ids_by_row.clone()),
            before
        );
        assert_eq!(
            json!(terrain.native_cell_coord(crate::map::cell_index::NativeCellIdentity::Dummy)),
            original["dummy"]
        );
        if redirect_case {
            assert!(!returned);
            assert_eq!(
                zones.get_zone_id_native(&terrain, (7, 5), MovementZone::Normal, true),
                Some(2)
            );
        }
        count += 1;
    }
    assert_eq!(count, 9);
}

#[test]
fn connectivity_consumes_retained_classes_and_matches_original_thirteen_rows() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_connectivity.json",
    ))
    .unwrap();
    let originals = corpus["cases"].as_array().unwrap();
    assert_eq!(originals.len(), 9);
    for original in originals {
        let input = &original["input"];
        let classes: Vec<u8> = serde_json::from_value(input["classes"].clone()).unwrap();
        let levels: Vec<u8> = serde_json::from_value(input["levels"].clone()).unwrap();
        let cell_levels: Vec<u8> = input
            .get("cell_levels")
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .unwrap_or_else(|| levels.clone());
        // Current terrain deliberately differs from the retained native class
        // plane. Connectivity must not silently perform another whole-map
        // Recalc before consuming that plane. The ninth case independently
        // changes current cell heights; connectivity still consumes cached ones.
        let terrain = crate::sim::pathfinding::zone_map_tests::terrain_from_zone_classes(
            16,
            16,
            &[0; 256],
            &cell_levels,
        );
        let path = PathGrid::from_resolved_terrain(&terrain);
        let records: Vec<_> = input["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(record)
            .collect();
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &path,
            &terrain,
            &records,
            16,
            16,
            Some((8, 8)),
        );
        let base = &mut zones.base_topology;
        base.movement_classes = classes.clone();
        base.levels = levels.clone();
        base.zone_ids.fill(0xabcd);
        let before_hierarchy = format!("{:?}", zones.hierarchy);
        zones.rebuild_base_connectivity_preserving_hierarchy(&path, &terrain, &records);
        let base = &zones.base_topology;
        assert_eq!(base.movement_classes, classes, "{}", input["name"]);
        assert_eq!(base.levels, levels, "{}", input["name"]);
        assert_eq!(
            json!(base.zone_ids),
            original["base_ids"],
            "{}",
            input["name"]
        );
        // Native's cluster count is the highest base label it assigned.
        assert_eq!(
            json!(base.zone_ids.iter().copied().max().unwrap_or(0)),
            original["zone_count"],
            "{}",
            input["name"]
        );
        assert_eq!(
            json!(base.raw_zone_ids_by_row),
            original["raw_rows"],
            "{}",
            input["name"]
        );
        assert_eq!(format!("{:?}", zones.hierarchy), before_hierarchy);
    }
}

#[test]
fn recalc_publication_updates_only_touched_class_and_unsigned_height() {
    let mut terrain = crate::sim::pathfinding::zone_map_tests::terrain_from_zone_classes(
        4, 4, &[0; 16], &[0; 16],
    );
    let path = PathGrid::from_resolved_terrain(&terrain);
    let mut zones = ZoneGrid::build_with_terrain(&path, &terrain, &[], 4, 4);
    let before_hierarchy = format!("{:?}", zones.hierarchy);
    let before_base = zones.base_topology.clone();
    for (x, level, class) in [(1, 255, 3), (2, 4, 2)] {
        let cell = terrain.cell_mut(x, 1).unwrap();
        cell.level = level;
        cell.zone_type = class;
    }
    assert!(zones.refresh_base_cell_attributes_at(&terrain, 1, 1));
    let base = &zones.base_topology;
    let mut expected_classes = before_base.movement_classes.clone();
    let mut expected_levels = before_base.levels.clone();
    expected_classes[5] = 3;
    expected_levels[5] = 255;
    assert_eq!(base.movement_classes, expected_classes);
    assert_eq!(base.levels, expected_levels);
    assert_eq!(base.zone_ids, before_base.zone_ids);
    assert_eq!(base.raw_zone_ids_by_row, before_base.raw_zone_ids_by_row);
    assert_eq!(format!("{:?}", zones.hierarchy), before_hierarchy);
}
