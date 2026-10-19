use super::build::{
    TerrainBuildProjection, TileMetadata, apply_canonical_land_to_metadata,
    apply_land_type_semantics, apply_selected_presentation_metadata,
    canonical_ramp_from_slope_type, classify_overlay_effects, cliff_back_normal_reclass_applies,
    height_in_pixels_from_tmp, materialize_map_load_cells, merge_tmp_file_metadata,
    merge_tmp_metadata, metadata_from_set_name, ordinary_variant_selection_enabled,
    yr_cell_land_type_from_tmp,
};
use super::cliffs::DestroyableCliffFamily;
use super::tubes::{
    AUTO_TUBE_DIRECTIONS, auto_tube_direction_for_tile, build_auto_low_bridge_tubes,
};
use super::*;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::mix_hash::mix_hash;
use crate::assets::tmp_file::TmpTile;
use crate::map::overlay::{OverlayDataPack, TerrainObject};
use crate::map::tube_facts::TubeSource;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{TerrainClass, TerrainRules};
use crate::sim::rng::SimRng;
use crate::util::save_codec;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct Gsi0404AssetDirectory(PathBuf);

impl Gsi0404AssetDirectory {
    fn new() -> Self {
        static NEXT_DIRECTORY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vera20k-gsi-04-04-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create GSI-04.04 asset directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).expect("write GSI-04.04 asset fixture");
    }
}

impl Drop for Gsi0404AssetDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gsi_04_04_mix_bytes(entry_name: &str, body: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend_from_slice(&mix_hash(entry_name).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend_from_slice(body);
    data
}

fn gsi_04_04_asset_manager_with_loose_tmp(
    tmp_filename: &str,
    tmp_bytes: &[u8],
) -> (Gsi0404AssetDirectory, AssetManager) {
    let directory = Gsi0404AssetDirectory::new();
    let empty_mix = gsi_04_04_mix_bytes("gsi0404.bin", b"fixture");
    for name in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
    ] {
        directory.write(name, &empty_mix);
    }
    directory.write(tmp_filename, tmp_bytes);
    let manager = AssetManager::new(
        directory.path(),
        MediaArchiveMode::Numbered { media_index: 2 },
    )
    .expect("construct synthetic retail archive stack");
    (directory, manager)
}

fn gsi_04_04_sparse_tmp_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&88u32.to_le_bytes());
    data.extend_from_slice(&45u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

fn gsi_04_02_last_tiles_tmp_bytes(
    terrain_type: u8,
    radar_left: [u8; 3],
    radar_right: [u8; 3],
) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&4u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.extend_from_slice(&[0u8; 40]);
    data.push(3);
    data.push(terrain_type);
    data.push(0);
    data.extend_from_slice(&radar_left);
    data.extend_from_slice(&radar_right);
    data.extend_from_slice(&[0u8; 3]);
    data.extend(1u8..=16);
    data
}

fn gsi_04_02_last_tiles_theater() -> TheaterData {
    synthetic_lat_theater_from_ini(
        b"[General]\nRoughTile=2\nClearToRoughLat=3\n\
          [TileSet0000]\nTilesInSet=3\nFileName=base\nSetName=Clear\n\
          [TileSet0001]\nTilesInSet=5\nLastTilesInSet=2\nFileName=old\nSetName=Clear\n\
          [TileSet0002]\nTilesInSet=1\nFileName=rough\nSetName=Rough\n\
          [TileSet0003]\nTilesInSet=16\nFileName=lat\nSetName=Rough LAT\n",
    )
}

fn gsi_04_02_asset_manager_with_loose_tmps(
    files: &[(&str, &[u8])],
) -> (Gsi0404AssetDirectory, AssetManager) {
    let directory = Gsi0404AssetDirectory::new();
    let empty_mix = gsi_04_04_mix_bytes("gsi0402.bin", b"fixture");
    for name in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
    ] {
        directory.write(name, &empty_mix);
    }
    for &(name, bytes) in files {
        directory.write(name, bytes);
    }
    let manager = AssetManager::new(
        directory.path(),
        MediaArchiveMode::Numbered { media_index: 2 },
    )
    .expect("construct compatibility-translation asset stack");
    (directory, manager)
}

fn make_map(
    cells: Vec<MapCell>,
    overlays: Vec<OverlayEntry>,
    terrain_objects: Vec<TerrainObject>,
) -> MapFile {
    MapFile {
        header: crate::map::map_file::MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width: 4,
            height: 4,
            local_left: 0,
            local_top: 0,
            local_width: 4,
            local_height: 4,
        },
        basic: crate::map::basic::BasicSection::default(),
        ranking: crate::map::ranking::RankingSection::default(),
        preview: crate::map::preview::PreviewSection::default(),
        cells,
        iso_map_pack_lookups: Vec::new(),
        entities: Vec::new(),
        overlays,
        authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
        smudges: Vec::new(),
        terrain_objects,
        waypoints: HashMap::new(),
        cell_tags: HashMap::new(),
        tags: HashMap::new(),
        triggers: HashMap::new(),
        events: HashMap::new(),
        actions: HashMap::new(),
        local_variables: HashMap::new(),
        trigger_graph: crate::map::trigger_graph::TriggerGraph::default(),
        special_flags: crate::map::basic::SpecialFlagsSection::default(),
        ini: IniFile::from_str(""),
    }
}

#[test]
fn scenario_land_speed_rows_survive_nonempty_and_empty_grid_construction() {
    // Actual startup/Battle/map rule layering supplies the sole speed-row
    // authority. The grid must retain it even without a bridge catalog.
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    for cells in [
        Vec::new(),
        vec![MapCell {
            rx: 0,
            ry: 0,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
    ] {
        let map = make_map(cells, Vec::new(), Vec::new());
        let grid = ResolvedTerrainGrid::build(
            &map,
            None,
            None,
            Some(&retail.rules.terrain_rules),
            None,
            false,
            0,
        );
        assert_eq!(grid.cells.is_empty(), map.cells.is_empty());
        assert!(grid.bridge_recalc_catalog.is_none());
        for land in LandType::ALL {
            let row = retail
                .rules
                .terrain_rules
                .semantics_for_land_type(land.as_index())
                .expect("retail authors every land row");
            assert_eq!(
                grid.land_speed_costs(i32::from(land.as_index())),
                row.speed_costs
            );
        }
    }
}

/// The production builder binds the scenario registry's `Wall=` and
/// `Crushable=` for every overlay identity, on empty and nonempty grids, so
/// `0x004834A0` reads retail sandbags as a crushable wall.
#[test]
fn scenario_overlay_wall_flags_bind_with_the_grid() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&*retail.fixed_art));
    assert!(!registry.is_empty());
    let sandbags = registry.id_for_name("GASAND").expect("[GASAND]");
    for cells in [
        Vec::new(),
        vec![MapCell {
            rx: 0,
            ry: 0,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
    ] {
        let map = make_map(cells, Vec::new(), Vec::new());
        let grid = ResolvedTerrainGrid::build(
            &map,
            None,
            None,
            Some(&retail.rules.terrain_rules),
            Some(&registry),
            false,
            0,
        );
        for id in 0..=u8::MAX {
            assert_eq!(
                grid.overlay_passability(i32::from(id)),
                registry.flags(id).map(|flags| OverlayPassability {
                    wall: flags.wall,
                    crushable: flags.crushable,
                }),
                "overlay {id}"
            );
        }
        assert_eq!(grid.overlay_passability(-1), None);
        assert_eq!(
            grid.overlay_passability(i32::from(sandbags)),
            Some(OverlayPassability {
                wall: true,
                crushable: true,
            })
        );
    }
}

/// The retail Clear test cells and land rows carry the `[Clear]` row a
/// retail Battle scenario's rules give the production land reader.
#[test]
fn retail_clear_test_rows_are_the_retail_scenario_clear_row() {
    let clear = |rules: &TerrainRules| {
        rules
            .semantics_for_land_type(LandType::Clear.as_index())
            .expect("Clear row")
            .speed_costs
    };
    assert_eq!(
        clear(&test_retail_clear_rules()),
        TEST_RETAIL_CLEAR_SPEED_COSTS
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert_eq!(
        clear(&retail.rules.terrain_rules),
        TEST_RETAIL_CLEAR_SPEED_COSTS
    );
}

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

    let mut oversized = make_map(Vec::new(), Vec::new(), Vec::new());
    oversized.header.width = N + 1;
    oversized.header.height = M;
    let (rejected, rejected_stats) = build_production_grid_without_theater(&oversized, &mut cache);
    assert_eq!((rejected.width(), rejected.height()), (0, 0));
    assert!(rejected.cells.is_empty());
    assert_eq!(rejected.iter().count(), 0);
    assert!(
        rejected
            .native_allocated
            .as_ref()
            .is_some_and(Vec::is_empty)
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

fn synthetic_theater_with_wood_bridge_set() -> TheaterData {
    let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=20\nFileName=wood\nSetName=Wood Bridge\n";
    let lookup = crate::map::theater::parse_tileset_ini(ini, "tem").unwrap();
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: Vec::new(),
        bridge_set: None,
        wood_bridge_set: Some(1),
        slope_set_pieces: None,
        bridge_middle_1: None,
        bridge_middle_2: None,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
        rmg_tiles: crate::map::theater::RmgTileKeys::default(),
    }
}

fn synthetic_theater_from_ini(ini: &[u8]) -> TheaterData {
    let lookup = crate::map::theater::parse_tileset_ini(ini, "tem").unwrap();
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: ini.to_vec(),
        bridge_set: None,
        wood_bridge_set: None,
        slope_set_pieces: None,
        bridge_middle_1: None,
        bridge_middle_2: None,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
        rmg_tiles: crate::map::theater::RmgTileKeys::default(),
    }
}

/// [`synthetic_theater_from_ini`] with the RMG/LAT globals theater init
/// resolves from its `[General]` section.
fn synthetic_lat_theater_from_ini(ini: &[u8]) -> TheaterData {
    let mut theater = synthetic_theater_from_ini(ini);
    let parsed = IniFile::from_bytes(ini).expect("synthetic theater INI parses");
    theater.rmg_tiles = crate::map::theater::resolve_rmg_tile_keys(
        &theater.lookup,
        parsed.section("General"),
        None,
    );
    theater
}

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
    let wet = gsi_04_02_last_tiles_tmp_bytes(2, [1, 2, 3], [4, 5, 6]);
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
        assert_eq!(grid.cell(3, 3).unwrap().is_water, semantic_water);
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
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        assert_eq!(
            runtime.simulation.projectiles.get(100).is_none(),
            admit,
            "tile{tile}: semantic water is not the native Level predicate"
        );
    }
}

fn synthetic_automatic_shell_theater() -> TheaterData {
    let mut theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=5\nFileName=tunnel\nSetName=Tunnels\n\n\
          [TileSet0001]\nTilesInSet=5\nFileName=track\nSetName=Track Tunnels\n\n\
          [TileSet0002]\nTilesInSet=5\nFileName=dirt\nSetName=Dirt Tunnels\n\n\
          [TileSet0003]\nTilesInSet=5\nFileName=dtunn\nSetName=Dirt Track Tunnels\n",
    );
    theater.tunnels = Some(0);
    theater.track_tunnels = Some(1);
    theater.dirt_tunnels = Some(2);
    theater.dirt_track_tunnels = Some(3);
    theater.automatic_tube_bases = [0, 5, 10, 15];
    theater
}

fn make_test_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    test_flat_cell(rx, ry)
}

#[derive(Default)]
struct LoadRecalcTestEffects {
    tube_requests: Vec<AutomaticTubeRequest>,
    tube_allocation: Option<AutomaticTubeAllocation>,
    requests: Vec<TerrainTileAnimation>,
    fail: bool,
}

impl LoadCellRecalcEffects for LoadRecalcTestEffects {
    type Error = &'static str;

    fn allocate_automatic_tube(
        &mut self,
        request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error> {
        self.tube_requests.push(request);
        Ok(self
            .tube_allocation
            .unwrap_or(AutomaticTubeAllocation::AllocationNull))
    }

    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error> {
        self.requests.push(request.clone());
        if self.fail {
            Err("fixture failure")
        } else {
            Ok(())
        }
    }
}

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
    cache.complete_theater_registry_load(Some(5), None);
    let mut deterministic_main = || 0;
    let mut fill = |_low, _high| 0;
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
    cache.complete_theater_registry_load(Some(5), None);
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
    cache.complete_theater_registry_load(Some(5), None);
    let mut forbidden_main = || panic!("Fill and misses do not draw Main");
    let mut selector = cache.begin_load(&mut forbidden_main);
    let mut fill = |_low, _high| 0;

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
fn gsi_04_01_resize_clears_bridge_bits_without_replacing_dummy_identity() {
    let dummy = SharedCellDummy::fresh();
    let retained = dummy.clone();
    dummy.stamp_coord(-7, 11);
    dummy.set_level_slope(-3, 9);
    dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_ANCHOR_SELF);
    assert_eq!(
        dummy.bridge_flags_0x1180(),
        BRIDGE_FLAG_ANCHOR_SELF,
        "the promoted live anchor bit is represented independently"
    );

    dummy.reconstruct_for_map_resize();

    assert!(dummy.same_identity(&retained));
    assert_eq!(
        dummy.snapshot(),
        SharedCellDummySnapshot {
            coord: (0, 0),
            level: 0,
            slope_type: 0,
            bridge_flags_0x1180: 0,
        }
    );
}

#[test]
fn gsi_04_01_isomap_misses_stamp_raw_coord_without_payload_leak() {
    let mut map = make_map(
        vec![
            MapCell {
                rx: 2,
                ry: 1,
                tile_index: 41,
                sub_tile: 4,
                z: 5,
            },
            MapCell {
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
    terrain.write_native_cell_state(dummy, 17);
    assert_eq!(terrain.native_cell_identity((8, 9)), dummy);
    assert_eq!(terrain.native_cell_coord(real), (1, 1));
    assert_eq!(terrain.native_cell_coord(dummy), (8, 9));
    assert_eq!(terrain.native_cell_flags(real), 0xa401_1380);
    assert_eq!(terrain.native_cell_flags(dummy), 0xb502_0380);
    assert_eq!(terrain.native_cell_state(dummy), 17);
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
fn authored_fill_defers_all_executable_projection() {
    let mut theater = synthetic_theater_from_ini(
        b"[General]\nBridgeSet=0\nBridgeMiddle1=1\nBridgeMiddle2=5\n\
          [TileSet0000]\nTilesInSet=8\nFileName=tunnel\nSetName=Tunnel FX\n\
          [Tunnel FX]\nTile01Anim=TUNNELFX\nTile01AttachesTo=0\n",
    );
    theater.bridge_set = Some(0);
    theater.bridge_middle_1 = Some(1);
    theater.bridge_middle_2 = Some(5);
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
            None,
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
    assert_eq!(eager_cell.bridge_facts.state_byte, 99);
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
    assert_eq!(grid.cell(2, 1).unwrap().bridge_facts.overlay_id, Some(0x18));
    assert_eq!(grid.cell(1, 2).unwrap().bridge_facts.overlay_id, Some(0x18));
}

#[test]
fn gsi_04_12_generated_materialized_overlays_never_replay_fixed_map_mark() {
    let map = make_map(
        vec![MapCell {
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
    assert_eq!(
        generated.cell(2, 2).unwrap().bridge_facts.overlay_id,
        Some(0x18),
        "source marking suppresses only Mark, not the materialized overlay"
    );
    assert_eq!(
        generated.cell(1, 1).unwrap().bridge_facts.overlay_id,
        Some(0x5C),
        "the generated low-deck rectangle remains present"
    );
}

#[test]
fn gsi_04_03a_level_prefill_wraps_before_absolute_last_isomap_overwrite() {
    let mut map = make_map(
        vec![
            MapCell {
                rx: 2,
                ry: 2,
                tile_index: 98,
                sub_tile: 6,
                z: 5,
            },
            MapCell {
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
    assert_eq!(cache.cached_clear_tile_base(), 0);
    assert_eq!(cache.cached_water_set_base(), 0);

    // Stock RMG preview A publishes both resolved bases for the following
    // ordinary load B. Repeating preview A preserves the same values.
    cache.complete_theater_registry_load(Some(37), Some(50));
    assert_eq!(cache.cached_clear_tile_base(), 37);
    assert_eq!(cache.cached_water_set_base(), 50);
    cache.complete_theater_registry_load(Some(37), Some(50));
    assert_eq!(cache.cached_water_set_base(), 50);

    let seed = 0xA55A_1234;
    let mut expected_scenario = SimRng::new(seed);
    let expected_tiles: Vec<_> = (0..10)
        .map(|_| 50 + expected_scenario.next_range_u32_inclusive(0, 3) as i32)
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

    // A registry with neither role publishes native reset sentinels only
    // after its own Fill, for use by the following load.
    cache.complete_theater_registry_load(None, None);
    assert_eq!(cache.cached_clear_tile_base(), -1);
    assert_eq!(cache.cached_water_set_base(), -1);
}

#[test]
fn gsi_04_02_clear_and_unknown_use_prior_cached_clear_and_equal_scenario_bounds() {
    for fill in ["Clear", "unknown"] {
        let mut map = make_map(Vec::new(), Vec::new(), Vec::new());
        map.header.width = 3;
        map.header.height = 2;
        map.header.fill = fill.to_string();

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        // The already parsed current theater may resolve ClearTile=0, but
        // Fill snapshots the prior process-global value (37).
        cache.complete_theater_registry_load(Some(37), Some(50));
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
                .all(|cell| { cell.tile_index == 37 && cell.sub_tile == 0 && cell.z == 0 })
        );
        assert_eq!(scenario_calls, 10);
        assert_eq!(scenario_rng.logical_state(), scenario_before);
        assert_eq!(selector.map_fill_scenario_advance_count(), 0);
        assert_eq!(selector.raw_draw_count(), 0);
    }
}

#[test]
fn gsi_04_02_production_build_prefills_before_selector_and_skips_outside_slots() {
    let mut map = make_map(
        vec![MapCell {
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
    // Preview A published 50, while this already-parsed ordinary theater B
    // resolves 1. B's Fill must retain A until B publishes afterward.
    theater.rmg_tiles.water_set = Some(1);

    let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    cache.complete_theater_registry_load(Some(37), Some(50));
    let seed = 0xCAFE_BABE;
    let mut scenario_rng = SimRng::new(seed);
    let mut expected_scenario = scenario_rng.clone();
    let expected_first = 50 + expected_scenario.next_range_u32_inclusive(0, 3) as i32;
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

    // The production orchestrator publishes the current theater only
    // after Fill and variant resolution have released their snapshots.
    cache.complete_theater_registry_load(theater.rmg_tiles.clear_tile, theater.rmg_tiles.water_set);
    assert_eq!(cache.cached_clear_tile_base(), 0);
    assert_eq!(cache.cached_water_set_base(), 1);
}

#[test]
fn deploy_tube_neighborhood_matches_original_cell_body() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_simple_deploy.json",
    ))
    .unwrap();
    let fixture = &corpus["tube_admission"];
    for row in fixture["rows"].as_array().unwrap() {
        let mask = row["mask"].as_u64().unwrap();
        let mut cells: Vec<_> = (0..16)
            .flat_map(|y| (0..16).map(move |x| make_test_cell(x, y)))
            .collect();
        for (bit, coord) in fixture["mask_cells"].as_array().unwrap().iter().enumerate() {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let x = coord[0].as_u64().unwrap() as usize;
            let y = coord[1].as_u64().unwrap() as usize;
            cells[y * 16 + x].tube_index = Some(TubeId(0));
            cells[y * 16 + x].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        }
        let tube = TubeFact {
            entry: (10, 10),
            exit: (14, 14),
            direction: 0,
            path_steps: Vec::new(),
            source: TubeSource::AutoLowBridge,
        };
        let grid = ResolvedTerrainGrid::from_cells_with_tubes(16, 16, cells, vec![tube]);
        assert_eq!(
            grid.is_tube_deploy_neighborhood((10, 10)),
            row["refused"].as_bool().unwrap(),
            "mask {mask}"
        );
    }
}

#[test]
/// `CellClass::IsTubeCell` 0x00484AB0 requires BOTH a tube index inside
/// `[0, g_TubeCount)` and `cell+0xEC` (LandType) == 10, but
/// `CellClass::GetTubeAtCell` 0x00484F20 tests only the index and hands
/// back the record regardless of land type. The asymmetry is deliberate in
/// the binary and load-bearing for tube exits, whose facing is read from
/// the record after the cell has already been left behind. Keep both sides
/// of it.
fn tube_at_cell_ignores_land_type_while_is_tube_cell_requires_it() {
    let mut cells = vec![make_test_cell(0, 0), make_test_cell(1, 0)];
    cells[0].tube_index = Some(TubeId(0));
    cells[0].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
    cells[1].tube_index = Some(TubeId(0));
    // Land type deliberately NOT tunnel.
    let tube = TubeFact {
        entry: (0, 0),
        exit: (1, 0),
        direction: 2,
        path_steps: Vec::new(),
        source: TubeSource::AutoLowBridge,
    };
    let grid = ResolvedTerrainGrid::from_cells_with_tubes(2, 1, cells, vec![tube]);

    assert!(grid.cell(0, 0).expect("cell").is_low_bridge_tube_cell());
    assert!(
        !grid.cell(1, 0).expect("cell").is_low_bridge_tube_cell(),
        "0x00484AB0 requires LandType 10"
    );
    assert!(
        grid.tube_at_cell(1, 0).is_some(),
        "0x00484F20 applies no LandType test"
    );
}

#[test]
fn automatic_shell_directions_are_indexed_by_tile_offset_for_every_family() {
    let theater = synthetic_automatic_shell_theater();
    let mut cells = Vec::new();
    for (rx, set_start) in [0i32, 5, 10, 15].into_iter().enumerate() {
        for offset in 0..4i32 {
            let mut cell = make_test_cell((rx * 4 + offset as usize) as u16, 0);
            cell.final_tile_index = set_start + offset;
            cell.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
            cells.push(cell);
        }
    }
    let mut tubes = Vec::new();

    build_auto_low_bridge_tubes(&mut cells, 16, 1, Some(&theater), &mut tubes);

    assert_eq!(tubes.len(), 16);
    for (index, tube) in tubes.iter().enumerate() {
        let expected_direction = i32::from(AUTO_TUBE_DIRECTIONS[index % 4]);
        let coord = (index as u16, 0);
        assert_eq!(tube.entry, coord);
        assert_eq!(tube.exit, coord);
        assert_eq!(tube.direction, expected_direction);
        assert!(tube.path_steps.is_empty());
        assert_eq!(tube.source, TubeSource::AutoLowBridge);
        assert_eq!(cells[index].tube_index, Some(TubeId(index as u16)));
    }
}

#[test]
fn automatic_shell_predicate_rejects_each_native_boundary_failure() {
    let theater = synthetic_automatic_shell_theater();
    assert_eq!(yr_cell_land_type_from_tmp(5), YR_CELL_LAND_TUNNEL);
    assert_ne!(yr_cell_land_type_from_tmp(4), YR_CELL_LAND_TUNNEL);
    assert_eq!(auto_tube_direction_for_tile(0, Some(&theater)), Some(2));
    assert_eq!(auto_tube_direction_for_tile(3, Some(&theater)), Some(0));
    assert_eq!(
        auto_tube_direction_for_tile(4, Some(&theater)),
        None,
        "the fifth tile of the configured band is excluded"
    );
    assert_eq!(auto_tube_direction_for_tile(0, None), None);

    let mut no_families = synthetic_automatic_shell_theater();
    no_families.tunnels = None;
    no_families.track_tunnels = None;
    no_families.dirt_tunnels = None;
    no_families.dirt_track_tunnels = None;
    no_families.automatic_tube_bases = [-1; 4];
    assert_eq!(
        auto_tube_direction_for_tile(0, Some(&no_families)),
        Some(4),
        "missing native bases remain signed -1 and admit tile zero as ordinal one"
    );

    let mut zero_length = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=0\nFileName=empty\nSetName=Empty Tunnel\n\n\
          [TileSet0001]\nTilesInSet=5\nFileName=next\nSetName=Following Set\n",
    );
    zero_length.tunnels = Some(0);
    zero_length.automatic_tube_bases[0] = 0;
    for (tile_id, direction) in AUTO_TUBE_DIRECTIONS.into_iter().enumerate() {
        assert_eq!(
            auto_tube_direction_for_tile(tile_id as i32, Some(&zero_length)),
            Some(direction),
            "native compares the zero-length set's cumulative base without its count"
        );
    }
    assert_eq!(auto_tube_direction_for_tile(4, Some(&zero_length)), None);

    let mut wrong_land = make_test_cell(0, 0);
    wrong_land.final_tile_index = 0;
    wrong_land.yr_cell_land_type = LandType::Road.as_index();
    let mut existing = make_test_cell(1, 0);
    existing.final_tile_index = 0;
    existing.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
    existing.tube_index = Some(TubeId(0));
    let mut fifth_tile = make_test_cell(2, 0);
    fifth_tile.final_tile_index = 4;
    fifth_tile.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
    let mut cells = vec![wrong_land, existing, fifth_tile];
    let mut tubes = vec![TubeFact::explicit((1, 0), (1, 0), 2, Vec::new())];

    build_auto_low_bridge_tubes(&mut cells, 3, 1, Some(&theater), &mut tubes);

    assert_eq!(tubes.len(), 1);
    assert_eq!(cells[0].tube_index, None);
    assert_eq!(cells[1].tube_index, Some(TubeId(0)));
    assert_eq!(cells[2].tube_index, None);
}

#[test]
fn authored_recalc_constructs_and_retains_one_automatic_tube_inline() {
    let mut theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=tunnel\nSetName=Tunnel\n",
    );
    theater.automatic_tube_bases = [0, -1, -1, -1];
    let tmp = gsi_04_02_last_tiles_tmp_bytes(5, [1, 2, 3], [4, 5, 6]);
    let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("tunnel01.tem", &tmp);
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
    let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
    let map = make_map(Vec::new(), Vec::new(), Vec::new());
    let mut grid =
        ResolvedTerrainGrid::from_cells(2, 1, vec![make_test_cell(0, 0), make_test_cell(1, 0)]);
    let mut state = LoadCellRecalcState::for_authored_load(
        &map,
        &theater,
        &assets,
        &terrain_rules,
        &registry,
        false,
        0,
        grid.cells.len(),
        PixelConversionBounds::default(),
    );
    let mut effects = LoadRecalcTestEffects {
        tube_allocation: Some(AutomaticTubeAllocation::Allocated {
            native_unique_id: 1_010_001,
            registry_append_allowed: true,
        }),
        ..LoadRecalcTestEffects::default()
    };

    grid.recalc_authored_load_cell(&mut state, 1, FinalizedOverlayCell::default(), &mut effects)
        .expect("first authored tunnel Recalc");

    assert_eq!(
        effects.tube_requests,
        vec![AutomaticTubeRequest {
            cell: (1, 0),
            direction: 2,
        }]
    );
    assert_eq!(grid.cells[1].tube_index, Some(TubeId(0)));
    assert_eq!(grid.tube_native_id(TubeId(0)), Some(1_010_001));
    assert_eq!(
        grid.tube_at_cell(1, 0).unwrap().source,
        TubeSource::AutomaticRecalcShell
    );

    grid.recalc_authored_load_cell(&mut state, 1, FinalizedOverlayCell::default(), &mut effects)
        .expect("later authored tunnel Recalc");
    assert_eq!(
        effects.tube_requests.len(),
        1,
        "the published in-range signed index suppresses duplicate construction"
    );
}

#[test]
fn binding_explicit_native_tubes_publishes_identity_and_suppresses_rebind() {
    let mut grid =
        ResolvedTerrainGrid::from_cells(2, 1, vec![make_test_cell(0, 0), make_test_cell(1, 0)]);
    let receipt = crate::map::tubes::NativeMapTubeReceipt {
        entries: vec![crate::map::tubes::ConstructedMapTube {
            fact: TubeFact::explicit((1, 0), (0, 0), 6, vec![2, 4]),
            native_init: crate::map::tubes::TubeNativeInit {
                source_entry_ordinal: 0,
                native_unique_id: 1_010_000,
            },
        }],
    };

    assert_eq!(grid.bind_native_map_tubes(receipt).unwrap(), 1);
    assert_eq!(grid.cells[1].tube_index, Some(TubeId(0)));
    assert_eq!(grid.tube_native_id(TubeId(0)), Some(1_010_000));
    assert_eq!(grid.tube_at_cell(1, 0).unwrap().path_steps, vec![2, 4]);
    assert!(matches!(
        grid.bind_native_map_tubes(crate::map::tubes::NativeMapTubeReceipt::default()),
        Err(NativeTubeRegistryBindError::RegistryAlreadyPopulated)
    ));
}

#[test]
fn direction_8_steps_through_cell_tube() {
    let mut cells = vec![
        make_test_cell(0, 0),
        make_test_cell(1, 0),
        make_test_cell(2, 0),
    ];
    cells[1].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
    cells[1].tube_index = Some(TubeId(0));
    let tube = TubeFact {
        entry: (1, 0),
        exit: (2, 0),
        direction: 2,
        path_steps: Vec::new(),
        source: TubeSource::AutoLowBridge,
    };
    let grid = ResolvedTerrainGrid::from_cells_with_tubes(3, 1, cells, vec![tube]);

    assert_eq!(grid.step_coord_by_direction((1, 0), 8), Some((2, 0)));
    assert_eq!(grid.walk_directions_from((0, 0), &[2, 8]), Some((2, 0)));
}

#[test]
fn direction_8_without_valid_tube_returns_zero_coord() {
    let grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);

    assert_eq!(grid.step_coord_by_direction((0, 0), 8), Some((0, 0)));
}

#[test]
fn invalid_non_8_direction_does_not_wrap() {
    let grid = crate::map::resolved_terrain::test_grid(3, 3, make_test_cell);

    assert_eq!(grid.step_coord_by_direction((1, 1), 9), None);
    assert_eq!(grid.step_coord_by_direction((1, 1), 255), None);
}

#[test]
fn gsi_02_11_clear_fallback_calls_selector_even_with_pristine_only() {
    let map = make_map(
        vec![MapCell {
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
fn test_resolved_grid_preserves_raw_fields_and_fills_clear_cells() {
    let map = make_map(
        vec![MapCell {
            rx: 1,
            ry: 1,
            tile_index: 5,
            sub_tile: 3,
            z: 2,
        }],
        Vec::new(),
        Vec::new(),
    );
    let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 0);
    assert_eq!(grid.width(), 2);
    assert_eq!(grid.height(), 2);

    let cell = grid.cell(1, 1).expect("resolved cell");
    assert_eq!(cell.source_tile_index, 5);
    assert_eq!(cell.source_sub_tile, 3);
    assert_eq!(cell.final_tile_index, 5);
    assert_eq!(cell.final_sub_tile, 3);
    assert_eq!(cell.level, 2);
    assert!(!cell.filled_clear);

    let clear = grid.cell(0, 0).expect("filled clear");
    assert!(clear.filled_clear);
    assert_eq!(clear.final_tile_index, 0);
    assert_eq!(clear.level, 0);
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

    let presentation = crate::map::terrain::build_terrain_grid_from_resolved(&grid, None, None);
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
                rx: 0,
                ry: 0,
                tile_index: 9,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: 10,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 2,
                ry: 0,
                tile_index: 25,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
fn gsi_04_03a_special_terrain_identity_is_not_promoted_to_hard_blocking() {
    let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=10\nFileName=plain\nSetName=Plain Terrain\n";
    let mut theater = synthetic_theater_from_ini(ini);
    theater.cliff_ranges.cliff_set = Some(10);
    let map = make_map(
        vec![MapCell {
            rx: 0,
            ry: 0,
            tile_index: 10,
            sub_tile: 0,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    let cell = grid.cell(0, 0).expect("special-identity cell");

    assert!(theater.cliff_ranges.is_special_terrain_tile(10, 0));
    assert!(!cell.is_cliff_like);
    assert!(!cell.base_ground_walk_blocked);
    assert!(!cell.ground_walk_blocked);
    assert_eq!(cell.terrain_class, TerrainClass::Clear);
}

#[test]
fn shore_set_name_alone_does_not_block_ground() {
    let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=shore\nSetName=Shore Pieces\n";
    let theater = synthetic_theater_from_ini(ini);
    let map = make_map(
        vec![MapCell {
            rx: 0,
            ry: 0,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    let cell = grid.cell(0, 0).expect("shore cell");

    assert!(!cell.is_cliff_like);
    assert!(!cell.base_ground_walk_blocked);
    assert!(!cell.ground_walk_blocked);
    assert_eq!(cell.terrain_class, TerrainClass::Clear);
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
                rx: 5,
                ry: 5,
                tile_index: 5,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 5,
                ry: 4,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 4,
                ry: 5,
                tile_index: 6,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 6,
                ry: 5,
                tile_index: 1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
fn gsi_04_03c_sparse_pristine_subtile_uses_header_height() {
    let tmp = TmpFile {
        template_width: 1,
        template_height: 1,
        tile_width: 60,
        tile_height: 45,
        tiles: vec![None],
    };
    let mut metadata = TileMetadata::default();
    let mut warned = HashSet::new();

    merge_tmp_file_metadata(&mut metadata, &tmp, 0, None, &mut warned);

    assert_eq!(metadata.height_in_pixels, 1);
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
        let mut metadata = metadata_from_set_name(Some("Road"), None);
        let mut warned = HashSet::new();
        merge_tmp_file_metadata(&mut metadata, &tmp, sub_tile, None, &mut warned);

        assert_eq!(metadata.subtile_entry_valid, Some(false));
        assert_eq!(metadata.land_type, LandType::Clear.as_index());
        assert_eq!(metadata.yr_cell_land_type, LandType::Clear.as_index());
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
                rx: 1,
                ry: 0,
                tile_index: theater::NO_TILE,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                rx: 3,
                ry: 0,
                tile_index: theater::NO_TILE,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                rx: 1,
                ry: 1,
                tile_index: 0,
                sub_tile: 1,
                z: 0,
            },
            MapCell {
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
        assert_eq!(cell.yr_cell_land_type, LandType::Clear.as_index());
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

    let with_cliff_back =
        ResolvedTerrainGrid::build(&map, Some(&theater), Some(&assets), None, None, false, 2);
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
fn gsi_04_04_merge_tmp_metadata_maps_raw_rock_to_canonical_land() {
    let mut metadata = TileMetadata::default();
    let tile = TmpTile {
        height: 4,
        terrain_type: 7,
        ramp_type: 2,
        radar_left: [100, 120, 80],
        radar_right: [90, 110, 70],
        pixels: Vec::new(),
        depth: Vec::new(),
        pixel_width: 60,
        pixel_height: 30,
        relative_extra_y: 0,
        offset_x: -5,
        offset_y: -6,
        has_damaged_data: false,
    };
    merge_tmp_metadata(&mut metadata, &tile);
    assert_eq!(metadata.land_type, LandType::Rock.as_index());
    assert_eq!(metadata.slope_type, 2);
    assert_eq!(metadata.template_height, 4);
    assert_eq!(metadata.render_offset_x, -5);
    assert_eq!(metadata.render_offset_y, -6);
    assert!(metadata.has_ramp);
    assert!(metadata.has_tmp_metadata);
    assert_eq!(metadata.radar_left, [100, 120, 80]);
    assert_eq!(metadata.radar_right, [90, 110, 70]);
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
    let mut warned = HashSet::new();
    let mut metadata = TileMetadata::default();
    merge_tmp_file_metadata(&mut metadata, &pristine, 0, None, &mut warned);
    let pristine_metadata = metadata.clone();
    let mut selected_metadata = TileMetadata::default();
    merge_tmp_file_metadata(
        &mut selected_metadata,
        &divergent_selected,
        0,
        None,
        &mut warned,
    );
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
    merge_tmp_file_metadata(
        &mut sparse_selected_metadata,
        &sparse_suffix,
        0,
        None,
        &mut warned,
    );
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

#[test]
fn test_canonical_ramp_detection_only_marks_slope_types_one_to_four() {
    assert_eq!(canonical_ramp_from_slope_type(1), Some(RampDirection::West));
    assert_eq!(
        canonical_ramp_from_slope_type(4),
        Some(RampDirection::South)
    );
    assert_eq!(canonical_ramp_from_slope_type(0), None);
    assert_eq!(canonical_ramp_from_slope_type(7), None);
}

#[test]
fn test_bridge_overlay_creates_upper_layer_without_ground_block() {
    // BRIDGE1 is hardcoded at overlay index 24 in the original engine.
    // Build a registry large enough so index 24 resolves to "BRIDGE1".
    let mut ini_str = String::from("[OverlayTypes]\n");
    for i in 0..24 {
        ini_str.push_str(&format!("{i}=FILLER{i}\n"));
    }
    ini_str.push_str("24=BRIDGE1\n");
    let ini = IniFile::from_str(&ini_str);
    let reg = OverlayTypeRegistry::from_ini(&ini, None);
    let effects = classify_overlay_effects(
        Some(&vec![&OverlayEntry {
            rx: 0,
            ry: 0,
            overlay_id: 24,
            frame: 0,
        }]),
        Some(&reg),
        3,
        0,
    );
    assert!(effects.has_bridge_deck);
    assert!(!effects.overlay_blocks);
    assert_eq!(
        effects
            .bridge_layer
            .as_ref()
            .map(|b| b.overlay_name.as_str()),
        Some("BRIDGE1")
    );
    // BRIDGE1 = EastWest high bridge: deck_level = ground(3) + HighBridgeHeight(4) = 7.
    assert_eq!(effects.bridge_layer.as_ref().map(|b| b.deck_level), Some(7));
    assert_eq!(
        effects.bridge_layer.as_ref().map(|b| b.direction),
        Some(BridgeDirection::EastWest)
    );
}

#[test]
fn bridge_overlay_stamps_anchor_structural_and_transition_flags_without_theater() {
    let mut ini_str = String::from("[OverlayTypes]\n");
    for i in 0..24 {
        ini_str.push_str(&format!("{i}=FILLER{i}\n"));
    }
    ini_str.push_str("24=BRIDGE1\n");
    let reg = OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini_str), None);
    let map = make_map(
        vec![MapCell {
            rx: 5,
            ry: 5,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        vec![OverlayEntry {
            rx: 5,
            ry: 5,
            overlay_id: 0x18,
            frame: 0,
        }],
        Vec::new(),
    );

    let grid = ResolvedTerrainGrid::build(&map, None, None, None, Some(&reg), false, 0);
    let facts = grid.cell(5, 5).expect("bridge cell").bridge_facts;

    assert!(facts.has_flag(BRIDGE_FLAG_ANCHOR_SELF));
    assert!(facts.has_flag(BRIDGE_FLAG_STRUCTURAL));
    assert!(facts.has_flag(crate::map::bridge_facts::BRIDGE_FLAG_TRANSITION));
}

#[test]
fn gsi_04_04_overlay_zone_type_priority_matches_recalc_zone_type_on_load() {
    let ini = IniFile::from_str(
        "\
[OverlayTypes]
0=SANDBAG
1=HARDWALL
2=ROCKOVL
3=RUBBLE
[Clear]
Wheel=100%
[Rock]
Wheel=0%
[SANDBAG]
Crushable=yes
Wall=yes
Land=Clear
[HARDWALL]
Wall=yes
Land=Clear
[ROCKOVL]
Land=Rock
[RUBBLE]
IsRubble=yes
",
    );
    let reg = OverlayTypeRegistry::from_ini(&ini, None);
    let map = make_map(
        vec![
            MapCell {
                rx: 0,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 2,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 3,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
        ],
        vec![
            OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 0,
                frame: 0,
            },
            OverlayEntry {
                rx: 1,
                ry: 0,
                overlay_id: 1,
                frame: 0,
            },
            OverlayEntry {
                rx: 2,
                ry: 0,
                overlay_id: 2,
                frame: 0,
            },
            OverlayEntry {
                rx: 3,
                ry: 0,
                overlay_id: 3,
                frame: 0,
            },
        ],
        (0..4)
            .map(|rx| TerrainObject {
                rx,
                ry: 0,
                name: "DEFAULT_OCCUPATION".to_string(),
            })
            .collect(),
    );

    let grid = ResolvedTerrainGrid::build(&map, None, None, None, Some(&reg), false, 0);

    assert_eq!(grid.cell(0, 0).unwrap().zone_type, zone_class::CRUSHABLE);
    assert!(!grid.cell(0, 0).unwrap().overlay_blocks);
    assert_eq!(grid.cell(1, 0).unwrap().zone_type, zone_class::WALL);
    assert!(grid.cell(1, 0).unwrap().overlay_blocks);
    assert_eq!(grid.cell(2, 0).unwrap().zone_type, zone_class::IMPASSABLE);
    assert!(grid.cell(2, 0).unwrap().overlay_blocks);
    assert_eq!(grid.cell(3, 0).unwrap().zone_type, zone_class::GROUND);
    assert!(!grid.cell(3, 0).unwrap().overlay_blocks);
}

#[test]
fn gsi_04_04_load_selects_current_theater_terrain_occupation() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
[TerrainTypes]
0=TIBTRE03
1=ZEROTREE
[TIBTRE03]
TemperateOccupationBits=4
SnowOccupationBits=7
[ZEROTREE]
TemperateOccupationBits=0
SnowOccupationBits=0
",
    ))
    .expect("terrain rules");
    let mut map = make_map(
        vec![
            MapCell {
                rx: 0,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        vec![
            TerrainObject {
                rx: 0,
                ry: 0,
                name: "TIBTRE03".to_string(),
            },
            TerrainObject {
                rx: 1,
                ry: 0,
                name: "ZEROTREE".to_string(),
            },
        ],
    );

    let temperate = ResolvedTerrainGrid::build_inner(
        &map,
        None,
        None,
        None,
        None,
        Some(&rules.terrain_object_types),
        false,
        0,
        OverlayLoadSource::Authored,
        TerrainBuildProjection::EagerCompatibility,
        None,
        None,
        None,
        PixelConversionBounds::default(),
    );
    let temperate_tree = temperate.cell(0, 0).unwrap();
    assert_eq!(temperate_tree.terrain_object_occupation, Some(4));
    assert!(temperate_tree.terrain_object_blocks);
    assert_eq!(temperate_tree.zone_type, zone_class::BUILDING);
    assert!(!temperate_tree.base_build_blocked);
    let zero_tree = temperate.cell(1, 0).unwrap();
    assert_eq!(zero_tree.terrain_object_occupation, Some(0));
    assert!(!zero_tree.terrain_object_blocks);
    assert_eq!(zero_tree.zone_type, zone_class::BUILDING);

    map.header.theater = "SNOW".to_string();
    let snow = ResolvedTerrainGrid::build_inner(
        &map,
        None,
        None,
        None,
        None,
        Some(&rules.terrain_object_types),
        false,
        0,
        OverlayLoadSource::Authored,
        TerrainBuildProjection::EagerCompatibility,
        None,
        None,
        None,
        PixelConversionBounds::default(),
    );
    let snow_tree = snow.cell(0, 0).unwrap();
    assert_eq!(snow_tree.terrain_object_occupation, Some(7));
    assert!(snow_tree.terrain_object_blocks);
    assert_eq!(snow_tree.zone_type, zone_class::WALL);
}

#[test]
fn gsi_04_04_outside_precedes_overlay_land_and_terrain_occupation() {
    let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
    let outside = !playfield.contains_raised(0, 0, 0, 0);
    assert!(outside);
    assert_eq!(
        recalc_zone_type(
            outside,
            Some(zone_class::CRUSHABLE),
            LandType::Water.as_index(),
            Some(0),
            Some(7),
        ),
        zone_class::OUTSIDE
    );
}

#[test]
fn gsi_04_04_load_land_writer_retains_arbitrary_profiles_and_pristine_base() {
    let ini = IniFile::from_str(
        "\
[OverlayTypes]
0=ROADKEEP
1=ROADRESTORE
2=WALLLAND
3=RAILLAND
4=WATERKEEP
5=ROUGHKEEP
[Road]
Foot=80%
Wheel=55%
[Wall]
Wheel=40%
[Railroad]
Wheel=60%
[Water]
Float=66%
Wheel=44%
[Rough]
Foot=77%
Wheel=33%
[ROADKEEP]
Land=Road
NoUseTileLandType=yes
[ROADRESTORE]
Land=Road
NoUseTileLandType=no
[WALLLAND]
Land=Wall
NoUseTileLandType=no
[RAILLAND]
Land=Railroad
NoUseTileLandType=no
[WATERKEEP]
Land=Water
NoUseTileLandType=yes
[ROUGHKEEP]
Land=Rough
NoUseTileLandType=yes
",
    );
    let registry = OverlayTypeRegistry::from_ini(&ini, None);
    let cells = (0..6)
        .map(|rx| MapCell {
            rx,
            ry: 0,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        })
        .collect();
    let overlays = (0..6)
        .map(|overlay_id| OverlayEntry {
            rx: u16::from(overlay_id),
            ry: 0,
            overlay_id,
            frame: 0,
        })
        .collect();
    let map = make_map(cells, overlays, Vec::new());

    let grid = ResolvedTerrainGrid::build(&map, None, None, None, Some(&registry), false, 0);
    for (rx, overlay_id, land, class) in [
        (0, 0, LandType::Road, TerrainClass::Road),
        (2, 2, LandType::Wall, TerrainClass::Wall),
        (3, 3, LandType::Railroad, TerrainClass::Railroad),
        (4, 4, LandType::Water, TerrainClass::Water),
        (5, 5, LandType::Rough, TerrainClass::Rough),
    ] {
        let cell = grid.cell(rx, 0).expect("retained load cell");
        assert_eq!(cell.base_land_type, LandType::Clear.as_index());
        assert_eq!(cell.land_type, land.as_index(), "overlay {overlay_id}");
        assert_eq!(cell.terrain_class, class, "overlay {overlay_id}");
        assert_eq!(
            cell.speed_costs,
            registry
                .flags(overlay_id)
                .unwrap()
                .land_speed_costs
                .unwrap(),
            "overlay {overlay_id} profile"
        );
        assert_eq!(cell.is_water, land == LandType::Water);
        assert_eq!(cell.is_road, land == LandType::Road);
        assert_eq!(cell.is_rough, land == LandType::Rough);
    }

    let restored = grid.cell(1, 0).expect("ordinary restoration cell");
    assert_eq!(restored.land_type, restored.base_land_type);
    assert_eq!(restored.terrain_class, restored.base_terrain_class);
    assert_eq!(restored.speed_costs, restored.base_speed_costs);
}

#[test]
fn low_bridge_overlay_land_replaces_the_tiles_ground_block_at_load() {
    // Low bridges (LOBRDG01..28 wood, LOBRDB01..28 concrete) are overlays
    // laid straight onto the water tiles they span, with `Land=Road` and
    // `NoUseTileLandType=yes`. `CellClass__RecalcAttributes` @ 0x0047D2B0
    // takes its early branch for exactly that pair and returns before the
    // tile's own subtile land type is read, so nothing of the water
    // survives — LandType is the only land attribute gamemd stores.
    let ini = IniFile::from_str(
        "[OverlayTypes]
0=LOBRDB10
1=WATERKEEP
[Road]
Foot=100%
Track=100%
Wheel=100%
[Water]
Float=100%
[LOBRDB10]
Land=Road
NoUseTileLandType=yes
[WATERKEEP]
Land=Water
NoUseTileLandType=yes
",
    );
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    // The blocked-ness of the replacement row travels with the land.
    assert!(!registry.flags(0).expect("bridge flags").land_ground_blocked);
    assert!(registry.flags(1).expect("water flags").land_ground_blocked);

    // Start from a water tile: blocked to ground, water-classed.
    let mut metadata = TileMetadata {
        has_tmp_metadata: true,
        land_type: LandType::Water.as_index(),
        yr_cell_land_type: LandType::Water.as_index(),
        terrain_class: TerrainClass::Water,
        is_water: true,
        ground_blocked: true,
        ..TileMetadata::default()
    };
    let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
    assert!(base_ground_walk_blocked);

    let overlay = OverlayEntry {
        rx: 0,
        ry: 0,
        overlay_id: 0,
        frame: 1,
    };
    let effects = classify_overlay_effects(Some(&vec![&overlay]), Some(&registry), 0, 0);
    let land = effects.effective_land.expect("bridge retains Land=Road");
    assert_eq!(land, LandType::Road);
    assert!(!effects.effective_land_ground_blocked);

    apply_canonical_land_to_metadata(
        &mut metadata,
        land,
        effects.effective_land_speed_costs,
        effects.effective_land_ground_blocked,
    );

    assert_eq!(metadata.land_type, LandType::Road.as_index());
    assert_eq!(metadata.yr_cell_land_type, LandType::Road.as_index());
    assert!(!metadata.is_water);
    // The regression: `ground_blocked` was the one land-derived field the
    // override skipped, so the deck stayed impassable while every other
    // attribute said Road.
    assert!(!metadata.ground_blocked);
    let ground_walk_blocked =
        (canonical_ramp.is_none() && metadata.ground_blocked) || effects.overlay_blocks;
    assert!(!ground_walk_blocked);
    // The pristine snapshot is untouched, so overlay removal still restores water.
    assert!(base_ground_walk_blocked);

    let theater = synthetic_automatic_shell_theater();
    let mut cell = make_test_cell(0, 0);
    cell.final_tile_index = 0;
    cell.yr_cell_land_type = metadata.yr_cell_land_type;
    let mut cells = vec![cell];
    let mut tubes = Vec::new();
    build_auto_low_bridge_tubes(&mut cells, 1, 1, Some(&theater), &mut tubes);
    assert!(
        tubes.is_empty() && cells[0].tube_index.is_none(),
        "the low Road early branch cannot reach automatic-shell construction"
    );
}

#[test]
fn gsi_04_04_load_tiberium_slope_branches_retain_only_early_copied_land() {
    let ini = IniFile::from_str(
        "\
[OverlayTypes]
0=ORE
1=STOCKORE
2=WALLORE
3=RAILORE
[Tiberium]
Wheel=80%
[ORE]
Tiberium=yes
NoUseTileLandType=no
[STOCKORE]
Tiberium=yes
[WALLORE]
Tiberium=yes
Land=Wall
NoUseTileLandType=no
[RAILORE]
Tiberium=yes
Land=Railroad
NoUseTileLandType=no
",
    );
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    for (overlay_id, land, early_branch) in [
        (0, LandType::Tiberium, false),
        (1, LandType::Tiberium, true),
        (2, LandType::Wall, true),
        (3, LandType::Railroad, true),
    ] {
        let flags = registry.flags(overlay_id).expect("overlay flags");
        assert_eq!(uses_early_recalc_land_branch(flags), early_branch);
        for slope in [0, 1, 4, 5] {
            let overlay = OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id,
                frame: 7,
            };
            let entries = vec![&overlay];
            let effects = classify_overlay_effects(Some(&entries), Some(&registry), 0, slope);
            let clears = clears_tiberium_on_slope(flags, slope);
            assert_eq!(clears, slope != 0 && (early_branch || slope >= 5));
            let retains_current_land = !clears || early_branch;
            assert_eq!(
                effects.effective_land,
                retains_current_land.then_some(land),
                "overlay={overlay_id} slope={slope}"
            );
            assert_eq!(effects.claims_cell_attributes, early_branch);

            let mut current = metadata_from_set_name(Some("Clear"), Some(0));
            if let Some(effective_land) = effects.effective_land {
                apply_canonical_land_to_metadata(
                    &mut current,
                    effective_land,
                    effects.effective_land_speed_costs,
                    effects.effective_land_ground_blocked,
                );
            }
            let expected_land = if retains_current_land {
                land
            } else {
                LandType::Clear
            };
            assert_eq!(current.land_type, expected_land.as_index());
            assert_eq!(current.yr_cell_land_type, expected_land.as_index());
            assert_eq!(current.terrain_class, expected_land.terrain_class());
            assert_eq!(current.is_water, expected_land.is_water());
            assert_eq!(current.is_cliff_like, expected_land.is_cliff_like());
            assert_eq!(current.is_rough, expected_land.is_rough());
            assert_eq!(current.is_road, expected_land.is_road());
            assert_eq!(
                current.speed_costs,
                if retains_current_land {
                    flags.land_speed_costs.unwrap_or_default()
                } else {
                    SpeedCostProfile::default()
                }
            );
            assert_eq!(
                recalc_zone_type(
                    false,
                    effects.overlay_zone_type,
                    current.land_type,
                    current.speed_costs.wheel,
                    None,
                ),
                zone_class::GROUND
            );
            if clears {
                assert_eq!(effects.overlay_zone_type, None);
                assert!(!effects.overlay_blocks);
            }
        }
    }
}

#[test]
fn gsi_04_04_rules_backed_land_type_uses_canonical_tmp_conversion() {
    let terrain_rules =
        TerrainRules::from_ini(&IniFile::from_str("[Rough]\nBuildable=yes\nTrack=75%\n"));
    let mut metadata = metadata_from_set_name(Some("Water"), Some(2));
    let tile = TmpTile {
        height: 0,
        terrain_type: 14,
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
        has_damaged_data: false,
    };
    merge_tmp_metadata(&mut metadata, &tile);
    let mut warned = HashSet::new();
    apply_land_type_semantics(&mut metadata, Some(&terrain_rules), &mut warned);

    assert_eq!(metadata.terrain_class, TerrainClass::Rough);
    assert!(metadata.is_rough);
    assert!(!metadata.is_water);
    assert!(!metadata.ground_blocked);
    assert!(!metadata.build_blocked);
}

#[test]
fn test_unknown_land_type_keeps_tileset_fallback() {
    // Use a LandType byte outside the 0-15 range (all 0-15 are now mapped).
    // Byte 200 is genuinely unknown and should fall back to tileset-name heuristics.
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
    let mut metadata = metadata_from_set_name(Some("Water Cliffs"), Some(5));
    let tile = TmpTile {
        height: 0,
        terrain_type: 200,
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
        has_damaged_data: false,
    };
    merge_tmp_metadata(&mut metadata, &tile);
    let mut warned = HashSet::new();
    apply_land_type_semantics(&mut metadata, Some(&terrain_rules), &mut warned);

    assert_eq!(metadata.terrain_class, TerrainClass::Water);
    assert!(metadata.is_water);
    assert!(metadata.is_cliff_like);
    assert!(metadata.ground_blocked);
    assert_eq!(warned, HashSet::from([200]));
}

#[test]
fn test_tileset_water_fallback_sets_water_land_type() {
    let metadata = metadata_from_set_name(Some("TEMPERATE WATER"), Some(5));
    assert!(metadata.is_water);
    assert_eq!(
        metadata.land_type,
        crate::rules::terrain_rules::LandType::Water.as_index()
    );
}

#[test]
fn test_canonical_ramp_is_ground_passable_but_stays_non_buildable() {
    let map = make_map(
        vec![MapCell {
            rx: 0,
            ry: 0,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str("[Cliff]\nBuildable=no\n"));
    let mut metadata = TileMetadata {
        has_tmp_metadata: true,
        raw_land_type: 15,
        land_type: crate::rules::terrain_rules::tmp_terrain_to_land_type(15).as_index(),
        slope_type: 2,
        terrain_class: TerrainClass::Cliff,
        ground_blocked: true,
        build_blocked: true,
        is_cliff_like: true,
        has_ramp: true,
        ..TileMetadata::default()
    };
    let mut warned = HashSet::new();
    apply_land_type_semantics(&mut metadata, Some(&terrain_rules), &mut warned);

    let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
    assert!(!base_ground_walk_blocked);
    let grid = ResolvedTerrainGrid::from_cells(
        1,
        1,
        vec![ResolvedTerrainCell {
            land_type: metadata.land_type,
            yr_cell_land_type: metadata.yr_cell_land_type,
            slope_type: metadata.slope_type,
            terrain_class: metadata.terrain_class,
            speed_costs: metadata.speed_costs,
            is_cliff_like: true,
            has_ramp: true,
            canonical_ramp,
            base_build_blocked: true,
            ..crate::map::resolved_terrain::test_flat_cell(0, 0)
        }],
    );
    let cell = grid.cell(0, 0).expect("resolved ramp cell");
    assert_eq!(cell.canonical_ramp, Some(RampDirection::North));
    assert!(!cell.ground_walk_blocked);
    assert_eq!(map.header.width, 4);
}

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
        state.lat_config = Some(lat_config.clone());
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
    state.lat_config = Some(lat_config);
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
    state.lat_config = Some(lat::LatConfig::from_tile_ids(
        &crate::map::rmg::tiles::TileIds::from_keys(&theater.rmg_tiles),
    ));
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

include!("recalc_native_tests.rs");
include!("saved_terrain_tests.rs");

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
    assert_eq!(grid.cells[0].bridge_facts.overlay_id, None);
    assert_eq!(grid.cells[0].bridge_facts.state_byte, 0);
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
        center.yr_cell_land_type = land.as_index();
        center.terrain_class = land.terrain_class();
        center.is_road = land == LandType::Road;
        center.base_land_type = land.as_index();
        center.base_yr_cell_land_type = land.as_index();
        center.base_terrain_class = land.terrain_class();
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
    let map = make_map(
        vec![
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                rx: 1,
                ry: 1,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 3,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
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

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 2);
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
    let overlay_ini = IniFile::from_str("[OverlayTypes]\n0=CLAIM\n[CLAIM]\nLand=Wall\n");
    let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
    let map = make_map(
        vec![
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
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

    let mut grid =
        ResolvedTerrainGrid::build(&map, Some(&theater), None, None, Some(&registry), false, 2);
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
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
    let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
    let cell = grid.cell(1, 1).unwrap();
    assert!(
        cell.ground_walk_blocked,
        "Cell at base of cliff should be blocked"
    );
    assert!(
        cell.is_cliff_like,
        "Cell at base of cliff should be cliff-like"
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
        cell.yr_cell_land_type,
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
                rx: 1,
                ry: 0,
                tile_index: theater::NO_TILE,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
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
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 3,
            },
            MapCell {
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
    let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
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
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 4,
            },
            MapCell {
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
fn cliff_back_impassability_ignores_small_height_diff() {
    let map = make_map(
        vec![
            MapCell {
                rx: 0,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                rx: 1,
                ry: 0,
                tile_index: -1,
                sub_tile: 0,
                z: 3,
            },
            MapCell {
                rx: 0,
                ry: 1,
                tile_index: -1,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
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
    let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
    let cell = grid.cell(1, 1).unwrap();
    assert!(
        !cell.ground_walk_blocked,
        "Height diff 3 should NOT trigger (threshold is 4)"
    );
}

/// `TheaterData` with a single tileset that declares tile animations in its
/// `SetName`-named section, shaped like retail `[Waterfalls]` /
/// `[Tunnel Floor]` blocks.
fn theater_with_tile_anims() -> TheaterData {
    let ini = b"[TileSet0000]
TilesInSet=4
FileName=wf
SetName=Waterfalls

                    [Waterfalls]
                    Tile01Anim=WA01X
Tile01XOffset=-30
Tile01YOffset=59
                    Tile01AttachesTo=0
Tile01ZAdjust=0
                    Tile03Anim=TUNTOP01
Tile03XOffset=-48
Tile03YOffset=-37
                    Tile03AttachesTo=2
Tile03ZAdjust=-10
";
    let lookup = theater::parse_tileset_ini(ini, "tem").expect("synthetic theater parses");
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: Vec::new(),
        bridge_set: None,
        wood_bridge_set: None,
        slope_set_pieces: None,
        bridge_middle_1: None,
        bridge_middle_2: None,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
        rmg_tiles: crate::map::theater::RmgTileKeys::default(),
    }
}

fn anim_cell(rx: u16, ry: u16, tile_index: i32, sub_tile: u8, z: u8) -> MapCell {
    MapCell {
        rx,
        ry,
        tile_index,
        sub_tile,
        z,
    }
}

#[test]
fn tile_anim_live_and_eager_use_bounds_and_wrapping_world_adds() {
    let tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("wf01.tem", &tmp);
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
    let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
    // Bounds are match inputs. Rejection zeroes only the offset; the Anim
    // still attaches at the cell centre. Negative offsets have no lower guard.
    for (x, y, bounds, expected) in [
        (
            -30,
            59,
            PixelConversionBounds {
                width: 1,
                height: 59,
            },
            (128, 128),
        ),
        (
            -30,
            59,
            PixelConversionBounds {
                width: 1,
                height: 60,
            },
            (503, 759),
        ),
        (
            -30,
            59,
            PixelConversionBounds {
                width: -30,
                height: 60,
            },
            (128, 128),
        ),
        // Deterministic rational policy, then signed32 wrapping world add.
        (
            -503_312_600,
            0,
            PixelConversionBounds::default(),
            (-2_147_483_637, -2_147_483_403),
        ),
    ] {
        let mut theater = theater_with_tile_anims();
        let ini = format!(
            "[TileSet0000]\nTilesInSet=4\nFileName=wf\nSetName=Waterfalls\n[Waterfalls]\nTile01Anim=WA01X\nTile01XOffset={x}\nTile01YOffset={y}\nTile01AttachesTo=0\n"
        );
        theater.lookup = theater::parse_tileset_ini(ini.as_bytes(), "tem").unwrap();
        let map = make_map(vec![anim_cell(0, 0, 0, 0, 0)], Vec::new(), Vec::new());
        let eager = ResolvedTerrainGrid::build_inner(
            &map,
            Some(&theater),
            Some(&assets),
            Some(&terrain_rules),
            Some(&registry),
            None,
            false,
            0,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
            bounds,
        );
        assert_eq!(eager.tile_animations().len(), 1);
        let eager_anim = &eager.tile_animations()[0];
        assert_eq!(
            (eager_anim.world_x, eager_anim.world_y),
            expected,
            "eager {x},{y} {bounds:?}"
        );
        let mut live = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        let mut state = LoadCellRecalcState::for_authored_load(
            &map,
            &theater,
            &assets,
            &terrain_rules,
            &registry,
            false,
            0,
            1,
            bounds,
        );
        let mut effects = LoadRecalcTestEffects::default();
        live.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .unwrap();
        assert_eq!(effects.requests.len(), 1);
        let live_anim = &effects.requests[0];
        assert_eq!(
            (live_anim.world_x, live_anim.world_y),
            expected,
            "live {x},{y} {bounds:?}"
        );
    }
}

#[test]
fn authored_load_recalc_latches_tile_anim_only_after_synchronous_success() {
    let theater = theater_with_tile_anims();
    let tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("wf01.tem", &tmp);
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
    let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
    let map = make_map(Vec::new(), Vec::new(), Vec::new());
    let mut grid =
        ResolvedTerrainGrid::from_cells(2, 1, vec![make_test_cell(0, 0), make_test_cell(1, 0)]);
    let mut state = LoadCellRecalcState::for_authored_load(
        &map,
        &theater,
        &assets,
        &terrain_rules,
        &registry,
        false,
        0,
        grid.cells.len(),
        PixelConversionBounds::default(),
    );
    let mut effects = LoadRecalcTestEffects {
        fail: true,
        ..LoadRecalcTestEffects::default()
    };

    let failed = grid.recalc_authored_load_cell(
        &mut state,
        0,
        FinalizedOverlayCell::default(),
        &mut effects,
    );
    assert!(matches!(
        failed,
        Err(LoadCellRecalcError::Effect("fixture failure"))
    ));
    assert_eq!(effects.requests.len(), 1);
    assert!(!state.terrain_anim_latched[0]);

    effects.fail = false;
    grid.recalc_authored_load_cell(&mut state, 0, FinalizedOverlayCell::default(), &mut effects)
        .expect("successful synchronous terrain-Anim construction");
    assert_eq!(effects.requests.len(), 2);
    assert!(state.terrain_anim_latched[0]);

    grid.recalc_authored_load_cell(&mut state, 0, FinalizedOverlayCell::default(), &mut effects)
        .expect("latched Recalc");
    assert_eq!(
        effects.requests.len(),
        2,
        "the post-data Recalc must not duplicate an inline successful Anim"
    );

    grid.recalc_authored_load_cell(&mut state, 1, FinalizedOverlayCell::default(), &mut effects)
        .expect("second cell terrain-Anim construction");
    assert_eq!(effects.requests.len(), 3);
    assert!(state.terrain_anim_latched[1]);

    assert!(state.clear_terrain_anim_latch(0));
    assert!(
        state.terrain_anim_latched[1],
        "clearing one final-sweep receiver must not expose a future cell"
    );
    grid.recalc_authored_load_cell(&mut state, 0, FinalizedOverlayCell::default(), &mut effects)
        .expect("post-object unlatched Recalc");
    assert_eq!(effects.requests.len(), 4);
    assert!(state.terrain_anim_latched[0]);

    assert!(state.clear_terrain_anim_latch(1));
    grid.recalc_authored_load_cell(&mut state, 1, FinalizedOverlayCell::default(), &mut effects)
        .expect("second post-object receiver Recalc");
    assert_eq!(effects.requests.len(), 5);
    assert!(state.terrain_anim_latched[1]);
}

#[test]
fn gsi_13_04_tile_anim_offset_matches_native_float_transform() {
    // The engine converts the pixel offset through a float 3x4 matrix whose
    // first two rows are [+s, 2s, 0, 0] and [-s, 2s, 0, 0], with s the
    // single-precision constant 4.2667, then truncates toward zero. This
    // pins the integer form against that reference across a range that
    // comfortably contains every stock offset (max magnitude 60).
    const S: f32 = 4.2667;
    const S2: f32 = 8.5334;
    for px in -128i32..=128 {
        for py in -128i32..=128 {
            let reference_x = (S * px as f32 + S2 * py as f32).trunc() as i32;
            let reference_y = (S2 * py as f32 - S * px as f32).trunc() as i32;
            assert_eq!(
                PixelConversionBounds::default().offset_to_leptons(px, py),
                (reference_x, reference_y),
                "pixel offset ({px}, {py})"
            );
        }
    }
}

#[test]
fn gsi_13_04_tile_anim_spawns_only_on_the_attaches_to_subtile() {
    let theater = theater_with_tile_anims();
    // Tile 0 declares AttachesTo=0; tile 2 declares AttachesTo=2.
    let map = make_map(
        vec![
            anim_cell(0, 0, 0, 0, 0),
            anim_cell(1, 0, 0, 1, 0),
            anim_cell(0, 1, 2, 2, 0),
            anim_cell(1, 1, 2, 0, 0),
        ],
        Vec::new(),
        Vec::new(),
    );
    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    let anims = grid.tile_animations();
    assert_eq!(anims.len(), 2, "{anims:?}");
    assert_eq!(anims[0].rx, 0);
    assert_eq!(anims[0].ry, 0);
    assert_eq!(anims[0].anim_name, "WA01X");
    assert_eq!(anims[1].rx, 0);
    assert_eq!(anims[1].ry, 1);
    assert_eq!(anims[1].anim_name, "TUNTOP01");
    assert_eq!(anims[1].z_adjust, -10);
}

#[test]
fn gsi_13_04_tile_anim_world_coord_adds_cell_centre_and_ground_height() {
    let theater = theater_with_tile_anims();
    let map = make_map(vec![anim_cell(2, 3, 0, 0, 5)], Vec::new(), Vec::new());
    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    let anim = &grid.tile_animations()[0];
    let (offset_x, offset_y) = PixelConversionBounds::default().offset_to_leptons(-30, 59);
    assert_eq!(anim.world_x, offset_x + 2 * 256 + 128);
    assert_eq!(anim.world_y, offset_y + 3 * 256 + 128);
    assert_eq!(
        anim.world_z,
        5 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
    );
}

#[test]
fn gsi_13_04_tile_anim_skipped_for_no_tile_cells() {
    // A no-tile cell takes the engine's early return before the animation
    // spawn, so ClearTile presentation never inherits an attachment.
    let theater = theater_with_tile_anims();
    let map = make_map(vec![anim_cell(0, 0, -1, 0, 0)], Vec::new(), Vec::new());
    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    assert!(grid.tile_animations().is_empty());
}

#[test]
fn gsi_13_04_tile_anims_are_ordered_by_the_native_anti_diagonal_sweep() {
    let theater = theater_with_tile_anims();
    let map = make_map(
        vec![
            anim_cell(2, 0, 0, 0, 0),
            anim_cell(0, 1, 0, 0, 0),
            anim_cell(1, 0, 0, 0, 0),
            anim_cell(0, 0, 0, 0, 0),
        ],
        Vec::new(),
        Vec::new(),
    );
    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    let order: Vec<(u16, u16)> = grid
        .tile_animations()
        .iter()
        .map(|anim| (anim.rx, anim.ry))
        .collect();
    assert_eq!(order, vec![(0, 0), (0, 1), (1, 0), (2, 0)]);
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
        .collapse_destroyable_cliff_terrain(4, 1, |_, _| {})
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
                .collapse_destroyable_cliff_terrain(selected.0, selected.1, |_, _| {})
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
