//! Dynamic navigation publication and terrain refresh: structures, bibs and
//! bridges composing into navigation, active tube leaves preempting the mission
//! host, in-tick terrain-object refresh and radar terrain dirty marks.

use super::*;

#[test]
fn dynamic_navigation_publication_composes_structures_bibs_and_bridges() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAREFN\n\
         [GAREFN]\nStrength=100\nBib=yes\n",
        ),
        &IniFile::from_str("[GAREFN]\nFoundation=4x3\n"),
    )
    .expect("dynamic navigation rules");
    let mut terrain = gsi_04_10_clear_terrain(16, 16);
    for rx in [1, 3] {
        let cell = terrain.cell_mut_for_test(rx, 1).expect("bridgehead cell");
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    {
        let cell = terrain.cell_mut_for_test(2, 1).expect("bridge body cell");
        cell.ground_walk_blocked = true;
        cell.base_build_blocked = true;
        cell.bridge_walkable = true;
        cell.has_bridge_deck = true;
        cell.bridge_deck_level = 4;
    }
    // A legacy deck (no bit0x100) is intact while its +44 decodes.
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(16, 16);
    overlays.cell_mut_for_test(2, 1).overlay_id = Some(0xCD);

    let mut sim = Simulation::new();
    *sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        &terrain, true, 10,
    ));
    *sim.resolved_terrain = Some(terrain);
    *sim.overlay_grid = Some(overlays);
    let mut building = make_test_entity("GAREFN", EntityCategory::Structure);
    building.cell_x = 8;
    building.cell_y = 8;
    assert_eq!(sim.spawn_from_map(&[building], Some(&rules)), 1);
    let placed = sim.substrate.entities.values().next().unwrap();
    assert!(placed.lifecycle.cell_marked());
    assert!(
        sim.substrate
            .occupancy
            .contains_entity(8, 9, placed.stable_id())
    );

    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = sim.path_grid().expect("published navigation");
    assert!(!grid.is_walkable(8, 9));
    assert!(!grid.is_walkable(10, 9));
    assert!(grid.is_walkable(11, 9), "Bib must relax the east edge");
    assert!(grid.cell(1, 1).expect("west bridgehead").transition);
    assert!(grid.cell(2, 1).expect("bridge body").bridge_walkable);
    assert!(grid.cell(3, 1).expect("east bridgehead").transition);
    let normal_zones = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
        .expect("normal movement zones");
    assert_ne!(
        normal_zones.zone_at(0, 0, MovementLayer::Ground),
        crate::sim::pathfinding::zone_map::ZONE_INVALID,
        "canonical publication must assign a reachable ground cell"
    );

    sim.overlay_cell_mut_for_test(2, 1).overlay_id = Some(0xE8);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let collapsed_grid = sim.path_grid().expect("collapsed navigation publication");
    assert!(
        !collapsed_grid
            .cell(2, 1)
            .expect("collapsed bridge body")
            .bridge_walkable,
        "canonical publication must project the live bridge runtime state"
    );
}

#[test]
fn gsi_04_15_active_tube_leaf_preempts_unit_and_infantry_mission_host() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
         [InfantryTypes]\n0=TESTINF\n\
         [VehicleTypes]\n0=TESTUNIT\n\
         [AircraftTypes]\n[BuildingTypes]\n0=TESTBUILD\n\
         [TESTINF]\nSpeed=4\nPrimary=TESTGUN\n\
         [TESTUNIT]\nSpeed=4\nPrimary=TESTGUN\n\
         [TESTBUILD]\nStrength=100\nArmor=none\nPrimary=TESTGUN\nThreatPosed=30\n\
         [TESTGUN]\nDamage=1\nROF=100\nRange=6\nWarhead=TESTWH\n\
         [TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("TubeMovement host rules");

    for (category, type_name) in [
        (EntityCategory::Unit, "TESTUNIT"),
        (EntityCategory::Infantry, "TESTINF"),
    ] {
        let mut clear = gsi_04_10_clear_terrain(3, 1);
        clear.cells_mut_for_test()[0].tube_index = Some(TubeId(0));
        let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
            3,
            1,
            clear.cells().to_vec(),
            vec![TubeFact::explicit((0, 0), (1, 0), 2, vec![2])],
        );
        let path = PathGrid::from_resolved_terrain(&terrain);

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_name);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            1,
            0,
            0,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            type_ref,
            category,
            0,
            5,
            true,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(false);
        if category == EntityCategory::Unit {
            entity.seldom.mega_mission =
                crate::sim::mission::mega_mission::MegaMission::attack_move(Some(
                    crate::sim::combat::TargetKind::Cell(2, 0),
                ));
        } else {
            entity
                .attack_target
                .set_for_test(Some(TargetKind::Cell(0, 0)));
            entity
                .passive_scan
                .set_passively_acquired_target_for_test(true);
        }
        entity.locomotor = Some(LocomotorState::for_test_kind(match category {
            EntityCategory::Unit => LocomotorKind::Drive,
            EntityCategory::Infantry => LocomotorKind::Walk,
            _ => unreachable!("fixture is Foot only"),
        }));
        if category == EntityCategory::Unit {
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .install_drive_state_for_test(Some(DriveLocomotionRuntime::default()))
            );
        } else {
            entity
                .navigation
                .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(2)));
        }
        for _ in 0..41 {
            entity.mission.count_ai_visit();
        }
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
            TubeId(0),
            1,
            DriveCoord {
                x: 384,
                y: 128,
                z: 17,
            },
        ));
        sim.substrate.entities.insert(entity);
        let original_building_owner = sim.interner.intern("Russians");
        // Keep the later combat/capture target adjacent to the Tube exit.
        // A marked building on the exit itself correctly blocks finalization.
        let mut building = GameEntity::new_at_frame_zero_for_test(
            2,
            2,
            0,
            0,
            0,
            original_building_owner,
            crate::sim::combat::Health::new(100),
            sim.interner.intern("TESTBUILD"),
            EntityCategory::Structure,
            0,
            5,
            true,
        );
        building.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(building);
        // The player has seen the building: the campaign scan gate needs +0x41B.
        crate::sim::arena_fixture::discover(&mut sim, 2);
        sim.add_entity_occupancy(2);
        *sim.fog = crate::sim::vision::FogState::new(3, 1);
        crate::sim::vision::reveal_radius(&mut sim.fog, owner, 0, 0, 3);
        sim.set_logic_order_for_test(vec![1]);
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::LiveReadyInputProvider { rules: &rules },
        )
        .expect("queue remains pending through TubeMovement");
        *sim.resolved_terrain = Some(terrain);

        sim.advance_tick(&[], Some(&rules), Some(&path), None, 67);

        {
            let entity = sim.substrate.entities.get(1).expect("Tube mover survives");
            // The leaf's Per_Cell_Process(2) promotes the queue: the Unit
            // override (`0x0073603F`) at its arrival checkpoint
            // (`0x0073ACC2` Ready_To_Commence, `0x0073ACD1` Commence), the
            // Infantry one (`0x0051BA9B`) through its tail's Commence
            // (`0x0051A8C5`), which asks no Ready.
            assert_eq!(entity.mission.ai_counter(), 0, "{category:?}");
            assert_eq!(
                entity.mission.current(),
                MissionId::from_known(MissionType::Move),
                "{category:?}"
            );
            assert_eq!(entity.mission.queued(), MissionId::NONE, "{category:?}");
            assert!(entity.low_bridge_tube_state.is_none(), "{category:?}");
            assert!(entity.lifecycle.cell_marked(), "{category:?}");
            assert!(
                sim.substrate
                    .occupancy
                    .contains_entity(1, 0, entity.stable_id()),
                "{category:?} final Tube leaf runs exactly once and restores occupancy"
            );
            if category == EntityCategory::Unit {
                assert!(entity.attack_target.target().is_none());
                assert!(!entity.passive_scan.acquired());
                assert!(crate::sim::movement::motion_query::is_moving(&entity) != Some(true));
                assert_eq!(
                    entity.seldom.mega_mission.destination(),
                    Some(crate::sim::combat::TargetKind::Cell(2, 0))
                );
                assert!(entity.seldom.mega_mission.is_attack_move());
            } else {
                assert!(entity.passive_scan.acquired());
                assert!(matches!(
                    entity.attack_target.target(),
                    Some(crate::sim::combat::TargetKind::Cell(0, 0))
                ));
                assert_eq!(
                    entity.navigation.nav_com(),
                    Some(crate::sim::components::NavTargetRef::object(2)),
                    "Tube final retains the authoritative building destination"
                );
                assert!(
                    crate::sim::movement::motion_query::is_moving(&entity) != Some(true),
                    "Tube final does not walk into the adjacent building"
                );
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(2)
                        .map(|building| building.owner()),
                    Some(original_building_owner),
                    "Tube final does not run a building-entry receiver beside the destination"
                );
                assert!(
                    sim.substrate
                        .entities
                        .get(2)
                        .is_some_and(|building| building
                            .building
                            .pending_c4_detonation
                            .pending()
                            .is_none()),
                    "Tube final returns before Mission_Enter; no C4 claim is made"
                );
            }
        }
        if category == EntityCategory::Unit {
            // `TESTBUILD` carries `Primary=`/`ThreatPosed=30` so that it stays a
            // legal auto-acquire target: the human-attacker building gate in
            // `TechnoClass::Evaluate_Candidate @ 0x006F85AB` refuses an enemy
            // building with no weapon or no posed threat, which is a targeting
            // fact this Tube-ordering test is not about.
            sim.foot_arrival_target_handler(1, &rules, &mut ObjectAiCtx::default());
            assert!(matches!(
                sim.substrate
                    .entities
                    .get(1)
                    .and_then(|entity| entity.attack_target.target()),
                Some(crate::sim::combat::TargetKind::Entity(2))
            ));
        }
    }
}

fn gsi_04_10_terrain_object(
    sim: &mut Simulation,
    stable_id: u64,
    cell: (u16, u16),
    occupation_bits: u8,
) -> crate::sim::terrain_object::TerrainObjectState {
    {
        let mut terrain = crate::sim::terrain_object::TerrainObjectState::for_test(
            stable_id,
            sim.interner.intern("TREE01"),
            cell.0,
            cell.1,
        );
        terrain.set_health_for_test(10);
        terrain.set_max_health_for_test(10);
        terrain.set_occupation_bits_for_test(occupation_bits);
        terrain
    }
}

#[test]
fn gsi_04_10_in_tick_refresh_updates_the_tail_path_before_consumers() {
    use crate::sim::terrain_object::{mark_terrain_occupation, unmark_terrain_occupation};

    let mut sim = Simulation::new();
    *sim.resolved_terrain = Some(gsi_04_10_clear_terrain(2, 1));
    let tree = gsi_04_10_terrain_object(&mut sim, 1, (0, 0), 7);
    mark_terrain_occupation(&tree, sim.resolved_terrain.as_mut());
    let mut input_path_grid = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
        sim.overlay_grid.as_ref(),
    );
    input_path_grid.set_blocked(1, 0, true);
    assert!(!input_path_grid.is_walkable(0, 0));

    unmark_terrain_occupation(&tree, sim.resolved_terrain.as_mut());
    *sim.path_grid = Some(Arc::new(input_path_grid));
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    sim.finish_terrain_navigation_changes(&rules, &[(0, 0)]);
    let phase_six_consumer_grid = sim.path_grid_snapshot().expect("tail grid");

    assert!(phase_six_consumer_grid.is_walkable(0, 0));
    assert_eq!(phase_six_consumer_grid.terrain_object_cell_bits_at(0, 0), 0);
    assert!(phase_six_consumer_grid.is_walkable_for_infantry(0, 0));
    assert!(
        !phase_six_consumer_grid.is_walkable(1, 0),
        "unrelated dynamic blockers from the input grid must survive"
    );
}

#[test]
fn gsi_04_10_zero_occupation_removal_forces_ground_zone_with_same_walkability() {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;
    use crate::sim::terrain_object::{mark_terrain_occupation, unmark_terrain_occupation};

    let mut sim = Simulation::new();
    *sim.resolved_terrain = Some(gsi_04_10_clear_terrain(1, 1));
    let tree = gsi_04_10_terrain_object(&mut sim, 1, (0, 0), 0);
    mark_terrain_occupation(&tree, sim.resolved_terrain.as_mut());
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(0, 0)
            .unwrap()
            .zone_type,
        zone_class::BUILDING
    );
    let input_path_grid = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
        sim.overlay_grid.as_ref(),
    );
    assert!(input_path_grid.is_walkable(0, 0));
    sim.rebuild_zone_grid_full(std::sync::Arc::new(input_path_grid.clone()));
    assert_eq!(
        sim.zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "OccupationBits=0 is a reduced Building zone even though PathGrid is walkable"
    );

    unmark_terrain_occupation(&tree, sim.resolved_terrain.as_mut());
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    sim.finish_terrain_navigation_changes(&rules, &[(0, 0)]);
    let tail_path_grid = sim.path_grid_snapshot().expect("tail grid");

    assert_eq!(*tail_path_grid, input_path_grid);
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(0, 0)
            .unwrap()
            .zone_type,
        zone_class::GROUND
    );
    assert_ne!(
        sim.zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "Limbo's AssignOrphaned must observe the reduced-zone change despite identical PathGrid cells"
    );
}

/// The radar-terrain batch de-duplicates across marks, the generation moves
/// only when a cell is new, and an acknowledged cell re-arms.
#[test]
fn radar_terrain_dirty_marks_deduplicate_and_clear() {
    let mut sim = Simulation::new();
    sim.dirty_cells.mark_radar([(1, 1), (2, 2), (1, 1)]);
    assert_eq!(sim.dirty_cells.radar(), [(1, 1), (2, 2)]);
    let generation = sim.dirty_cells.radar_generation();
    sim.dirty_cells.mark_radar([(2, 2)]);
    assert_eq!(sim.dirty_cells.radar_generation(), generation);
    sim.dirty_cells.mark_radar([(3, 3)]);
    sim.dirty_cells.mark_radar([(3, 3), (4, 4)]);
    assert_eq!(sim.dirty_cells.radar(), [(1, 1), (2, 2), (3, 3), (4, 4)]);
    assert!(!sim.dirty_cells.take_radar().is_empty());
    sim.dirty_cells.mark_radar([(1, 1)]);
    assert_eq!(sim.dirty_cells.radar(), [(1, 1)]);
}
