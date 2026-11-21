//! Authored-load recalc and cliffs: LAT cardinal reads, the CliffBack
//! writers and their gates, reclass filters, and destroyable cliff collapse
//! and origin recovery.

use super::*;

#[test]
fn authored_load_lat_reads_live_cardinals_in_north_east_south_west_order() {
    let theater = synthetic_lat_theater_from_ini(
        b"[General]\nRoughTile=0\nClearToRoughLat=1\n\
          [TileSet0000]\nTilesInSet=1\nFileName=base\nSetName=Rough\n\
          [TileSet0001]\nTilesInSet=16\nFileName=lat\nSetName=Rough LAT\n",
    );
    let lat_config = lat::LatConfig::from_tile_ids(&crate::map::rmg::tiles::TileIds::from_keys(
        &theater.rmg_tiles,
    ));
    let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
    let cardinals = [(1usize, 0usize), (2, 1), (1, 2), (0, 1)];

    for (bit, (nx, ny)) in cardinals.into_iter().enumerate() {
        let mut cells = (0..3)
            .flat_map(|ry| (0..3).map(move |rx| make_test_cell(rx, ry)))
            .collect::<Vec<_>>();
        cells[ny * 3 + nx].final_tile_index = 99;
        let mut grid = ResolvedTerrainGrid::from_cells(3, 3, cells);
        let mut state = LoadCellRecalcState::synthetic(&registry, grid.cells.len());
        state.lat_config = Some(std::borrow::Cow::Owned(lat_config.clone()));
        state.slope_config = Some(lat::SlopeFixupConfig {
            ramp_base: -1,
            ramp_smooth: -1,
        });

        grid.apply_authored_load_lat_slope(&state, 4);

        assert_eq!(
            grid.cells[4].final_tile_index,
            1 + (1_i32 << bit),
            "cardinal slot {bit} must retain native N/E/S/W mask polarity"
        );
    }

    let mut edge = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    let mut state = LoadCellRecalcState::synthetic(&registry, 1);
    state.lat_config = Some(std::borrow::Cow::Owned(lat_config));
    state.slope_config = Some(lat::SlopeFixupConfig {
        ramp_base: -1,
        ramp_smooth: -1,
    });
    edge.apply_authored_load_lat_slope(&state, 0);
    assert_eq!(
        edge.cells[0].final_tile_index, 16,
        "all four missing neighbors must contribute through dummy tile 0xFFFF"
    );
    assert_eq!(
        edge.dummy_cell_requested_coord(),
        (-1, 0),
        "the final W lookup stamps the one shared dummy"
    );

    let mut guarded = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    guarded.cells[0].final_tile_index = 99;
    guarded.stamp_dummy_cell_requested_coord(7, 8);
    let mut state = LoadCellRecalcState::synthetic(&registry, 1);
    state.lat_config = Some(std::borrow::Cow::Owned(lat::LatConfig::from_tile_ids(
        &crate::map::rmg::tiles::TileIds::from_keys(&theater.rmg_tiles),
    )));
    state.slope_config = Some(lat::SlopeFixupConfig {
        ramp_base: 100,
        ramp_smooth: 500,
    });
    guarded.apply_authored_load_lat_slope(&state, 0);
    assert_eq!(guarded.cells[0].final_tile_index, 99);
    assert_eq!(
        guarded.dummy_cell_requested_coord(),
        (7, 8),
        "failed LAT and ramp guards must issue no neighbor lookup"
    );

    let mut ramp = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    ramp.cells[0].final_tile_index = 100;
    ramp.cells[0].slope_type = 1;
    ramp.apply_authored_load_lat_slope(&state, 0);
    assert_eq!(ramp.cells[0].final_tile_index, 502);
    assert_eq!(
        ramp.dummy_cell_requested_coord(),
        (1, 0),
        "slope 1 must probe W then E and leave the E request stamped"
    );

    let mut sloped_neighbors = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    sloped_neighbors.cells[0].final_tile_index = 100;
    sloped_neighbors.cells[0].slope_type = 1;
    sloped_neighbors.test_set_dummy_cell_level_slope(0, 9);
    sloped_neighbors.apply_authored_load_lat_slope(&state, 0);
    assert_eq!(sloped_neighbors.cells[0].final_tile_index, 100);
    assert_eq!(sloped_neighbors.dummy_cell_requested_coord(), (1, 0));
}

#[test]
fn authored_load_cliff_back_uses_exact_six_probes_and_signed_levels() {
    const OFFSETS: [(i16, i16); 6] = [(0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)];
    let center = (3i16, 3i16);
    let center_index = 3 * 7 + 3;

    for (dx, dy) in OFFSETS {
        let mut cells = (0..7)
            .flat_map(|ry| (0..7).map(move |rx| make_test_cell(rx, ry)))
            .collect::<Vec<_>>();
        let nx = usize::try_from(center.0 + dx).unwrap();
        let ny = usize::try_from(center.1 + dy).unwrap();
        cells[ny * 7 + nx].level = 4;
        let grid = ResolvedTerrainGrid::from_cells(7, 7, cells);
        assert!(
            grid.authored_load_cell_is_behind_cliff(center_index),
            "verified probe ({dx},{dy}) must reach the four-level boundary"
        );
    }

    let mut excluded = (0..7)
        .flat_map(|ry| (0..7).map(move |rx| make_test_cell(rx, ry)))
        .collect::<Vec<_>>();
    excluded[4 * 7 + 5].level = 4;
    let excluded = ResolvedTerrainGrid::from_cells(7, 7, excluded);
    assert!(
        !excluded.authored_load_cell_is_behind_cliff(center_index),
        "the adjacent but unprobed (+2,+1) cell must not affect CliffBack"
    );

    let mut signed = (0..7)
        .flat_map(|ry| (0..7).map(move |rx| make_test_cell(rx, ry)))
        .collect::<Vec<_>>();
    signed[center_index].level = u8::MAX;
    signed[2 * 7 + 3].level = 3;
    let signed = ResolvedTerrainGrid::from_cells(7, 7, signed);
    assert!(
        signed.authored_load_cell_is_behind_cliff(center_index),
        "signed -1 plus four reaches signed +3"
    );
}

#[test]
fn authored_load_cliff_back_nonzero_modes_scan_but_only_two_writes() {
    let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);

    for mode in [0u8, 1, 3, u8::MAX] {
        let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        grid.stamp_dummy_cell_requested_coord(7, 8);
        if mode != 0 {
            grid.test_set_dummy_cell_level_slope(4, 0);
        }
        let mut state = LoadCellRecalcState::synthetic(&registry, 1);
        state.cliff_back_impassability = mode;
        grid.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("mode scan Recalc");

        assert_eq!(grid.cells[0].land_type, LandType::Clear.as_index());
        assert_eq!(
            grid.dummy_cell_requested_coord(),
            if mode == 0 { (7, 8) } else { (0, -1) },
            "mode {mode} scan side effect"
        );
    }

    let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    grid.test_set_dummy_cell_level_slope(4, 0);
    let mut state = LoadCellRecalcState::synthetic(&registry, 1);
    state.cliff_back_impassability = 2;
    grid.recalc_authored_load_cell(
        &mut state,
        0,
        FinalizedOverlayCell::default(),
        &mut LoadRecalcTestEffects::default(),
    )
    .expect("mode-2 writer Recalc");
    assert_eq!(grid.cells[0].land_type, LandType::Rock.as_index());
    assert_eq!(
        grid.dummy_cell_requested_coord(),
        (0, -1),
        "the first qualifying probe short-circuits the mode-2 scan"
    );
}

#[test]
fn authored_load_early_overlay_refreshes_tmp_slope_before_resource_clear() {
    let theater =
        synthetic_theater_from_ini(b"[TileSet0000]\nTilesInSet=1\nFileName=early\nSetName=Clear\n");
    let mut tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    tmp[62] = 1;
    let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("early01.tem", &tmp);
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
    let registry = OverlayTypeRegistry::from_ini(
        &IniFile::from_str(
            "[OverlayTypes]\n0=EARLY_RESOURCE\n\
             [EARLY_RESOURCE]\nTiberium=yes\nLand=Tiberium\nNoUseTileLandType=yes\n",
        ),
        None,
    );
    let map = make_map(Vec::new(), Vec::new(), Vec::new());
    let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    assert_eq!(
        grid.cells[0].slope_type, 0,
        "fixture starts with stale flat cache"
    );
    let mut state = LoadCellRecalcState::for_authored_load(
        &map,
        &theater,
        &assets,
        &terrain_rules,
        &registry,
        false,
        0,
        1,
        PixelConversionBounds::default(),
    );

    let outcome = grid
        .recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::from_parts(0, 7),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("early resource Recalc");

    assert_eq!(grid.cells[0].slope_type, 1);
    assert_eq!(outcome.finalized, FinalizedOverlayCell::default());
    assert_eq!(
        grid.cells[0].land_type,
        LandType::Tiberium.as_index(),
        "the early branch copies overlay Land before clearing its pair"
    );

    let mut unregistered = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    unregistered.cells[0].final_tile_index = 99;
    unregistered.cells[0].slope_type = 1;
    let outcome = unregistered
        .recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::from_parts(0, 9),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("unregistered early receiver Recalc");
    assert_eq!(unregistered.cells[0].slope_type, 1);
    assert_eq!(outcome.finalized, FinalizedOverlayCell::default());

    let sparse_theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=absent\nSetName=Clear\n",
    );
    let mut sparse = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
    sparse.cells[0].slope_type = 1;
    let mut sparse_state = LoadCellRecalcState::for_authored_load(
        &map,
        &sparse_theater,
        &assets,
        &terrain_rules,
        &registry,
        false,
        0,
        1,
        PixelConversionBounds::default(),
    );
    let outcome = sparse
        .recalc_authored_load_cell(
            &mut sparse_state,
            0,
            FinalizedOverlayCell::from_parts(0, 11),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("registered missing-TMP early receiver Recalc");
    assert_eq!(sparse.cells[0].slope_type, 0);
    assert_eq!(outcome.finalized, FinalizedOverlayCell::from_parts(0, 11));
}

#[test]
fn authored_load_recalc_keeps_three_cliff_back_writer_gates_distinct() {
    let overlay_ini = IniFile::from_str(
        "[OverlayTypes]\n0=EARLYROAD\n\
         [EARLYROAD]\nLand=Road\nNoUseTileLandType=yes\n",
    );
    let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
    let make_grid = |land: LandType| {
        let mut cells = (0..3)
            .flat_map(|ry| (0..3).map(move |rx| make_test_cell(rx, ry)))
            .collect::<Vec<_>>();
        let center = &mut cells[4];
        center.land_type = land.as_index();
        center.base_land_type = land.as_index();
        cells[1].level = 4;
        ResolvedTerrainGrid::from_cells(3, 3, cells)
    };

    let mut early = make_grid(LandType::Road);
    let mut early_state = LoadCellRecalcState::synthetic(&registry, early.cells.len());
    early_state.cliff_back_impassability = 2;
    early
        .recalc_authored_load_cell(
            &mut early_state,
            4,
            FinalizedOverlayCell::from_parts(0, 0),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("early overlay Recalc");
    assert_eq!(early.cells[4].land_type, LandType::Rock.as_index());
    assert_eq!(
        early.cells[4].base_land_type,
        LandType::Road.as_index(),
        "copy-1 reclasses the claimed current land without widening the base gate"
    );
    assert!(early.cells[4].ground_walk_blocked);
    assert_eq!(early.cells[4].zone_type, zone_class::IMPASSABLE);
    assert!(!early.cells[4].base_build_blocked);

    let mut normal_clear = make_grid(LandType::Clear);
    let mut normal_clear_state =
        LoadCellRecalcState::synthetic(&registry, normal_clear.cells.len());
    normal_clear_state.cliff_back_impassability = 2;
    normal_clear
        .recalc_authored_load_cell(
            &mut normal_clear_state,
            4,
            FinalizedOverlayCell::default(),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("normal Clear Recalc");
    assert_eq!(normal_clear.cells[4].land_type, LandType::Rock.as_index());
    assert_eq!(
        normal_clear.cells[4].base_land_type,
        LandType::Rock.as_index()
    );

    let mut normal_road = make_grid(LandType::Road);
    let mut normal_road_state = LoadCellRecalcState::synthetic(&registry, normal_road.cells.len());
    normal_road_state.cliff_back_impassability = 2;
    normal_road
        .recalc_authored_load_cell(
            &mut normal_road_state,
            4,
            FinalizedOverlayCell::default(),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("normal Road Recalc");
    assert_eq!(normal_road.cells[4].land_type, LandType::Road.as_index());
    assert_eq!(
        normal_road.cells[4].base_land_type,
        LandType::Road.as_index()
    );

    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=missing\nSetName=Clear\n",
    );
    let (_directory, assets) =
        gsi_04_04_asset_manager_with_loose_tmp("unrelated.tem", b"not a TMP");
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
    let map = make_map(Vec::new(), Vec::new(), Vec::new());
    let mut sparse = make_grid(LandType::Clear);
    sparse.cells[4].final_sub_tile = 7;
    let mut sparse_state = LoadCellRecalcState::for_authored_load(
        &map,
        &theater,
        &assets,
        &terrain_rules,
        &registry,
        false,
        2,
        sparse.cells.len(),
        PixelConversionBounds::default(),
    );
    sparse
        .recalc_authored_load_cell(
            &mut sparse_state,
            4,
            FinalizedOverlayCell::default(),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("sparse-subtile Recalc");
    assert_eq!(sparse.cells[4].final_tile_index, 0xFFFF);
    assert_eq!(sparse.cells[4].final_sub_tile, 0);
    assert_eq!(sparse.cells[4].land_type, LandType::Rock.as_index());
    assert_eq!(sparse.cells[4].base_land_type, LandType::Rock.as_index());
}

#[test]
fn gsi_04_04_normal_cliff_back_writer_accepts_clear_but_not_road() {
    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n\
          [TileSet0001]\nTilesInSet=1\nFileName=road\nSetName=Road\n",
    );
    let (_directory, assets) = clear_and_road_tmp_assets(&theater);
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 3,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 3,
                ry: 1,
                tile_index: 1,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );

    let rules = clear_and_road_rules();
    let grid = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&rules),
        None,
        false,
        2,
    );
    let clear = grid.cell(1, 1).expect("ordinary Clear cell");
    assert_eq!(clear.land_type, LandType::Rock.as_index());
    assert_eq!(clear.base_land_type, LandType::Rock.as_index());
    assert_eq!(clear.zone_type, zone_class::IMPASSABLE);

    let road = grid.cell(3, 1).expect("ordinary Road cell");
    assert_eq!(road.land_type, LandType::Road.as_index());
    assert_eq!(road.base_land_type, LandType::Road.as_index());
    assert_eq!(road.zone_type, zone_class::GROUND);
}

#[test]
fn gsi_04_04_early_overlay_writer_does_not_bake_rock_into_road_base() {
    let theater =
        synthetic_theater_from_ini(b"[TileSet0000]\nTilesInSet=1\nFileName=road\nSetName=Road\n");
    let (_directory, assets) = clear_and_road_tmp_assets(&theater);
    let overlay_ini = IniFile::from_str("[OverlayTypes]\n0=CLAIM\n[CLAIM]\nLand=Wall\n");
    let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
        ],
        vec![OverlayEntry {
            rx: 1,
            ry: 1,
            overlay_id: 0,
            frame: 0,
        }],
        Vec::new(),
    );

    let mut grid = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        None,
        Some(&registry),
        false,
        2,
    );
    let claimed = grid.cell(1, 1).expect("overlay-claimed Road cell");
    assert_eq!(claimed.land_type, LandType::Rock.as_index());
    assert_eq!(claimed.base_land_type, LandType::Road.as_index());
    assert_eq!(claimed.zone_type, zone_class::IMPASSABLE);

    let mut overlays = crate::sim::overlay_grid::OverlayGrid::from_overlay_entries(
        &map.overlays,
        grid.width,
        grid.height,
    );
    assert_eq!(overlays.clear_overlay(1, 1), Some(0));
    crate::sim::overlay_grid::recalc_overlay_passability(&mut overlays, &mut grid, &registry, 1, 1);
    let restored = grid.cell(1, 1).expect("restored Road cell");
    assert_eq!(restored.land_type, LandType::Road.as_index());
    assert_eq!(restored.base_land_type, LandType::Road.as_index());
    assert_eq!(restored.zone_type, zone_class::GROUND);
}

#[test]
fn gsi_04_04_cliff_back_neighbor_lookup_keeps_fixed_stride_alias() {
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 511,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 4,
            },
        ],
        Vec::new(),
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
    assert_eq!(
        grid.cell(0, 0).expect("alias source").land_type,
        LandType::Rock.as_index(),
        "packed (-1,1) aliases canonical fixed-stride slot (511,0)"
    );
}

#[test]
fn gsi_04_04_invalid_clear_cliff_back_writer_bakes_base() {
    // Cell (1,1) at level 0, cell (1,0) at level 4.
    // Neighbor offset (0,-1) means (1,0) is checked from (1,1).
    // Height diff = 4 >= 4 → cell (1,1) should be marked impassable.
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    let rules = clear_and_road_rules();
    let grid = ResolvedTerrainGrid::build(&map, None, None, Some(&rules), None, false, 2);
    let cell = grid.cell(1, 1).unwrap();
    assert!(
        cell.ground_walk_blocked,
        "Cell at base of cliff should be blocked"
    );
    assert_eq!(
        cell.land_type,
        crate::rules::terrain_rules::LandType::Rock.as_index(),
        "Cell at base of cliff should have Rock land type"
    );
    // The reclass precedes zone derivation: the reduced zone becomes
    // Impassable, and the base (pre-overlay) snapshot carries the reclass
    // so an overlay add/remove cycle cannot restore the original clear
    // terrain (levels never change at runtime).
    assert_eq!(cell.zone_type, zone_class::IMPASSABLE);
    assert_eq!(
        cell.base_land_type,
        crate::rules::terrain_rules::LandType::Rock.as_index()
    );
    assert!(cell.base_ground_walk_blocked);
    assert!(cell.base_build_blocked);
    assert_eq!(
        cell.land_type,
        crate::rules::terrain_rules::LandType::Rock.as_index()
    );
}

#[test]
fn gsi_04_04_sentinel_tile_uses_final_cliff_back_copy_for_overlay_water() {
    let overlay_ini =
        IniFile::from_str("[OverlayTypes]\n0=WATEROVERLAY\n[WATEROVERLAY]\nLand=Water\n");
    let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: theater::NO_TILE,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: theater::NO_TILE,
                sub_tile: 7,
                z: 0,
            },
        ],
        vec![OverlayEntry {
            rx: 1,
            ry: 1,
            overlay_id: 0,
            frame: 0,
        }],
        Vec::new(),
    );

    let without_cliff_back =
        ResolvedTerrainGrid::build(&map, None, None, None, Some(&registry), false, 0);
    let water = without_cliff_back
        .cell(1, 1)
        .expect("sentinel overlay-water cell");
    assert_eq!(water.final_tile_index, theater::NO_TILE);
    assert_eq!(water.final_sub_tile, 7);
    assert_eq!(water.land_type, LandType::Water.as_index());

    let with_cliff_back =
        ResolvedTerrainGrid::build(&map, None, None, None, Some(&registry), false, 2);
    let rock = with_cliff_back
        .cell(1, 1)
        .expect("final-copy CliffBack cell");
    assert_eq!(rock.final_tile_index, theater::NO_TILE);
    assert_eq!(rock.final_sub_tile, 7);
    assert_eq!(rock.land_type, LandType::Rock.as_index());
    assert_eq!(rock.base_land_type, LandType::Rock.as_index());
    assert_eq!(rock.zone_type, zone_class::IMPASSABLE);
}

#[test]
fn gsi_04_03b_cliff_back_compares_levels_as_signed_bytes() {
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 3,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0xff,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    let rules = clear_and_road_rules();
    let grid = ResolvedTerrainGrid::build(&map, None, None, Some(&rules), None, false, 2);
    let cell = grid.cell(1, 1).expect("signed -1 cell");
    assert!(
        cell.ground_walk_blocked,
        "signed -1 to +3 is the exact four-level CliffBack boundary"
    );
}

#[test]
fn cliff_back_impassability_skips_when_disabled() {
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    // cliff_back_impassability = 0 → disabled
    let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 0);
    let cell = grid.cell(1, 1).unwrap();
    assert!(
        !cell.ground_walk_blocked,
        "Should NOT be blocked when disabled"
    );
}

#[test]
fn gsi_04_04_normal_cliff_back_reclass_filter_includes_ice() {
    use crate::rules::terrain_rules::LandType;
    for land in [
        LandType::Clear,
        LandType::Water,
        LandType::Beach,
        LandType::Ice,
    ] {
        assert!(
            cliff_back_normal_reclass_applies(land.as_index()),
            "{land:?} must be reclassed to Rock behind a cliff"
        );
    }
    for land in [
        LandType::Road,
        LandType::Rock,
        LandType::Wall,
        LandType::Tiberium,
        LandType::Railroad,
        LandType::Rough,
        LandType::Tunnel,
        LandType::Weeds,
    ] {
        assert!(
            !cliff_back_normal_reclass_applies(land.as_index()),
            "{land:?} must keep its own land type behind a cliff"
        );
    }
}

#[test]
fn destroyable_cliff_a_collapses_old_sparse_footprint_then_stamps_two_halves() {
    let mut cells = Vec::new();
    for ry in 0..4 {
        for rx in 0..6 {
            let mut cell = make_test_cell(rx, ry);
            let sub_tile = (ry * 6 + rx) as u8;
            if ![0, 5, 18, 23].contains(&usize::from(sub_tile)) {
                cell.final_tile_index = 100;
                cell.final_sub_tile = sub_tile;
            }
            cell.level = 1;
            cells.push(cell);
        }
    }
    let mut grid = ResolvedTerrainGrid::from_cells(6, 4, cells);
    grid.test_install_destroyable_cliff_catalog(100);

    let mutation = grid
        .collapse_destroyable_cliff_terrain(4, 1, None, |_, _| {})
        .expect("family-A destroyable cliff");

    assert_eq!(mutation.family, DestroyableCliffFamily::A);
    assert_eq!(mutation.origin, (0, 0));
    assert_eq!(mutation.original_footprint.len(), 20);
    assert_eq!(
        mutation.animation_cells,
        (0..3)
            .flat_map(|ry| (0..5).map(move |rx| (rx, ry)))
            .collect::<Vec<_>>(),
    );
    assert_eq!(mutation.changed_cells.len(), 20);
    for ry in 0..4 {
        for rx in 0..6 {
            let cell = grid.cell(rx, ry).unwrap();
            if [(0, 0), (5, 0), (0, 3), (5, 3)].contains(&(rx, ry)) {
                assert_eq!(cell.final_tile_index, 0, "sparse holes stay untouched");
                continue;
            }
            let (tile, sub_tile, slope) = if rx < 3 {
                (200, ry * 3 + rx, 1)
            } else {
                (201, ry * 3 + (rx - 3), 2)
            };
            assert_eq!(cell.final_tile_index, tile);
            assert_eq!(u16::from(cell.final_sub_tile), sub_tile);
            assert_eq!(cell.slope_type, slope);
            assert_eq!(cell.level, 1, "old height subtracts before replacement add");
            assert!(!cell.has_bridge_deck);
            assert_eq!(cell.bridge_facts, BridgeCellFacts::default());
        }
    }
}

#[test]
fn destroyable_cliff_collapse_turns_land_under_a_higher_neighbour_to_rock() {
    use crate::rules::terrain_rules::{LandType, LandTypeSemantics, SpeedCostProfile};
    let mut cells = Vec::new();
    for ry in 0..6u16 {
        for rx in 0..8u16 {
            let mut cell = make_test_cell(rx, ry);
            cell.level = 1;
            if (1..7).contains(&rx) && (1..5).contains(&ry) {
                let sub_tile = (ry - 1) * 6 + (rx - 1);
                if ![0, 5, 18, 23].contains(&sub_tile) {
                    cell.final_tile_index = 100;
                    cell.final_sub_tile = sub_tile as u8;
                }
            }
            cells.push(cell);
        }
    }
    // (3, 0) stands four levels over the stamped (3, 1)'s final Level 1.
    cells[3].level = 5;
    let mut grid = ResolvedTerrainGrid::from_cells(8, 6, cells);
    grid.test_install_destroyable_cliff_catalog(100);
    let rock = LandTypeSemantics {
        buildable: false,
        ground_blocked: true,
        speed_costs: SpeedCostProfile::ZERO,
    };

    grid.collapse_destroyable_cliff_terrain(4, 2, Some(&rock), |_, _| {})
        .expect("family-A destroyable cliff");

    let behind = grid.cell(3, 1).unwrap();
    assert_eq!(behind.land_type, LandType::Rock.as_index());
    assert_eq!(behind.base_land_type, LandType::Rock.as_index());
    assert!(behind.ground_walk_blocked);
    assert_eq!(behind.zone_type, zone_class::IMPASSABLE);
    let open = grid.cell(3, 3).unwrap();
    assert_eq!(open.land_type, LandType::Clear.as_index());
    assert!(!open.ground_walk_blocked);
}

#[test]
fn destroyable_cliff_origin_recovery_accepts_every_present_a_and_b_subtile() {
    for (family, tile, width, height, holes) in [
        (
            DestroyableCliffFamily::A,
            100,
            6u16,
            4u16,
            &[0usize, 5, 18, 23][..],
        ),
        (
            DestroyableCliffFamily::B,
            101,
            4u16,
            6u16,
            &[0usize, 3, 20, 23][..],
        ),
    ] {
        for selected_subtile in 0..usize::from(width * height) {
            if holes.contains(&selected_subtile) {
                continue;
            }
            let mut cells = Vec::new();
            for ry in 0..height {
                for rx in 0..width {
                    let mut cell = make_test_cell(rx, ry);
                    let sub_tile = usize::from(ry * width + rx);
                    if !holes.contains(&sub_tile) {
                        cell.final_tile_index = tile;
                        cell.final_sub_tile = sub_tile as u8;
                    }
                    cell.level = 1;
                    cells.push(cell);
                }
            }
            let mut grid = ResolvedTerrainGrid::from_cells(width, height, cells);
            grid.test_install_destroyable_cliff_catalog(100);
            let selected = (
                (selected_subtile % usize::from(width)) as u16,
                (selected_subtile / usize::from(width)) as u16,
            );

            let mutation = grid
                .collapse_destroyable_cliff_terrain(selected.0, selected.1, None, |_, _| {})
                .expect("present sparse subtile selects its family");

            assert_eq!(mutation.family, family);
            assert_eq!(mutation.origin, (0, 0));
            assert_eq!(mutation.original_footprint.len(), 20);
            assert_eq!(mutation.changed_cells.len(), 20);
            for ry in 0..height {
                for rx in 0..width {
                    let index = usize::from(ry * width + rx);
                    let cell = grid.cell(rx, ry).unwrap();
                    if holes.contains(&index) {
                        assert_eq!(cell.final_tile_index, 0, "sparse hole {index}");
                        continue;
                    }
                    let (expected_tile, expected_subtile, expected_slope) = match family {
                        DestroyableCliffFamily::A if rx < 3 => (200, usize::from(ry * 3 + rx), 1),
                        DestroyableCliffFamily::A => (201, usize::from(ry * 3 + (rx - 3)), 2),
                        DestroyableCliffFamily::B if ry < 3 => (203, usize::from(ry * 4 + rx), 4),
                        DestroyableCliffFamily::B => (202, usize::from((ry - 3) * 4 + rx), 3),
                    };
                    assert_eq!(cell.final_tile_index, expected_tile);
                    assert_eq!(usize::from(cell.final_sub_tile), expected_subtile);
                    assert_eq!(cell.slope_type, expected_slope);
                    assert_eq!(cell.level, 1);
                }
            }
        }
    }
}
