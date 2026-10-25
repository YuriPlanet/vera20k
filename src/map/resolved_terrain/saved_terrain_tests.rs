// Included in resolved_terrain::tests to share the existing TMP/asset fixtures.

#[test]
fn saved_terrain_roundtrip_retains_materialized_values_without_constructor_effects() {
    let mut grid = test_grid(2, 2, test_clear_cell);
    grid.native_allocated = Some(vec![true, false, true, true]);
    grid.radar_color_valid = vec![true, false, true, false];
    grid.cells[0].radar_left = [0; 3];
    grid.cells[0].radar_right = [0; 3];
    grid.cells[1].radar_left = [123, 45, 67];
    grid.cells[2].source_tile_index = 1234;
    grid.cells[2].final_tile_index = 0xffff;
    grid.cells[2].variant = 7;
    grid.cells[2].terrain_object_occupation = Some(7);
    grid.cells[2].bridge_facts.raw_flags = 0x1_2580;
    grid.cells[2].tube_index = Some(TubeId(0));
    grid.native_tube_indices = [-32768, 234, 0, -1]
        .map(NativeTubeCellIndex::from_raw)
        .to_vec();
    grid.tube_facts = vec![TubeFact::explicit((0xffff, 3), (7, 4), -19, vec![3, -1, 7])];
    grid.tube_native_ids = vec![Some(42_013)];
    grid.tile_animations = vec![TerrainTileAnimation {
        rx: 1,
        ry: 1,
        anim_name: "WA01X".into(),
        world_x: 357,
        world_y: -461,
        world_z: 512,
        z_adjust: -7,
    }];
    grid.cell_mut(1, 1).unwrap().level = 7;
    let saved = grid.capture_saved_template().unwrap();
    let value = serde_json::to_value(&saved).unwrap();
    let encoded = save_codec::serialize(&saved).unwrap();
    let decoded: SavedTerrainTemplate = save_codec::deserialize(&encoded).unwrap();
    let dummy = SharedCellDummy::fresh();
    let restored = decoded.restore_values(dummy.clone()).unwrap();
    assert_eq!(restored.cells(), grid.cells());
    assert_eq!(restored.native_allocated, grid.native_allocated);
    assert_eq!(restored.radar_color_valid, grid.radar_color_valid);
    assert_eq!(restored.native_tube_indices, grid.native_tube_indices);
    assert_eq!(restored.tube_native_ids, grid.tube_native_ids);
    assert_eq!(restored.tile_animations(), grid.tile_animations());
    assert!(restored.shared_cell_dummy().same_identity(&dummy));
    assert!(
        !restored
            .shared_cell_dummy()
            .same_identity(&grid.shared_cell_dummy())
    );
    assert_eq!(restored.mutation_epoch(), 0);
    assert!(restored.bridge_recalc_catalog.is_none());
    assert_eq!(
        serde_json::to_value(restored.capture_saved_template().unwrap()).unwrap(),
        value
    );
}

#[test]
fn saved_terrain_rejects_malformed_planes_coordinates_and_references() {
    let good =
        serde_json::to_value(test_flat_ground_grid(2).capture_saved_template().unwrap()).unwrap();
    let mut corruptions = Vec::new();
    for field in ["cells", "radar_color_valid", "native_tube_indices"] {
        let mut bad = good.clone();
        bad[field].as_array_mut().unwrap().pop();
        corruptions.push(bad);
    }
    let mut bad = good.clone();
    bad["native_allocated"] = serde_json::json!([true]);
    corruptions.push(bad);
    let mut bad = good.clone();
    bad["cells"][0]["rx"] = serde_json::json!(1);
    corruptions.push(bad);
    let mut bad = good.clone();
    bad["cells"][0]["tube_index"] = serde_json::json!(0);
    corruptions.push(bad);
    let mut bad = good.clone();
    bad["tube_native_ids"] = serde_json::json!([42]);
    corruptions.push(bad);
    let mut bad = good.clone();
    bad["width"] = serde_json::json!(513);
    corruptions.push(bad);
    let mut bad = good;
    bad["tile_animations"] = serde_json::json!([{
        "rx": 8, "ry": 0, "anim_name": "WA01X", "world_x": 0,
        "world_y": 0, "world_z": 0, "z_adjust": 0,
    }]);
    corruptions.push(bad);
    for bad in corruptions {
        let saved: SavedTerrainTemplate = serde_json::from_value(bad).unwrap();
        assert!(saved.validate().is_err());
        assert!(saved.restore_values(SharedCellDummy::fresh()).is_err());
    }
}

#[test]
fn saved_terrain_load_selector_matches_executed_native_cells_and_main_continuation() {
    use crate::map::tile_variant_selector::TileVariantSelectorCache;
    let corpus = bridge_catalog_corpus();
    for row in corpus["load_selector_cases"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let files = input["files"].as_u64().unwrap() as usize;
        let mut bytes = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
        if input["damaged_header"].as_bool().unwrap() {
            bytes[20 + 36..20 + 40].copy_from_slice(&4u32.to_le_bytes());
        }
        let names: Vec<_> = (0..files)
            .map(|i| {
                if i == 0 {
                    "tile01.tem".to_string()
                } else {
                    format!("tile01{}.tem", char::from(b'a' + i as u8 - 1))
                }
            })
            .collect();
        let sources: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), bytes.as_slice()))
            .collect();
        let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&sources);
        let mut theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=tile\nSetName=Clear\n",
        );
        theater.rmg_tiles.clear_tile = Some(0);
        crate::map::theater::resolve_contiguous_variant_chains_for_test(
            &mut theater.lookup,
            &assets,
        );
        assert_eq!(theater.lookup.total_file_count(0) as usize, files, "{name}");
        let rules = TerrainRules::from_ini(&IniFile::from_str("[Clear]\nWheel=100%\n"));
        let mut grid = test_flat_ground_grid(3);
        grid.native_allocated = Some(vec![false; 9]);
        for coord in row["visited_cells"].as_array().unwrap() {
            let (x, y) = (
                coord[0].as_u64().unwrap() as usize,
                coord[1].as_u64().unwrap() as usize,
            );
            let index = y * 3 + x;
            grid.native_allocated.as_mut().unwrap()[index] = true;
            grid.cells[index].final_tile_index = if input["sentinel"].as_bool().unwrap() {
                if name == "cold_clear_sentinel_count1_initializes" {
                    // Generated maps retain NO_TILE as -1. It represents the
                    // same native FFFF sentinel this executed case supplies;
                    // the other sentinel controls retain the positive form.
                    theater::NO_TILE
                } else {
                    0xffff
                }
            } else {
                0
            };
            grid.cells[index].final_sub_tile = 0;
            grid.cells[index].bridge_facts.raw_flags = input["cell_flags"].as_u64().unwrap() as u32;
        }
        let before_cells = grid.cells.clone();
        let mut cache = if input["warm"].as_bool().unwrap() {
            let table: [u8; 64] = row["table_before"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            TileVariantSelectorCache::initialized_for_test(table)
        } else {
            TileVariantSelectorCache::default()
        };
        let mut main = SimRng::new(input["seed"].as_u64().unwrap());
        for _ in 0..input["advance"].as_u64().unwrap() {
            main.next_u32();
        }
        assert_eq!(
            main.native_state_hex(),
            row["before"]["main"].as_str().unwrap(),
            "{name} before"
        );
        let catalog = BridgeRecalcCatalog::for_tiles(&theater, &assets, &rules, false, 0, [0]);
        let mut draws = Vec::new();
        {
            let mut draw = || {
                let value = main.next_u32();
                draws.push(value);
                value
            };
            let mut selector = cache.begin_load(&mut draw);
            let actual: Vec<_> = grid
                .native_cell_iterator(2)
                .map(|cell| {
                    catalog
                        .select_loaded_variant(cell, 0, &mut selector)
                        .unwrap()
                })
                .collect();
            let expected: Vec<_> = row["selected_files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(actual, expected, "{name} native visit/selection");
            assert_eq!(
                selector.raw_draw_count() as u64,
                row["raw_draw_count"].as_u64().unwrap(),
                "{name}"
            );
            if row["flag_after"].as_u64().unwrap() != 0 {
                let table: Vec<_> = selector
                    .initialized_table()
                    .unwrap()
                    .into_iter()
                    .map(u64::from)
                    .collect();
                assert_eq!(serde_json::json!(table), row["table_after"], "{name} table");
            } else {
                assert!(selector.initialized_table().is_none(), "{name}");
            }
        }
        assert_eq!(
            serde_json::json!(draws),
            row["raw_draws"],
            "{name} raw draws"
        );
        assert_eq!(
            main.native_state_hex(),
            row["after"]["main"].as_str().unwrap(),
            "{name} continuation"
        );
        assert_eq!(
            grid.cells, before_cells,
            "{name} scan leaves cells unchanged"
        );
    }
}

#[test]
fn saved_terrain_cold_load_then_runtime_selection_shares_process_authority() {
    use crate::map::tile_variant_selector::TileVariantSelectorCache;
    let bytes = gsi_04_02_last_tiles_tmp_bytes(0, [0; 3], [0; 3]);
    let names: Vec<_> = std::iter::once("tile01.tem".to_string())
        .chain(std::iter::once("tile02.tem".to_string()))
        .chain((b'a'..=b'g').map(|suffix| format!("tile02{}.tem", char::from(suffix))))
        .collect();
    let sources: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), bytes.as_slice()))
        .collect();
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&sources);
    let mut theater =
        synthetic_theater_from_ini(b"[TileSet0000]\nTilesInSet=2\nFileName=tile\nSetName=Clear\n");
    theater.bridge_set = Some(0);
    theater.rmg_tiles.clear_tile = Some(0);
    crate::map::theater::resolve_contiguous_variant_chains_for_test(&mut theater.lookup, &assets);
    let rules = TerrainRules::from_ini(&IniFile::from_str("[Clear]\nWheel=100%\n"));
    let mut grid = test_flat_ground_grid(3);
    grid.native_allocated = Some(vec![false; 9]);
    grid.native_allocated.as_mut().unwrap()[7] = true;
    grid.cells[7].final_tile_index = 0;
    let snapshot = grid.capture_saved_template().unwrap();
    let mut baseline = snapshot
        .clone()
        .restore_values(SharedCellDummy::fresh())
        .unwrap();
    let mut loaded = snapshot.restore_values(SharedCellDummy::fresh()).unwrap();
    let mut process = TileVariantSelectorCache::default();
    let mut candidate = process.detached();
    let mut main = SimRng::new(0);
    let before = main.native_state_hex();
    {
        let mut draw = || main.next_u32();
        let mut selector = candidate.begin_load(&mut draw);
        loaded
            .finish_loaded_assets(
                &theater,
                &assets,
                &rules,
                &OverlayTypeRegistry::empty(),
                false,
                0,
                2,
                &mut selector,
            )
            .unwrap();
        assert_eq!(selector.raw_draw_count(), 0);
    }
    baseline
        .bind_saved_template_assets(
            &theater,
            &assets,
            &rules,
            &OverlayTypeRegistry::empty(),
            false,
            0,
            candidate.selector(),
        )
        .unwrap();
    assert!(!candidate.is_initialized());
    assert!(!process.is_initialized());
    assert_eq!(main.native_state_hex(), before);
    assert!(
        loaded.radar_color_valid[7],
        "valid black TMP survives rebinding"
    );
    process.adopt_prepared(candidate).unwrap();
    loaded.bind_tile_variant_selector(process.selector());
    baseline.bind_tile_variant_selector(process.selector());
    loaded.cells[7].final_tile_index = 1;
    let mut draws = 0;
    loaded
        .refresh_resident_bridge_presentation(7, &mut || {
            draws += 1;
            main.next_u32()
        })
        .unwrap();
    let corpus = bridge_catalog_corpus();
    let native = corpus["load_selector_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "cold_table_count8_seed0")
        .unwrap();
    assert_eq!(draws, native["raw_draw_count"].as_u64().unwrap());
    assert_eq!(
        loaded.cells[7].variant as u64,
        native["selected"].as_u64().unwrap()
    );
    assert_eq!(
        main.native_state_hex(),
        native["after"]["main"].as_str().unwrap()
    );
    assert!(
        process.is_initialized(),
        "runtime warmed the retained process table"
    );
    baseline.cells[7].final_tile_index = 1;
    baseline
        .refresh_resident_bridge_presentation(7, &mut || panic!("shared table must remain warm"))
        .unwrap();
    assert_eq!(baseline.cells[7].variant, loaded.cells[7].variant);

    // The native scan sees the restored live replacement, not the immutable
    // baseline. A baseline-only scan would incorrectly leave this cache cold.
    let mut restored = grid
        .capture_saved_template()
        .unwrap()
        .restore_values(SharedCellDummy::fresh())
        .unwrap();
    let mut changed = DynamicTerrainCellState::capture(&restored.cells[7]);
    changed.final_tile_index = 1;
    assert!(restored.apply_dynamic_cell_state(1, 2, &changed));
    let gameplay_before = (
        restored.cells[7].land_type,
        restored.cells[7].level,
        restored.cells[7].bridge_facts,
        restored.cells[7].tube_index,
    );
    let mut process = TileVariantSelectorCache::default();
    let mut main = SimRng::new(0);
    {
        let mut draw = || main.next_u32();
        let mut selector = process.begin_load(&mut draw);
        restored
            .finish_loaded_assets(
                &theater,
                &assets,
                &rules,
                &OverlayTypeRegistry::empty(),
                false,
                0,
                2,
                &mut selector,
            )
            .unwrap();
        assert_eq!(
            selector.raw_draw_count() as u64,
            native["raw_draw_count"].as_u64().unwrap()
        );
    }
    assert_eq!(
        restored.cells[7].variant as u64,
        native["selected"].as_u64().unwrap()
    );
    assert_eq!(
        main.native_state_hex(),
        native["after"]["main"].as_str().unwrap()
    );
    assert_eq!(
        (
            restored.cells[7].land_type,
            restored.cells[7].level,
            restored.cells[7].bridge_facts,
            restored.cells[7].tube_index
        ),
        gameplay_before
    );
}
