//! Overlay land writers at load: bridge overlays, zone-type priority, terrain
//! occupation, tiberium slope branches and TMP ramp conversion.

use super::*;

#[test]
fn test_bridge_overlay_creates_upper_layer_without_ground_block() {
    // BRIDGE1 is hardcoded at overlay index 24 in the original engine.
    // Build a registry large enough so index 24 resolves to "BRIDGE1". Its
    // unauthored section leaves Land=Clear, so [Clear] is authored as retail
    // has it: the zero row of a missing one has Wheel 0, which blocks the cell.
    let mut ini_str = String::from("[Clear]\nWheel=100%\n[OverlayTypes]\n");
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
            legacy_ice_byte: 0,
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
                legacy_ice_byte: 0,
                rx: 0,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 1,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
                rx: 2,
                ry: 0,
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
TemperateOccupationBits=8
SnowOccupationBits=-1
",
    ))
    .expect("terrain rules");
    let mut map = make_map(
        vec![
            MapCell {
                legacy_ice_byte: 0,
                rx: 0,
                ry: 1,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            },
            MapCell {
                legacy_ice_byte: 0,
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
                ry: 1,
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
    let temperate_tree = temperate.cell(0, 1).unwrap();
    assert_eq!(temperate_tree.terrain_object_occupation, Some(4));
    assert!(temperate_tree.terrain_object_blocks);
    assert_eq!(temperate_tree.zone_type, zone_class::BUILDING);
    assert!(!temperate_tree.base_build_blocked);
    let zero_tree = temperate.cell(1, 0).unwrap();
    assert_eq!(zero_tree.terrain_object_occupation, Some(8));
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
    let snow_tree = snow.cell(0, 1).unwrap();
    assert_eq!(snow_tree.terrain_object_occupation, Some(7));
    assert!(snow_tree.terrain_object_blocks);
    assert_eq!(snow_tree.zone_type, zone_class::WALL);
    let signed_tree = snow.cell(1, 0).unwrap();
    assert_eq!(signed_tree.terrain_object_occupation, Some(-1));
    assert!(signed_tree.terrain_object_blocks);
    assert_eq!(signed_tree.zone_type, zone_class::BUILDING);
}

#[test]
fn gsi_04_04_outside_precedes_overlay_land_and_terrain_occupation() {
    let playfield = PlayfieldBounds::from_normalized_local_size(34, 0, 0, 34, 42);
    let outside = !playfield.contains_height_aware_packed(0, 0, 0, 0);
    assert!(outside);
    assert_eq!(
        recalc_zone_type(
            outside,
            Some(zone_class::CRUSHABLE),
            LandType::Water.as_index(),
            Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
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
            legacy_ice_byte: 0,
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
    for (rx, overlay_id, land) in [
        (0, 0, LandType::Road),
        (2, 2, LandType::Wall),
        (3, 3, LandType::Railroad),
        (4, 4, LandType::Water),
        (5, 5, LandType::Rough),
    ] {
        let cell = grid.cell(rx, 0).expect("retained load cell");
        assert_eq!(cell.base_land_type, LandType::Clear.as_index());
        assert_eq!(cell.land_type, land.as_index(), "overlay {overlay_id}");
        assert_eq!(
            cell.speed_costs,
            registry
                .flags(overlay_id)
                .unwrap()
                .land_speed_costs
                .unwrap(),
            "overlay {overlay_id} profile"
        );
    }

    let restored = grid.cell(1, 0).expect("ordinary restoration cell");
    assert_eq!(restored.land_type, restored.base_land_type);
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

    // Start from a water tile: Water land, blocked to ground.
    let mut metadata = TileMetadata {
        has_tmp_metadata: true,
        land_type: LandType::Water.as_index(),
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
    cell.land_type = metadata.land_type;
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
    // Each Land row these overlays copy is authored and passable, with its own
    // Wheel; an unauthored one is the zero row, whose Wheel 0 would make the
    // overlay's reduced zone impassable.
    let ini = IniFile::from_str(
        "\
[OverlayTypes]
0=ORE
1=STOCKORE
2=WALLORE
3=RAILORE
[Tiberium]
Wheel=80%
[Wall]
Wheel=60%
[Railroad]
Wheel=40%
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

            let mut current = TileMetadata {
                tileset_index: Some(0),
                ..TileMetadata::default()
            };
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
fn a_tmp_ramp_byte_past_the_slope_tables_is_flat_ground() {
    let tile = |ramp_type| TmpTile {
        height: 0,
        terrain_type: 14,
        ramp_type,
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
    let mut last_slope = TileMetadata::default();
    merge_tmp_metadata(&mut last_slope, &tile(20));
    assert_eq!(last_slope.slope_type, 20);
    assert!(last_slope.has_ramp);
    for ramp_type in [21, 255] {
        let mut metadata = TileMetadata::default();
        merge_tmp_metadata(&mut metadata, &tile(ramp_type));
        assert_eq!(metadata.slope_type, 0, "ramp byte {ramp_type}");
        assert!(!metadata.has_ramp);
        // Every ground-height read takes the stored slope.
        assert!(crate::util::lepton::ground_height_leptons(0, metadata.slope_type, 0, 0).is_ok());
    }
}

#[test]
fn gsi_04_04_rules_backed_land_type_uses_canonical_tmp_conversion() {
    let terrain_rules =
        TerrainRules::from_ini(&IniFile::from_str("[Rough]\nBuildable=yes\nTrack=75%\n"));
    let mut metadata = TileMetadata {
        tileset_index: Some(2),
        ..TileMetadata::default()
    };
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
    apply_land_type_semantics(&mut metadata, Some(&terrain_rules));

    assert_eq!(metadata.land_type, LandType::Rough.as_index());
    assert_eq!(
        metadata.speed_costs.track,
        Some(crate::util::native_x87::NativeF32Bits::from_bits(
            0x3f40_0000
        ))
    );
    assert!(!metadata.ground_blocked);
    assert!(!metadata.build_blocked);
}

#[test]
fn test_canonical_ramp_is_ground_passable_but_stays_non_buildable() {
    let map = make_map(
        vec![MapCell {
            legacy_ice_byte: 0,
            rx: 0,
            ry: 0,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        }],
        Vec::new(),
        Vec::new(),
    );
    let terrain_rules = TerrainRules::from_ini(&IniFile::from_str("[Rock]\nBuildable=no\n"));
    let mut metadata = TileMetadata {
        has_tmp_metadata: true,
        raw_land_type: 15,
        land_type: crate::rules::terrain_rules::tmp_terrain_to_land_type(15).as_index(),
        slope_type: 2,
        has_ramp: true,
        ..TileMetadata::default()
    };
    apply_land_type_semantics(&mut metadata, Some(&terrain_rules));

    assert!(metadata.ground_blocked && metadata.build_blocked);
    let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
    assert!(!base_ground_walk_blocked);
    let grid = ResolvedTerrainGrid::from_cells(
        1,
        1,
        vec![ResolvedTerrainCell {
            land_type: metadata.land_type,
            slope_type: metadata.slope_type,
            speed_costs: metadata.speed_costs,
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
