// Original-instruction comparisons for the production bridge-record factory.
// Inputs are sparse native cell-table fixtures, not a recreated Rust reference.

#[test]
fn native_bridge_records_match_original_executable() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_records.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 83);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let size = (
            case["size"][0].as_i64().unwrap() as i32,
            case["size"][1].as_i64().unwrap() as i32,
        );
        let int = |row: &serde_json::Value, i: usize| row[i].as_i64().unwrap() as i32;
        let base = make_bridge_terrain().cell(4, 0).unwrap().clone();
        let mut cells = Vec::new();
        let mut allocated = Vec::new();
        for y in 0..32u16 {
            for x in 0..32u16 {
                let mut cell = base.clone();
                cell.rx = x;
                cell.ry = y;
                let hole = case["holes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| int(p, 0) == i32::from(x) && int(p, 1) == i32::from(y));
                if crate::map::authored_overlay::NativeOverlayMapShape::new(size.0, size.1)
                    .admits(x as i16, y as i16)
                    && !hole
                {
                    allocated.push((x, y));
                }
                if let Some(row) = case["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| int(r, 0) == i32::from(x) && int(r, 1) == i32::from(y))
                {
                    cell.final_tile_index = int(row, 2);
                    cell.final_sub_tile = int(row, 3) as u8;
                    cell.bridge_facts.raw_flags = int(row, 4) as u32;
                    cell.yr_cell_land_type = int(row, 5) as u8;
                    let ordinal = int(row, 6);
                    cell.tube_index = (ordinal >= 0).then_some(TubeId(ordinal as u16));
                }
                cells.push(cell);
            }
        }
        let tubes = case["tubes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                TubeFact::explicit(
                    (int(r, 0) as u16, int(r, 1) as u16),
                    (int(r, 2) as u16, int(r, 3) as u16),
                    2,
                    vec![2; int(r, 4) as usize],
                )
            })
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells_with_tubes(32, 32, cells, tubes);
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        terrain.stamp_dummy_cell_requested_coord(1234, -2345);
        let visited: Vec<_> = terrain
            .native_cell_iterator(size.0)
            .map(|c| (c.rx as i16, c.ry as i16))
            .collect();
        let native_visited: Vec<_> = case["visited"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (int(r, 0) as i16, int(r, 1) as i16))
            .collect();
        assert_eq!(
            visited, native_visited,
            "{name}: iterator/first-null boundary"
        );
        let mut state =
            BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 300, size);
        let actual: Vec<_> = state
            .endpoint_records()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "a": [r.endpoint_a.0 as i16, r.endpoint_a.1 as i16],
                    "b": [r.endpoint_b.0 as i16, r.endpoint_b.1 as i16],
                    "active": r.active, "kind": if r.is_high() { 0 } else { 1 },
                })
            })
            .collect();
        assert_eq!(
            serde_json::json!(actual),
            case["records"],
            "{name}: records"
        );
        let dummy = terrain.shared_cell_dummy().snapshot();
        assert_eq!(
            dummy.coord,
            (int(&case["dummy_coord"], 0), int(&case["dummy_coord"], 1)),
            "{name}: retained dummy stamp"
        );
        let tubes: Vec<_> = state
            .endpoint_records()
            .iter()
            .filter(|r| !r.is_high())
            .map(|r| r.endpoint_a)
            .collect();
        for a in tubes {
            state.invalidate_bridge_zones(&terrain, (a.0 as i16, a.1 as i16));
        }
        assert!(
            state
                .endpoint_records()
                .iter()
                .filter(|r| !r.is_high())
                .all(|r| r.active),
            "{name}: 56DA10 never matches a tube record"
        );
    }
}
