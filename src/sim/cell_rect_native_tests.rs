// Native executable comparison of the production lookup/playfield owners.
// Entries/fixture limits: tools/spatial_oracle/map_queries.py and
// docs/research/PHASE3_MAP_SPATIAL_NATIVE_COMPARISON_20260910.md.
// Included within cell_rect::tests to share its plain terrain fixture.

fn spatial_native_vectors() -> serde_json::Value {
    serde_json::from_str(include_str!("../../tools/spatial_oracle/map_queries.json")).unwrap()
}

#[test]
fn retained_playfield_query_does_not_relookup_a_dummy_alias() {
    use crate::map::cell_index::NativeCellIdentity;
    let terrain = flat_terrain(33, 33);
    let real = terrain.native_cell_identity((11, 10));
    terrain.stamp_dummy_cell_requested_coord(11, 10);
    terrain.test_set_dummy_cell_level_slope(4, 0);
    let before = terrain.shared_cell_dummy().snapshot();
    let bounds = Some(PlayfieldBounds {
        base: 16,
        off_fc: 0,
        off_100: 2,
        off_104: 16,
        off_108: 16,
    });
    // unit_entry_boundary's original578540 calls admit level0 and refuse4
    // here. Retaining the dummy must read its own level, even though a fresh
    // coordinate lookup would select the allocated real Cell instead.
    assert!(retained_cell_is_in_playfield(real, bounds, &terrain));
    assert!(!retained_cell_is_in_playfield(
        NativeCellIdentity::Dummy,
        bounds,
        &terrain
    ));
    assert_eq!(terrain.shared_cell_dummy().snapshot(), before);
}

fn native_pair(value: &serde_json::Value) -> (i32, i32) {
    (
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
    )
}

fn native_fixture_terrain(vectors: &serde_json::Value) -> ResolvedTerrainGrid {
    let mut terrain = flat_terrain(512, 82);
    let allocated = vectors["allocated"].as_array().unwrap();
    assert_eq!(allocated.len(), 7);
    let coords: Vec<_> = allocated
        .iter()
        .map(|row| {
            let (x, y) = native_pair(row);
            let cell = terrain.cell_mut(x as u16, y as u16).unwrap();
            cell.level = row[2].as_i64().unwrap() as u8;
            cell.slope_type = row[3].as_u64().unwrap() as u8;
            (x as u16, y as u16)
        })
        .collect();
    terrain.test_set_native_allocated_cells(&coords);
    terrain
}

#[test]
fn map_lookup_matches_native_executable_pointer_and_dummy_effects() {
    let vectors = spatial_native_vectors();
    let terrain = native_fixture_terrain(&vectors);
    let seed = native_pair(&vectors["dummy_seed"]);
    let rows = vectors["lookups"].as_array().unwrap();
    assert_eq!(rows.len(), 114);
    let mut packed_count = 0;
    let mut real_count = 0;
    for row in rows {
        terrain.stamp_dummy_cell_requested_coord(seed.0, seed.1);
        terrain.test_set_dummy_cell_level_slope(-7, 1);
        let (x, y) = native_pair(&row["xy"]);
        let cell = match row["kind"].as_str().unwrap() {
            "packed_lookup" => {
                packed_count += 1;
                get_cellclass_fallback(Some(&terrain), x, y)
            }
            "world_lookup" => get_cellclass_fallback_leptons(Some(&terrain), x, y),
            unexpected => panic!("unexpected native query {unexpected}"),
        };
        let real = match cell {
            CellRef::Real(cell) => {
                real_count += 1;
                Some((i32::from(cell.rx), i32::from(cell.ry)))
            }
            CellRef::Dummy { cell } => {
                assert!(cell.same_identity(&terrain.shared_cell_dummy()));
                None
            }
        };
        let expected = (!row["real"].is_null()).then(|| native_pair(&row["real"]));
        assert_eq!(real, expected, "native pointer selection: {row}");
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            native_pair(&row["dummy_coord"]),
            "native miss-only stamp: {row}"
        );
        assert_eq!(terrain.dummy_cell_level_slope(), (-7, 1));
    }
    assert_eq!(packed_count, 16);
    assert!(
        real_count > 10,
        "corpus must exercise real and dummy branches"
    );
}

#[test]
fn map_playfield_matches_native_executable_modes_heights_and_stamps() {
    let vectors = spatial_native_vectors();
    let terrain = native_fixture_terrain(&vectors);
    let seed = native_pair(&vectors["dummy_seed"]);
    let rows = vectors["predicates"].as_array().unwrap();
    assert_eq!(rows.len(), 2010);
    let mut inside_count = 0;
    let mut geometry_count = 0;
    let mut wrapper_count = 0;
    for row in rows {
        let fields = row["bounds"].as_array().unwrap();
        let field = |index: usize| fields[index].as_i64().unwrap() as i32;
        let bounds = PlayfieldBounds::from_normalized_local_size(
            field(0),
            field(1),
            field(2),
            field(3),
            field(4),
        );
        let dummy = native_pair(&row["dummy"]);
        terrain.stamp_dummy_cell_requested_coord(seed.0, seed.1);
        terrain.test_set_dummy_cell_level_slope(dummy.0 as i8, dummy.1 as u8);
        let xy = native_pair(&row["xy"]);
        let inside = match row["kind"].as_str().unwrap() {
            "world_playfield" => {
                wrapper_count += 1;
                cell_is_in_playfield_leptons((xy.0, xy.1, -559038737), Some(bounds), Some(&terrain))
            }
            "playfield" if row["mode"].as_u64().unwrap() as u8 == 0 => {
                geometry_count += 1;
                cell_is_in_playfield_geometry_only(xy, bounds)
            }
            "playfield" => cell_is_in_playfield_height_aware(xy, Some(bounds), Some(&terrain)),
            unexpected => panic!("unexpected native query {unexpected}"),
        };
        inside_count += usize::from(inside);
        assert_eq!(
            inside,
            row["inside"].as_bool().unwrap(),
            "native verdict: {row}"
        );
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            native_pair(&row["dummy_coord"]),
            "native mode/lookup side effects: {row}"
        );
        assert_eq!(
            terrain.dummy_cell_level_slope(),
            (dummy.0 as i8, dummy.1 as u8)
        );
    }
    assert_eq!(wrapper_count, 92);
    assert!(geometry_count > 100);
    assert!(inside_count > 100 && inside_count < rows.len());
}

#[test]
fn map_localsize_normalization_matches_native_executed_prefix() {
    let vectors = spatial_native_vectors();
    let rows = vectors["normalizations"].as_array().unwrap();
    assert_eq!(rows.len(), 15);
    for row in rows {
        let local = std::array::from_fn(|i| row["local"][i].as_i64().unwrap() as i32);
        let actual = PlayfieldBounds::from_map_header(&map_header_with_rects(
            native_pair(&row["size"]),
            local,
        ));
        let expected: [i32; 4] =
            std::array::from_fn(|i| row["normalized"][i].as_i64().unwrap() as i32);
        assert_eq!(
            [
                actual.off_fc,
                actual.off_100,
                actual.off_104,
                actual.off_108
            ],
            expected,
            "native 567230 through 5672D3, before redraw/Techno effects: {row}"
        );
        // Physical signed INI input through the parser and initial sim seam.
        let size = native_pair(&row["size"]);
        let text = format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,{},{}\nLocalSize={},{},{},{}\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n",
            size.0, size.1, local[0], local[1], local[2], local[3],
        );
        let map = crate::map::map_file::MapFile::from_bytes(text.as_bytes()).unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        sim.install_playfield_from_map_header(&map.header);
        assert_eq!(
            sim.playfield_bounds,
            Some(actual),
            "physical map input: {row}"
        );
    }
}

#[test]
fn map_retained_dummy_alias_matches_native_query_sequence() {
    let vectors = spatial_native_vectors();
    let terrain = native_fixture_terrain(&vectors);
    let seed = native_pair(&vectors["dummy_seed"]);
    terrain.stamp_dummy_cell_requested_coord(seed.0, seed.1);
    terrain.test_set_dummy_cell_level_slope(-7, 1);
    let rows = vectors["retained_sequence"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let mut retained = None;
    for row in rows {
        let (x, y) = native_pair(&row["xy"]);
        let selected = get_cellclass_fallback(Some(&terrain), x, y);
        if retained.is_none() {
            retained = Some(selected.clone());
        }
        let old = retained.as_ref().unwrap();
        assert_eq!(old == &selected, row["returns_retained"].as_bool().unwrap());
        let snapshot = old.dummy_snapshot().unwrap();
        assert_eq!(snapshot.coord, native_pair(&row["retained_coord"]));
        let height = native_pair(&row["retained_height"]);
        assert_eq!(
            (snapshot.level as u8, snapshot.slope_type),
            (height.0 as u8, height.1 as u8)
        );
    }
}
