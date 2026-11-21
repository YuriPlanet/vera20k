//! TMP tile catalog and presentation: the native TMP catalog, projectile
//! levels, Clear fallback and selector calls, no-tile and repair tiles,
//! final-LAT tiberium, height in pixels and sparse or out-of-range subtiles.

use super::*;

#[test]
fn native_tmp_catalog_covers_unused_pristine_types_and_live_subtile_replacements() {
    let mut theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n\
          [TileSet0001]\nTilesInSet=1\nFileName=unused\nSetName=Unused\n\
          [TileSet0002]\nTilesInSet=1\nFileName=bad\nSetName=Bad\n\
          [TileSet0003]\nTilesInSet=1\nFileName=missing\nSetName=Missing\n",
    );
    let clear = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    // Three pristine slots: heights 36, sparse/base 30, and 37. There are
    // deliberately no image planes: this unused type only needs headers.
    let mut unused = vec![0u8; 28 + 2 * 52];
    for (offset, value) in [
        (0, 3u32),
        (4, 1),
        (8, 60),
        (12, 30),
        (16, 28),
        (24, 80),
        (28 + 4, 100),
        (28 + 24, 94),
        (28 + 36, 1),
        (80 + 4, 100),
        (80 + 24, 93),
        (80 + 36, 1),
    ] {
        unused[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut variant = gsi_04_04_sparse_tmp_bytes();
    variant[12..16].copy_from_slice(&99u32.to_le_bytes());
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        ("clear01.tem", &clear),
        ("unused01.tem", &unused),
        ("unused01a.tem", &variant),
        ("bad01.tem", b"truncated"),
        ("unregistered01.tem", &variant),
    ]);
    crate::map::theater::resolve_contiguous_variant_chains_for_test(&mut theater.lookup, &assets);
    assert_eq!(
        theater.lookup.variant_count(1),
        1,
        "fixture has a real suffix variant"
    );
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 1,
            ry: 1,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        vec![],
        vec![],
    );
    let mut grid =
        ResolvedTerrainGrid::build(&map, Some(&theater), Some(&assets), None, None, false, 0);
    assert!(grid.cells().iter().all(|cell| cell.final_tile_index != 1));
    assert_eq!(
        grid.native_tmp_draw_heights.len(),
        2,
        "only registered readable pristine headers are cached"
    );
    for (subtile, expected) in [(0, 36), (1, 30), (2, 37), (4, 30), (255, 36)] {
        assert_eq!(grid.native_tmp_draw_height(1, subtile), expected);
    }
    assert_eq!(
        grid.native_tmp_draw_height(2, 0),
        30,
        "corrupt type fallback"
    );
    assert_eq!(
        grid.native_tmp_draw_height(3, 0),
        30,
        "missing type fallback"
    );
    for sentinel in [-1, 0xff, 0xffff] {
        assert_eq!(
            grid.native_tmp_draw_height(sentinel, 5),
            4,
            "clear sentinel always selects pristine subtile zero"
        );
    }

    let cell = grid.cell_mut(1, 1).unwrap();
    cell.final_tile_index = 1;
    cell.final_sub_tile = 5;
    cell.variant = 1;
    let cell = grid.cell(1, 1).unwrap();
    assert_eq!(
        grid.native_tmp_draw_height(cell.final_tile_index, cell.final_sub_tile),
        37,
        "a live replacement resolves its unused pristine type and modulo, independently of selected variant 99"
    );
}

#[test]
fn projectile_level_uses_loaded_theater_interval_through_runtime() {
    use crate::sim::projectile::*;
    use crate::sim::runtime::SimRuntime;
    use crate::sim::world::{Simulation, TickLane};
    let ini=b"[General]\nWaterSet=1\n[TileSet0000]\nTilesInSet=1\nFileName=wet\nSetName=Water\n[TileSet0001]\nTilesInSet=14\nFileName=band\nSetName=Clear\n";
    let palette = [0u8; 768];
    // TMP terrain byte 9 reads Water through GetSubtileLandType's table.
    let wet = gsi_04_02_last_tiles_tmp_bytes(9, [1, 2, 3], [4, 5, 6]);
    let dry = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    let (directory, mut assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        ("temperatmd.ini", ini),
        ("isotem.pal", &palette),
        ("unittem.pal", &palette),
        ("temperat.pal", &palette),
        ("wet01.tem", &wet),
        ("band01.tem", &dry),
    ]);
    let empty_mix = gsi_04_04_mix_bytes("fixture.bin", b"fixture");
    for name in ["temperat.mix", "tem.mix", "isotemmd.mix", "isotemp.mix"] {
        directory.write(name, &empty_mix);
    }
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE")
        .expect("production theater loader");
    assert_eq!(theater.rmg_tiles.water_set, Some(1));
    for (tile, semantic_water, admit) in [(0, true, true), (1, false, false)] {
        let map = make_map(
            vec![MapCell {
                legacy_ice_byte: 0,
                rx: 3,
                ry: 3,
                tile_index: tile,
                sub_tile: 0,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );
        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut draw = || 0u32;
        let mut selector = cache.begin_load(&mut draw);
        let mut fill = |_: u32, _: u32| 0u32;
        let mut grid = ResolvedTerrainGrid::build_with_variant_selector(
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
        );
        assert_eq!(grid.projectile_water_set_base(), 1);
        assert_eq!(
            grid.cell(3, 3).unwrap().land_type == LandType::Water.as_index(),
            semantic_water
        );
        assert_eq!(grid.cell(3, 3).unwrap().final_tile_index, tile);
        let mut sim = Simulation::new();
        sim.install_playfield_from_map_header(&map.header);
        grid.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
        sim.resolved_terrain = Some(grid);
        let key = sim.interner.intern("TEST");
        sim.admit_projectile(
            100,
            ProjectileSpawn {
                native_unique_id: 0,
                line_trail: None,
                flat: false,
                source_id: 999,
                origin: ProjectileCoord::new(896, 896, 500),
                target: ProjectileTarget::None,
                initial_target_position: ProjectileCoord::new(1408, 896, 0),
                payload: ProjectilePayload::new(0, key, None, false),
                speed_leptons_per_frame: 16,
                velocity: ProjectileVelocity::new(16, 0, 0),
                trajectory: ProjectileTrajectory::Ballistic,
                guidance: None,
                visual: ProjectileVisualState::new(0, 0, 0),
                arm_frames: 0,
                fuse_frames: None,
                ranged_fuse: false,
                tracks_target: false,
                target_expiry: TargetExpiryPolicy::Expire,
                collision: ProjectileCollisionPolicy {
                    level: true,
                    ..ProjectileCollisionPolicy::NONE
                },
            },
        );
        let mut runtime = SimRuntime::from_simulation(sim);
        let _ = runtime
            .advance_frame(
                &[],
                16,
                TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert_eq!(
            runtime.simulation.projectiles.get(100).is_none(),
            admit,
            "tile{tile}: semantic water is not the native Level predicate"
        );
    }
}

#[test]
fn gsi_02_11_clear_fallback_calls_selector_even_with_pristine_only() {
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 1,
            ry: 4,
            tile_index: theater::NO_TILE,
            sub_tile: 7,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );
    let theater = synthetic_theater_from_ini(
        b"[General]\nClearTile=0\n[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n",
    );
    assert_eq!(theater.lookup.total_file_count(0), 1);

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut draws = 0u32;
    let mut raw_draw = || {
        let value = draws;
        draws = draws.wrapping_add(1);
        value
    };
    let mut scenario_fill_ranged = |_low, _high| 0;
    {
        let mut selector = cache.begin_load(&mut raw_draw);
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
        assert_eq!(grid.cell(1, 4).expect("fallback cell").variant, 0);
        assert!(selector.generated_table());
        assert!(selector.raw_draw_count() > 0);
    }
    assert!(draws > 0);
}

#[test]
fn gsi_02_11_positive_registry_oor_uses_clear_and_invokes_selector() {
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 1,
            ry: 4,
            tile_index: 1,
            sub_tile: 7,
            z: 2,
        }],
        Vec::new(),
        Vec::new(),
    );
    let theater = synthetic_theater_from_ini(
        b"[General]\nClearTile=0\n[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n",
    );
    assert_eq!(theater.lookup.len(), 1);
    assert_eq!(theater.lookup.total_file_count(0), 1);

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut draws = 0u32;
    let mut raw_draw = || {
        let value = draws;
        draws = draws.wrapping_add(1);
        value
    };
    let mut scenario_fill_ranged = |_low, _high| 0;
    {
        let mut selector = cache.begin_load(&mut raw_draw);
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
        let cell = grid.cell(1, 4).expect("positive out-of-range cell");
        assert_eq!(cell.final_tile_index, 1);
        assert_eq!(cell.final_sub_tile, 7);
        assert_eq!(grid.presentation_tile(cell), (0, 0));
        assert_eq!(cell.variant, 0);
        assert!(selector.generated_table());
        assert!(selector.raw_draw_count() > 0);
    }
    assert!(draws > 0);
}

#[test]
fn no_tile_presentation_uses_theater_clear_tile_without_rewriting_semantics() {
    let ini = b"[TileSet0000]\n\
                TilesInSet=37\n\
                FileName=plain\n\
                SetName=Plain\n\
                \n\
                [TileSet0001]\n\
                TilesInSet=1\n\
                FileName=clear\n\
                SetName=Clear\n";
    let mut theater_data = synthetic_theater_from_ini(ini);
    theater_data.rmg_tiles.clear_tile = Some(37);
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 1,
            ry: 0,
            tile_index: theater::NO_TILE,
            sub_tile: 7,
            z: 3,
        }],
        Vec::new(),
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater_data), None, None, None, false, 0);
    let sentinel = grid.cell(1, 0).expect("sentinel cell");

    assert_eq!(sentinel.final_tile_index, theater::NO_TILE);
    assert_eq!(sentinel.final_sub_tile, 7);
    assert_eq!(sentinel.level, 3);
    assert_eq!(grid.presentation_tile(sentinel), (37, 0));

    let presentation = crate::map::terrain::build_terrain_grid_from_resolved(&grid, None);
    let rendered = presentation
        .cells
        .iter()
        .find(|cell| (cell.rx, cell.ry) == (1, 0))
        .expect("sentinel reaches presentation grid");
    assert_eq!(rendered.tile_id, 37);
    assert_eq!(rendered.sub_tile, 0);
    assert_eq!(rendered.z, 3);

    let used = crate::map::theater::collect_used_tiles(
        &presentation
            .cells
            .iter()
            .map(|cell| (cell.tile_id as i32, cell.sub_tile))
            .collect::<Vec<_>>(),
    );
    assert!(used.contains(&TileKey {
        tile_id: 37,
        sub_tile: 0,
        variant: 0,
    }));
}

#[test]
fn wood_bridge_repair_tile_uses_first_16_tiles_of_wood_bridge_set() {
    let theater = synthetic_theater_with_wood_bridge_set();
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 0,
                tile_index: 9,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: 10,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 2,
                ry: 0,
                tile_index: 25,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 3,
                ry: 0,
                tile_index: 26,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);

    assert!(!grid.cell(0, 0).unwrap().is_wood_bridge_repair_tile);
    assert!(grid.cell(1, 0).unwrap().is_wood_bridge_repair_tile);
    assert!(grid.cell(2, 0).unwrap().is_wood_bridge_repair_tile);
    assert!(!grid.cell(3, 0).unwrap().is_wood_bridge_repair_tile);
}

#[test]
fn resolved_terrain_allow_tiberium_uses_final_lat_tile() {
    let ini = b"[General]\n\
                RoughTile=1\n\
                ClearToRoughLat=2\n\
                \n\
                [TileSet0000]\n\
                SetName=Clear\n\
                FileName=clear\n\
                TilesInSet=5\n\
                \n\
                [TileSet0001]\n\
                SetName=Rough\n\
                FileName=rough\n\
                TilesInSet=5\n\
                \n\
                [TileSet0002]\n\
                SetName=ClearToRough\n\
                FileName=crgh\n\
                TilesInSet=16\n\
                AllowTiberium=true\n";
    let theater = synthetic_lat_theater_from_ini(ini);
    let map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 5,
                ry: 5,
                tile_index: 5,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 5,
                ry: 4,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 4,
                ry: 5,
                tile_index: 6,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 6,
                ry: 5,
                tile_index: 1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 5,
                ry: 6,
                tile_index: 7,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, true, 0);
    let cell = grid.cell(5, 5).expect("center LAT cell");

    assert_eq!(cell.source_tile_index, 5);
    assert_eq!(cell.final_tile_index, 25);
    assert!(cell.allows_tiberium);
}

#[test]
fn gsi_04_03c_height_in_pixels_uses_signed_effective_height_formula() {
    for (relative_extra_y, expected) in [(0, 0), (-1, 0), (-15, 1), (-30, 2), (15, -1)] {
        assert_eq!(
            height_in_pixels_from_tmp(30, relative_extra_y),
            Some(expected),
            "relative extra-Y {relative_extra_y}"
        );
    }
}

#[test]
fn gsi_04_04_sparse_and_positive_oob_subtiles_clear_cell_land_and_slope() {
    let rock = TmpTile {
        height: 7,
        terrain_type: 7,
        ramp_type: 3,
        radar_left: [1, 2, 3],
        radar_right: [4, 5, 6],
        pixels: Vec::new(),
        depth: Vec::new(),
        pixel_width: 60,
        pixel_height: 30,
        relative_extra_y: -30,
        offset_x: -4,
        offset_y: -5,
        has_damaged_data: false,
    };
    let tmp = TmpFile {
        template_width: 2,
        template_height: 1,
        tile_width: 60,
        tile_height: 45,
        tiles: vec![Some(rock), None],
    };

    for sub_tile in [1, 2] {
        let mut metadata = TileMetadata {
            land_type: LandType::Road.as_index(),
            ..TileMetadata::default()
        };
        merge_tmp_file_metadata(&mut metadata, &tmp, sub_tile, None);

        assert_eq!(metadata.subtile_entry_valid, Some(false));
        assert_eq!(metadata.land_type, LandType::Clear.as_index());
        assert_eq!(metadata.slope_type, 0);
        assert!(!metadata.has_ramp);
        assert_eq!(
            metadata.height_in_pixels, 1,
            "header fallback remains Cell-owned for subtile {sub_tile}"
        );
    }
}

#[test]
fn gsi_04_04_valid_tile_sparse_and_oob_entries_materialize_no_tile_cells() {
    let mut theater = synthetic_theater_from_ini(
        b"[General]\nClearTile=0\n\
          [TileSet0000]\nTilesInSet=1\nFileName=sparse\nSetName=Wood Bridge\n\
          Morphable=true\nAllowTiberium=true\nTile01Anim=SPARSEANIM\n\
          Tile01AttachesTo=1\n",
    );
    theater.wood_bridge_set = Some(0);
    let tmp_filename = theater
        .lookup
        .filename_for_variant(0, 0)
        .expect("synthetic tile filename")
        .to_string();
    let (_directory, assets) =
        gsi_04_04_asset_manager_with_loose_tmp(&tmp_filename, &gsi_04_04_sparse_tmp_bytes());
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
                rx: 3,
                ry: 0,
                tile_index: theater::NO_TILE,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 1,
                tile_index: 0,
                sub_tile: 1,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 3,
                ry: 1,
                tile_index: 0,
                sub_tile: 2,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );

    let without_cliff_back =
        ResolvedTerrainGrid::build(&map, Some(&theater), Some(&assets), None, None, false, 0);
    assert!(without_cliff_back.tile_animations().is_empty());
    for (rx, source_sub_tile) in [(1, 1), (3, 2)] {
        let cell = without_cliff_back
            .cell(rx, 1)
            .expect("materialized sparse/OOB cell");
        assert_eq!(cell.source_tile_index, 0);
        assert_eq!(cell.source_sub_tile, source_sub_tile);
        assert_eq!(cell.final_tile_index, 0xFFFF);
        assert_eq!(cell.final_sub_tile, 0);
        assert_eq!(cell.land_type, LandType::Clear.as_index());
        assert_eq!(cell.land_type, LandType::Clear.as_index());
        assert_eq!(cell.slope_type, 0);
        assert!(!cell.has_ramp);
        assert_eq!(cell.template_height, 0);
        assert_eq!(cell.height_in_pixels, 1);
        assert_eq!(cell.tileset_index, None);
        assert_eq!(cell.render_offset_x, 0);
        assert_eq!(cell.render_offset_y, 0);
        assert_eq!(cell.radar_left, [0, 0, 0]);
        assert_eq!(cell.radar_right, [0, 0, 0]);
        assert!(!cell.accepts_smudge);
        assert!(!cell.allows_tiberium);
        assert!(!cell.is_wood_bridge_repair_tile);
        assert_eq!(cell.variant, 0);
        assert_eq!(without_cliff_back.presentation_tile(cell), (0, 0));
    }

    // CliffBack writes Rock's row, which only rules supply; these leave it
    // unauthored, so it is the zero row (Wheel 0: impassable).
    let rules = clear_and_road_rules();
    let with_cliff_back = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&rules),
        None,
        false,
        2,
    );
    for rx in [1, 3] {
        let cell = with_cliff_back.cell(rx, 1).expect("copy-2 CliffBack cell");
        assert_eq!(cell.final_tile_index, 0xFFFF);
        assert_eq!(cell.final_sub_tile, 0);
        assert_eq!(cell.slope_type, 0);
        assert_eq!(cell.height_in_pixels, 1);
        assert_eq!(cell.land_type, LandType::Rock.as_index());
        assert_eq!(cell.base_land_type, LandType::Rock.as_index());
        assert_eq!(cell.zone_type, zone_class::IMPASSABLE);
    }
}

#[test]
fn gsi_04_03c_pristine_owns_height_in_pixels_across_selected_presentation() {
    let pristine = TmpFile {
        template_width: 1,
        template_height: 1,
        tile_width: 60,
        tile_height: 30,
        tiles: vec![Some(TmpTile {
            height: 2,
            terrain_type: 1,
            ramp_type: 1,
            radar_left: [1, 2, 3],
            radar_right: [4, 5, 6],
            pixels: Vec::new(),
            depth: Vec::new(),
            pixel_width: 60,
            pixel_height: 30,
            relative_extra_y: 0,
            offset_x: -3,
            offset_y: -4,
            has_damaged_data: false,
        })],
    };
    let divergent_selected = TmpFile {
        template_width: 2,
        template_height: 1,
        tile_width: 60,
        tile_height: 30,
        tiles: vec![
            Some(TmpTile {
                height: 7,
                terrain_type: 7,
                ramp_type: 3,
                radar_left: [101, 102, 103],
                radar_right: [104, 105, 106],
                pixels: Vec::new(),
                depth: Vec::new(),
                pixel_width: 64,
                pixel_height: 35,
                relative_extra_y: -30,
                offset_x: -8,
                offset_y: -9,
                has_damaged_data: true,
            }),
            None,
        ],
    };
    let mut metadata = TileMetadata::default();
    merge_tmp_file_metadata(&mut metadata, &pristine, 0, None);
    let pristine_metadata = metadata.clone();
    let mut selected_metadata = TileMetadata::default();
    merge_tmp_file_metadata(&mut selected_metadata, &divergent_selected, 0, None);
    assert_ne!(
        selected_metadata.raw_land_type,
        pristine_metadata.raw_land_type
    );
    assert_ne!(selected_metadata.slope_type, pristine_metadata.slope_type);
    assert_ne!(
        selected_metadata.template_height,
        pristine_metadata.template_height
    );
    assert_ne!(
        selected_metadata.has_damaged_data,
        pristine_metadata.has_damaged_data
    );
    assert_eq!(pristine_metadata.height_in_pixels, 0);
    assert_eq!(selected_metadata.height_in_pixels, 2);

    apply_selected_presentation_metadata(&mut metadata, &selected_metadata);

    assert_eq!(metadata.raw_land_type, pristine_metadata.raw_land_type);
    assert_eq!(metadata.land_type, pristine_metadata.land_type);
    assert_eq!(metadata.slope_type, pristine_metadata.slope_type);
    assert_eq!(metadata.template_height, pristine_metadata.template_height);
    assert_eq!(
        metadata.height_in_pixels,
        pristine_metadata.height_in_pixels
    );
    assert_eq!(
        metadata.template_width_cells,
        pristine_metadata.template_width_cells
    );
    assert_eq!(
        metadata.has_damaged_data,
        pristine_metadata.has_damaged_data
    );
    assert_eq!(metadata.radar_left, [101, 102, 103]);
    assert_eq!(metadata.radar_right, [104, 105, 106]);
    assert_eq!(metadata.render_offset_x, -8);
    assert_eq!(metadata.render_offset_y, -9);

    let sparse_suffix = TmpFile {
        template_width: 2,
        template_height: 1,
        tile_width: 60,
        tile_height: 30,
        tiles: vec![None, None],
    };
    let mut sparse_selected_metadata = TileMetadata::default();
    merge_tmp_file_metadata(&mut sparse_selected_metadata, &sparse_suffix, 0, None);
    let mut sparse_result = pristine_metadata.clone();
    apply_selected_presentation_metadata(&mut sparse_result, &sparse_selected_metadata);
    assert_eq!(sparse_result.raw_land_type, pristine_metadata.raw_land_type);
    assert_eq!(sparse_result.slope_type, pristine_metadata.slope_type);
    assert_eq!(
        sparse_result.template_height,
        pristine_metadata.template_height
    );
    assert_eq!(
        sparse_result.height_in_pixels,
        pristine_metadata.height_in_pixels
    );
    assert_eq!(
        sparse_result.has_damaged_data,
        pristine_metadata.has_damaged_data
    );
    assert_eq!(sparse_result.radar_left, [0, 0, 0]);
    assert_eq!(sparse_result.radar_right, [0, 0, 0]);
    assert_eq!(sparse_result.render_offset_x, 0);
    assert_eq!(sparse_result.render_offset_y, 0);
}

#[test]
fn gsi_02_11_damaged_tmp_cell_bypasses_selector_without_table_generation() {
    let mut metadata = TileMetadata::default();
    let damaged_tile = TmpTile {
        height: 0,
        terrain_type: 0,
        ramp_type: 0,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        pixels: Vec::new(),
        depth: Vec::new(),
        pixel_width: 60,
        pixel_height: 30,
        relative_extra_y: 0,
        offset_x: 0,
        offset_y: 0,
        has_damaged_data: true,
    };
    merge_tmp_metadata(&mut metadata, &damaged_tile);
    assert!(metadata.has_damaged_data);

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut draws = 0usize;
    let mut raw_draw = || {
        draws += 1;
        0
    };
    let mut variant = 0u8;
    {
        let mut selector = cache.begin_load(&mut raw_draw);
        if ordinary_variant_selection_enabled(8, false, metadata.has_damaged_data) {
            variant = selector.select_variant(4, 4, 0, 1, 1, 8);
        }
        assert!(!selector.generated_table());
        assert_eq!(selector.raw_draw_count(), 0);
    }
    assert_eq!(variant, 0);
    assert_eq!(draws, 0);
    assert!(!cache.is_initialized());
}
