//! Scenario load fill: last-tiles pack materialization, authored fill
//! projection, overlay-pack anchors, level prefill and the fill walk over
//! Size and scenario bounds.

use super::*;

#[test]
fn gsi_04_02_last_tiles_pack_materialization_precedes_tmp_lat_and_final_state() {
    let mut theater = gsi_04_02_last_tiles_theater();
    let raw_legacy = gsi_04_02_last_tiles_tmp_bytes(6, [11, 12, 13], [14, 15, 16]);
    let rough = gsi_04_02_last_tiles_tmp_bytes(3, [1, 2, 3], [4, 5, 6]);
    let rough_suffix = gsi_04_02_last_tiles_tmp_bytes(9, [31, 32, 33], [34, 35, 36]);
    let lat = gsi_04_02_last_tiles_tmp_bytes(7, [21, 22, 23], [24, 25, 26]);
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        ("old03.tem", &raw_legacy),
        ("rough01.tem", &rough),
        ("rough01a.tem", &rough_suffix),
        ("lat16.tem", &lat),
    ]);
    crate::map::theater::resolve_contiguous_variant_chains_for_test(&mut theater.lookup, &assets);
    assert_eq!(theater.lookup.translate_legacy_map_tile_index(5), 8);
    assert_eq!(theater.lookup.total_file_count(8), 2);
    let mut map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 3,
            ry: 2,
            tile_index: 5,
            sub_tile: 0,
            z: 4,
        }],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 3;
    map.header.height = 2;
    map.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
        raw_x: -509,
        raw_y: 3,
        canonical: Some((3, 2)),
    }];

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut deterministic_main = || 0;
    // Fill's whole tile index is the draw; 5 is the fixture's LastTiles
    // boundary, which only Pack5 cells translate (to 8).
    let mut fill = |_low, _high| 5;
    let without_lat = {
        let mut selector = cache.begin_load(&mut deterministic_main);
        ResolvedTerrainGrid::build_with_variant_selector(
            &map,
            Some(&theater),
            Some(&assets),
            None,
            None,
            None,
            false,
            0,
            &mut fill,
            &mut selector,
        )
    };
    let translated = without_lat.cell(3, 2).expect("accepted Pack5 cell");
    assert_eq!(translated.source_tile_index, 8);
    assert_eq!(translated.final_tile_index, 8);
    assert_eq!(translated.tileset_index, Some(2));
    assert!(translated.variant > 0);
    assert_eq!(translated.variant, 1);
    assert_eq!(translated.radar_left, [31, 32, 33]);
    assert_eq!(translated.radar_right, [34, 35, 36]);
    assert_eq!(translated.level, 4);

    let fill_cell = without_lat.cell(1, 3).expect("prefilled real CellClass");
    assert_eq!(fill_cell.source_tile_index, 5);
    assert_ne!(fill_cell.source_tile_index, 8);

    let with_lat = {
        let mut selector = cache.begin_load(&mut deterministic_main);
        ResolvedTerrainGrid::build_with_variant_selector(
            &map,
            Some(&theater),
            Some(&assets),
            None,
            None,
            None,
            true,
            0,
            &mut fill,
            &mut selector,
        )
    };
    let lat_fixed = with_lat.cell(3, 2).expect("LAT-fixed Pack5 cell");
    assert_eq!(lat_fixed.source_tile_index, 8);
    assert_eq!(lat_fixed.final_tile_index, 24);
    assert_eq!(lat_fixed.tileset_index, Some(3));
    assert_eq!(lat_fixed.radar_left, [21, 22, 23]);
    assert_eq!(lat_fixed.radar_right, [24, 25, 26]);
}

#[test]
fn gsi_04_02_last_tiles_provenance_gates_both_materialization_paths() {
    let theater = gsi_04_02_last_tiles_theater();
    let mut pack = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 2,
            ry: 2,
            tile_index: 5,
            sub_tile: 0,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );
    pack.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
        raw_x: -510,
        raw_y: 3,
        canonical: Some((2, 2)),
    }];
    pack.header.width = 3;
    pack.header.height = 2;

    let selector_free_pack =
        ResolvedTerrainGrid::build(&pack, Some(&theater), None, None, None, false, 0);
    assert_eq!(
        selector_free_pack
            .cell(2, 2)
            .expect("selector-free Pack5 cell")
            .source_tile_index,
        8
    );

    let mut manual = make_map(pack.cells.clone(), Vec::new(), Vec::new());
    manual.header.width = 3;
    manual.header.height = 2;
    let selector_free_manual =
        ResolvedTerrainGrid::build(&manual, Some(&theater), None, None, None, false, 0);
    assert_eq!(
        selector_free_manual
            .cell(2, 2)
            .expect("manual actual-ID cell")
            .source_tile_index,
        5
    );

    let theaterless_pack = ResolvedTerrainGrid::build(&pack, None, None, None, None, false, 0);
    assert_eq!(
        theaterless_pack
            .cell(2, 2)
            .expect("theater-less synthetic Pack cell")
            .source_tile_index,
        5
    );

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut forbidden_main = || panic!("compatibility fixture has no variants");
    let mut fill = |_low, _high| 0;
    let production_manual = {
        let mut selector = cache.begin_load(&mut forbidden_main);
        ResolvedTerrainGrid::build_with_variant_selector(
            &manual,
            Some(&theater),
            None,
            None,
            None,
            None,
            false,
            0,
            &mut fill,
            &mut selector,
        )
    };
    assert_eq!(
        production_manual
            .cell(2, 2)
            .expect("production manual actual-ID cell")
            .source_tile_index,
        5
    );
    let production_pack = {
        let mut selector = cache.begin_load(&mut forbidden_main);
        ResolvedTerrainGrid::build_with_variant_selector(
            &pack,
            Some(&theater),
            None,
            None,
            None,
            None,
            false,
            0,
            &mut fill,
            &mut selector,
        )
    };
    assert_eq!(
        production_pack
            .cell(2, 2)
            .expect("production Pack5 cell")
            .source_tile_index,
        8
    );
}

#[test]
fn gsi_04_02_last_tiles_does_not_translate_fill_or_dummy_only_pack_misses() {
    let theater = gsi_04_02_last_tiles_theater();
    let mut map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 99,
            ry: 99,
            tile_index: 5,
            sub_tile: 7,
            z: 9,
        }],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 3;
    map.header.height = 2;
    map.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
        raw_x: 99,
        raw_y: 99,
        canonical: None,
    }];
    let dummy = SharedCellDummy::fresh();
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut forbidden_main = || panic!("Fill and misses do not draw Main");
    let mut selector = cache.begin_load(&mut forbidden_main);
    // Fill's whole tile index is the draw; 5 is the fixture's LastTiles
    // boundary, which only Pack5 cells translate (to 8).
    let mut fill = |_low, _high| 5;

    let cells = materialize_map_load_cells(
        &map,
        Some(&theater.lookup),
        &mut selector,
        &mut fill,
        &dummy,
    );

    assert!(!cells.is_empty());
    assert!(cells.iter().all(|cell| cell.tile_index == 5));
    assert!(cells.iter().all(|cell| cell.tile_index != 8));
    assert_eq!(dummy.snapshot().coord, (99, 99));
}

#[test]
fn authored_fill_defers_all_executable_projection() {
    let mut theater = synthetic_theater_from_ini(
        b"[General]\nBridgeSet=0\nBridgeMiddle1=1\nBridgeMiddle2=5\n\
          [TileSet0000]\nTilesInSet=8\nFileName=tunnel\nSetName=Tunnel FX\n\
          [Tunnel FX]\nTile01Anim=TUNNELFX\nTile01AttachesTo=0\n",
    );
    theater.bridge_set = Some(0);
    theater.high_bridge_rim_tiles = crate::map::bridge_rim_tiles::HighBridgeRimTiles {
        base: 0,
        middle: [1, 5],
        ..crate::map::bridge_rim_tiles::HighBridgeRimTiles::ABSENT
    };
    theater.tunnels = Some(0);
    let tmp_name = theater
        .lookup
        .filename_for_variant(0, 0)
        .expect("tunnel TMP name")
        .to_string();
    let suffix_name = tmp_name.replace(".tem", "a.tem");
    let tmp = gsi_04_02_last_tiles_tmp_bytes(5, [1, 2, 3], [4, 5, 6]);
    let suffix_tmp = gsi_04_02_last_tiles_tmp_bytes(5, [7, 8, 9], [10, 11, 12]);
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        (tmp_name.as_str(), tmp.as_slice()),
        (suffix_name.as_str(), suffix_tmp.as_slice()),
    ]);
    crate::map::theater::resolve_contiguous_variant_chains_for_test(&mut theater.lookup, &assets);
    assert_eq!(theater.lookup.total_file_count(0), 2);
    let mut map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 2,
            ry: 3,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        vec![OverlayEntry {
            rx: 2,
            ry: 3,
            overlay_id: 0x18,
            frame: 7,
        }],
        vec![TerrainObject {
            rx: 2,
            ry: 3,
            name: "TREE".to_string(),
        }],
    );
    map.replace_unconsumed_overlay_data(OverlayDataPack::from_cells([(2, 3, 99)]))
        .expect("fixture overlay data remains unconsumed");
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[TerrainTypes]\n0=TREE\n[TREE]\nTemperateOccupationBits=7\n",
    ))
    .expect("admitted Terrain blocker");

    let mut eager_scenario_rng = SimRng::new(0x401);
    let mut eager_fill_calls = 0usize;
    let mut eager_fill = |low, high| {
        eager_fill_calls += 1;
        eager_scenario_rng.next_range_u32_inclusive(low, high)
    };
    let mut eager_main_rng = SimRng::new(0x402);
    let mut eager_main_draw = || eager_main_rng.next_u32();
    let mut eager_cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let (eager, eager_selector_metrics) = {
        let mut selector = eager_cache.begin_load(&mut eager_main_draw);
        let grid = ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
            &map,
            Some(&theater),
            Some(&assets),
            None,
            None,
            Some(&rules.terrain_object_types),
            true,
            2,
            &mut eager_fill,
            &mut selector,
            SharedCellDummy::fresh(),
            OverlayLoadSource::Authored,
            PixelConversionBounds::default(),
        );
        let metrics = (
            selector.generated_table(),
            selector.map_fill_scenario_advance_count(),
            selector.raw_draw_count(),
        );
        (grid, metrics)
    };
    let eager_cell = eager.cell(2, 3).expect("eager fixture cell");
    assert!(eager_cell.bridge_facts.has_structural_bridge());
    assert_eq!(eager_cell.terrain_object_occupation, Some(7));
    assert!(!eager.tile_animations().is_empty());

    let mut pending_scenario_rng = SimRng::new(0x401);
    let mut pending_fill_calls = 0usize;
    let mut pending_fill = |low, high| {
        pending_fill_calls += 1;
        pending_scenario_rng.next_range_u32_inclusive(low, high)
    };
    let mut pending_main_rng = SimRng::new(0x402);
    let mut pending_main_draw = || pending_main_rng.next_u32();
    let mut pending_cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let dummy = SharedCellDummy::fresh();
    let (pending, pending_selector_metrics) = {
        let mut selector = pending_cache.begin_load(&mut pending_main_draw);
        let fill =
            ResolvedTerrainGrid::build_pending_authored_with_variant_selector_and_shared_dummy(
                &map,
                Some(&theater),
                Some(&assets),
                None,
                None,
                true,
                2,
                &mut pending_fill,
                &mut selector,
                dummy.clone(),
            );
        let metrics = (
            selector.generated_table(),
            selector.map_fill_scenario_advance_count(),
            selector.raw_draw_count(),
        );
        (fill, metrics)
    };
    assert_eq!(pending_fill_calls, eager_fill_calls);
    assert_eq!(
        pending_scenario_rng.logical_state(),
        eager_scenario_rng.logical_state()
    );
    assert_eq!(
        pending_main_rng.logical_state(),
        eager_main_rng.logical_state()
    );
    assert_eq!(pending_selector_metrics, eager_selector_metrics);
    assert!(pending_selector_metrics.0);
    assert!(pending_selector_metrics.2 > 0);
    assert_eq!(
        pending
            .pending_grid()
            .iter()
            .map(|cell| (cell.rx, cell.ry, cell.variant))
            .collect::<Vec<_>>(),
        eager
            .iter()
            .map(|cell| (cell.rx, cell.ry, cell.variant))
            .collect::<Vec<_>>()
    );
    let pending_grid = pending.pending_grid();
    let pending_cell = pending_grid.cell(2, 3).expect("pending fixture cell");
    assert_eq!(
        pending_cell.final_tile_index,
        pending_cell.source_tile_index
    );
    assert_eq!(pending_cell.bridge_facts, BridgeCellFacts::default());
    assert_eq!(pending_cell.terrain_object_occupation, None);
    assert!(!pending_cell.terrain_object_blocks);
    assert!(pending_grid.tile_animations().is_empty());
    assert!(pending_grid.tube_facts().is_empty());
    assert_eq!(pending_cell.tube_index, None);
    assert_eq!(dummy.snapshot().bridge_flags_0x1180, 0);
    assert_eq!(
        map.overlay_data_pack()
            .expect("pending Fill retains authored overlay data")
            .byte_at(2, 3),
        99
    );

    let consumed = pending.into_pending_grid();
    assert_eq!(consumed.cell(2, 3).unwrap().source_tile_index, 0);

    let lat_theater = synthetic_lat_theater_from_ini(
        b"[General]\nRoughTile=1\nClearToRoughLat=2\n\
          [TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=5\n\
          [TileSet0001]\nSetName=Rough\nFileName=rough\nTilesInSet=5\n\
          [TileSet0002]\nSetName=ClearToRough\nFileName=crgh\nTilesInSet=16\n",
    );
    let lat_map = make_map(
        vec![
            anim_cell(5, 5, 5, 0, 0),
            anim_cell(5, 4, 0, 0, 0),
            anim_cell(4, 5, 6, 0, 0),
            anim_cell(6, 5, 1, 0, 0),
            anim_cell(5, 6, 7, 0, 0),
        ],
        Vec::new(),
        Vec::new(),
    );
    let pending_lat = ResolvedTerrainGrid::build_inner(
        &lat_map,
        Some(&lat_theater),
        None,
        None,
        None,
        None,
        true,
        0,
        OverlayLoadSource::Authored,
        TerrainBuildProjection::PendingAuthored,
        None,
        None,
        None,
        PixelConversionBounds::default(),
    );
    assert_eq!(pending_lat.cell(5, 5).unwrap().final_tile_index, 5);

    let cliff_map = make_map(
        vec![anim_cell(1, 1, 0, 0, 0), anim_cell(1, 0, 0, 0, 4)],
        Vec::new(),
        Vec::new(),
    );
    let pending_cliff = ResolvedTerrainGrid::build_inner(
        &cliff_map,
        None,
        None,
        None,
        None,
        None,
        false,
        2,
        OverlayLoadSource::Authored,
        TerrainBuildProjection::PendingAuthored,
        None,
        None,
        None,
        PixelConversionBounds::default(),
    );
    let cliff_cell = pending_cliff.cell(1, 1).unwrap();
    assert_eq!(cliff_cell.land_type, LandType::Clear.as_index());
    assert!(!cliff_cell.ground_walk_blocked);
}

#[test]
fn eager_authored_fill_recovers_the_deck_from_both_bridgeheads() {
    let mut theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=8\nFileName=bridge\nSetName=Bridge\n",
    );
    theater.high_bridge_rim_tiles = crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
        0,
        b"[General]\nBridgeTopRight1=5\nBridgeBottomLeft1=7\n",
    );
    let mut map = make_map(
        [
            (9, 10, 4, 8),
            (10, 0, 4, 12),
            (11, 7, 0, 0),
            (12, 0, 0, 0),
            (13, 9, 6, 2),
        ]
        .map(|(ry, z, tile_index, sub_tile)| MapCell {
            rx: 10,
            ry,
            z,
            tile_index,
            sub_tile,
            legacy_ice_byte: 0,
        })
        .to_vec(),
        (10..=12)
            .map(|ry| OverlayEntry {
                rx: 10,
                ry,
                overlay_id: 0x19,
                frame: 0,
            })
            .collect(),
        Vec::new(),
    );
    map.header.width = 10;
    map.header.height = 10;
    let load = |map: &MapFile| {
        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut rng = SimRng::new(0x401);
        let mut draw = || rng.next_u32();
        let mut selector = cache.begin_load(&mut draw);
        ResolvedTerrainGrid::build_with_variant_selector(
            map,
            Some(&theater),
            None,
            None,
            None,
            None,
            false,
            0,
            &mut |low, _| low,
            &mut selector,
        )
    };
    let grid = load(&map);
    assert_eq!(grid.corrected_high_bridge_deck_level((10, 10)), Some(9));
    assert_eq!(grid.corrected_high_bridge_deck_level((10, 11)), Some(9));
    assert_eq!(grid.corrected_high_bridge_deck_level((10, 12)), Some(9));

    // A missing far rim must not let its depressed anchor override the start road.
    map.cells.retain(|cell| (cell.rx, cell.ry) != (10, 13));
    let grid = load(&map);
    assert_eq!(grid.corrected_high_bridge_deck_level((10, 11)), Some(10));
}

#[test]
fn newurban_wooden_high_bridge_uses_both_tmp_approaches() {
    let Some((_directory, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let theater = crate::map::theater::load_theater(&mut assets, "NEWURBAN").unwrap();
    let wood = theater.lookup.bounds()[usize::from(theater.wood_bridge_set.unwrap())].start;
    let mut cells = Vec::new();
    let mut overlays = Vec::new();
    // The pristine NewUrban head TMPs Ovrpsb01/02 (+X) and 03/06 (+Y).
    // Different road levels make each lower end decisive in one direction.
    for (horizontal, start, end, id) in [
        (true, (0, 8, 10, 7), (1, 4, 9), 0xed),
        (false, (2, 12, 9, 7), (5, 2, 10), 0xee),
    ] {
        let coord = |along: u16| {
            if horizontal {
                (along + 15, 10)
            } else {
                (10, along + 15)
            }
        };
        let (start_tile, anchor_sub, start_level, road_sub) = start;
        let (end_tile, end_sub, end_level) = end;
        for (along, relative, sub_tile, z) in [
            (0, start_tile, road_sub, start_level),
            (1, start_tile, anchor_sub, 0),
            (4, end_tile, end_sub, end_level),
        ] {
            let (rx, ry) = coord(along);
            cells.push(MapCell {
                rx,
                ry,
                tile_index: i32::from(wood + relative),
                sub_tile,
                z,
                legacy_ice_byte: 0,
            });
        }
        for along in 1..=3 {
            let (rx, ry) = coord(along);
            overlays.push(OverlayEntry {
                rx,
                ry,
                overlay_id: id,
                frame: 0,
            });
        }
    }
    let mut map = make_map(cells, overlays, Vec::new());
    map.header.theater = "NEWURBAN".into();
    map.header.width = 16;
    map.header.height = 12;
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut rng = SimRng::new(0x401);
    let mut draw = || rng.next_u32();
    let mut selector = cache.begin_load(&mut draw);
    let mut build = |registry| {
        ResolvedTerrainGrid::build_with_variant_selector(
            &map,
            Some(&theater),
            Some(&assets),
            None,
            registry,
            None,
            false,
            0,
            &mut |low, _| low,
            &mut selector,
        )
    };
    let blocked = OverlayTypeRegistry::empty();
    let unresolved = build(Some(&blocked));
    let grid = build(None);
    for coord in [(16, 10), (18, 10), (10, 16), (10, 18)] {
        assert_eq!(
            grid.corrected_high_bridge_deck_level(coord),
            Some(9),
            "{coord:?}"
        );
        assert_eq!(
            unresolved.corrected_high_bridge_deck_level(coord),
            None,
            "unbound {coord:?}"
        );
    }
}

#[test]
fn gsi_04_01_production_overlaypack_stamps_two_anchors_in_row_major_order() {
    let mut map = make_map(
        Vec::new(),
        vec![
            OverlayEntry {
                rx: 2,
                ry: 1,
                overlay_id: 0x18,
                frame: 0,
            },
            OverlayEntry {
                rx: 1,
                ry: 2,
                overlay_id: 0x18,
                frame: 0,
            },
        ],
        Vec::new(),
    );
    map.header.width = 2;
    map.header.height = 1;
    map.header.local_width = 2;
    map.header.local_height = 1;

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut scenario_rng = SimRng::new(0x401);
    let mut fill = |low, high| scenario_rng.next_range_u32_inclusive(low, high);
    let mut main_rng = SimRng::new(0x401);
    let mut main_draw = || main_rng.next_u32();
    let dummy = SharedCellDummy::fresh();
    let grid = {
        let mut selector = cache.begin_load(&mut main_draw);
        ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
            &map,
            None,
            None,
            None,
            None,
            None,
            false,
            0,
            &mut fill,
            &mut selector,
            dummy.clone(),
            OverlayLoadSource::Authored,
            PixelConversionBounds::default(),
        )
    };

    assert!(dummy.same_identity(&grid.shared_cell_dummy()));
    assert_eq!(
        dummy.snapshot(),
        SharedCellDummySnapshot {
            coord: (1, 3),
            level: 0,
            slope_type: 0,
            bridge_flags_0x1180: BRIDGE_FLAG_STRUCTURAL,
        },
        "the later row-major anchor owns the final dummy coord and bits"
    );
}

#[test]
fn gsi_04_12_generated_materialized_overlays_never_replay_fixed_map_mark() {
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 4,
            ry: 4,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        vec![
            OverlayEntry {
                rx: 2,
                ry: 2,
                overlay_id: 0x18,
                frame: 0,
            },
            OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 0x5C,
                frame: 0,
            },
        ],
        Vec::new(),
    );

    let authored = ResolvedTerrainGrid::build_inner(
        &map,
        None,
        None,
        None,
        None,
        None,
        false,
        0,
        OverlayLoadSource::Authored,
        TerrainBuildProjection::EagerCompatibility,
        None,
        None,
        None,
        PixelConversionBounds::default(),
    );
    let generated = ResolvedTerrainGrid::build_inner(
        &map,
        None,
        None,
        None,
        None,
        None,
        false,
        0,
        OverlayLoadSource::GeneratedMaterialized,
        TerrainBuildProjection::EagerCompatibility,
        None,
        None,
        None,
        PixelConversionBounds::default(),
    );

    assert!(
        authored
            .cell(2, 2)
            .unwrap()
            .bridge_facts
            .has_structural_bridge(),
        "the ordinary authored-map source still runs fixed OverlayClass::Mark"
    );
    assert_eq!(
        generated.cell(2, 2).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0,
        "generated direct cell writes must not be expanded a second time"
    );
}

#[test]
fn gsi_04_03a_level_prefill_wraps_before_absolute_last_isomap_overwrite() {
    let mut map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 2,
                ry: 2,
                tile_index: 98,
                sub_tile: 6,
                z: 5,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 2,
                ry: 2,
                tile_index: 99,
                sub_tile: 7,
                z: 9,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 3;
    map.header.height = 2;
    map.header.fill = " \twAtEr\r\n".to_string();
    map.header.level = 260;

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let seed = 0xA55A_1234;
    let mut expected_scenario = SimRng::new(seed);
    let expected_tiles: Vec<_> = (0..10)
        .map(|_| expected_scenario.next_range_u32_inclusive(0, 3) as i32)
        .collect();
    let mut scenario_rng = SimRng::new(seed);
    let mut scenario_calls = 0usize;
    let mut scenario_fill_ranged = |low, high| {
        scenario_calls += 1;
        scenario_rng.next_range_u32_inclusive(low, high)
    };
    let mut main_rng = SimRng::new(seed);
    let main_before = main_rng.logical_state();
    let mut main_draw = || main_rng.next_u32();
    {
        let mut selector = cache.begin_load(&mut main_draw);
        let cells = materialize_map_load_cells(
            &map,
            None,
            &mut selector,
            &mut scenario_fill_ranged,
            &SharedCellDummy::fresh(),
        );
        let coords: Vec<_> = cells.iter().map(|cell| (cell.rx, cell.ry)).collect();
        assert_eq!(
            coords,
            vec![
                (1, 3),
                (2, 2),
                (3, 1),
                (2, 3),
                (3, 2),
                (2, 4),
                (3, 3),
                (4, 2),
                (3, 4),
                (4, 3),
            ]
        );
        assert_eq!(selector.map_fill_scenario_advance_count(), 10);
        for (index, cell) in cells.iter().enumerate() {
            if index == 1 {
                assert_eq!((cell.tile_index, cell.sub_tile, cell.z), (99, 7, 9));
            } else {
                assert_eq!(
                    (cell.tile_index, cell.sub_tile, cell.z),
                    (expected_tiles[index], 0, 4)
                );
            }
        }
        assert!(!coords.contains(&(0, 0)));
        assert!(!coords.contains(&(1, 4)));
        assert!(!selector.generated_table());
        assert_eq!(selector.raw_draw_count(), 0);
    }
    assert_eq!(scenario_calls, 10);
    assert_eq!(
        scenario_rng.logical_state(),
        expected_scenario.logical_state()
    );
    assert_eq!(main_rng.logical_state(), main_before);
}

#[test]
fn gsi_04_02_clear_and_unknown_fill_zero_with_equal_scenario_bounds() {
    for fill in ["Clear", "unknown"] {
        let mut map = make_map(Vec::new(), Vec::new(), Vec::new());
        map.header.width = 3;
        map.header.height = 2;
        map.header.fill = fill.to_string();

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut forbidden_main = || panic!("Fill must not draw Main");
        let mut selector = cache.begin_load(&mut forbidden_main);
        let mut scenario_rng = SimRng::new(0x1234_5678);
        let scenario_before = scenario_rng.logical_state();
        let mut scenario_calls = 0usize;
        let mut scenario_fill_ranged = |low, high| {
            scenario_calls += 1;
            assert_eq!((low, high), (0, 0));
            scenario_rng.next_range_u32_inclusive(low, high)
        };
        let cells = materialize_map_load_cells(
            &map,
            None,
            &mut selector,
            &mut scenario_fill_ranged,
            &SharedCellDummy::fresh(),
        );

        assert_eq!(cells.len(), 10);
        assert!(
            cells
                .iter()
                .all(|cell| { cell.tile_index == 0 && cell.sub_tile == 0 && cell.z == 0 })
        );
        assert_eq!(scenario_calls, 10);
        assert_eq!(scenario_rng.logical_state(), scenario_before);
        assert_eq!(selector.map_fill_scenario_advance_count(), 0);
        assert_eq!(selector.raw_draw_count(), 0);
    }
}

/// On a Size one cell wide the load's fill walk (0x004ACF81..0x004ACFD6)
/// ends at (2, 1), where `Next @ 0x00578290` finds no cell: one Scenario draw
/// fills (1, 1), and the rest of the allocated diagonal keeps the CellClass
/// constructor's tile 0xFFFF and level 0.
#[test]
fn a_one_cell_wide_load_draws_once_and_fills_only_its_first_cell() {
    let mut map = make_map(Vec::new(), Vec::new(), Vec::new());
    map.header.width = 1;
    map.header.height = 3;
    map.header.level = 2;
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut forbidden_main = || panic!("Fill must not draw Main");
    let mut selector = cache.begin_load(&mut forbidden_main);
    let mut draws = 0;
    let mut fill = |_low, _high| {
        draws += 1;
        7
    };

    let cells = materialize_map_load_cells(
        &map,
        None,
        &mut selector,
        &mut fill,
        &SharedCellDummy::fresh(),
    );

    let cells: Vec<_> = cells
        .iter()
        .map(|cell| ((cell.rx, cell.ry), cell.tile_index, cell.z))
        .collect();
    assert_eq!(
        cells,
        vec![((1, 1), 7, 2), ((2, 2), 0xFFFF, 0), ((3, 3), 0xFFFF, 0)]
    );
    assert_eq!(draws, 1);
}

#[test]
fn gsi_04_02_production_build_prefills_before_selector_and_skips_outside_slots() {
    let mut map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 2,
            ry: 2,
            tile_index: theater::NO_TILE,
            sub_tile: 9,
            z: 4,
        }],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 3;
    map.header.height = 2;
    map.header.fill = "Water".to_string();
    let mut theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=64\nFileName=ground\nSetName=Ground\n",
    );
    theater.rmg_tiles.clear_tile = Some(0);
    // This theater resolves WaterSet 1; Fill reads no theater base.
    theater.rmg_tiles.water_set = Some(1);

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let seed = 0xCAFE_BABE;
    let mut scenario_rng = SimRng::new(seed);
    let mut expected_scenario = scenario_rng.clone();
    let expected_first = expected_scenario.next_range_u32_inclusive(0, 3) as i32;
    for _ in 1..10 {
        let _ = expected_scenario.next_range_u32_inclusive(0, 3);
    }
    let mut scenario_fill_ranged = |low, high| scenario_rng.next_range_u32_inclusive(low, high);
    let mut main_rng = SimRng::new(seed);
    let main_before = main_rng.logical_state();
    let mut main_draw = || main_rng.next_u32();
    let main_draw_count;
    {
        let mut selector = cache.begin_load(&mut main_draw);
        let grid = ResolvedTerrainGrid::build_with_variant_selector(
            &map,
            Some(&theater),
            None,
            None,
            None,
            None,
            false,
            0,
            &mut scenario_fill_ranged,
            &mut selector,
        );

        let first = grid.cell(1, 3).expect("allocated first diamond cell");
        assert_eq!(first.source_tile_index, expected_first);
        assert_eq!(first.final_tile_index, expected_first);
        assert!(!first.filled_clear);

        let explicit = grid.cell(2, 2).expect("explicit overwrite cell");
        assert_eq!(explicit.source_tile_index, theater::NO_TILE);
        assert_eq!(explicit.final_sub_tile, 9);
        assert_eq!(explicit.level, 4);
        assert_eq!(grid.presentation_tile(explicit), (0, 0));

        assert_eq!(grid.iter().count(), 10);
        assert!(grid.cell(0, 0).is_none());
        assert_eq!(
            crate::sim::cell_rect::get_cellclass_fallback(Some(&grid), 0, 0).dummy_snapshot(),
            Some(SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            })
        );
        let path_grid = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&grid);
        assert!(!path_grid.is_walkable(0, 0));
        assert_eq!(selector.map_fill_scenario_advance_count(), 10);
        assert!(selector.generated_table());
        assert!(selector.raw_draw_count() > 0);
        main_draw_count = selector.raw_draw_count();
    }
    assert_eq!(
        scenario_rng.logical_state(),
        expected_scenario.logical_state()
    );
    let mut expected_main = SimRng::new(seed);
    for _ in 0..main_draw_count {
        let _ = expected_main.next_u32();
    }
    assert_eq!(main_rng.logical_state(), expected_main.logical_state());
    assert_ne!(main_rng.logical_state(), main_before);
}
