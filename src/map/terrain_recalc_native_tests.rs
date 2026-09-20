// Included inside resolved_terrain::tests to share its real TMP/loader fixture.
// Native outputs: tools/spatial_oracle/terrain_recalc, original47D2B0 and callees.

/// Real resident TMP input matching the original bridge-constructor fixture.
/// Kept with the loader fixture so sim tests don't duplicate asset construction.
pub(crate) fn bridge_constructor_terrain() -> ResolvedTerrainGrid {
    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=source\nSetName=Plain\n",
    );
    let tmp = gsi_04_02_last_tiles_tmp_bytes(11, [0; 3], [0; 3]);
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[("source01.tem", &tmp)]);
    let rules = TerrainRules::from_ini(&IniFile::from_str(
        "[Clear]\nWheel=100%\n[Road]\nWheel=100%\n",
    ));
    let cells = (0..33)
        .flat_map(|y| {
            (0..33).map(move |x| {
                let mut cell = make_test_cell(x, y);
                cell.final_tile_index = 0;
                cell.final_sub_tile = 0;
                cell.level = 6;
                cell
            })
        })
        .collect();
    let mut grid = ResolvedTerrainGrid::from_cells(33, 33, cells);
    grid.native_allocated = Some(
        (0..33)
            .flat_map(|y| (0..33).map(move |x| (12..=20).contains(&x) && (12..=20).contains(&y)))
            .collect(),
    );
    grid.bridge_recalc_catalog = Some(Arc::new(BridgeRecalcCatalog::for_tiles(
        &theater,
        &assets,
        &rules,
        false,
        0,
        [0],
    )));
    grid
}

/// Real resident inputs for bridge_hierarchy's original586990/Recalc corpus.
/// Tile2 is invalid against the two-entry registry; tile0 has the supplied
/// slope used by the second-pass admission discriminator. No Recalc substitute.
pub(crate) fn install_bridge_batch_test_catalog(grid: &mut ResolvedTerrainGrid, slope: bool) {
    install_repair_test_catalog(grid, slope, true);
}

pub(crate) fn install_ordinary_repair_test_catalog(grid: &mut ResolvedTerrainGrid) {
    install_repair_test_catalog(grid, false, false);
}

fn install_repair_test_catalog(grid: &mut ResolvedTerrainGrid, slope: bool, wheel_only: bool) {
    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=2\nFileName=source\nSetName=Plain\n",
    );
    let mut first = gsi_04_02_last_tiles_tmp_bytes(11, [0; 3], [0; 3]);
    first[62] = u8::from(slope);
    let second = gsi_04_02_last_tiles_tmp_bytes(11, [0; 3], [0; 3]);
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        ("source01.tem", &first),
        ("source02.tem", &second),
    ]);
    let rules_text: String = crate::rules::terrain_rules::LandType::ALL
        .iter()
        .take(9)
        .map(|land| format!("[{}]\n{}", land.section_name(),if wheel_only {"Wheel=100%\n"} else {"Foot=100%\nTrack=100%\nWheel=100%\nFloat=100%\nHover=100%\nAmphibious=100%\nFloatBeach=100%\nBuildable=yes\n"}))
        .collect();
    let rules = TerrainRules::from_ini(&IniFile::from_str(&rules_text));
    grid.bridge_recalc_catalog = Some(Arc::new(
        BridgeRecalcCatalog::for_tiles(&theater, &assets, &rules, false, 0, [0, 1])
            // Live publication also refreshes presentation for the invalid
            // tile's clear fallback. Its single file always selects variant0;
            // supply the initialized process table required by that path.
            .with_fixture_files(0, &assets, &["source01.tem"], [0; 64]),
    ));
}

#[test]
fn recalc_pristine_metadata_and_level_override_match_original_instructions() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/terrain_recalc.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let flag = |key: &str| case[key].as_bool().unwrap();
        let number = |key: &str| case[key].as_i64().unwrap();
        let mut theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=other\nSetName=Other\n\
              Morphable=no\nAllowTiberium=no\n\
              [TileSet0001]\nTilesInSet=1\nFileName=source\nSetName=Source\n\
              Morphable=yes\nAllowTiberium=yes\n",
        );
        let mut source = gsi_04_02_last_tiles_tmp_bytes(11, [0; 3], [0; 3]);
        let mut other = source.clone();
        if flag("lat") {
            other[61] = 15;
            other[62] = 4;
            other[12..16].copy_from_slice(&34u32.to_le_bytes());
            other.resize(72 + 8 * 34 / 2, 1);
            theater.rmg_tiles.ramp_base = Some(1);
        }
        if flag("sparse") {
            source[16..20].copy_from_slice(&0u32.to_le_bytes());
            other = source.clone();
        }
        let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
            ("source01.tem", &source),
            ("other01.tem", &other),
        ]);
        let ini = IniFile::from_str(
            "[Clear]\nWheel=100%\n[Road]\nWheel=100%\n[Rock]\nWheel=0%\n\
             [Tiberium]\nWheel=0%\n\
             [OverlayTypes]\n0=EARLY\n1=ROAD\n2=RESOURCE\n\
             [EARLY]\nLand=Road\nNoUseTileLandType=yes\n\
             [ROAD]\nLand=Road\nNoUseTileLandType=no\n\
             [RESOURCE]\nLand=Tiberium\nTiberium=yes\nNoUseTileLandType=no\n",
        );
        let rules = TerrainRules::from_ini(&ini);
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut map = make_map(Vec::new(), Vec::new(), Vec::new());
        map.header.width = 16;
        map.header.height = 16;
        map.header.local_left = 0;
        map.header.local_top = 0;
        map.header.local_width = 16;
        map.header.local_height = 16;
        let cells = (0..33)
            .flat_map(|y| (0..33).map(move |x| make_test_cell(x, y)))
            .collect();
        let mut grid = ResolvedTerrainGrid::from_cells(33, 33, cells);
        let index = 16 * 33 + 16;
        grid.cells[index].final_tile_index = if flag("invalid") {
            2
        } else {
            i32::from(flag("lat"))
        };
        grid.cells[index].final_sub_tile = number("input_subtile") as u8;
        grid.cells[index].level = 4;
        grid.cells[index].height_in_pixels = 88;
        let mut resident = grid.clone();
        resident.bridge_recalc_catalog = Some(Arc::new(BridgeRecalcCatalog::for_tiles(
            &theater,
            &assets,
            &rules,
            flag("lat"),
            0,
            [0, 1],
        )));
        let mut state = LoadCellRecalcState::for_authored_load(
            &map,
            &theater,
            &assets,
            &rules,
            &registry,
            flag("lat"),
            0,
            grid.cells.len(),
            crate::util::pixel_conversion::PixelConversionBounds::default(),
        );
        let overlay = if flag("early") {
            FinalizedOverlayCell::from_parts(0, 0)
        } else {
            match case["overlay_land"].as_i64() {
                Some(1) => FinalizedOverlayCell::from_parts(1, 0),
                Some(5) => FinalizedOverlayCell::from_parts(2, 0),
                None => FinalizedOverlayCell::default(),
                other => panic!("unexpected fixture overlay land {other:?}"),
            }
        };
        grid.recalc_cell_attributes(
            &mut state,
            index,
            overlay,
            number("level_override") as i32,
            &mut LoadRecalcTestEffects::default(),
        )
        .unwrap();
        drop(state);
        drop(assets);
        drop(theater);
        resident
            .recalc_resident_bridge_cell(
                index,
                overlay,
                number("level_override") as i32,
                &registry,
                Some(PlayfieldBounds::from_map_header(&map.header)),
            )
            .unwrap();
        // Both adapters execute the same native-corresponding core. The
        // resident one has no TheaterData/AssetManager borrow or loader latch.
        assert_eq!(
            DynamicTerrainCellState::capture(&resident.cells[index]),
            DynamicTerrainCellState::capture(&grid.cells[index]),
            "{name}: resident and authored scalar projections"
        );
        let cell = &grid.cells[index];
        if flag("lat") {
            assert!(!cell.accepts_smudge, "{name}: final-tile Morphable query");
            assert!(
                !cell.allows_tiberium,
                "{name}: final-tile AllowTiberium query"
            );
        }
        if flag("invalid") || flag("sparse") {
            assert!(!cell.accepts_smudge, "{name}: tile0 Morphable fallback");
            assert!(cell.allows_tiberium, "{name}: invalid final-tile gate");
        }
        for (field, actual) in [
            ("tile", i64::from(cell.final_tile_index)),
            ("subtile", i64::from(cell.final_sub_tile)),
            ("level", i64::from(cell.level)),
            ("slope", i64::from(cell.slope_type)),
            ("height", i64::from(cell.height_in_pixels)),
            ("land", i64::from(cell.yr_cell_land_type)),
            ("zone", i64::from(cell.zone_type)),
        ] {
            assert_eq!(actual, number(field), "{name}: {field}");
        }
        if name == "valid_preserves_level" {
            let changed_bounds = PlayfieldBounds::from_normalized_local_size(16, 2, 20, 12, 1);
            resident
                .recalc_resident_bridge_cell(index, overlay, -1, &registry, Some(changed_bounds))
                .unwrap();
            assert!(
                resident.cells[index].outside_playfield,
                "runtime must use changed bounds"
            );
            assert_eq!(resident.cells[index].zone_type, zone_class::OUTSIDE);
            assert_eq!(resident.cells[index].level, 4);
        }
    }
}

#[test]
fn resident_recalc_distinguishes_uncached_unavailable_and_native_sparse() {
    let theater =
        synthetic_theater_from_ini(b"[TileSet0000]\nTilesInSet=5\nFileName=tile\nSetName=Plain\n");
    let valid = gsi_04_02_last_tiles_tmp_bytes(11, [0; 3], [0; 3]);
    let mut sparse = valid.clone();
    sparse[16..20].copy_from_slice(&0u32.to_le_bytes());
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        ("tile01.tem", &valid),
        ("tile02.tem", &sparse),
        ("tile04.tem", &[0; 4]),
    ]);
    let rules = TerrainRules::from_ini(&IniFile::from_str("[Clear]\nWheel=100%\n"));
    let catalog = Arc::new(BridgeRecalcCatalog::for_tiles(
        &theater,
        &assets,
        &rules,
        false,
        0,
        0..4,
    ));
    assert_eq!(
        catalog.metadata(0, 0).unwrap().unwrap().subtile_entry_valid,
        Some(true)
    );
    for (tile, sub) in [(0, 1), (0, 255), (1, 0)] {
        let metadata = catalog.metadata(tile, sub).unwrap().unwrap();
        assert!(metadata.tmp_file_valid);
        assert_eq!(metadata.subtile_entry_valid, Some(false));
    }
    for tile in [2, 3] {
        assert!(matches!(
            catalog.metadata(tile, 0),
            Err(BridgeRecalcCatalogError::Unavailable { .. })
        ));
    }
    assert!(matches!(
        catalog.metadata(4, 0),
        Err(BridgeRecalcCatalogError::Uncached { tile: 4 })
    ));
    for tile in [-1, 5, 0xFFFF, i32::MAX] {
        assert!(catalog.metadata(tile, 0).unwrap().is_none());
    }
    for tile in [2, 3, 4] {
        assert_resident_source_rejection_preserves_cell(catalog.clone(), tile);
    }
}

fn assert_resident_source_rejection_preserves_cell(catalog: Arc<BridgeRecalcCatalog>, tile: i32) {
    let registry = OverlayTypeRegistry::from_ini(
        &IniFile::from_str(
            "[Road]\nWheel=37%\n[OverlayTypes]\n0=EARLY\n\
             [EARLY]\nLand=Road\nNoUseTileLandType=yes\n",
        ),
        None,
    );
    let mut cell = make_test_cell(0, 0);
    cell.final_tile_index = tile;
    let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);
    grid.bridge_recalc_catalog = Some(catalog);
    let before = DynamicTerrainCellState::capture(&grid.cells[0]);
    assert!(matches!(
        grid.recalc_resident_bridge_cell(
            0,
            FinalizedOverlayCell::from_parts(0, 0),
            2,
            &registry,
            None,
        ),
        Err(LoadCellRecalcError::ResidentInput(_))
    ));
    assert_eq!(
        DynamicTerrainCellState::capture(&grid.cells[0]),
        before,
        "rejected early-overlay source {tile} must leave the live cell unchanged"
    );
}

#[test]
fn resident_recalc_rejects_unadmitted_lifecycle_requirements() {
    for (declaration, raw_land, effect) in [
        ("ShadowCaster=yes\n", 11, "declared shadow policy"),
        ("ShadowTiles=1\n", 11, "declared shadow policy"),
        (
            "[Plain]\nTile01Anim=FIRE\nTile01AttachesTo=0\n",
            11,
            "terrain animation attachment",
        ),
        ("", 5, "Tube land"),
    ] {
        let ini =
            format!("[TileSet0000]\nTilesInSet=1\nFileName=tile\nSetName=Plain\n{declaration}");
        let theater = synthetic_theater_from_ini(ini.as_bytes());
        let tmp = gsi_04_02_last_tiles_tmp_bytes(raw_land, [0; 3], [0; 3]);
        let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[("tile01.tem", &tmp)]);
        let catalog = BridgeRecalcCatalog::for_tiles(
            &theater,
            &assets,
            &TerrainRules::default(),
            false,
            0,
            [0],
        );
        assert!(
            matches!(catalog.metadata(0, 0),
                Err(BridgeRecalcCatalogError::Unsupported { effect: actual, .. }) if actual == effect
            ),
            "{effect}"
        );
        assert_resident_source_rejection_preserves_cell(Arc::new(catalog), 0);
    }
}

#[test]
fn current_tile_permission_queries_match_original_instruction_blocks() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/terrain_tile_permissions.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let tile = case["tile"].as_i64().unwrap() as i32;
        let tile0 = case["tile0_permission"].as_bool().unwrap();
        let yes_no = |value| if value { "yes" } else { "no" };
        let ini = format!(
            "[TileSet0000]\nTilesInSet=1\nFileName=zero\n\
             Morphable={}\nAllowTiberium={}\n\
             [TileSet0001]\nTilesInSet=1\nFileName=one\n\
             Morphable={}\nAllowTiberium={}\n",
            yes_no(tile0),
            yes_no(tile0),
            yes_no(!tile0),
            yes_no(!tile0),
        );
        let theater = synthetic_theater_from_ini(ini.as_bytes());
        assert_eq!(theater.lookup.len(), 2);
        let actual = current_tile_permissions(&theater.lookup, tile);
        assert_eq!(
            actual,
            (
                case["smudge"].as_bool().unwrap(),
                case["tiberium"].as_bool().unwrap()
            ),
            "tile={tile}, tile0_permission={tile0}"
        );
    }
}

fn bridge_catalog_corpus() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/bridge_recalc_presentation.json"
    ))
    .unwrap()
}

fn run_bridge_catalog_native_group(group: &serde_json::Value, files: &[Vec<u8>]) {
    let corpus = bridge_catalog_corpus();
    let table: [u8; 64] = corpus["process_table"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let names: Vec<String> = (0..files.len())
        .map(|index| {
            if index == 0 {
                "tile101.tem".into()
            } else {
                format!("tile101{}.tem", char::from(b'a' + index as u8 - 1))
            }
        })
        .collect();
    let sources: Vec<_> = names
        .iter()
        .zip(files)
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&sources);
    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=101\nFileName=tile\nSetName=Other\n",
    );
    let ini = IniFile::from_str(
        "[Clear]\nWheel=100%\n[Road]\nWheel=100%\n[Water]\nWheel=0%\n[Rock]\nWheel=0%\n[Wall]\nWheel=100%\n[Tiberium]\nWheel=0%\n[Beach]\nWheel=100%\n[Rough]\nWheel=50%\n[Ice]\nWheel=100%\n",
    );
    let rules = TerrainRules::from_ini(&ini);
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let catalog = Arc::new(
        BridgeRecalcCatalog::for_tiles(&theater, &assets, &rules, false, 0, [100])
            .with_fixture_files(
                100,
                &assets,
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
                table,
            ),
    );
    let mut grid = ResolvedTerrainGrid::from_cells(
        33,
        33,
        (0..33)
            .flat_map(|y| (0..33).map(move |x| make_test_cell(x, y)))
            .collect(),
    );
    grid.bridge_recalc_catalog = Some(catalog.clone());
    grid.tile_registry_len = Some(101);
    grid.clear_tile_id = 100;
    grid.native_tmp_draw_heights.insert(
        100,
        PristineTmpHeader {
            subtiles: TmpFile::pristine_header_fields_from_bytes(&files[0]).unwrap(),
            total_file_count: files.len(),
        },
    );
    drop(assets);
    drop(theater);
    let index = grid.index(16, 16).unwrap();
    for case in group["cases"].as_array().unwrap() {
        let x = case["coord"][0].as_i64().unwrap() as i16;
        let y = case["coord"][1].as_i64().unwrap() as i16;
        let sub = case["sub"].as_u64().unwrap() as u8;
        let flags = case["flags"].as_u64().unwrap() as u32;
        let mut source = make_test_cell(x as u16, y as u16);
        source.final_tile_index = case["input_tile"].as_i64().unwrap() as i32;
        source.final_sub_tile = sub;
        source.level = 6;
        source.height_in_pixels = 0;
        source.bridge_facts.raw_flags = flags;
        grid.cells[index] = source;
        if let Some(expected) = case.get("recalc") {
            grid.recalc_resident_bridge_cell(
                index,
                FinalizedOverlayCell::default(),
                -1,
                &registry,
                None,
            )
            .unwrap();
            let cell = &grid.cells[index];
            for (field, actual) in [
                ("tile", i64::from(cell.final_tile_index)),
                ("sub", i64::from(cell.final_sub_tile)),
                ("level", i64::from(cell.level)),
                ("slope", i64::from(cell.slope_type)),
                ("pixel_height", i64::from(cell.height_in_pixels)),
                ("land", i64::from(cell.yr_cell_land_type)),
                ("zone", i64::from(cell.zone_type)),
            ] {
                assert_eq!(
                    actual,
                    expected[field].as_i64().unwrap(),
                    "{} sub{sub} flags{flags:x}: {field}",
                    group["name"]
                );
            }
        }
        let pristine = catalog.metadata(100, sub).unwrap().unwrap();
        assert_eq!(pristine.has_damaged_data, case["gate"].as_bool().unwrap());
        let before = (
            grid.cells[index].yr_cell_land_type,
            grid.cells[index].slope_type,
            grid.cells[index].height_in_pixels,
        );
        grid.refresh_resident_bridge_presentation(index).unwrap();
        assert_eq!(
            before,
            (
                grid.cells[index].yr_cell_land_type,
                grid.cells[index].slope_type,
                grid.cells[index].height_in_pixels
            )
        );
        let selected = grid
            .pavement_draw_variant(16, 16)
            .unwrap_or(grid.cells[index].variant);
        assert_eq!(
            u64::from(selected),
            case["selected"].as_u64().unwrap(),
            "{} sub{sub} at{x},{y} flags{flags:x}",
            group["name"]
        );
        let radar = grid.current_tile_radar_metadata(16, 16);
        match case["radar"]["branch"].as_str().unwrap() {
            "color" => {
                let radar = radar.unwrap();
                assert!(radar.valid);
                let rgb: Vec<_> = radar.left.into_iter().chain(radar.right).collect();
                assert_eq!(
                    serde_json::json!(rgb),
                    case["radar"]["rgb"],
                    "{} sub{sub} flags{flags:x}",
                    group["name"]
                );
            }
            "gray" => assert!(!radar.unwrap().valid),
            "unmapped" => {
                assert!(
                    radar.is_none(),
                    "invalid native RGB pointer must not become pristine/gray"
                );
                assert!(
                    grid.damaged_radar_metadata[index]
                        .as_ref()
                        .unwrap()
                        .is_err()
                );
            }
            other => panic!("unknown native radar branch {other}"),
        }
        let draw_sub = if case["input_tile"] == 0xffff { 0 } else { sub };
        let tactical = catalog.tactical_metadata(100, draw_sub, selected).unwrap();
        // Native-selected RGB bytes identify the selected subrecord. Stored
        // raw X/Y are not decoded canvas-origin offsets; TMP bounds decoding
        // has its own existing geometry comparisons.
        let tactical_rgb: Vec<_> = tactical
            .radar_left
            .into_iter()
            .chain(tactical.radar_right)
            .collect();
        assert_eq!(serde_json::json!(tactical_rgb), case["tactical_rgb"]);
        // Restore onto a different cached entry, forcing residency to recover
        // the independent ordinary/damaged radar owner without loader assets.
        let state = DynamicTerrainCellState::capture(&grid.cells[index]);
        grid.cells[index].final_sub_tile = sub.wrapping_add(1);
        assert!(grid.apply_dynamic_cell_state(16, 16, &state));
        assert_eq!(grid.current_tile_radar_metadata(16, 16), radar);
    }
}

#[test]
fn resident_bridge_recalc_and_presentation_match_original_execution() {
    let corpus = bridge_catalog_corpus();
    for group in corpus["groups"].as_array().unwrap() {
        let Some(files) = group["files_hex"].as_array() else {
            continue;
        };
        let bytes: Vec<Vec<u8>> = files
            .iter()
            .map(|file| {
                file.as_str()
                    .unwrap()
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect()
            })
            .collect();
        run_bridge_catalog_native_group(group, &bytes);
    }
}

#[test]
#[ignore = "requires active retail assets; original NewUrban Wood suffix boundary"]
fn retail_bridge_recalc_and_presentation_match_original_execution() {
    let retail = std::env::var("RA2_DIR").expect("retail directory");
    let mut assets =
        crate::assets::asset_manager::AssetManager::new(std::path::Path::new(&retail)).unwrap();
    let theater = crate::map::theater::load_theater(&mut assets, "NEWURBAN").unwrap();
    let corpus = bridge_catalog_corpus();
    let group = corpus["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["name"] == "retail_ubn_wood03")
        .unwrap();
    let files: Vec<Vec<u8>> = group["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            let bytes = assets.get(file["name"].as_str().unwrap()).unwrap();
            let mut hash = crate::util::sha256::Sha256::new();
            hash.update(&bytes);
            assert_eq!(
                crate::util::sha256::digest_hex(hash.finalize()),
                file["sha256"].as_str().unwrap()
            );
            bytes.to_vec()
        })
        .collect();
    run_bridge_catalog_native_group(group, &files);
    // Exercise normal theater registration, including ordinary water below
    // the real c3y03 bridge span and both replacement families after assets drop.
    let table: [u8; 64] = corpus["process_table"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let rules = TerrainRules::default();
    let catalog =
        BridgeRecalcCatalog::for_runtime_bridges(&theater, &assets, &rules, false, 0, Some(table));
    let keys = crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_theater(&theater);
    let wood = theater.lookup.bounds()[usize::from(theater.wood_bridge_set.unwrap())].start;
    drop(assets);
    for tile in (keys.base..keys.base + 16)
        .chain(i32::from(wood)..i32::from(wood) + 16)
        .chain([314, 317, 319])
    {
        assert!(
            catalog.metadata(tile, 0).unwrap().unwrap().tmp_file_valid,
            "registered source{tile}"
        );
    }
}

#[test]
#[ignore = "requires RA2_DIR and VERA20K_C3Y02MD_MAP / VERA20K_C3Y03MD_MAP"]
fn retail_bridge_catalog_after_normal_map_loading() {
    let retail = std::env::var("RA2_DIR").expect("retail directory");
    let mut total_wood = 0;
    let mut total_ordinary = 0;
    for (variable, fallback) in [
        ("VERA20K_C3Y02MD_MAP", "c3y02md.map"),
        ("VERA20K_C3Y03MD_MAP", "c3y03md.map"),
    ] {
        let map = std::env::var(variable).unwrap_or_else(|_| fallback.into());
        let began = std::time::Instant::now();
        let scenario =
            crate::headless_scenario::load(std::path::Path::new(&retail), &map, 0xB21D6E5).unwrap();
        let loaded_in = began.elapsed();
        let sim = scenario.sim();
        let mut terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let concrete = terrain.concrete_bridge_set_base();
        let wood = terrain.wood_bridge_set_base();
        let indices: Vec<_> = terrain
            .cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                let in_concrete =
                    concrete >= 0 && (concrete..concrete + 16).contains(&cell.final_tile_index);
                let in_wood = wood >= 0 && (wood..wood + 16).contains(&cell.final_tile_index);
                if in_wood {
                    total_wood += 1;
                }
                let structural = cell.bridge_facts.raw_flags & 0x500 != 0;
                if structural && !in_concrete && !in_wood {
                    total_ordinary += 1;
                }
                (in_concrete || in_wood || structural).then_some(index)
            })
            .collect();
        assert!(!indices.is_empty());
        for &index in &indices {
            let cell = &terrain.cells[index];
            let coord = (cell.rx, cell.ry);
            let input = (cell.final_tile_index, cell.final_sub_tile);
            let overlay = sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .finalized_map_cell(coord.0, coord.1)
                .unwrap();
            terrain
                .recalc_resident_bridge_cell(
                    index,
                    overlay,
                    -1,
                    &scenario.runtime.resources.overlay_registry,
                    sim.playfield_bounds,
                )
                .unwrap_or_else(|error| panic!("{fallback} {coord:?} source{input:?}: {error}"));
            terrain
                .refresh_resident_bridge_presentation(index)
                .unwrap_or_else(|error| {
                    panic!("{fallback} {coord:?} presentation{input:?}: {error}")
                });
        }
        println!(
            "{fallback}: normal loading {loaded_in:?}; resident Recalc/presentation accepted {} bridge/under-span cells after loader assets dropped",
            indices.len()
        );
    }
    assert!(total_wood > 0, "actual authored Wood sources required");
    assert!(total_ordinary > 0, "ordinary under-span sources required");
    println!(
        "covered {total_wood} authored Wood cells and {total_ordinary} ordinary under-span cells"
    );
}

#[test]
fn repair_tile_queries_use_registered_names_and_live_index_validity() {
    let grid = bridge_constructor_terrain();
    assert_eq!(grid.current_tile_dimensions(0).unwrap(), (1, 1));
    assert!(grid.current_tile_dimensions(-1).is_err());
    assert!(grid.current_tile_dimensions(1).is_err());
    assert_eq!(
        grid.resolve_registered_tile_name("SOURCE01").unwrap(),
        Some(0)
    );
    assert_eq!(
        grid.resolve_registered_tile_name("source01.tem").unwrap(),
        None
    );
    assert_eq!(grid.resolve_registered_tile_name("absent").unwrap(), None);
    assert!(!grid.tile_allows_morph_placement(0).unwrap());
    assert!(grid.tile_allows_morph_placement(-1).unwrap());
    assert!(grid.tile_allows_morph_placement(1).unwrap());
    assert!(grid.tile_allows_morph_placement(0xffff).unwrap());
}
