// Original581F90/584550/5824A0 outputs, including mutable native padding.
// Batch586990 inputs also exercise full construction here; batch orchestration
// needs its own production-host comparison and is not certified by this test.
use crate::map::playfield::PlayfieldBounds;

pub(crate) fn hierarchy_native_bounds(input: &serde_json::Value) -> PlayfieldBounds {
    let size = &input["size"];
    let bounds = input
        .get("bounds")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([0, 0, size[0], size[1]]));
    PlayfieldBounds {
        base: size[0].as_i64().unwrap() as i32,
        off_fc: bounds[0].as_i64().unwrap() as i32,
        off_100: bounds[1].as_i64().unwrap() as i32,
        off_104: bounds[2].as_i64().unwrap() as i32,
        off_108: bounds[3].as_i64().unwrap() as i32,
    }
}

pub(crate) fn hierarchy_native_fixture(
    input: &serde_json::Value,
) -> (
    BaseZoneTopology,
    ResolvedTerrainGrid,
    Vec<BridgeEndpointRecord>,
) {
    let w = input["size"][0].as_i64().unwrap() as i32;
    let h = input["size"][1].as_i64().unwrap() as i32;
    let width = (w + h) as u16;
    let classes = input["classes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    let ids = input["base_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect();
    let mut base = hierarchy_base(classes, ids);
    base.levels = input["levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    base.native_bridge_source_size = Some((w, h));
    let mut terrain = redirect_terrain(width, width, Some(100), Some(200), |cell| {
        let i = usize::from(cell.ry) * usize::from(width) + usize::from(cell.rx);
        cell.level = base.levels[i];
        cell.zone_type = base.movement_classes[i];
        cell.final_tile_index = 2;
        cell.yr_cell_land_type = 3;
    });
    let mut allocated = Vec::new();
    for y in 0..i32::from(width) {
        for x in 0..i32::from(width) {
            if input["allocation"] != "size_diamond"
                || (w < x + y && x + y <= w + 2 * h && (x - y).abs() < w)
            {
                allocated.push((x as u16, y as u16));
            }
        }
    }
    terrain.test_set_native_allocated_cells(&allocated);
    terrain.shared_cell_dummy().stamp_coord(1234, -2345);
    let mut records = Vec::new();
    if let Some(rows) = input["records"].as_array() {
        for row in rows {
            let a = (
                row[0][0].as_u64().unwrap() as u16,
                row[0][1].as_u64().unwrap() as u16,
            );
            let b = (
                row[1][0].as_u64().unwrap() as u16,
                row[1][1].as_u64().unwrap() as u16,
            );
            terrain.cells[usize::from(a.1) * usize::from(width) + usize::from(a.0)]
                .final_tile_index = row[3].as_i64().unwrap() as i32;
            records.push(BridgeEndpointRecord {
                endpoint_a: a,
                endpoint_b: b,
                active: row[2].as_bool().unwrap(),
                bridge_kind: BridgeRecordKind::High,
            });
        }
    }
    (base, terrain, records)
}

#[derive(Default)]
struct HierarchyQueryTrace {
    queries: Vec<[i32; 2]>,
    dummy_writes: Vec<[i32; 2]>,
}

impl HierarchyQueryTrace {
    fn query(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        bounds: PlayfieldBounds,
        x: i32,
        y: i32,
    ) -> bool {
        let (x, y) = (i32::from(x as i16), i32::from(y as i16));
        self.queries.push([x, y]);
        if terrain
            .native_fixed_cell_index(x as i16, y as i16)
            .is_none()
        {
            self.dummy_writes.push([x, y]);
        }
        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (x, y),
            Some(bounds),
            Some(terrain),
        )
    }
}

fn hierarchy_native_sequence(actual: serde_json::Value, expected: &serde_json::Value, label: &str) {
    let actual = actual.as_array().unwrap();
    let expected = expected.as_array().unwrap();
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(
            a,
            b,
            "{label} at {i}; lengths {}/{}",
            actual.len(),
            expected.len()
        );
    }
    assert_eq!(actual.len(), expected.len(), "{label} length");
}

pub(crate) fn assert_native_hierarchy_graphs(
    hierarchy: &ZoneHierarchy,
    terrain: &ResolvedTerrainGrid,
    expected: &serde_json::Value,
    label: &str,
) {
    let width = usize::from(terrain.width());
    let side = width + 1;
    for level in 0..3 {
        let graph = hierarchy.level(level).unwrap();
        let native = &expected["graphs"][level];
        hierarchy_native_sequence(
            serde_json::json!(graph.cell_zone_ids()),
            &native["ids"],
            &format!("{label} level{level} IDs"),
        );
        for (i, expected) in native["ids"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                serde_json::json!(
                    graph
                        .native_zone_at(
                            ((i % width) as u16, (i / width) as u16),
                            Some((width as i32, 0))
                        )
                        .unwrap()
                ),
                *expected,
                "{label} level{level} consumer represented {i}"
            );
        }
        let padding: Vec<_> = (0..side * side)
            .filter(|i| i % side >= width || i / side >= width)
            .map(|i| graph.native_padding_zone(i))
            .collect();
        hierarchy_native_sequence(
            serde_json::json!(padding),
            &native["padding_ids"],
            &format!("{label} level{level} padding"),
        );
        // The consumer lookup must return these same retained padding IDs.
        // Side is determined by Size.W+Size.H+1; this decomposition preserves it.
        for (i, expected) in (0..side * side)
            .filter(|i| i % side >= width || i / side >= width)
            .zip(native["padding_ids"].as_array().unwrap())
        {
            assert_eq!(
                serde_json::json!(
                    graph
                        .native_zone_at(
                            ((i % side) as u16, (i / side) as u16),
                            Some((width as i32, 0))
                        )
                        .unwrap()
                ),
                *expected,
                "{label} level{level} consumer padding {i}"
            );
        }
        let records: Vec<_> = (0..graph.record_slot_count())
            .map(|i| {
                let r = graph.record(i as u16).unwrap();
                let edges: Vec<_> = graph
                    .edges(i as u16)
                    .iter()
                    .map(|e| [u32::from(e.neighbor), u32::from(e.flag)])
                    .collect();
                serde_json::json!({"parent":r.parent, "zone_type":r.zone_type, "edges":edges})
            })
            .collect();
        hierarchy_native_sequence(
            serde_json::json!(records),
            &native["records"],
            &format!("{label} level{level} records"),
        );
    }
}

fn assert_native_hierarchy_state(
    hierarchy: &ZoneHierarchy,
    terrain: &ResolvedTerrainGrid,
    trace: &HierarchyQueryTrace,
    expected: &serde_json::Value,
    label: &str,
) {
    assert_native_hierarchy_graphs(hierarchy, terrain, expected, label);
    hierarchy_native_sequence(
        serde_json::json!(trace.queries),
        &expected["queries"],
        &format!("{label} queries"),
    );
    hierarchy_native_sequence(
        serde_json::json!(trace.dummy_writes),
        &expected["dummy_writes"],
        &format!("{label} dummy writes"),
    );
    assert_eq!(
        serde_json::json!(terrain.shared_cell_dummy().snapshot().coord),
        expected["dummy"],
        "{label} final dummy"
    );
}

#[test]
fn bridge_hierarchy_native_full_and_local_graphs_queries_and_padding() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_hierarchy.json",
    ))
    .unwrap();
    let mut local_count = 0;
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["input"];
        let label = input["name"].as_str().unwrap();
        let (mut base, mut terrain, records) = hierarchy_native_fixture(input);
        let mut bounds = hierarchy_native_bounds(input);
        let width = terrain.width();
        let mut trace = HierarchyQueryTrace::default();
        let mut hierarchy = build_zone_hierarchy_with_query(
            &base,
            Some(&terrain),
            &records,
            width,
            width,
            &mut |x, y| trace.query(&terrain, bounds, x, y),
        );
        assert_native_hierarchy_state(&hierarchy, &terrain, &trace, &case["initial"], label);
        for (action, expected) in input["actions"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["states"].as_array().unwrap())
        {
            if action.get("cells").is_some() {
                break;
            }
            if let Some(changes) = action["changes"].as_array() {
                for change in changes {
                    let x = change["cell"][0].as_u64().unwrap() as u16;
                    let y = change["cell"][1].as_u64().unwrap() as u16;
                    let i = usize::from(y) * usize::from(width) + usize::from(x);
                    if let Some(v) = change["class"].as_u64() {
                        base.movement_classes[i] = v as u8;
                    }
                    if let Some(v) = change["level"].as_u64() {
                        base.levels[i] = v as u8;
                    }
                    if let Some(v) = change["base_id"].as_u64() {
                        base.zone_ids[i] = v as u16;
                    }
                    if let Some(v) = change["cell_level"].as_u64() {
                        terrain.cells[i].level = v as u8;
                    }
                    if change["clear_level0"].as_bool().unwrap_or(false) {
                        hierarchy.levels_mut()[0].set_zone_at(i32::from(x), i32::from(y), 0);
                    }
                }
            }
            if let Some(b) = action.get("bounds") {
                let mut adjusted = input.clone();
                adjusted["bounds"] = b.clone();
                bounds = hierarchy_native_bounds(&adjusted);
            }
            trace = HierarchyQueryTrace::default();
            let coord = (
                action["coord"][0].as_i64().unwrap() as i16,
                action["coord"][1].as_i64().unwrap() as i16,
            );
            assert_eq!(
                patch_zone_hierarchy_with_query(
                    &mut hierarchy,
                    &base,
                    &terrain,
                    &records,
                    coord,
                    width,
                    width,
                    &mut |x, y| trace.query(&terrain, bounds, x, y)
                ),
                LocalHierarchyPatchResult::Patched,
                "{label}"
            );
            assert_native_hierarchy_state(&hierarchy, &terrain, &trace, expected, label);
            assert_eq!(
                serde_json::json!(base.movement_classes),
                expected["classes"],
                "{label} retained classes"
            );
            assert_eq!(
                serde_json::json!(base.levels),
                expected["levels"],
                "{label} retained heights"
            );
            assert_eq!(
                serde_json::json!(base.zone_ids),
                expected["base_ids"],
                "{label} retained base IDs"
            );
            local_count += 1;
        }
    }
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 18);
    assert_eq!(local_count, 10);
}

#[test]
fn bridge_hierarchy_native_flood_last_neighbor_query_order_and_flag() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_hierarchy.json",
    ))
    .unwrap();
    for case in corpus["floods"].as_array().unwrap() {
        let input = &case["input"];
        let label = input["name"].as_str().unwrap();
        let (mut base, mut terrain, _) = hierarchy_native_fixture(input);
        let mut graph = ZoneLevelGraph::new(10).with_cell_zone_ids(vec![0; 256], 16, 16);
        let spec = &input["flood"];
        for y in 5..8 {
            for x in 5..9 {
                let seed = y == 6 && (x == 6 || x == 7);
                let horizontal =
                    spec["vertical"].as_bool().unwrap_or(false) && y == 6 && (x == 5 || x == 8);
                graph.set_zone_at(x, y, if seed || horizontal { 0 } else { 2 });
                base.zone_ids[(y * 16 + x) as usize] = if seed { 1 } else { 9 };
            }
        }
        let missing = spec["missing"].as_array().unwrap();
        let allocated: Vec<_> = (0..16u16)
            .flat_map(|y| (0..16u16).map(move |x| (x, y)))
            .filter(|&(x, y)| !missing.iter().any(|p| p == &serde_json::json!([x, y])))
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
        let mut buckets = HierarchyEdgeBuckets::new();
        if spec["seed_pair"].as_bool().unwrap_or(false) {
            buckets.register(2, 10, 1);
        }
        let mut trace = HierarchyQueryTrace::default();
        let bounds = hierarchy_native_bounds(input);
        let advance = flood_fill_hierarchy_scanline(
            (6, 6),
            10,
            1,
            HierarchyCells {
                base: &base,
                width: 16,
                height: 16,
                native_scan_offset: None,
            },
            &mut graph,
            HierarchyBlock {
                x_min: 6,
                x_max: 7,
                y_min: 6,
                y_max: 6,
            },
            &mut buckets,
            &mut |x, y| trace.query(&terrain, bounds, x, y),
        );
        let pairs: Vec<_> = buckets
            .buckets
            .iter()
            .flatten()
            .map(|e| {
                [
                    u32::from(e.existing),
                    u32::from(e.current),
                    u32::from(e.flag),
                ]
            })
            .collect();
        assert_eq!(serde_json::json!(pairs), case["pairs"], "{label} pairs");
        assert_eq!(
            serde_json::json!(advance),
            case["return_advance"],
            "{label} advance"
        );
        assert_eq!(
            serde_json::json!(trace.queries),
            case["queries"],
            "{label} queries"
        );
        assert_eq!(
            serde_json::json!(trace.dummy_writes),
            case["dummy_writes"],
            "{label} dummy writes"
        );
        assert_eq!(
            serde_json::json!(terrain.shared_cell_dummy().snapshot().coord),
            case["dummy"],
            "{label} dummy"
        );
    }
    assert_eq!(corpus["floods"].as_array().unwrap().len(), 3);
}

#[test]
fn bridge_hierarchy_native_world_endpoint_lookup_retains_boundary_zone() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_hierarchy.json",
    ))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["input"]["name"] == "retained_base0_padding_full")
        .unwrap();
    let (base, terrain, records) = hierarchy_native_fixture(&case["input"]);
    let bounds = hierarchy_native_bounds(&case["input"]);
    let width = terrain.width();
    let hierarchy = build_zone_hierarchy_with_query(
        &base,
        Some(&terrain),
        &records,
        width,
        width,
        &mut |x, y| {
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (x, y),
                Some(bounds),
                Some(&terrain),
            )
        },
    );
    let path = PathGrid::from_resolved_terrain(&terrain);
    let mut zones = super::super::zone_map::ZoneGrid::build_with_native_bridge_geometry(
        &path,
        &terrain,
        &records,
        width,
        width,
        base.native_bridge_source_size,
    );
    // Supply the already native-compared retained-cache hierarchy, as the
    // load-time reconstruction owner must do. This tests endpoint consumption.
    zones.replace_hierarchy(hierarchy);
    for level in 0..3 {
        let expected = case["initial"]["graphs"][level]["padding_ids"][8]
            .as_u64()
            .unwrap() as u16;
        assert_eq!(
            zones.hierarchy_zone_at_native(level, (19, 8)),
            Some(expected)
        );
        let aliased = case["initial"]["graphs"][level]["ids"][9 * 19 + 3]
            .as_u64()
            .unwrap() as u16;
        assert_eq!(
            zones.hierarchy_zone_at_native(level, (23, 8)),
            Some(aliased)
        );
    }
    assert!(zones.hierarchy_zone_at_native(2, (19, 8)).unwrap() != 0);
}

// Exercise the actual world construction and one-cell repair wrappers against
// native graphs. Query ordering itself remains covered by the traced core test.
#[test]
fn live_world_navigation_and_sentinel_repair_match_native_hierarchy_graphs() {
    use crate::sim::pathfinding::zone_incremental::{
        PackedZoneCoord, ZoneRepairKind, ZoneRepairOutcome, repair_zone_cell,
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_hierarchy.json",
    ))
    .unwrap();
    for name in [
        "clear_local",
        "sentinel_base_still_updates_live_hierarchy",
        "full_live_bounds_narrower_than_cached_classes",
    ] {
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["input"]["name"] == name)
            .unwrap();
        let input = &case["input"];
        let (_, terrain, records) = hierarchy_native_fixture(input);
        let bounds = hierarchy_native_bounds(input);
        let path = PathGrid::from_resolved_terrain(&terrain);
        let mut sim = crate::sim::world::Simulation::with_seed(0x584550);
        sim.playfield_bounds = Some(bounds);
        sim.bridge_state = Some(
            crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
                &terrain,
                true,
                300,
                (8, 8),
            ),
        );
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.rebuild_zone_grid(&path);
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let zones = sim.zone_grid.as_mut().unwrap();
        assert_native_hierarchy_graphs(
            zones.hierarchy_for(MovementZone::Normal).unwrap(),
            terrain,
            &case["initial"],
            name,
        );
        assert_eq!(
            serde_json::json!(terrain.shared_cell_dummy().snapshot().coord),
            case["initial"]["dummy"],
            "{name}: full-construction dummy"
        );
        if input["actions"].as_array().unwrap().is_empty() {
            continue;
        }
        if name.starts_with("sentinel") {
            zones.base_topology_mut().movement_classes[10 * 16 + 10] = zone_class::OUTSIDE;
            // This stale projection must not preempt the actual mode-one query.
            terrain.cell_mut(10, 10).unwrap().outside_playfield = true;
        }
        let outcome = repair_zone_cell(
            zones,
            PackedZoneCoord::new(10, 10),
            ZoneRepairKind::AssignOrphaned,
            &path,
            Some(bounds),
            terrain,
            &records,
        );
        assert_eq!(
            outcome,
            if name.starts_with("sentinel") {
                ZoneRepairOutcome::SentinelNoOp
            } else {
                ZoneRepairOutcome::Adopted { cluster: 1 }
            },
            "{name}"
        );
        assert_native_hierarchy_graphs(
            zones.hierarchy_for(MovementZone::Normal).unwrap(),
            terrain,
            &case["states"][0],
            name,
        );
        assert_eq!(
            serde_json::json!(terrain.shared_cell_dummy().snapshot().coord),
            case["states"][0]["dummy"],
            "{name}: local-repair dummy"
        );
    }
}
