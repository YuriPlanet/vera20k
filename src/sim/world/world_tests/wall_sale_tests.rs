//! Wall-sale tests: detach and restore of archived targets, ordered cleanup,
//! fixed-stride alias reversal, the eligibility gate and the first-match and
//! human-player gates.

use super::*;

#[test]
fn wall_sale_detach_restores_without_resetting_the_archived_targets_burst() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::MissionDispatchTimer;
    use crate::sim::mission::state::MissionTestFixture;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 77);
    let wall_owner = sim.interner.get("Americans").unwrap();
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
    grid.place_owned_wall(4, 4, overlays.id_for_name("GAWALL").unwrap(), 0, wall_owner);
    sim.overlay_grid = Some(grid);
    let mut actors = Vec::new();
    for x in [2, 3] {
        let id = sim
            .spawn_object("MTNK", "Americans", x, 2, 0, &rules)
            .unwrap();
        actors.push(id);
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor
            .attack_target
            .set_for_test(Some(TargetKind::Cell(6, 6)));
        actor
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Cell { rx: 7, ry: 7 }));
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Move),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        // Use the actual archive producer, not a supplied suspended target.
        assert!(sim.mission_override_movement_blocker(
            id,
            TargetKind::Cell(4, 4),
            &rules,
            &mut FrameEffects::default(),
        ));
        // Nonzero burst is an explicit component prior. Native6FCF5B resets
        // it only if the changed-to-NULL setter actually runs; stock MTNK's
        // single-shot burst does not by itself expose that difference.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .seldom
            .weapon_burst
            .complete_shot(2);
    }
    assert!(sim.apply_command_with_overlays(
        None,
        "Americans",
        &Command::SellWallAtCell { x: 4, y: 4 },
        &rules,
        Some(&overlays),
        &mut FrameEffects::default(),
    ));
    for id in actors {
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.attack_target.target(), Some(TargetKind::Cell(6, 6)));
        assert_eq!(
            actor.navigation.nav_com(),
            Some(NavTargetRef::Cell { rx: 7, ry: 7 })
        );
        assert_eq!(
            actor.mission.current(),
            MissionId::from_known(MissionType::Move)
        );
        assert_eq!(actor.mission.suspended(), MissionId::NONE);
        assert_eq!(
            actor.seldom.weapon_burst.index(),
            1,
            "4FD018 calls70D4A0: Restore replaces the wall target before the conditional NULL setter"
        );
    }
}

#[test]
fn gsi_04_07_wall_sell_ordered_cleanup_detach_navigation_and_zero_refund_rng() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, true);
    assert!(!rules.object("FIRSTWALL").unwrap().click_repairable);
    let mut sim = Simulation::with_seed(77);
    let (wall_owner, receiver) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let credits_before = sim.houses.get(&wall_owner).unwrap().economy().credits();
    let rng_before = sim.scenario_rng.state();

    let terrain = gsi_04_10_clear_terrain(8, 8);
    let mut retained_wall_counts = vec![0u8; 64];
    let adjust_source = |counts: &mut [u8], source: (u16, u16), add: bool| {
        const ADJACENT_8: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        for (dx, dy) in ADJACENT_8 {
            let x = (source.0 as i16).wrapping_add(dx);
            let y = (source.1 as i16).wrapping_add(dy);
            let Some(index) = terrain.native_fixed_cell_index(x, y) else {
                continue;
            };
            counts[index] = if add {
                counts[index].wrapping_add(1)
            } else {
                counts[index].wrapping_sub(1)
            };
        }
    };
    for source in [(4, 4), (4, 3), (5, 4)] {
        adjust_source(&mut retained_wall_counts, source, true);
    }
    let mut expected_after_sale = retained_wall_counts.clone();
    for cleanup_removed_source in [(4, 3), (5, 4)] {
        adjust_source(&mut expected_after_sale, cleanup_removed_source, false);
    }
    let mut grid = crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
        crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
            8,
            8,
            vec![(-1, 0); 64],
            retained_wall_counts,
        ),
    );
    grid.place_owned_wall(4, 4, 2, 0x03, wall_owner);
    // Damaged GAWALLs connected south/west to the sold cell. Sale cleanup
    // removes north first, then east, preserving each stale owner.
    grid.place_owned_wall(4, 3, 2, 0x24, wall_owner);
    grid.place_owned_wall(5, 4, 2, 0x28, wall_owner);
    sim.overlay_grid = Some(grid);
    sim.resolved_terrain = Some(terrain);
    {
        let grid = sim.overlay_grid.as_mut().unwrap();
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for cell in [(4, 4), (4, 3), (5, 4)] {
            let _ = crate::sim::overlay_grid::recalc_overlay_passability(
                grid, terrain, &overlays, cell.0, cell.1,
            );
        }
        let _ = grid.take_dirty_cells();
    }
    let path = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
    assert!(!path.is_walkable(4, 4));
    assert!(!path.is_walkable(4, 3));
    assert!(!path.is_walkable(5, 4));
    sim.rebuild_zone_grid_full(std::sync::Arc::new(path.clone()));
    let ground_zone_before = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
        .expect("normal zone map");
    assert_eq!(
        ground_zone_before.zone_at(4, 3, MovementLayer::Ground),
        crate::sim::pathfinding::zone_map::ZONE_INVALID,
        "the north cleanup candidate starts blocked and unassigned"
    );
    assert_eq!(
        ground_zone_before.zone_at(5, 4, MovementLayer::Ground),
        crate::sim::pathfinding::zone_map::ZONE_INVALID,
        "the east cleanup candidate starts blocked and unassigned"
    );
    let expected_ground_zone = ground_zone_before.zone_at(4, 2, MovementLayer::Ground);
    assert_ne!(
        expected_ground_zone,
        crate::sim::pathfinding::zone_map::ZONE_INVALID
    );
    sim.path_grid = Some(std::sync::Arc::new(path.clone()));

    for (id, target) in [(10, (4, 4)), (20, (4, 3))] {
        let mut listener = GameEntity::test_default(id, "E1", "Receiver", 2, 2);
        listener.set_owner_for_test(receiver);
        listener.set_type_ref_for_test(sim.interner.intern("E1"));
        listener
            .attack_target
            .set_for_test(Some(TargetKind::Cell(target.0, target.1)));
        sim.substrate.entities.insert(listener);
    }
    let hash_before_sale = sim.state_hash();
    super::world_commands::clear_wall_sell_zone_repair_test_trace();

    assert!(sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 4, y: 4 },
        &rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));

    let sold = sim.overlay_grid.as_ref().unwrap().cell(4, 4);
    assert_eq!(
        (sold.overlay_id, sold.overlay_data, sold.wall_owner),
        (None, 0, None)
    );
    let cleanup = sim.overlay_grid.as_ref().unwrap().cell(4, 3);
    assert_eq!(cleanup.overlay_id, None);
    assert_eq!(cleanup.wall_owner, Some(wall_owner));
    let east_cleanup = sim.overlay_grid.as_ref().unwrap().cell(5, 4);
    assert_eq!(east_cleanup.overlay_id, None);
    assert_eq!(east_cleanup.wall_owner, Some(wall_owner));
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .retained_neighbor_counts(),
        expected_after_sale.as_slice(),
        "native sale leaves the sold wall contribution stale and reverses only cleanup removals"
    );
    assert_eq!(
        expected_after_sale[5 * 8 + 3],
        1,
        "a cell adjacent only to the sold source proves that source was not decremented"
    );
    assert_eq!(
        sim.dirty_cells.tactical_for_test(),
        vec![(4, 3), (5, 4), (4, 5), (3, 4), (4, 4)]
    );
    assert_eq!(sim.dirty_cells.radar(), sim.dirty_cells.tactical_for_test());
    assert_eq!(
        sim.dirty_cells.radar_generation(),
        5,
        "sale publishes each first-unique cleanup radar call before recompute; the sold-cell tail is a duplicate"
    );
    assert!(
        sim.substrate
            .entities
            .get(10)
            .unwrap()
            .attack_target
            .target()
            .is_none()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(20)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Cell(4, 3)),
        "sale sweeps its source Cell; recursive cleanup does not sweep the neighbor"
    );
    assert!(sim.path_grid.as_deref().unwrap().is_walkable(4, 4));
    assert!(sim.path_grid.as_deref().unwrap().is_walkable(4, 3));
    assert!(sim.path_grid.as_deref().unwrap().is_walkable(5, 4));
    let projected = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("terrain"),
        sim.overlay_grid.as_ref(),
    );
    assert_eq!(
        sim.path_grid.as_deref().unwrap().diff_cells(&projected),
        Some(Vec::new()),
        "wall-sale tail must already publish the final path cells"
    );
    assert!(
        sim.zone_grid
            .as_ref()
            .expect("zone grid")
            .movement_classes_match(sim.resolved_terrain.as_ref().expect("terrain")),
        "ordered wall-sale repair must publish every reduced movement class"
    );
    let (zone_ids_ptr, repaired_zone_ids) = {
        let map = sim
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
            .expect("normal zone map before frame finalization");
        (
            map.zone_ids_slice().as_ptr() as usize,
            map.zone_ids_slice().to_vec(),
        )
    };
    assert!(
        sim.finalize_frame_overlays_and_navigation(&rules, Some(&overlays), false, false)
            .is_empty(),
        "the sold and cleanup cells are cleared, so no occupied render update remains"
    );
    let ground_zone_after = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
        .expect("normal zone map");
    assert_eq!(
        ground_zone_after.zone_ids_slice().as_ptr() as usize,
        zone_ids_ptr,
        "diff-empty finalization must retain the ordered wall-sale zone repair"
    );
    assert_eq!(ground_zone_after.zone_ids_slice(), repaired_zone_ids);
    assert_eq!(
        ground_zone_after.zone_at(4, 3, MovementLayer::Ground),
        expected_ground_zone,
        "cleanup-created removal receives its own orphan/graph repair"
    );
    assert_eq!(
        ground_zone_after.zone_at(4, 4, MovementLayer::Ground),
        expected_ground_zone,
        "the House sale tail separately repairs the sold cell"
    );
    assert_eq!(
        ground_zone_after.zone_at(5, 4, MovementLayer::Ground),
        expected_ground_zone,
        "the east cleanup removal is repaired after north"
    );
    let repair_trace = super::world_commands::take_wall_sell_zone_repair_test_trace();
    assert_eq!(
        repair_trace
            .iter()
            .map(|step| step.repair_cell)
            .collect::<Vec<_>>(),
        vec![(4, 3), (5, 4), (4, 4)],
        "cleanup repairs N then E; the sold cell remains the House tail"
    );
    assert_eq!(
        repair_trace[0].walkable_cross,
        [true, false, true, true, true],
        "north repair sees sold+north open while east remains blocked"
    );
    assert_eq!(
        repair_trace[0].movement_class_cross,
        [
            crate::map::resolved_terrain::zone_class::GROUND,
            crate::map::resolved_terrain::zone_class::WALL,
            crate::map::resolved_terrain::zone_class::GROUND,
            crate::map::resolved_terrain::zone_class::GROUND,
            crate::map::resolved_terrain::zone_class::GROUND,
        ]
    );
    for step in &repair_trace[1..] {
        assert_eq!(step.walkable_cross, [true; 5]);
        assert_eq!(
            step.movement_class_cross,
            [crate::map::resolved_terrain::zone_class::GROUND; 5]
        );
    }
    assert_eq!(
        sim.houses.get(&wall_owner).unwrap().economy().credits(),
        credits_before
    );
    assert_eq!(sim.scenario_rng.state(), rng_before);
    assert_ne!(sim.state_hash(), hash_before_sale);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::WallSold { receiver: event_receiver }] if *event_receiver == receiver
    ));
}

#[test]
fn wall_sale_cleanup_reaches_fixed_stride_alias_and_reverses_that_source_only() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut terrain = gsi_04_10_clear_terrain(512, 2);
    let mut retained = vec![0u8; 1024];
    let adjust_source = |counts: &mut [u8], source: (u16, u16), add: bool| {
        const ADJACENT_8: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        for (dx, dy) in ADJACENT_8 {
            let Some(index) = terrain.native_fixed_cell_index(
                (source.0 as i16).wrapping_add(dx),
                (source.1 as i16).wrapping_add(dy),
            ) else {
                continue;
            };
            counts[index] = if add {
                counts[index].wrapping_add(1)
            } else {
                counts[index].wrapping_sub(1)
            };
        }
    };
    for source in [(0, 1), (511, 0)] {
        adjust_source(&mut retained, source, true);
    }
    let mut expected = retained.clone();
    adjust_source(&mut expected, (511, 0), false);

    let mut grid = crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
        crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
            512,
            2,
            vec![(-1, 0); 1024],
            retained,
        ),
    );
    grid.place_owned_wall(0, 1, 2, 0x08, wall_owner);
    grid.place_owned_wall(511, 0, 2, 0x22, wall_owner);
    for (rx, ry) in [(0, 1), (511, 0)] {
        let _ = crate::sim::overlay_grid::recalc_overlay_passability(
            &mut grid,
            &mut terrain,
            &overlays,
            rx,
            ry,
        );
    }
    sim.overlay_grid = Some(grid);
    sim.resolved_terrain = Some(terrain);

    assert!(sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 1 },
        &rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));
    let grid = sim.overlay_grid.as_ref().expect("overlay authority");
    assert_eq!(grid.cell(0, 1).overlay_id, None);
    assert_eq!(grid.cell(511, 0).overlay_id, None);
    assert_eq!(
        grid.retained_neighbor_counts(),
        expected.as_slice(),
        "sale keeps the sold aliasing source but reverses the cleanup-removed aliased source"
    );
    assert!(sim.dirty_cells.tactical_for_test().contains(&(511, 0)));
    assert!(sim.dirty_cells.radar().contains(&(511, 0)));
}

#[test]
fn gsi_04_07_wall_sell_eligibility_gate_matrix_rejects_without_mutation() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(3, 3));

    let sell = |sim: &mut Simulation, rules: &RuleSet| {
        sim.apply_command_with_overlays(
            None,
            "Receiver",
            &Command::SellWallAtCell { x: 1, y: 1 },
            rules,
            Some(&overlays),
            &mut crate::sim::world::FrameEffects::default(),
        )
    };

    assert!(!sell(&mut sim, &rules), "absent overlay rejects");
    sim.overlay_grid.as_mut().unwrap().place_overlay(1, 1, 2, 0);
    assert!(!sell(&mut sim, &rules), "absent owner rejects");

    let missing_house = sim.interner.intern("MissingHouse");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(1, 1, 2, 0, missing_house);
    assert!(!sell(&mut sim, &rules), "unregistered owner rejects");

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(1, 1, 0, 0, wall_owner);
    assert!(!sell(&mut sim, &rules), "non-wall overlay rejects");

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(1, 1, 2, 0, wall_owner);
    let no_art_match = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=FIRSTWALL\n\
         [FIRSTWALL]\nWall=yes\nCost=100\n",
    ))
    .unwrap();
    assert!(
        !sell(&mut sim, &no_art_match),
        "missing ToOverlay match rejects"
    );
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(1, 1).overlay_id,
        Some(2)
    );
}

#[test]
fn gsi_04_07_wall_sell_first_match_and_split_human_gate_are_exact() {
    let (unsellable_rules, overlays) = gsi_04_07_wall_sell_rules(true, true);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
    grid.place_owned_wall(0, 2, 2, 0, wall_owner);
    sim.overlay_grid = Some(grid);
    assert!(!sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        &unsellable_rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(0, 2).overlay_id,
        Some(2)
    );

    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    assert!(sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        &rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));
    assert!(sim.sound_events.is_empty());

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(0, 2, 2, 0, wall_owner);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    assert!(!sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        &rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));
    sim.houses
        .get_mut(&wall_owner)
        .unwrap()
        .set_is_human_for_test(true);
    assert!(sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        &rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));
    assert!(!sim.apply_command_with_overlays(
        None,
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 0 },
        &rules,
        Some(&overlays),
        &mut crate::sim::world::FrameEffects::default(),
    ));
}
