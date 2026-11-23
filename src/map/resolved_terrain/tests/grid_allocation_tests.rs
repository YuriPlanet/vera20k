//! Grid allocation and cell lookup: the Size-diamond capacity edge, reloads
//! that replace a larger grid, the detached Dummy cell word, IsoMap misses,
//! bridge publication identity and lookups past 512 cells.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProductionBuildStats {
    fill_calls: usize,
    water_advances: usize,
    generated_selector_table: bool,
    main_draws: usize,
}

fn build_production_grid(
    map: &MapFile,
    theater_data: Option<&TheaterData>,
    cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    main_draw: &mut dyn FnMut() -> u32,
) -> (ResolvedTerrainGrid, ProductionBuildStats) {
    let mut fill_calls = 0usize;
    let mut scenario_fill_ranged = |_low, _high| {
        fill_calls += 1;
        0
    };
    let mut selector = cache.begin_load(main_draw);
    let grid = ResolvedTerrainGrid::build_with_variant_selector(
        map,
        theater_data,
        None,
        None,
        None,
        None,
        false,
        0,
        &mut scenario_fill_ranged,
        &mut selector,
    );
    let stats = ProductionBuildStats {
        fill_calls,
        water_advances: selector.map_fill_scenario_advance_count(),
        generated_selector_table: selector.generated_table(),
        main_draws: selector.raw_draw_count(),
    };
    (grid, stats)
}

fn build_production_grid_without_theater(
    map: &MapFile,
    cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
) -> (ResolvedTerrainGrid, ProductionBuildStats) {
    let mut unused_main_draw = || 0;
    build_production_grid(map, None, cache, &mut unused_main_draw)
}

/// `MapClass::Resize @ 0x00565C10` owns a fixed 512x512 table and allocates
/// exactly `M * (2N - 1)` Size-diamond cells. Rust deliberately rejects the
/// first malformed dimension sum that would cross that native capacity.
#[test]
fn gsi_04_01_production_size_diamond_closes_exact_capacity_edge() {
    const N: u32 = 511;
    const M: u32 = 1;
    const HIGHEST_ALLOCATED: (i32, i32) = (2, 511);

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut edge = make_map(Vec::new(), Vec::new(), Vec::new());
    edge.header.width = N;
    edge.header.height = M;
    let (grid, stats) = build_production_grid_without_theater(&edge, &mut cache);

    let expected_count = M as usize * (2 * N as usize - 1);
    assert_eq!(expected_count, 1_021);
    let highest_index =
        crate::map::cell_index::cell_linear_index(HIGHEST_ALLOCATED.0, HIGHEST_ALLOCATED.1)
            .expect("capacity-edge coordinate has a fixed-stride slot");
    let highest_index_usize =
        usize::try_from(highest_index).expect("fixed-stride table index fits usize");
    let allocated = grid
        .native_allocated
        .as_ref()
        .expect("production build records native allocation membership");
    assert_eq!((grid.width(), grid.height()), (512, 512));
    assert_eq!(grid.iter().count(), expected_count);
    assert_eq!(stats.fill_calls, expected_count);
    assert_eq!(
        allocated.iter().filter(|&&slot| slot).count(),
        expected_count
    );
    let fixed_stride = usize::try_from(crate::map::cell_index::CELL_ROW_STRIDE)
        .expect("native cell stride fits usize");
    let fixed_slot = |x: u32, y: u32| {
        usize::try_from(y).expect("native y fits usize") * fixed_stride
            + usize::try_from(x).expect("native x fits usize")
    };
    let mut expected_slots = vec![false; fixed_stride * 512];
    for x in 1u32..=511 {
        let y = 512 - x;
        let slot = fixed_slot(x, y);
        assert!(!expected_slots[slot], "duplicate sum-512 slot ({x}, {y})");
        expected_slots[slot] = true;
    }
    for x in 2u32..=511 {
        let y = 513 - x;
        let slot = fixed_slot(x, y);
        assert!(!expected_slots[slot], "duplicate sum-513 slot ({x}, {y})");
        expected_slots[slot] = true;
    }
    assert_eq!(expected_slots.iter().filter(|&&slot| slot).count(), 1_021);
    for y in 0u32..512 {
        for x in 0u32..512 {
            let slot = fixed_slot(x, y);
            let expected = expected_slots[slot];
            assert_eq!(
                allocated[slot], expected,
                "native allocation membership at ({x}, {y}), fixed slot {slot}"
            );
            let cell_x = u16::try_from(x).expect("native x fits CellClass coordinate");
            let cell_y = u16::try_from(y).expect("native y fits CellClass coordinate");
            assert_eq!(
                grid.cell(cell_x, cell_y).is_some(),
                expected,
                "grid lookup membership at ({x}, {y}), fixed slot {slot}"
            );
        }
    }
    assert_eq!(
        allocated.iter().rposition(|&slot| slot),
        Some(highest_index_usize)
    );
    assert_eq!(
        highest_index,
        511 * crate::map::cell_index::CELL_ROW_STRIDE + 2
    );
    assert!(grid.cell(2, 511).is_some());
    assert!(grid.cell(3, 511).is_none());

    // Sum 513 now selects the extended array; past its capacity is rejected.
    let mut oversized = make_map(Vec::new(), Vec::new(), Vec::new());
    oversized.header.width = 1024;
    oversized.header.height = M;
    let (rejected, rejected_stats) = build_production_grid_without_theater(&oversized, &mut cache);
    assert_eq!((rejected.width(), rejected.height()), (0, 0));
    assert!(rejected.cells.is_empty());
    assert_eq!(rejected.iter().count(), 0);
    assert!(
        rejected
            .native_allocated
            .as_ref()
            .is_some_and(|v| v.is_empty())
    );
    assert_eq!(rejected_stats.fill_calls, 0);
    assert!(rejected.cell(0, 0).is_none());
}

/// `MapClass::Clear @ 0x00565B00` destroys and nulls every prior slot before
/// an ordinary `MapClass::Resize @ 0x00565C10`, so a later smaller load must
/// expose only its own cells and membership. Resize then reconstructs the
/// fallback CellClass at its fixed `0x00ABDC50` address: the identity
/// survives, but its constructor-owned contents do not.
#[test]
fn gsi_04_01_smaller_production_load_replaces_larger_grid_without_stale_state() {
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut larger = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 4,
            ry: 8,
            tile_index: theater::NO_TILE,
            sub_tile: 0,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );
    larger.header.width = 5;
    larger.header.height = 4;
    larger.header.fill = "Water".to_string();
    let theater =
        synthetic_theater_from_ini(b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n");
    let mut main_rng = SimRng::new(0x0401_5EED);
    let mut main_draw = || main_rng.next_u32();
    let process_dummy = SharedCellDummy::fresh();

    process_dummy.reconstruct_for_map_resize();
    let (mut current, larger_stats) =
        build_production_grid(&larger, Some(&theater), &mut cache, &mut main_draw);
    current.bind_shared_cell_dummy(process_dummy.clone());
    assert_eq!(current.iter().count(), 4 * (2 * 5 - 1));
    assert_eq!(larger_stats.fill_calls, 36);
    assert_eq!(larger_stats.water_advances, 36);
    assert!(larger_stats.generated_selector_table);
    assert!(larger_stats.main_draws > 0);
    assert!(cache.is_initialized());
    assert!(current.cell(4, 8).is_some());
    current
        .cell_mut(4, 8)
        .expect("larger-only allocated cell")
        .level = 77;
    current.test_set_dummy_cell_level_slope(-7, 11);
    current.stamp_dummy_cell_requested_coord(-7, 11);
    let larger_dummy = current.shared_cell_dummy();

    let mut smaller = make_map(Vec::new(), Vec::new(), Vec::new());
    smaller.header.width = 2;
    smaller.header.height = 1;
    process_dummy.reconstruct_for_map_resize();
    let (mut smaller_grid, smaller_stats) =
        build_production_grid_without_theater(&smaller, &mut cache);
    smaller_grid.bind_shared_cell_dummy(process_dummy);
    assert!(larger_dummy.same_identity(&smaller_grid.shared_cell_dummy()));
    current = smaller_grid;

    let allocated_coords: Vec<_> = current.iter().map(|cell| (cell.rx, cell.ry)).collect();
    assert_eq!(allocated_coords, vec![(2, 1), (1, 2), (2, 2)]);
    assert_eq!((current.width(), current.height()), (3, 3));
    assert_eq!(current.cells.len(), 9);
    assert_eq!(current.iter().count(), 3);
    assert_eq!(
        current
            .native_allocated
            .as_ref()
            .expect("second production membership")
            .iter()
            .filter(|&&slot| slot)
            .count(),
        3
    );
    assert!(current.cell(1, 1).is_none());
    assert!(current.cell(4, 8).is_none());
    assert_eq!(current.dummy_cell_requested_coord(), (0, 0));
    assert_eq!(current.dummy_cell_level_slope(), (0, 0));

    assert_eq!(smaller_stats.fill_calls, 3);
    assert_eq!(smaller_stats.water_advances, 0);
    assert!(!smaller_stats.generated_selector_table);
    assert_eq!(smaller_stats.main_draws, 0);
    assert!(cache.is_initialized());
}

/// A detached Dummy stamps and reads a private copy of its cell word; the
/// shared word, other threads and other Dummies keep their own.
#[test]
fn a_detached_dummy_stamps_a_private_cell_word() {
    let dummy = SharedCellDummy::fresh();
    dummy.stamp_coord(-7, 11);
    dummy.set_level(-3);
    let other = SharedCellDummy::fresh();
    let (inside, forked, elsewhere) = dummy.detached(|| {
        dummy.clone().stamp_coord(40, -2);
        other.stamp_coord(5, 6);
        let elsewhere = std::thread::scope(|scope| {
            scope
                .spawn(|| dummy.snapshot().coord)
                .join()
                .expect("reader thread")
        });
        (
            dummy.snapshot(),
            dummy.fork_query_identity().snapshot().coord,
            elsewhere,
        )
    });
    assert_eq!(inside.coord, (40, -2));
    assert_eq!(inside.level, -3, "a stamp keeps the level byte");
    assert_eq!(forked, (40, -2), "a fork copies this thread's view");
    assert_eq!(elsewhere, (-7, 11), "another thread reads the shared word");
    assert_eq!(dummy.snapshot().coord, (-7, 11), "the shared word is kept");
    assert_eq!(
        other.snapshot().coord,
        (5, 6),
        "another Dummy is not detached"
    );
}

/// A run nested in another starts from the shared word, not the enclosing
/// copy, and a stamp that changed the copy counts even when a later one
/// restored it.
#[test]
fn a_nested_detached_run_starts_from_the_shared_word() {
    let dummy = SharedCellDummy::fresh();
    dummy.stamp_coord(-7, 11);
    let ((nested, after), changed) = dummy.detached_stamps(|| {
        dummy.stamp_coord(40, -2);
        let nested = dummy.detached(|| dummy.snapshot().coord);
        let after = dummy.snapshot().coord;
        dummy.stamp_coord(-7, 11);
        (nested, after)
    });
    assert_eq!(nested, (-7, 11), "a nested run starts from the shared word");
    assert_eq!(after, (40, -2), "the enclosing copy resumes after it");
    assert!(
        changed,
        "a stamp changed the copy before another restored it"
    );
    let ((), restamped) = dummy.detached_stamps(|| dummy.stamp_coord(-7, 11));
    assert!(
        !restamped,
        "restamping the word the copy holds changes nothing"
    );
}

#[test]
fn gsi_04_01_isomap_misses_stamp_raw_coord_without_payload_leak() {
    let mut map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 2,
                ry: 1,
                tile_index: 41,
                sub_tile: 4,
                z: 5,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 3,
                ry: 1,
                tile_index: 99,
                sub_tile: 9,
                z: 10,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 2;
    map.header.height = 1;
    map.iso_map_pack_lookups = vec![
        crate::map::map_file::IsoMapPackLookup {
            raw_x: -1,
            raw_y: 0,
            canonical: None,
        },
        crate::map::map_file::IsoMapPackLookup {
            raw_x: -510,
            raw_y: 2,
            canonical: Some((2, 1)),
        },
        crate::map::map_file::IsoMapPackLookup {
            raw_x: -509,
            raw_y: 2,
            canonical: Some((3, 1)),
        },
    ];

    let dummy = SharedCellDummy::fresh();
    dummy.stamp_coord(90, 91);
    dummy.set_level_slope(-7, 11);
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut forbidden_main = || panic!("materialization must not draw Main");
    let mut selector = cache.begin_load(&mut forbidden_main);
    let mut fill = |_low, _high| 0;

    let cells = materialize_map_load_cells(&map, None, &mut selector, &mut fill, &dummy);

    let real = cells
        .iter()
        .find(|cell| (cell.rx, cell.ry) == (2, 1))
        .expect("aliased lookup resolves to an allocated real cell");
    assert_eq!((real.tile_index, real.sub_tile, real.z), (41, 4, 5));
    assert!(
        cells.iter().all(|cell| cell.tile_index != 99),
        "null-slot payload must not leak into a real CellClass"
    );
    assert_eq!(
        dummy.snapshot(),
        SharedCellDummySnapshot {
            coord: (-509, 2),
            level: -7,
            slope_type: 11,
            bridge_flags_0x1180: 0,
        },
        "the last miss stamps its raw request and preserves non-coordinate bytes"
    );
}

#[test]
fn gsi_04_01_valid_isomap_lookup_does_not_stamp_dummy() {
    let mut map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 2,
            ry: 1,
            tile_index: 41,
            sub_tile: 4,
            z: 5,
        }],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 2;
    map.header.height = 1;
    map.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
        raw_x: -510,
        raw_y: 2,
        canonical: Some((2, 1)),
    }];

    let dummy = SharedCellDummy::fresh();
    dummy.stamp_coord(-7, 11);
    dummy.set_level_slope(-3, 9);
    dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_ANCHOR_SELF);
    let expected_dummy = dummy.snapshot();
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut forbidden_main = || panic!("materialization must not draw Main");
    let mut selector = cache.begin_load(&mut forbidden_main);
    let mut fill = |_low, _high| 0;

    let cells = materialize_map_load_cells(&map, None, &mut selector, &mut fill, &dummy);

    assert_eq!(dummy.snapshot(), expected_dummy);
    assert_eq!(
        cells
            .iter()
            .find(|cell| (cell.rx, cell.ry) == (2, 1))
            .map(|cell| (cell.tile_index, cell.sub_tile, cell.z)),
        Some((41, 4, 5))
    );
}

#[test]
fn gsi_04_01_runtime_setter_uses_native_real_or_dummy_order() {
    let mut grid = crate::map::resolved_terrain::test_grid(3, 3, make_test_cell);
    grid.test_set_native_allocated_cells(&[(1, 1)]);
    let dummy = grid.shared_cell_dummy();
    let stamp = BridgeFlagStamp::new((1, 1), 0, true);

    let _ = grid.apply_runtime_bridge_flag_stamp(stamp);

    assert_eq!(
        grid.cell(1, 1).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        "the allocated anchor mutates the real CellClass"
    );
    assert_eq!(
        dummy.snapshot(),
        SharedCellDummySnapshot {
            coord: (1, 2),
            level: 0,
            slope_type: 0,
            bridge_flags_0x1180: BRIDGE_FLAG_STRUCTURAL,
        },
        "the missing opposite is the last native lookup/writer"
    );
    assert_eq!(
        grid.cellclass_bridge_flags_0x1180(9, 7),
        BRIDGE_FLAG_STRUCTURAL,
        "later candidate-filter lookups observe the live dummy flags"
    );
    assert_eq!(dummy.snapshot().coord, (9, 7));

    let _ = grid.apply_runtime_bridge_flag_stamp(BridgeFlagStamp {
        set: false,
        ..stamp
    });
    assert_eq!(dummy.bridge_flags_0x1180(), 0);
    assert_eq!(
        grid.cell(1, 1).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        0
    );

    let _ = grid.apply_runtime_bridge_flag_stamp(stamp);
    assert_eq!(dummy.bridge_flags_0x1180(), BRIDGE_FLAG_STRUCTURAL);
}

#[test]
fn bridge_publication_retains_allocation_identity_across_other_lookups() {
    let mut terrain = crate::map::resolved_terrain::test_grid(3, 3, make_test_cell);
    terrain.test_set_native_allocated_cells(&[(1, 1)]);
    let real = terrain.native_cell_identity((-511, 2));
    assert_eq!(real, NativeCellIdentity::Real(4));
    let dummy = terrain.native_cell_identity((-1, -1));
    terrain.write_native_cell_flags(real, 0xa401_1380);
    terrain.write_native_cell_flags(dummy, 0xb502_0380);
    terrain.write_native_cell_anchor(real, Some(dummy));
    terrain.write_native_cell_anchor(dummy, Some(real));
    terrain.shared_cell_dummy().write_overlay_state(17);
    assert_eq!(terrain.native_cell_identity((8, 9)), dummy);
    assert_eq!(terrain.native_cell_coord(real), (1, 1));
    assert_eq!(terrain.native_cell_coord(dummy), (8, 9));
    assert_eq!(terrain.native_cell_flags(real), 0xa401_1380);
    assert_eq!(terrain.native_cell_flags(dummy), 0xb502_0380);
    assert_eq!(terrain.shared_cell_dummy().overlay_identity_state().1, 17);
    assert_eq!(terrain.native_cell_anchor(real), Some(dummy));
    assert_eq!(terrain.native_cell_anchor(dummy), Some(real));
    assert_eq!(
        terrain.shared_cell_dummy().snapshot().coord,
        (8, 9),
        "retained reads do not repeat lookup"
    );
}

#[test]
fn bridge_publication_overlapping_mark_preserves_literal_anchor_pointer() {
    let mut terrain = crate::map::resolved_terrain::test_grid(7, 7, make_test_cell);
    let first = terrain.native_cell_identity((3, 3));
    let second = terrain.native_cell_identity((4, 3));
    terrain.apply_runtime_bridge_mark_stamp(
        BridgeFlagStamp::new((3, 3), 6, true),
        BridgeStampFamily::Nesw,
    );
    assert_eq!(terrain.native_cell_anchor(second), Some(first));
    terrain.apply_runtime_bridge_mark_stamp(
        BridgeFlagStamp::new((4, 3), 6, true),
        BridgeStampFamily::Nesw,
    );
    assert_eq!(
        terrain
            .cell(4, 3)
            .unwrap()
            .bridge_facts
            .anchor
            .unwrap()
            .anchor,
        (4, 3)
    );
    assert_eq!(
        terrain.native_cell_anchor(second),
        Some(first),
        "native anchor slot preserves +2C despite its derived self relation"
    );
    terrain.apply_runtime_bridge_mark_stamp(
        BridgeFlagStamp::new((4, 3), 6, false),
        BridgeStampFamily::Nesw,
    );
    assert_eq!(terrain.native_cell_anchor(second), Some(first));
    assert_eq!(
        terrain.native_cell_anchor(first),
        None,
        "F1 clears its literal pointer"
    );
    let retained = DynamicTerrainCellState::capture(terrain.cell(4, 3).unwrap());
    let bytes = save_codec::serialize(&retained).unwrap();
    let restored: DynamicTerrainCellState = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(restored, retained);
}

#[test]
fn bridge_publication_dummy_masked_writers_and_resize_preserve_native_residue() {
    let dummy = SharedCellDummy::fresh();
    dummy.write_raw_flags(0xff81_ffff);
    dummy.set_bridge_flags_0x1180(0);
    assert_eq!(
        dummy.raw_flags(),
        0xff81_ffff & !MODELED_CELLCLASS_BRIDGE_FLAG_MASK
    );
    dummy.test_set_retained_bridge_flags(0);
    assert_eq!(
        dummy.raw_flags(),
        0xff81_ffff & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK
    );
    dummy.write_native_anchor(Some(NativeCellIdentity::Real(13)));
    dummy.write_overlay_identity_state(24, 15);
    dummy.stamp_coord(9, -4);
    let prepared = SharedCellDummy::fresh();
    prepared.adopt_prepared_load_state(&dummy);
    assert_eq!(prepared.raw_flags(), dummy.raw_flags());
    assert_eq!(prepared.native_anchor(), dummy.native_anchor());
    assert_eq!(prepared.overlay_identity_state(), (24, 15));
    dummy.reconstruct_for_map_resize();
    assert_eq!(
        dummy.raw_flags(),
        0xff80_0000,
        "original47BCE1 constructor AND"
    );
    assert_eq!(dummy.native_anchor(), None);
    assert_eq!(dummy.overlay_identity_state(), (-1, 0));
    assert_eq!(dummy.snapshot().coord, (0, 0));
}

#[test]
fn large_map_production_diamond_lookup_and_path_cross_512() {
    let mut map = make_map(Vec::new(), Vec::new(), Vec::new());
    map.header.width = 500;
    map.header.height = 500;
    map.header.local_left = 2;
    map.header.local_top = 4;
    map.header.local_width = 496;
    map.header.local_height = 492;
    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let (grid, stats) = build_production_grid_without_theater(&map, &mut cache);
    assert_eq!((grid.width(), grid.height()), (1000, 1000));
    assert_eq!(grid.iter().count(), 499_500);
    assert_eq!(stats.fill_calls, 499_500);
    let first = grid.native_fixed_cell_index(700, 600).unwrap();
    let second = grid.native_fixed_cell_index(188, 601).unwrap();
    assert_ne!(
        crate::sim::wave::WaveRecordedCell::real(700, 600).identity,
        crate::sim::wave::WaveRecordedCell::real(188, 601).identity
    );
    assert_ne!(
        first, second,
        "a high coordinate must not alias the old 512 stride"
    );
    assert_eq!((grid.cells()[first].rx, grid.cells()[first].ry), (700, 600));
    let path_grid = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&grid);
    let route = crate::sim::pathfinding::find_path(&path_grid, (700, 600), (740, 640)).unwrap();
    assert_eq!(route.last(), Some(&(740, 640)));
    map.header.width = 128;
    map.header.height = 128;
    let (retail, _) = build_production_grid_without_theater(&map, &mut cache);
    assert_eq!(retail.cell_grid(), crate::map::cell_index::CellGrid::Retail);
    assert_eq!(
        retail.native_fixed_cell_index(712, 99),
        retail.native_fixed_cell_index(200, 100)
    );
    assert!(retail.native_fixed_cell_index(200, 100).is_some());
    assert_eq!((grid.cells()[first].rx, grid.cells()[first].ry), (700, 600));
}
