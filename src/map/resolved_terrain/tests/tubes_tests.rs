//! Tunnel tubes: deploy neighborhoods, tube-cell lookups, automatic shell
//! tubes built at recalc, explicit tube binding and direction-8 stepping.

use super::*;

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
            cells[y * 16 + x].land_type = LandType::Tunnel.as_index();
        }
        let tube = TubeFact {
            entry: (10, 10),
            exit: (14, 14),
            direction: 0,
            path_steps: Vec::new(),
            source: TubeSource::AutoLowBridge,
        };
        let grid = ResolvedTerrainGrid::from_cells_with_tubes(16, 16, cells, vec![tube]);
        let cells = NativeCellQuery::isolated(&grid);
        assert_eq!(
            cells.is_near_tube_northwest(cells.lookup((10, 10))),
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
    cells[0].land_type = LandType::Tunnel.as_index();
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
            cell.land_type = LandType::Tunnel.as_index();
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
    wrong_land.land_type = LandType::Road.as_index();
    let mut existing = make_test_cell(1, 0);
    existing.final_tile_index = 0;
    existing.land_type = LandType::Tunnel.as_index();
    existing.tube_index = Some(TubeId(0));
    let mut fifth_tile = make_test_cell(2, 0);
    fifth_tile.final_tile_index = 4;
    fifth_tile.land_type = LandType::Tunnel.as_index();
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
    cells[1].land_type = LandType::Tunnel.as_index();
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
