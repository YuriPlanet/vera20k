#[test]
fn native_tube_hierarchy_pairs_match_original_executable() {
    use crate::map::tube_facts::{TubeFact, TubeId};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tube_hierarchy.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 65);
    for case in cases {
        let coord =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let rows = case["tubes"].as_array().unwrap();
        let tubes: Vec<_> = rows
            .iter()
            .map(|t| {
                TubeFact::explicit(
                    coord(&t["entry"]),
                    coord(&t["exit"]),
                    t["direction"].as_i64().unwrap() as i32,
                    t["path"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_i64().unwrap() as i32)
                        .collect(),
                )
            })
            .collect();
        let missing: Vec<_> = case["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        let allocated: Vec<_> = rows
            .iter()
            .enumerate()
            .filter(|(i, _)| !missing.contains(i))
            .filter_map(|(i, t)| {
                let (x, y) = coord(&t["cell"]);
                let index = i32::from(y as i16) * 512 + i32::from(x as i16);
                (0..0x40000).contains(&index).then_some((
                    i,
                    (index % 512) as u16,
                    (index / 512) as u16,
                ))
            })
            .collect();
        let tw = allocated.iter().map(|(_, x, _)| x + 1).max().unwrap_or(1);
        let th = allocated.iter().map(|(_, _, y)| y + 1).max().unwrap_or(1);
        let mut cells = redirect_terrain(tw, th, None, None, |_| {}).cells;
        for &(i, x, y) in &allocated {
            let cell = &mut cells[usize::from(y) * usize::from(tw) + usize::from(x)];
            cell.tube_index = Some(TubeId(i as u16));
            cell.final_tile_index = if i == 0 {
                case["high_tile"].as_i64().unwrap_or(0) as i32
            } else {
                0
            };
        }
        let mut terrain = ResolvedTerrainGrid::from_cells_with_tubes(tw, th, cells, tubes);
        terrain.test_set_native_allocated_cells(
            &allocated
                .iter()
                .map(|&(_, x, y)| (x, y))
                .collect::<Vec<_>>(),
        );
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(1234, -2345);
        dummy.write_raw_tube_index(case["dummy_index"].as_i64().unwrap() as i16);
        let zones: Vec<_> = case["zones"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u16)
            .collect();
        let record = BridgeEndpointRecord {
            endpoint_a: coord(&case["a"]),
            endpoint_b: coord(&case["b"]),
            active: true,
            bridge_kind: BridgeRecordKind::Low,
        };
        let mut buckets = HierarchyEdgeBuckets::new();
        for seed in case["seed_pairs"].as_array().unwrap() {
            register_native_hierarchy_pair(
                &mut buckets,
                seed[0].as_u64().unwrap() as u16,
                seed[1].as_u64().unwrap() as u16,
                seed[2].as_u64().unwrap() as u8,
            );
        }
        let width = case["width"].as_u64().unwrap() as u16;
        register_bridge_hierarchy_edges_for_record(
            &mut buckets,
            &zones,
            &terrain,
            &record,
            width,
            width,
            Some((
                case["size"][0].as_i64().unwrap() as i32,
                case["size"][1].as_i64().unwrap() as i32,
            )),
        );
        let actual: Vec<_> = buckets
            .buckets
            .iter()
            .enumerate()
            .flat_map(|(i, bucket)| {
                bucket.iter().map(move |e| {
                    [
                        i as u32,
                        (u32::from(e.existing) << 16) | u32::from(e.current),
                        u32::from(e.flag),
                    ]
                })
            })
            .collect();
        assert_eq!(serde_json::json!(actual), case["edges"], "{}", case["name"]);
        assert_eq!(
            serde_json::json!(dummy.snapshot().coord),
            case["dummy_coord"],
            "{} dummy",
            case["name"]
        );
    }
}

#[test]
fn tube_hierarchy_generated_records_connect_full_and_local_precheck() {
    use super::super::zone_hierarchy::{
        ZonePrecheckExclusions, ZonePrecheckOutcome, zone_precheck_flat,
    };
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::bridge_state::BridgeRuntimeState;
    let (width, height) = (64, 64);
    let mut cells = redirect_terrain(width, height, None, None, |cell| {
        if cell.rx == 32 {
            cell.zone_type = zone_class::OUTSIDE;
        }
    })
    .cells;
    let tubes: Vec<_> = [15u16, 16, 17]
        .into_iter()
        .enumerate()
        .map(|(i, x)| {
            let cell = &mut cells[32 * usize::from(width) + usize::from(x)];
            cell.tube_index = Some(TubeId(i as u16));
            cell.yr_cell_land_type = LandType::Tunnel.as_index();
            TubeFact::explicit((x, 32), (x + 32, 32), 0, vec![2; 32])
        })
        .collect();
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(width, height, cells, tubes);
    let bridges =
        BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 100, (32, 32));
    let records = bridges.endpoint_records();
    assert!(
        records
            .iter()
            .any(|r| r.endpoint_a == (16, 32) && r.endpoint_b == (48, 32))
    );
    let path = PathGrid::from_resolved_terrain(&terrain);
    let base = build_base_zone_topology(
        &path,
        &terrain,
        records,
        width,
        height,
        bridges.native_zone_source_size(),
    );
    let precheck = |hierarchy: &ZoneHierarchy| {
        let graph = hierarchy.level(0).unwrap();
        zone_precheck_flat(
            hierarchy,
            graph.zone_at(16, 32),
            graph.zone_at(48, 32),
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap()
    };
    let mut hierarchy = build_zone_hierarchy(&base, Some(&terrain), &[], width, height);
    assert!(
        matches!(precheck(&hierarchy), ZonePrecheckOutcome::Failed),
        "the class barrier separates the hierarchy without Tube pairs"
    );
    let full = build_zone_hierarchy(&base, Some(&terrain), records, width, height);
    assert!(
        matches!(precheck(&full), ZonePrecheckOutcome::Passed(_)),
        "generated records connect the live three-level precheck"
    );
    let zone_grid = super::super::zone_map::ZoneGrid::build_with_native_bridge_geometry(
        &path,
        &terrain,
        records,
        width,
        height,
        bridges.native_zone_source_size(),
    );
    let counts = super::super::BlockerNeighborCounts::new(width, height);
    let route = super::super::zone_search::find_path_zoned_marker(
        &path,
        (16, 32),
        (48, 32),
        None,
        None,
        Some(&zone_grid),
        MovementZone::Normal,
        Some(MovementZone::Normal),
        Some(&terrain),
        None,
        None,
        Some(&counts),
        0,
        false,
        false,
        true,
        None,
    )
    .expect("generated Tube graph reaches live flat route selection");
    assert!(
        route
            .windows(2)
            .any(|pair| pair[0].0.abs_diff(pair[1].0) > 1),
        "the class barrier is crossed by the Tube path transition"
    );
    for level in 0..3 {
        let graph = full.level(level).unwrap();
        for (a, b) in [
            ((16, 32), (48, 32)),
            ((17, 32), (49, 32)),
            ((15, 32), (47, 32)),
        ] {
            assert_zero_edge(graph, a, b);
        }
    }
    assert_eq!(
        incremental_rebuild_zone_hierarchy_around_cell(
            &mut hierarchy,
            &base,
            &terrain,
            records,
            (16, 32),
            width,
            height
        ),
        LocalHierarchyPatchResult::Patched
    );
    assert!(
        matches!(precheck(&hierarchy), ZonePrecheckOutcome::Passed(_)),
        "local repair publishes the Tube edges to the same precheck"
    );
}

#[test]
fn tube_hierarchy_native_read_tubes_tail_publishes_retained_dummy_index() {
    use crate::map::tube_facts::TubeFact;
    use crate::map::tubes::{ConstructedMapTube, NativeMapTubeReceipt, TubeNativeInit};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tube_hierarchy.json",
    ))
    .unwrap();
    let writes = corpus["dummy_writes"].as_array().unwrap();
    for count in 1..=writes.len() {
        let mut terrain = redirect_terrain(2, 1, None, None, |_| {});
        terrain.test_set_native_allocated_cells(&[(1, 0)]);
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(1234, -2345);
        terrain
            .bind_native_map_tubes(NativeMapTubeReceipt {
                entries: writes[..count]
                    .iter()
                    .map(|row| ConstructedMapTube {
                        fact: TubeFact::explicit(
                            (
                                row["coord"][0].as_i64().unwrap() as u16,
                                row["coord"][1].as_i64().unwrap() as u16,
                            ),
                            (1, 0),
                            0,
                            vec![],
                        ),
                        native_init: TubeNativeInit {
                            source_entry_ordinal: row["ordinal"].as_u64().unwrap() as usize,
                            native_unique_id: 123,
                        },
                    })
                    .collect(),
            })
            .unwrap();
        let expected = &writes[count - 1];
        assert_eq!(
            dummy.raw_tube_index(),
            expected["dummy_index"].as_i64().unwrap() as i16
        );
        assert_eq!(
            dummy.snapshot().coord,
            (
                expected["dummy_coord"][0].as_i64().unwrap() as i32,
                expected["dummy_coord"][1].as_i64().unwrap() as i32
            )
        );
        assert_eq!(
            terrain.raw_tube_index_at_native_coord((1, 0)),
            expected["real_index"].as_i64().unwrap() as i16
        );
    }
}

#[test]
fn tube_hierarchy_constant_walk_endpoints_match_original_executable() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tube_hierarchy.json",
    ))
    .unwrap();
    let terrain = redirect_terrain(1, 1, None, None, |_| {});
    let cases = corpus["constant_walks"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let coord =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let token = case["token"].as_i64().unwrap() as i32;
        assert_eq!(
            hierarchy_walk_tube_path(&terrain, coord(&case["start"]), &[token]),
            Ok(coord(&case["end"])),
            "raw token {token}"
        );
    }
}
