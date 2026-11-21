//! Land rows and land types: scenario speed rows on the grid, overlay wall
//! flags, the retail Clear row, and how the TMP subtile (never the tileset
//! name) decides a cell's land and its rules-row attributes.

use super::*;

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
            legacy_ice_byte: 0,
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

#[test]
fn fractional_retail_land_row_reaches_rhino_entry_and_drive() {
    use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
    use crate::sim::movement::track_process::TrackFamily;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &IniFile::from_str("[Clear]\nTrack=0.5%\nWheel=1.00001%\n"),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE")
        .expect("retail temperate theater");
    let mut map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 12,
                ry: 12,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 13,
                ry: 12,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
        ],
        Vec::new(),
        Vec::new(),
    );
    map.header.width = 16;
    map.header.height = 16;
    map.header.local_width = 16;
    map.header.local_height = 16;
    let bounds = crate::sim::cell_rect::PlayfieldBounds::from_map_header(&map.header);
    let mut variants = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
    let mut raw_draw = || 0;
    let mut selector = variants.begin_load(&mut raw_draw);
    let mut fill = |low, _high| low;
    let terrain = ResolvedTerrainGrid::build_with_variant_selector(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&rules.terrain_rules),
        Some(rules.overlay_registry()),
        None,
        false,
        rules.general.cliff_back_impassability,
        &mut fill,
        &mut selector,
    );
    let cells = NativeCellQuery::canonical(&terrain);
    assert!(crate::sim::cell_rect::retained_cell_is_in_playfield(
        cells.lookup((13, 12)),
        Some(bounds),
        &cells
    ));
    assert_eq!(terrain.cell(13, 12).unwrap().zone_type, zone_class::GROUND);
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 29);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.install_bridge_runtime(
        crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
            &terrain,
            true,
            rules.bridge_rules.strength,
            (map.header.width as i32, map.header.height as i32),
        ),
    );
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.install_playfield_from_map_header(&map.header);
    sim.session.set_map_width_for_test(map.header.width as u16);
    sim.session
        .set_map_height_for_test(map.header.height as u16);
    assert!(sim.rebuild_dynamic_navigation(rules));
    let tank = sim
        .spawn_object_with_overlay_registry(
            "HTNK",
            "Russians",
            12,
            12,
            64,
            rules,
            rules.overlay_registry(),
        )
        .expect("Rhino enters the fractional Clear row");
    assert_eq!(
        sim.mover_can_enter(
            tank,
            (13, 12),
            InfantryEntryArgs::REPAIR,
            EntryQueryMode::CheckLocomotor,
            rules,
        )
        .unwrap(),
        0
    );
    assert!(sim.set_unit_destination(
        tank,
        crate::sim::components::NavTargetRef::cell(13, 12),
        rules,
        true,
        &mut crate::sim::world::FrameEffects::default()
    ));
    sim.session.set_binary_frame_for_test(101);
    sim.entity_mut_for_test(tank)
        .unwrap()
        .mission_leaf
        .set_foot_idle_entry_latch(0);
    let grid = sim.path_grid().cloned();
    sim.process_ground_locomotor_for_test(tank, Some(rules), grid.as_ref(), None)
        .unwrap();
    let tank = sim.entities().get(tank).unwrap();
    let value = tank
        .locomotor
        .as_ref()
        .unwrap()
        .track_target_fraction_native(TrackFamily::Drive)
        .unwrap();
    let value = f64::from_bits(value.bits());
    assert!(
        value > 0.0 && value < 0.01,
        "fractional land value must reach Drive, got {value}"
    );
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
            legacy_ice_byte: 0,
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
        test_retail_clear_speed_costs()
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert_eq!(
        clear(&retail.rules.terrain_rules),
        test_retail_clear_speed_costs()
    );
}

#[test]
fn gsi_04_03a_special_terrain_identity_is_not_promoted_to_hard_blocking() {
    let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=10\nFileName=plain\nSetName=Plain Terrain\n";
    let mut theater = synthetic_theater_from_ini(ini);
    theater.cliff_ranges.cliff_set = Some(10);
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
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
    assert_eq!(cell.land_type, LandType::Clear.as_index());
    assert!(!cell.base_ground_walk_blocked);
    assert!(!cell.ground_walk_blocked);
}

/// Land comes from the TMP subtile alone; overlay and CliffBack writers
/// replace it later. RecalcAttributes (`0x0047D2B0`) rewrites `+0xEC` on every
/// path, and GetSubtileLandType (`0x00544BE0`) answers 0, Clear, for a missing
/// image or entry (`0x00544BEC`/`0x00544C03` -> `0x00544C13`), so with no
/// assets every cell is Clear however its tileset is named.
#[test]
fn tileset_names_never_decide_land_without_a_tmp() {
    let names = [
        "Water",
        "Water Cliffs",
        "Cliff Set",
        "Rock",
        "Paved Roads",
        "Rough ground",
        "Shore Pieces",
    ];
    let mut ini = String::new();
    for (index, name) in names.iter().enumerate() {
        ini.push_str(&format!(
            "[TileSet{index:04}]\nTilesInSet=1\nFileName=set{index}\nSetName={name}\n"
        ));
    }
    let theater = synthetic_theater_from_ini(ini.as_bytes());
    let cells = (0..names.len())
        .map(|index| MapCell {
            legacy_ice_byte: 0,
            rx: index as u16,
            ry: 0,
            tile_index: index as i32,
            sub_tile: 0,
            z: 0,
        })
        .collect();
    let map = make_map(cells, Vec::new(), Vec::new());

    let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
    for (index, name) in names.iter().enumerate() {
        let cell = grid.cell(index as u16, 0).expect("named-set cell");
        assert_eq!(cell.tileset_index, Some(index as u16), "{name}");
        assert_eq!(cell.land_type, LandType::Clear.as_index(), "{name}");
        assert_eq!(cell.base_land_type, LandType::Clear.as_index(), "{name}");
    }
}

/// The TMP decides a cell's land and the land's rules row its attributes; a
/// land with no row gets the same attributes whatever its tileset is named.
#[test]
fn tileset_names_never_decide_land_row_attributes() {
    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n\
          [TileSet0001]\nTilesInSet=1\nFileName=wcliff\nSetName=Water Cliffs\n",
    );
    let clear_tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    let filenames = [0, 1].map(|tile| {
        theater
            .lookup
            .filename_for_variant(tile, 0)
            .expect("registered tile filename")
            .to_string()
    });
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        (filenames[0].as_str(), clear_tmp.as_slice()),
        (filenames[1].as_str(), clear_tmp.as_slice()),
    ]);
    let map = make_map(
        (0..2)
            .map(|tile| MapCell {
                legacy_ice_byte: 0,
                rx: tile,
                ry: 0,
                tile_index: i32::from(tile),
                sub_tile: 0,
                z: 0,
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    );
    let no_rows = TerrainRules::from_ini(&IniFile::from_str(""));

    let grid = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&no_rows),
        None,
        false,
        0,
    );
    let attributes = |cell: &ResolvedTerrainCell| {
        (
            cell.land_type,
            cell.speed_costs,
            cell.ground_walk_blocked,
            cell.base_ground_walk_blocked,
            cell.base_build_blocked,
        )
    };
    let plain = grid.cell(0, 0).expect("Clear-named cell");
    let named = grid.cell(1, 0).expect("Water Cliffs-named cell");
    assert_eq!(named.land_type, LandType::Clear.as_index());
    assert_eq!(attributes(named), attributes(plain));
}

/// A land type no rules layer authors keeps the zero row: the reader at
/// `0x00674000` skips its section (`JZ` at `0x00674022`) and the table at
/// `0x0089EA40` is zero in the image. The CliffBack writer reclasses to Rock
/// with that same row, so a TMP Rock cell and a CliffBack Rock cell carry the
/// same attributes.
#[test]
fn an_unauthored_rock_row_is_zero_for_tmp_and_cliff_back_rock_alike() {
    let theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n\
          [TileSet0001]\nTilesInSet=1\nFileName=rock\nSetName=Rock\n",
    );
    let filenames = [0, 1].map(|tile| {
        theater
            .lookup
            .filename_for_variant(tile, 0)
            .expect("registered tile filename")
            .to_string()
    });
    // TMP terrain bytes 0 and 7 read Clear and Rock (table `0x008288E4`).
    let clear_tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
    let rock_tmp = gsi_04_02_last_tiles_tmp_bytes(7, [1, 2, 3], [4, 5, 6]);
    let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
        (filenames[0].as_str(), clear_tmp.as_slice()),
        (filenames[1].as_str(), rock_tmp.as_slice()),
    ]);
    let cell = |rx, ry, tile_index, z| MapCell {
        legacy_ice_byte: 0,
        rx,
        ry,
        tile_index,
        sub_tile: 0,
        z,
    };
    // (1,1) sits four levels below (1,0), its (0,-1) probe: CliffBack.
    let map = make_map(
        vec![cell(1, 0, 0, 4), cell(1, 1, 0, 0), cell(3, 1, 1, 0)],
        Vec::new(),
        Vec::new(),
    );
    let rules = TerrainRules::from_ini(&IniFile::from_str("[Clear]\nBuildable=yes\n"));

    let grid = ResolvedTerrainGrid::build(
        &map,
        Some(&theater),
        Some(&assets),
        Some(&rules),
        None,
        false,
        2,
    );
    let tmp_rock = grid.cell(3, 1).expect("TMP Rock cell");
    let cliff_back = grid.cell(1, 1).expect("CliffBack cell");
    assert_eq!(tmp_rock.land_type, LandType::Rock.as_index());
    assert_eq!(cliff_back.land_type, LandType::Rock.as_index());
    let zero = SpeedCostProfile {
        foot: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        track: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        wheel: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        float: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        amphibious: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        float_beach: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        hover: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
        winged: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
    };
    let attributes = |cell: &ResolvedTerrainCell| {
        (
            cell.speed_costs,
            cell.ground_walk_blocked,
            cell.base_speed_costs,
            cell.base_ground_walk_blocked,
            cell.base_build_blocked,
            cell.zone_type,
        )
    };
    assert_eq!(
        attributes(tmp_rock),
        (zero, true, zero, true, true, zone_class::IMPASSABLE)
    );
    assert_eq!(attributes(cliff_back), attributes(tmp_rock));
}
