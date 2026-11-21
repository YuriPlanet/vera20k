//! Bridge tests: spawn height on bridges, destroyed bridge arms, structural and
//! concrete bridge collapse (damage dispatch, fallout, determinism, navigation and
//! snapshot) and water movers passing under a structural bridge.

use super::*;

fn single_bridge_cell(rx: u16, ry: u16, deck_level: u8) -> ResolvedTerrainGrid {
    let mut cells = Vec::new();
    for y in 0..=ry {
        for x in 0..=rx {
            cells.push(crate::map::resolved_terrain::ResolvedTerrainCell {
                has_bridge_deck: x == rx && y == ry,
                bridge_walkable: x == rx && y == ry,
                bridge_transition: x == rx && y == ry,
                bridge_deck_level: if x == rx && y == ry { deck_level } else { 0 },
                ..crate::map::resolved_terrain::test_flat_cell(x, y)
            });
        }
    }
    ResolvedTerrainGrid::from_cells(rx + 1, ry + 1, cells)
}

/// Structural deck fixture for the576BA0 ->47E040 damage path, matching
/// bridge_body_publication's native anchor25/state15 collapse input.
/// All four deck slots come from the actual direction6 flag stamp; their
/// overlay fields come back in the returned grid.
fn structural_bridge_for_damage_dispatch() -> (
    ResolvedTerrainGrid,
    BridgeRuntimeState,
    crate::sim::overlay_grid::OverlayGrid,
) {
    use crate::map::bridge_facts::{BridgeFlagStamp, BridgeStampFamily};
    let mut terrain = ResolvedTerrainGrid::from_cells(
        10,
        10,
        (0..10)
            .flat_map(|y| (0..10).map(move |x| common_raw_test_terrain_cell(x, y, 0, false)))
            .collect(),
    );
    terrain.apply_runtime_bridge_mark_stamp(
        BridgeFlagStamp::new((5, 5), 6, true),
        BridgeStampFamily::Nesw,
    );
    // The constructing setter's four slot visits store state 9 (direction
    // 6); the anchor then carries overlay 25 at state 15.
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    for x in [3, 4, 6] {
        overlays.cell_mut_for_test(x, 5).overlay_data = 9;
    }
    let anchor = overlays.cell_mut_for_test(5, 5);
    anchor.overlay_id = Some(25);
    anchor.overlay_data = 15;
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 15);
    for x in 3..=6 {
        assert!(
            terrain
                .cell(x, 5)
                .unwrap()
                .bridge_facts
                .has_structural_bridge()
        );
    }
    (terrain, state, overlays)
}

/// The direction6 structural deck over water: every cell takes
/// `water_terrain`'s ground, the deck spans (3..=6, 5).
fn water_under_structural_bridge() -> (ResolvedTerrainGrid, BridgeRuntimeState) {
    let (mut terrain, _, _) = structural_bridge_for_damage_dispatch();
    let water = water_terrain(1, 1).cells()[0].clone();
    for cell in terrain.cells_mut_for_test() {
        cell.land_type = water.land_type;
        cell.speed_costs = water.speed_costs;
        cell.ground_walk_blocked = water.ground_walk_blocked;
        cell.zone_type = water.zone_type;
        cell.base_ground_walk_blocked = water.base_ground_walk_blocked;
        cell.base_build_blocked = water.base_build_blocked;
        cell.base_land_type = water.base_land_type;
        cell.base_speed_costs = water.base_speed_costs;
    }
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 15);
    (terrain, state)
}

/// Supplied structural47DD70 callback input with a real direction6 stamp.
/// Ground variants exercise DropIn without selecting a concrete overlay driver.
fn structural_bridge_for_fallout(
    ground_level: u8,
    water_below: bool,
) -> (ResolvedTerrainGrid, BridgeRuntimeState) {
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
    let (mut terrain, _, _) = structural_bridge_for_damage_dispatch();
    for x in 3..=6 {
        let cell = terrain.cell_mut_for_test(x, 5).unwrap();
        cell.level = ground_level;
        cell.bridge_deck_level = ground_level + 4;
        if water_below {
            let costs = SpeedCostProfile {
                float: Some(crate::util::native_x87::NativeF32Bits::ONE),
                winged: Some(crate::util::native_x87::NativeF32Bits::ONE),
                ..Default::default()
            };
            cell.land_type = LandType::Water.as_index();
            cell.speed_costs = costs;
            cell.zone_type = 4;
            cell.ground_walk_blocked = true;
            cell.base_ground_walk_blocked = true;
            cell.base_build_blocked = true;
            cell.base_land_type = LandType::Water.as_index();
            cell.base_speed_costs = costs;
        }
    }
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 15);
    (terrain, state)
}

fn assert_structural_bridge_collapsed(sim: &Simulation) {
    for x in 3..=6 {
        let cell = sim.resolved_terrain.as_ref().unwrap().cell(x, 5).unwrap();
        assert_eq!(cell.bridge_facts.raw_flags, 0x400, "collapsed cell({x},5)");
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(x, 5).overlay_data,
            0
        );
        assert!(!cell.has_bridge_deck);
        assert!(!cell.bridge_walkable);
        assert!(!cell.bridge_transition);
        assert!(!cell.bridge_facts.has_structural_bridge());
        let published = sim.path_grid().unwrap().cell(x, 5).unwrap();
        assert!(!published.bridge_walkable);
        assert!(!published.transition);
    }
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None,
        "the structural body clears its anchor overlay"
    );
}

/// Water movers on Hover (HoverLocomotionClass Process 0x00514310).
fn naval_bridge_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=BOAT\n1=DRED\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [BOAT]\nStrength=300\nArmor=heavy\nSpeed=6\nMovementZone=Water\nSpeedType=Float\nNaval=yes\nLocomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\n\
         [DRED]\nStrength=600\nArmor=heavy\nSpeed=5\nMovementZone=Water\nSpeedType=Float\nNaval=yes\nTooBigToFitUnderBridge=yes\nLocomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n",
    );
    RuleSet::from_ini(&ini).expect("naval bridge test rules should parse")
}

/// ReadUnits `0x007434F3..0x00743510` writes OnBridge from HIGH and places
/// the unit at ground + `0xB1D0AC`, with or without HasBridge and independent
/// of the navigation fixture's cached deck level 3. Original controls:
/// anytown_damage/unit_unlimbo.json authored_rows.
#[test]
fn test_spawn_from_map_high_unit_uses_native_ground_plus_bridge_height() {
    let raised_corner = ResolvedTerrainGrid::from_cells(
        6,
        6,
        (0..6u16)
            .flat_map(|ry| {
                (0..6u16).map(
                    move |rx| crate::map::resolved_terrain::ResolvedTerrainCell {
                        level: u8::from((rx, ry) == (5, 5)),
                        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
                    },
                )
            })
            .collect(),
    );
    for (resolved, z, z_leptons) in [
        (single_bridge_cell(5, 5, 3), 4, 416),
        (raised_corner, 5, 520),
    ] {
        let mut sim = Simulation::new();
        install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
        sim.install_resolved_terrain_for_new_map(resolved.clone());
        let count = sim.spawn_from_map_with_resolved(
            &[MapEntity {
                cell_x: 5,
                cell_y: 5,
                high: true,
                ..make_test_entity("MTNK", EntityCategory::Unit)
            }],
            Some(&combat_test_rules()),
            Some(&resolved),
        );

        assert_eq!(count, 1);
        let e = sim.substrate.entities.get(1).expect("spawned entity");
        assert_eq!(e.position.z(), z);
        assert_eq!(e.position.exact_z_leptons(), Some(z_leptons));
        assert!(e.on_bridge());
        let loco = e.locomotor.as_ref().expect("loco");
        assert_eq!(loco.layer(), MovementLayer::Bridge);
    }
}

/// Production spawns read the live CellClass level: a level rewritten after
/// load (bridge body, cliff collapse) is what the next object stands on.
#[test]
fn spawn_object_reads_the_live_terrain_level() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
         [E1]\nStrength=125\nSpeed=4\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("GI fixture");
    let mut sim = Simulation::new();
    // Ordinary Unlimbo still requires the real Clear/Foot speed row after
    // the level changes; a flat cell with unresolved rows is not passable.
    let terrain = crate::map::resolved_terrain::test_flat_ground_grid(6);
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.install_resolved_terrain_for_new_map(terrain);
    // The whole plateau is raised after load, as a level write would.
    for ry in 0..6 {
        for rx in 0..6 {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(rx, ry)
                .unwrap();
            cell.level = 4;
        }
    }
    let id = sim
        .spawn_object("E1", "Americans", 3, 3, 64, &rules)
        .expect("GI spawns");
    assert_eq!(sim.entities().get(id).unwrap().position.z(), 4);
}

/// Structural47DD70 relayers its deck member and starts DropAsBomb while
/// retaining its world height. Landing death runs on the later ObjectAI visit.
#[test]
fn test_destroyed_bridge_arms_fall_over_water_below() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = structural_bridge_for_fallout(0, true);
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            cell_x: 5,
            cell_y: 5,
            high: true,
            ..make_test_entity("MTNK", EntityCategory::Unit)
        }],
        Some(&combat_test_rules()),
        Some(&resolved),
    );

    let rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    // Supplied CellClass::BlowUpBridge47DD70 callback. Ordinary concrete
    // damage57CCF0 does not enter this structural ground/deck receiver.
    blow_up_bridge_cell_fallout(&mut sim, &rules, 5, 5, None, &mut FrameEffects::default());

    // DropAsBomb leaves HP and Location intact until ObjectAI lands it.
    let e = sim
        .substrate
        .entities
        .get(1)
        .expect("deck unit remains until landing");
    assert_eq!(
        e.health.current(),
        300,
        "DropIn never harms — authored full health uses MTNK Strength300"
    );
    assert_eq!(e.position.z(), 4, "Location remains at deck height");
    assert!(e.was_dropped_as_bomb() && e.is_falling_down());
    assert!(!e.on_bridge());
    let loco = e.locomotor.as_ref().expect("locomotor");
    assert_eq!(
        loco.layer(),
        MovementLayer::Bridge,
        "DropAsBomb preserves locomotor state"
    );
    assert!(crate::sim::movement::motion_query::is_moving(&e) != Some(true));
}

/// Structural47DD70 callback: the ground-list receiver takes its copied HP
/// through C4Warhead damage before the deck DropIn pass. This checks immediate
/// death/target cleanup at that supplied callback boundary.
#[test]
fn test_bridge_collapse_kills_ground_unit_under_destroyed_cell() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [CombatDamage]\nC4Warhead=Super\n\
         [Warheads]\n0=Super\n[Super]\nInfDeath=2\nPenetratesBunker=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = structural_bridge_for_fallout(0, false);
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    // Spawn at (5, 5), then explicitly model the native lower object-list
    // occupant below the structural bridge deck. Constructor admission rightly
    // selects the deck on this cell; the collapse path itself needs a ground-list
    // witness.
    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            cell_x: 5,
            cell_y: 5,
            ..make_test_entity("MTNK", EntityCategory::Unit)
        }],
        Some(&rules),
        Some(&resolved),
    );
    let id = sim
        .substrate
        .entities
        .iter_sorted()
        .next()
        .map(|(id, _)| id)
        .expect("ground unit spawned");
    sim.remove_entity_occupancy(id);
    let unit = sim.substrate.entities.get_mut(id).unwrap();
    unit.set_on_bridge(false);
    if let Some(locomotor) = unit.locomotor.as_mut() {
        locomotor.set_layer_for_test(MovementLayer::Ground);
    }
    sim.add_entity_occupancy(id);
    assert!(
        !sim.substrate.entities.get(id).unwrap().on_bridge(),
        "ground layer"
    );

    sim.resolve_type_handles(&rules);
    // Supplied CellClass::BlowUpBridge47DD70 callback. Ordinary concrete
    // damage57CCF0 does not enter this structural ground/deck receiver.
    blow_up_bridge_cell_fallout(&mut sim, &rules, 5, 5, None, &mut FrameEffects::default());

    let e = sim
        .substrate
        .entities
        .get(id)
        .expect("ground unit retained until pending deletion drains");
    assert_eq!(e.health.current(), 0, "kill_ground_occupants_at zeroed HP");
    assert!(
        e.death.is_dying(),
        "direct receiver completed death before returning"
    );
    assert!(e.attack_target.target().is_none());
    assert!(crate::sim::movement::motion_query::is_moving(&e) != Some(true));
}

fn concrete_damage_fixture_coord(row: &serde_json::Value) -> (u16, u16) {
    (
        (row[0].as_i64().unwrap() - 83) as u16,
        (row[1].as_i64().unwrap() - 85) as u16,
    )
}

/// Native ordinary-controller cells translated around (17,15), with empty
/// occupants and the fixture's raw11/Road TMP catalog. This is not a map-load replay.
fn concrete_damage_fixture(
    case_name: &str,
    strength: i32,
) -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    serde_json::Value,
) {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_ordinary_damage.json",
    ))
    .unwrap();
    let native = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == case_name)
        .unwrap()
        .clone();
    // Original OverlayType reader outputs in anytown_damage/next_family_native:
    //205..230 use Road; destroyed231/232 retain the underlying TMP land.
    let mut overlay_rules = String::new();
    for overlay in 205..=232 {
        overlay_rules.push_str(&format!(
            "[O{overlay}]\nLand=Road\nNoUseTileLandType={}\n",
            if overlay < 231 { "yes" } else { "no" },
        ));
    }
    let (mut sim, rules, registry) = super::entry_test_fixture::fixture_with_rules(&overlay_rules);
    let recalc_native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_recalc.json",
    ))
    .unwrap();
    let pristine_land = recalc_native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "valid_preserves_level")
        .unwrap()["land"]
        .as_u64()
        .unwrap() as u8;
    for row in native["input"]["cells"].as_array().unwrap() {
        let (x, y) = concrete_damage_fixture_coord(row);
        let overlay = row[5].as_u64().unwrap() as u8;
        // The common cell starts with placeholder Clear fields. Bind its
        // pristine catalog attributes before the overlay's early Land branch.
        // install_ordinary_repair_test_catalog supplies TMP byte+29=11;
        // original544BE0/table8288E4 and the Recalc corpus return Road1.
        let empty = sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .finalized_map_cell(x, y)
            .unwrap();
        assert!(empty.overlay_id().is_none());
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let index = terrain.index(x, y).unwrap();
        terrain
            .recalc_resident_bridge_cell(index, empty, -1, &registry, sim.playfield_bounds)
            .unwrap();
        let cell = terrain.cell(x, y).unwrap();
        assert_eq!(cell.final_tile_index, 0);
        assert_eq!(cell.final_sub_tile, 0);
        assert_eq!(cell.base_land_type, pristine_land);
        assert_eq!(cell.land_type, pristine_land);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(x, y, overlay, 0);
        let finalized = sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .finalized_map_cell(x, y)
            .unwrap();
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let index = terrain.index(x, y).unwrap();
        let outcome = terrain
            .recalc_resident_bridge_cell(index, finalized, -1, &registry, sim.playfield_bounds)
            .unwrap();
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .write_finalized_map_cell(x, y, outcome.finalized);
    }
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain_with_map_size(
        sim.resolved_terrain.as_ref().unwrap(),
        true,
        strength,
        (16, 16),
    ));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    (sim, rules, registry, native)
}

fn assert_concrete_damage_result(sim: &Simulation, native: &serde_json::Value) {
    use crate::map::resolved_terrain::BridgeDirection;
    use crate::rules::terrain_rules::LandType;
    for row in native["result"]["final"].as_array().unwrap() {
        let (x, y) = concrete_damage_fixture_coord(row);
        let expected = row[2].as_u64().unwrap() as u8;
        let cell = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(x, y).overlay_id,
            Some(expected)
        );
        assert_eq!(
            cell.bridge_facts.raw_flags, 0,
            "ordinary overlays never stamp a structural deck"
        );
        // This legacy metadata flag denotes bridge overlay identity too. Low
        // overlays keep it after collapse, without adding a raised movement layer.
        assert!(cell.has_bridge_deck);
        let layer = cell.bridge_layer.as_ref().unwrap();
        assert_eq!(layer.direction, BridgeDirection::Low);
        assert_eq!(layer.deck_level, cell.level);
        assert_eq!(cell.bridge_deck_level, cell.level);
        assert!(!cell.bridge_walkable);
        // Destroyed232 uses the pristine TMP, which is also Road in this
        // synthetic catalog. It does not imply Water or Clear after collapse.
        assert_eq!(cell.base_land_type, LandType::Road.as_index());
        assert_eq!(cell.land_type, LandType::Road.as_index());
        assert!(
            !sim.path_grid()
                .unwrap()
                .is_walkable_on_layer(x, y, MovementLayer::Bridge)
        );
    }
}

/// Area-damage bridge admission reaches the concrete57CCF0/57D530 owner.
/// The native concrete_220_across_0 corpus supplies the damaged center triple
/// and longitudinal neighbors; publication must retain its complete raw result.
#[test]
fn test_bridge_walker_collapses_full_3_cell_strip_on_single_hit() {
    let (mut sim, rules, _registry, native) = concrete_damage_fixture("concrete_220_across_0", 300);
    let (rx, ry) = concrete_damage_fixture_coord(&native["input"]["start"]);
    let changed =
        crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            &mut sim,
            &rules,
            &[BridgeDamageEvent {
                rx,
                ry,
                damage: 15,
                warhead_ref: crate::sim::intern::InternedId::default(),
                is_ion_cannon: true,
                impact_z_leptons: 0,
            }],
            &mut crate::sim::world::FrameEffects::default(),
        );
    assert_eq!(changed, native["result"]["returned"].as_u64() == Some(1));
    assert_concrete_damage_result(&sim, &native);
    for x in 16..=18 {
        assert!(sim.dirty_cells.radar().contains(&(x, 15)));
    }
}

/// Structural576BA0/47E040 collapse must publish navigation immediately,
/// preserving unrelated foundations, pinned readers, order resumption and
/// the same derived projection after snapshot restoration.
#[test]
fn test_structural_bridge_collapse_preserves_dynamic_navigation_and_snapshot() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state, overlays) = structural_bridge_for_damage_dispatch();
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);
    sim.overlay_grid = Some(overlays);
    crate::sim::arena_fixture::supply_native_map(&mut sim);

    // This navigation fixture supplies a foundation at the native (0,0)
    // CanPlaceAt sentinel. Explicit synthetic PlaceAnywhere admits that
    // already-placed prior; its 4x3 ART and all blocker geometry stay fixed.
    // Retail Building placement admission is not what this test measures.
    let rules = combat_test_rules_with_override("[GACNST]\nPlaceAnywhere=yes\n");
    let mut building = make_test_entity("GACNST", EntityCategory::Structure);
    building.cell_x = 0;
    building.cell_y = 0;
    let mut mover = make_test_entity("MTNK", EntityCategory::Unit);
    mover.cell_x = 6;
    mover.cell_y = 0;
    assert_eq!(sim.spawn_from_map(&[building, mover], Some(&rules)), 2);
    sim.resolve_type_handles(&rules);
    // A Hover vehicle: the resumed order below reaches its Move_To through
    // the Unit setter.
    sim.substrate.entities.get_mut(2).unwrap().locomotor =
        Some(LocomotorState::for_test_kind(LocomotorKind::Hover));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let before_path = sim.path_grid_snapshot().unwrap();
    for ry in 0..3 {
        for rx in 0..4 {
            assert!(sim.substrate.occupancy.contains_entity(rx, ry, 1));
            assert!(!before_path.is_walkable(rx, ry));
        }
    }
    assert!(
        crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
            &mut sim,
            &rules,
            &[BridgeDamageEvent {
                rx: 5,
                ry: 5,
                damage: 15,
                warhead_ref: crate::sim::intern::InternedId::default(),
                is_ion_cannon: true,
                impact_z_leptons: 416, // Native structural deck over level0.
            }],
        )
    );
    assert_structural_bridge_collapsed(&sim);

    // A bridge refresh must publish the complete world projection before the
    // next reader, including unrelated foundations. No end-frame repair here.
    for ry in 0..3 {
        for rx in 0..4 {
            assert!(sim.substrate.occupancy.contains_entity(rx, ry, 1));
            assert!(
                !sim.path_grid().unwrap().is_walkable(rx, ry),
                "bridge collapse lost unrelated foundation blocker at {rx},{ry}"
            );
            assert!(!before_path.is_walkable(rx, ry), "pinned reader changed");
        }
    }
    let collapsed = sim.path_grid_snapshot().unwrap();
    assert_ne!(before_path.cell(5, 5), collapsed.cell(5, 5));
    // The pinned reader predates bridge fallout. Both receipt paths must
    // return and publish the current projection without mutating that Arc.
    for changed_cells in [&[][..], &[(6, 0)][..]] {
        sim.finish_terrain_navigation_changes(&rules, changed_cells);
        let tail = sim.path_grid_snapshot().unwrap();
        assert_eq!(tail.as_ref(), collapsed.as_ref());
        if changed_cells.is_empty() {
            assert!(
                Arc::ptr_eq(&tail, &collapsed),
                "no-change readers reuse the immutable projection"
            );
        }
        assert_eq!(sim.path_grid(), Some(collapsed.as_ref()));
        assert_ne!(before_path.cell(5, 5), tail.cell(5, 5));

        // An attack-move's resume (ContinueMegaMission) reaches the Hover
        // Move_To before any frame-final projection rebuild; its Process
        // searches the live grid.
        let unit = sim.substrate.entities.get_mut(2).unwrap();
        unit.seldom.mega_mission = crate::sim::mission::mega_mission::MegaMission::attack_move(
            Some(crate::sim::combat::TargetKind::Cell(6, 3)),
        );
        assert!(sim.foot_continue_mega_mission(
            2,
            &rules,
            &mut crate::sim::world::FrameEffects::default(),
        ));
        let unit = sim.substrate.entities.get(2).unwrap();
        assert_eq!(
            unit.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::cell(6, 3))
        );
        assert!(
            unit.locomotor
                .as_ref()
                .and_then(|loco| loco.hover_runtime())
                .is_some_and(|hover| hover.is_moving())
        );
    }

    // The persisted bridge/entity owners must reconstruct the same navigation
    // projection; a save/load must not be what repairs a lost foundation.
    let expected_path = sim.path_grid_snapshot().unwrap();
    let terrain_cache = sim.resolved_terrain.as_ref().unwrap().clone();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bridge-nav", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(terrain_cache, Default::default(), &rules);
    assert!(restored.rebuild_dynamic_navigation(&rules));
    assert_eq!(restored.path_grid(), Some(expected_path.as_ref()));
}

/// Repeatability of the actual concrete collapse and its longitudinal writes.
/// Native raw outputs reject a no-op; independent runs pin hash/RNG agreement.
#[test]
fn test_bridge_collapse_is_deterministic_under_replay() {
    fn run_one(seed: u64) -> (u64, crate::sim::rng::SimRngLogicalState) {
        let (mut sim, rules, _registry, native) =
            concrete_damage_fixture("concrete_220_across_0", 1500);
        sim.initialize_random_number_system(seed as u32);
        let before_hash = sim.state_hash();
        let scenario_before = sim.scenario_rng.logical_state();
        let main_before = sim.main_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        let (rx, ry) = concrete_damage_fixture_coord(&native["input"]["start"]);
        assert!(crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            &mut sim,
            &rules,
            &[BridgeDamageEvent {
                rx, ry, damage: 2000,
                warhead_ref: crate::sim::intern::InternedId::default(),
                is_ion_cannon: false, impact_z_leptons: 0,
            }],
            &mut crate::sim::world::FrameEffects::default(),
        ));
        assert_concrete_damage_result(&sim, &native);
        assert_ne!(sim.state_hash(), before_hash);
        assert_ne!(sim.scenario_rng.logical_state(), scenario_before);
        assert_eq!(sim.main_rng.logical_state(), main_before);
        assert_eq!(sim.mapgen_rng.logical_state(), mapgen_before);
        (sim.state_hash(), sim.scenario_rng.logical_state())
    }
    for seed in [0xCAFE_F00D, 0xFEED_BEEF] {
        assert_eq!(
            run_one(seed),
            run_one(seed),
            "concrete collapse replay seed{seed}"
        );
    }
}

/// Native489E87..48A2C4 direct205 admission supplies the one-draw receipt.
/// Join that gate to the real first-damage controller: raw cells change, while
/// its false return means there was no collapse or structural BlowUp/debris.
#[test]
fn test_bridge_dispatcher_consumes_one_path_gate_draw_per_non_ion_event() {
    let admission: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_damage_admission.json",
    ))
    .unwrap();
    let gate = admission["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == "direct_205")
        .unwrap();
    let strength = gate["ranged_calls"][0]["high"].as_i64().unwrap() as i32;
    let (mut sim, rules, _registry, native) =
        concrete_damage_fixture("concrete_205_across_0", strength);
    // Seed31 and damage2000 are the saved harness defaults for direct_205.
    sim.initialize_random_number_system(31);
    let before_hash = sim.state_hash();
    let main_before = sim.main_rng.logical_state();
    let mapgen_before = sim.mapgen_rng.logical_state();
    let (rx, ry) = concrete_damage_fixture_coord(&native["input"]["start"]);
    let collapsed =
        crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(
            &mut sim,
            &rules,
            &[BridgeDamageEvent {
                rx,
                ry,
                damage: 2000,
                warhead_ref: crate::sim::intern::InternedId::default(),
                is_ion_cannon: false,
                impact_z_leptons: 0,
            }],
            &mut crate::sim::world::FrameEffects::default(),
        );
    assert!(!collapsed);
    assert_concrete_damage_result(&sim, &native);
    assert_ne!(sim.state_hash(), before_hash);
    let state = sim.scenario_rng.logical_view();
    assert_eq!(
        serde_json::json!([state.index_a, state.index_b]),
        gate["rng_indices"]
    );
    assert_eq!(
        state.index_a as u64,
        gate["raw_draw_count"].as_u64().unwrap()
    );
    let mut continuation = sim.scenario_rng.clone();
    let next: Vec<_> = (0..4).map(|_| continuation.next_u32()).collect();
    assert_eq!(
        serde_json::json!(next),
        gate["next_rng"],
        "native gate continuation"
    );
    assert_eq!(sim.main_rng.logical_state(), main_before);
    assert_eq!(sim.mapgen_rng.logical_state(), mapgen_before);
}

/// Installs `resolved` and publishes its navigation (path grid, zones and
/// bridge records) with the Map Size that Hover's Find_Path reads.
fn install_water_move_fixture(
    sim: &mut Simulation,
    rules: &RuleSet,
    resolved: ResolvedTerrainGrid,
) -> PathGrid {
    install_rectangular_test_playfield(sim, resolved.width(), resolved.height());
    sim.playfield_size_height = Some(i32::from(resolved.height()));
    sim.resolved_terrain = Some(resolved);
    assert!(sim.rebuild_dynamic_navigation(rules));
    sim.path_grid().expect("published path grid").clone()
}

/// Orders the Hover water mover `id` to `cell` through the Unit setter and
/// runs frames until the order ends. Returns the cells it stood in.
fn drive_water_mover(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    id: u64,
    cell: (u16, u16),
) -> Vec<(u16, u16, bool)> {
    assert!(sim.set_unit_destination(
        id,
        crate::sim::components::NavTargetRef::cell(cell.0, cell.1),
        rules,
        true,
        &mut crate::sim::world::FrameEffects::default(),
    ));
    // Plain OverlayTypes 0..=25: the structural deck's anchor holds 25, and
    // Unit Can_Enter_Cell reads the type of any overlay it meets.
    let overlay_ini = IniFile::from_str(&format!(
        "[OverlayTypes]\n{}",
        (0..=25).map(|i| format!("{i}=O{i}\n")).collect::<String>()
    ));
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&overlay_ini, None);
    let mut visited = Vec::new();
    for _ in 0..600 {
        let _ = sim.advance_tick(&[], Some(rules), Some(grid), Some(&registry), 67);
        let mover = sim.substrate.entities.get(id).expect("mover");
        let here = (mover.position.rx(), mover.position.ry(), mover.on_bridge());
        if visited.last() != Some(&here) {
            visited.push(here);
        }
        if mover.navigation.nav_com().is_none() {
            return visited;
        }
    }
    panic!("the water mover's order never ended: {visited:?}");
}

/// A water mover crossing under a structural deck stays on the water layer
/// and passes; TooBigToFitUnderBridge (DRED) is rendering-only in retail.
#[test]
fn test_water_movers_pass_under_a_structural_bridge() {
    let rules = naval_bridge_test_rules();
    for mover in ["BOAT", "DRED"] {
        let mut sim = Simulation::new();
        let (resolved, bridge_state) = water_under_structural_bridge();
        sim.bridge_state = Some(bridge_state);
        let grid = install_water_move_fixture(&mut sim, &rules, resolved);
        let id = sim
            .spawn_object(mover, "Americans", 5, 3, 64, &rules)
            .expect("spawn water mover");

        let visited = drive_water_mover(&mut sim, &rules, &grid, id, (5, 7));
        assert_eq!(
            visited.last().map(|&(rx, ry, _)| (rx, ry)),
            Some((5, 7)),
            "{mover}"
        );
        assert!(
            visited.contains(&(5, 5, false)),
            "{mover} crosses under the deck: {visited:?}"
        );
        assert!(
            visited.iter().all(|&(_, _, on_bridge)| !on_bridge),
            "{mover} stays on the water layer: {visited:?}"
        );
    }
}
