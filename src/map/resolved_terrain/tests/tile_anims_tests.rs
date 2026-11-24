//! Tile animations: live and eager spawning, the load-recalc latch, offsets,
//! world coordinates and the native anti-diagonal sweep order.

use super::*;

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
        high_bridge_rim_tiles: crate::map::bridge_rim_tiles::HighBridgeRimTiles::ABSENT,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
        rmg_tiles: crate::map::theater::RmgTileKeys::default(),
        ice_sets: None,
        terrain_files: crate::map::theater::TerrainFiles::GameFolder,
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
