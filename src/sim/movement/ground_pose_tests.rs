//! Production movement regressions for Object Z write cadence.

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, MovementTarget, ShipLocomotionRuntime,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

fn terrain() -> ResolvedTerrainGrid {
    ResolvedTerrainGrid::from_cells(
        8,
        8,
        (0..8)
            .flat_map(|y| (0..8).map(move |x| cell(x, y)))
            .collect(),
    )
}

fn mover(sim: &mut Simulation, kind: LocomotorKind) -> GameEntity {
    let mut entity = GameEntity::test_default(1, "MOVER", "Americans", 3, 3);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("MOVER");
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity.facing = 0;
    entity.position.exact_z_leptons = Some(731);
    entity.movement_target = Some(MovementTarget {
        path: vec![(3, 3), (3, 2), (3, 1)],
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        // Gives budget4 in both Drive's integer division and Ship's existing
        // fixed-point frame product (60 * fixed(1/15) truncates to3).
        speed: SimFixed::from_num(61),
        current_speed: SimFixed::from_num(61),
        move_dir_x: SIM_ZERO,
        move_dir_y: SimFixed::from_num(-256),
        move_dir_len: SimFixed::from_num(256),
        final_goal: Some((3, 1)),
        ..Default::default()
    });
    match kind {
        LocomotorKind::Drive => {
            entity.foot_speed.applied_fraction = SIM_ONE;
            entity.drive_locomotion = Some(DriveLocomotionRuntime {
                target_speed_fraction: SIM_ONE,
                ..Default::default()
            })
        }
        LocomotorKind::Ship => {
            entity.foot_speed.applied_fraction = SIM_ONE;
            entity.ship_locomotion = Some(ShipLocomotionRuntime {
                target_speed_fraction: SIM_ONE,
                ..Default::default()
            })
        }
        LocomotorKind::Walk => {
            entity.category = EntityCategory::Infantry;
            entity.is_voxel = false;
            entity.sub_cell = Some(0);
        }
        _ => unreachable!(),
    }
    entity
}

fn seed_track(entity: &mut GameEntity, kind: LocomotorKind, cursor: i32, head: DriveCoord) {
    let track = crate::sim::components::TrackProgress {
        turn_index: 0,
        cursor,
        reversed: false,
        residual: 0,
    };
    match kind {
        LocomotorKind::Drive => {
            let state = entity.drive_locomotion.get_or_insert_with(Default::default);
            state.head_to = Some(head);
            state.track = track;
            state.track_valid = true;
        }
        LocomotorKind::Ship => {
            let state = entity.ship_locomotion.get_or_insert_with(Default::default);
            state.head_to = Some(head);
            state.track = track;
            state.track_valid = true;
        }
        _ => unreachable!(),
    }
}

fn track(entity: &GameEntity) -> crate::sim::components::TrackProgress {
    match entity.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => entity.drive_locomotion.as_ref().unwrap().track,
        LocomotorKind::Ship => entity.ship_locomotion.as_ref().unwrap().track,
        _ => unreachable!(),
    }
}

fn insert(sim: &mut Simulation, mut entity: GameEntity) {
    // This fixture inserts a live object-list entry, so its lifecycle mark
    // must agree. Forced terminal relinking intentionally respects this flag.
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity.occupancy_enter_order = sim.substrate.next_occupancy_enter_order.next();
    sim.substrate.occupancy.add(
        entity.position.rx,
        entity.position.ry,
        1,
        if entity.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        },
        entity.sub_cell,
        crate::sim::occupancy::CellListInsertion::from_category(entity.category),
    );
    sim.substrate.entities.insert(entity);
}

fn tick(sim: &mut Simulation, terrain: &ResolvedTerrainGrid, grid: &PathGrid, frame: u32) {
    tick_with_rules(sim, terrain, grid, frame, None);
}

fn tick_with_rules(
    sim: &mut Simulation,
    terrain: &ResolvedTerrainGrid,
    grid: &PathGrid,
    frame: u32,
    rules: Option<&RuleSet>,
) {
    super::movement_tick::tick_movement_object_with_grids(
        &mut sim.substrate.entities,
        1,
        Some(grid),
        &Default::default(),
        &Default::default(),
        &mut sim.substrate.occupancy,
        &mut sim.substrate.cell_occupation,
        &mut sim.substrate.raw_cell_occupation,
        &mut sim.substrate.next_occupancy_enter_order,
        &mut sim.scenario_rng,
        u64::from(frame),
        frame,
        None,
        Some(terrain),
        None,
        None,
        None,
        &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        SIM_ZERO,
        9,
        60,
        &mut sim.interner,
        rules,
        &mut Vec::new(),
        &mut Vec::new(),
    );
}

#[test]
fn drive_ship_paid_points_sample_before_residual_xy_and_no_paid_point_retains_raw_z() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut terrain = terrain();
        terrain.cell_mut(3, 3).unwrap().slope_type = 2;
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let mut sim = Simulation::with_seed(3);
        let mut entity = mover(&mut sim, kind);
        seed_track(&mut entity, kind, 1, DriveCoord::cell(3, 2, 731));
        // Retail straight-north point0 is (0,245); headY=-128 gives117.
        entity.position.sub_y = SimFixed::from_num(117);
        insert(&mut sim, entity);
        let rng_before = sim.scenario_rng.state();

        tick(&mut sim, &terrain, &grid, 0); // budget4: residual only
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(track(entity).cursor, 1, "{kind:?}");
        assert_eq!(entity.position.sub_y.to_num::<i32>(), 111, "{kind:?}");
        assert_eq!(entity.position.exact_z_leptons, Some(731), "{kind:?}");

        tick(&mut sim, &terrain, &grid, 1); // budget8: point1, residual1
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(track(entity).cursor, 2, "{kind:?}");
        assert_eq!(entity.position.sub_y.to_num::<i32>(), 105, "{kind:?}");
        let paid_z = crate::util::lepton::ground_height_leptons(0, 2, 896, 874).unwrap();
        let final_xy_z = crate::util::lepton::ground_height_leptons(0, 2, 896, 873).unwrap();
        assert_eq!(entity.position.exact_z_leptons, Some(paid_z), "{kind:?}");
        // The native slope kernel is already oracle-covered. This test checks
        // the production caller selects point1's Y106, before residual Y105.
        assert_eq!(
            sim.scenario_rng.state(),
            rng_before,
            "height updates draw no RNG"
        );
        assert_ne!(
            paid_z, final_xy_z,
            "fixture distinguishes paid and residual samples"
        );
    }
}

#[test]
fn residual_bridge_crossing_preserves_z_and_defers_path_consumption_until_paid_point() {
    for leaving in [false, true] {
        let mut terrain = terrain();
        let mut grid = PathGrid::from_resolved_terrain(&terrain);
        for (y, bridge) in [(3, leaving), (2, !leaving), (1, !leaving)] {
            let level = if bridge { 0 } else { 4 };
            let cell = terrain.cell_mut(3, y).unwrap();
            cell.level = level;
            cell.bridge_facts.raw_flags = if bridge {
                crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
            } else {
                0
            };
            grid.set_bridge_cell_decoupled_for_test(3, y, level, bridge, bridge, 4, false);
        }
        let mut sim = Simulation::with_seed(3);
        let mut entity = mover(&mut sim, LocomotorKind::Drive);
        entity.on_bridge = leaving;
        entity.position.z = 4;
        entity.position.sub_y = SimFixed::from_num(7);
        let start_layer = if leaving {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let end_layer = if leaving {
            MovementLayer::Ground
        } else {
            MovementLayer::Bridge
        };
        entity.locomotor.as_mut().unwrap().layer = start_layer;
        let target = entity.movement_target.as_mut().unwrap();
        target.speed = SimFixed::from_num(105); // budget7, strict paid gate >7
        target.path_layers = vec![start_layer, end_layer, end_layer];
        // This route continues beyond the first retained segment. Native
        // owner NavCom must survive that terminal; a route without it admits
        // EnterIdleMode and its Foot SetSpeedFraction(0) at that first end.
        super::navcom::set_destination_internal_cell(&mut entity, (3, 1), Some(&terrain));
        assert_eq!(
            entity.drive_locomotion.as_ref().unwrap().destination,
            Some(DriveCoord::cell(3, 1, 416))
        );
        // Retained cursor 11 is the next sample after paid point10 at Y7.
        seed_track(
            &mut entity,
            LocomotorKind::Drive,
            11,
            DriveCoord::cell(3, 2, 731),
        );
        insert(&mut sim, entity);

        tick(&mut sim, &terrain, &grid, 0);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!((entity.position.rx, entity.position.ry), (3, 2));
        // Native residual corpus: wallet7 stores factor0x3F7FFFFF, so an
        // eleven-lepton negative delta truncates to -10 rather than -11.
        assert_eq!(entity.position.sub_y.to_num::<i32>(), 253);
        assert_eq!(entity.position.exact_z_leptons, Some(731));
        assert_eq!(entity.on_bridge, !leaving);
        assert_eq!(entity.movement_target.as_ref().unwrap().next_index, 1);
        assert_eq!(
            track(entity).cursor,
            11,
            "residual relinking does not pay a point"
        );
        assert_eq!(sim.substrate.occupancy.count_on_layer(3, 3, start_layer), 0);
        assert_eq!(sim.substrate.occupancy.count_on_layer(3, 2, end_layer), 1);
        let order_after_residual = entity.occupancy_enter_order;

        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .movement_target
            .as_mut()
            .unwrap()
            .speed = SimFixed::from_num(30);
        tick(&mut sim, &terrain, &grid, 1);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(entity.position.exact_z_leptons, Some(416));
        assert_eq!(entity.movement_target.as_ref().unwrap().next_index, 2);
        assert_eq!(
            entity.occupancy_enter_order, order_after_residual,
            "no duplicate cell entry at paid point"
        );
        assert_eq!(entity.locomotor.as_ref().unwrap().layer, end_layer);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .movement_target
            .as_mut()
            .unwrap()
            .speed = SimFixed::from_num(105);
        tick(&mut sim, &terrain, &grid, 2);
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(track(entity).cursor, 13);
        assert_eq!(entity.movement_target.as_ref().unwrap().next_index, 2);
        assert_eq!(entity.position.exact_z_leptons, Some(416));
        for frame in 3..160 {
            tick(&mut sim, &terrain, &grid, frame);
            if sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .movement_target
                .is_none()
            {
                break;
            }
        }
        let entity = sim.substrate.entities.get(1).unwrap();
        assert!(
            entity.movement_target.is_none(),
            "residual rebasing must not strand the path cursor: leaving={leaving}, xyz={:?}, track={:?}, speed={:?}, target={:?}",
            ground_pose::position_world_coord(&entity.position),
            track(entity),
            entity.foot_speed,
            entity.movement_target,
        );
        assert!(entity.navigation.nav_com.is_none());
        assert!(
            entity
                .drive_locomotion
                .as_ref()
                .unwrap()
                .destination
                .is_none()
        );
        assert_eq!((entity.position.rx, entity.position.ry), (3, 1));
        assert_eq!(entity.position.exact_z_leptons, Some(416));
        assert_eq!(sim.substrate.occupancy.count_on_layer(3, 1, end_layer), 1);
    }
}

#[test]
fn walking_subcell_motion_refreshes_exact_ramp_height() {
    let mut terrain = terrain();
    terrain.cell_mut(3, 3).unwrap().slope_type = 2;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::with_seed(3);
    let entity = mover(&mut sim, LocomotorKind::Walk);
    let before = ground_pose::position_world_coord(&entity.position);
    insert(&mut sim, entity);
    // Native fresh-head Process75BCBD returns before numeric movement. The
    // subsequent paid-head turns own this test's surface-height writes.
    tick(&mut sim, &terrain, &grid, 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(ground_pose::position_world_coord(&entity.position), before);
    assert!(entity.locomotor.as_ref().unwrap().step_head().is_some());
    for frame in 1..4 {
        tick(&mut sim, &terrain, &grid, frame);
        let entity = sim.substrate.entities.get(1).unwrap();
        let xy = ground_pose::position_world_xy(&entity.position);
        let expected = crate::util::lepton::ground_height_leptons(0, 2, xy[0], xy[1]).unwrap();
        assert_eq!(entity.position.exact_z_leptons, Some(expected));
        assert_ne!(entity.position.exact_z_leptons, Some(731));
    }
}

#[test]
fn moving_ramp_snapshot_continues_residual_bridge_crossing_through_paid_points() {
    use crate::sim::snapshot::GameSnapshot;

    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut terrain = terrain();
        terrain.cell_mut(3, 3).unwrap().level = 4;
        terrain.cell_mut(3, 3).unwrap().slope_type = 2;
        terrain.cell_mut(3, 2).unwrap().slope_type = 2;
        let mut grid = PathGrid::from_resolved_terrain(&terrain);
        grid.set_bridge_cell_decoupled_for_test(3, 2, 0, true, true, 4, true);
        let mut sim = Simulation::with_seed(71);
        assert_eq!(sim.allocate_stable_id(), 1);
        sim.rebuild_caches_after_load(terrain.clone(), Default::default(), Vec::new(), Vec::new());
        let mut entity = mover(&mut sim, kind);
        entity.position.z = 4;
        entity.position.sub_y = SimFixed::from_num(7);
        entity.position.exact_z_leptons = ground_pose::ground_surface_z_at(
            ground_pose::position_world_xy(&entity.position),
            false,
            Some(&terrain),
            Some(&grid),
        );
        let prior_paid_z = entity.position.exact_z_leptons;
        let target = entity.movement_target.as_mut().unwrap();
        // Budget7 for both locomotors, including Ship's fixed-point product.
        target.speed = SimFixed::from_num(106);
        target.path_layers = vec![
            MovementLayer::Ground,
            MovementLayer::Bridge,
            MovementLayer::Bridge,
        ];
        seed_track(&mut entity, kind, 11, DriveCoord::cell(3, 2, 731));
        insert(&mut sim, entity);
        tick(&mut sim, &terrain, &grid, 0);
        sim.session.tick = 1;
        sim.session.binary_frame = 1;
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!((entity.position.rx, entity.position.ry), (3, 2), "{kind:?}");
        assert!(entity.on_bridge, "{kind:?}");
        assert_eq!(entity.position.exact_z_leptons, prior_paid_z, "{kind:?}");
        assert_ne!(
            entity.position.exact_z_leptons,
            ground_pose::ground_surface_z_at(
                ground_pose::position_world_xy(&entity.position),
                true,
                Some(&terrain),
                Some(&grid)
            ),
            "snapshot must capture the native residual height lag: {kind:?}"
        );
        assert_eq!(entity.movement_target.as_ref().unwrap().next_index, 1);
        assert_eq!(
            (
                track(entity).cursor,
                track(entity).residual,
                ground_pose::position_world_xy(&entity.position)[1] / 256
            ),
            (11, 7, 2)
        );

        // Native load resets Scenario RNG. Keep the original at that same
        // canonical point, while retaining the independent process streams.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = GameSnapshot::save(&sim, 0, 0, "moving-ramp-residual", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("moving ramp snapshot")
            .sim;
        restored.main_rng = sim.main_rng.clone();
        restored.mapgen_rng = sim.mapgen_rng.clone();
        restored
            .restore_after_snapshot_load()
            .expect("moving ramp identities and occupation");
        restored.rebuild_caches_after_load(
            terrain.clone(),
            Default::default(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            restored.state_hash(),
            sim.state_hash(),
            "restored residual state: {kind:?}"
        );
        assert_eq!(
            restored
                .substrate
                .occupancy
                .count_on_layer(3, 2, MovementLayer::Bridge),
            1
        );
        let restored_terrain = restored
            .resolved_terrain
            .clone()
            .expect("restored terrain cache");

        for frame in 1..=3 {
            tick(&mut sim, &terrain, &grid, frame);
            tick(&mut restored, &restored_terrain, &grid, frame);
            sim.session.tick = u64::from(frame) + 1;
            sim.session.binary_frame = frame + 1;
            restored.session.tick = sim.session.tick;
            restored.session.binary_frame = sim.session.binary_frame;
            let original = sim.substrate.entities.get(1).unwrap();
            let loaded = restored.substrate.entities.get(1).unwrap();
            let pose = |entity: &GameEntity| {
                (
                    entity.position.rx,
                    entity.position.ry,
                    entity.position.sub_x,
                    entity.position.sub_y,
                    entity.position.z,
                    entity.position.exact_z_leptons,
                    entity.on_bridge,
                    entity.movement_target.as_ref().unwrap().next_index,
                )
            };
            assert_eq!(
                pose(loaded),
                pose(original),
                "paid continuation {kind:?}, frame {frame}"
            );
            assert!(loaded.on_bridge);
            assert_eq!(loaded.movement_target.as_ref().unwrap().next_index, 2);
            assert_eq!(track(loaded).cursor, 11 + frame as i32);
            assert_ne!(
                loaded.position.exact_z_leptons, prior_paid_z,
                "the first actual paid point must replace the saved height lag"
            );
            assert_eq!(
                restored.state_hash(),
                sim.state_hash(),
                "whole-world paid continuation {kind:?}, frame {frame}"
            );
        }
    }
}

#[test]
fn walking_bridge_entry_commits_new_surface_and_object_list_plane() {
    let mut terrain = terrain();
    terrain.cell_mut(3, 3).unwrap().level = 4;
    terrain.cell_mut(3, 2).unwrap().slope_type = 2;
    let mut grid = PathGrid::from_resolved_terrain(&terrain);
    grid.set_bridge_cell_decoupled_for_test(3, 2, 0, true, true, 4, true);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Walk);
    entity.position.z = 4;
    entity.position.sub_y = SimFixed::from_num(2);
    entity.movement_target.as_mut().unwrap().path_layers = vec![
        MovementLayer::Ground,
        MovementLayer::Bridge,
        MovementLayer::Bridge,
    ];
    let before = ground_pose::position_world_coord(&entity.position);
    insert(&mut sim, entity);
    tick(&mut sim, &terrain, &grid, 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(ground_pose::position_world_coord(&entity.position), before);
    assert!(entity.locomotor.as_ref().unwrap().step_head().is_some());
    // The next Process advances the paid step across the bridge boundary.
    tick(&mut sim, &terrain, &grid, 1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!((entity.position.rx, entity.position.ry), (3, 2));
    assert!(entity.on_bridge);
    let xy = ground_pose::position_world_xy(&entity.position);
    assert_eq!(
        entity.position.exact_z_leptons,
        Some(crate::util::lepton::ground_height_leptons(0, 2, xy[0], xy[1]).unwrap() + 416)
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 3, MovementLayer::Ground),
        0
    );
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 2, MovementLayer::Bridge),
        1
    );
}

#[test]
fn fresh_walk_refusal_preserves_xyz_before_head_selection() {
    let mut terrain = terrain();
    terrain.cell_mut(3, 3).unwrap().slope_type = 2;
    terrain.cell_mut(3, 2).unwrap().level = 2;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Walk);
    entity.position.sub_y = SimFixed::from_num(2);
    insert(&mut sim, entity);
    let mut blocker = GameEntity::test_default(2, "BLOCKER", "Americans", 3, 2);
    blocker.owner = sim.intern("Americans");
    blocker.type_ref = sim.intern("BLOCKER");
    blocker.movement_target = Some(MovementTarget {
        path: vec![(3, 2), (4, 2)],
        next_index: 1,
        speed: SimFixed::from_num(61),
        final_goal: Some((4, 2)),
        ..Default::default()
    });
    sim.substrate.occupancy.add(
        3,
        2,
        2,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::from_category(blocker.category),
    );
    sim.substrate.entities.insert(blocker);
    tick(&mut sim, &terrain, &grid, 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!((entity.position.rx, entity.position.ry), (3, 3));
    assert!(
        entity.movement_target.is_some(),
        "waiting refusal must not run arrival finalizer"
    );
    assert_eq!(
        (entity.position.sub_x, entity.position.sub_y),
        (SimFixed::from_num(128), SimFixed::from_num(2)),
        "75B690 admission precedes paid SetCoords; a refusal does not take a provisional step"
    );
    assert_eq!(entity.position.exact_z_leptons, Some(731));
    assert_eq!(entity.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(
        sim.substrate
            .occupancy
            .count_on_layer(3, 3, MovementLayer::Ground),
        1
    );
}

#[test]
fn terminal_drive_snap_updates_ramp_height_with_stashed_teleport_owner() {
    let mut terrain = terrain();
    terrain.cell_mut(3, 3).unwrap().slope_type = 2;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Drive);
    let mut loco = LocomotorState::for_test_kind(LocomotorKind::Teleport);
    assert!(loco.begin_drive_piggyback_for_teleporter(0));
    entity.locomotor = Some(loco);
    let target = entity.movement_target.as_mut().unwrap();
    target.path = vec![(3, 3)];
    target.path_layers = vec![MovementLayer::Ground];
    target.next_index = 1;
    target.final_goal = Some((3, 3));
    target.speed = SimFixed::from_num(120);
    seed_track(
        &mut entity,
        LocomotorKind::Drive,
        23,
        DriveCoord::cell(3, 3, 731),
    );
    entity.position.sub_y = SimFixed::from_num(131);
    insert(&mut sim, entity);
    tick(&mut sim, &terrain, &grid, 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.movement_target.is_none());
    assert!(super::track_head::committed_track_head(entity).is_none());
    assert_eq!(
        (entity.position.sub_x, entity.position.sub_y),
        (SimFixed::from_num(128), SimFixed::from_num(128))
    );
    assert_eq!(entity.position.exact_z_leptons, Some(52));
    assert_eq!(
        entity.locomotor.as_ref().unwrap().effective_kind(),
        LocomotorKind::Teleport
    );
}

#[test]
fn terminal_centre_height_commits_before_next_process_turn_without_finalizer() {
    let mut terrain = terrain();
    terrain.cell_mut(3, 3).unwrap().slope_type = 2;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Drive);
    let target = entity.movement_target.as_mut().unwrap();
    target.path = vec![(3, 3), (4, 3)];
    target.path_layers = vec![MovementLayer::Ground; 2];
    target.final_goal = Some((4, 3));
    target.move_dir_x = SimFixed::from_num(256);
    target.move_dir_y = SIM_ZERO;
    target.speed = SimFixed::from_num(120);
    seed_track(
        &mut entity,
        LocomotorKind::Drive,
        23,
        DriveCoord::cell(3, 3, 731),
    );
    entity.position.sub_y = SimFixed::from_num(131);
    insert(&mut sim, entity);
    tick(&mut sim, &terrain, &grid, 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.movement_target.is_some(),
        "unfinished route must not run finalizer"
    );
    assert!(super::track_head::committed_track_head(entity).is_none());
    // Native terminal4B22AF returns through4B1F5C/4B25F9 after the
    // selector is retired. Fresh movement selection waits for the next Process.
    assert_eq!(entity.facing_target, None);
    assert_eq!(entity.position.sub_y, SimFixed::from_num(128));
    // The last real table point is Y131 (height53); the final snap is Y128.
    assert_eq!(entity.position.exact_z_leptons, Some(52));
    tick(&mut sim, &terrain, &grid, 1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.facing_target, Some(0x40));
    assert_eq!(entity.position.exact_z_leptons, Some(52));
    assert!(entity.movement_target.is_some());
}

#[test]
fn surface_query_uses_live_fixed_slot_alias_and_stamps_nonzero_shared_dummy() {
    let mut terrain = terrain();
    terrain.cell_mut(0, 2).unwrap().level = 3;
    assert_eq!(
        ground_pose::ground_surface_z_at([512 * 256, 256], false, Some(&terrain), None),
        Some(312)
    );
    let dummy = terrain.shared_cell_dummy();
    dummy.set_level_slope(2, 1);
    dummy.stamp_coord(9, 9);
    assert_eq!(
        ground_pose::ground_surface_z_at(
            [500 * 256 + 128, 501 * 256 + 128],
            true,
            Some(&terrain),
            None
        ),
        Some(676)
    );
    assert_eq!(dummy.snapshot().coord, (500, 501));
}

#[test]
fn live_surface_and_coordinate_setter_match_all_native_ramp_vectors() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/ramp_height_vectors.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(
        cases.len(),
        158,
        "review coverage when the native fixture changes"
    );
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let coord = &case["coord"];
        let xy = [
            coord[0].as_i64().unwrap() as i32,
            coord[1].as_i64().unwrap() as i32,
        ];
        let missing = case["missing"].as_bool().unwrap();
        let level = case["level"].as_i64().unwrap() as i8;
        let slope = case["ramp"].as_u64().unwrap() as u8;
        // All real probes resolve one native 512-wide slot, including the
        // negative-X alias (-1,1) -> (511,0). Missing cases use the live dummy.
        let mut terrain = ResolvedTerrainGrid::from_cells(
            512,
            4,
            if missing {
                Vec::new()
            } else {
                (0..4)
                    .flat_map(|y| (0..512).map(move |x| cell(x, y)))
                    .collect()
            },
        );
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(9, 9);
        if missing {
            dummy.set_level_slope(level, slope);
        } else {
            let stored = &case["stored_cell"];
            let cell = terrain
                .cell_mut(
                    stored[0].as_u64().unwrap() as u16,
                    stored[1].as_u64().unwrap() as u16,
                )
                .unwrap();
            cell.level = level as u8;
            cell.slope_type = slope;
        }
        assert_eq!(
            ground_pose::ground_surface_z_at(xy, false, Some(&terrain), None),
            Some(case["native"]["ground_z"].as_i64().unwrap() as i32),
            "{name}"
        );
        let mut position = GameEntity::test_default(1, "MOVER", "Americans", 0, 0).position;
        position.sub_x = SimFixed::from_num(xy[0]);
        position.sub_y = SimFixed::from_num(xy[1]);
        position.exact_z_leptons = Some(coord[2].as_i64().unwrap() as i32);
        let on_bridge = case["on_bridge"].as_bool().unwrap();
        assert!(ground_pose::commit_ground_height(
            &mut position,
            on_bridge,
            Some(&terrain),
            None
        ));
        // Production movement requests height zero. The native oracle also
        // probes arbitrary requested heights; undo only that explicit add.
        let requested = case["requested_height"].as_i64().unwrap() as i32;
        let native_raw = case["native"]["set_height_raw_z"].as_i64().unwrap() as i32;
        assert_eq!(
            position.exact_z_leptons,
            Some(native_raw.wrapping_sub(requested)),
            "{name}"
        );
        assert_eq!(ground_pose::position_world_xy(&position), xy, "{name}");
        if missing {
            let expected = &case["native"]["dummy_coord"];
            assert_eq!(
                dummy.snapshot().coord,
                (
                    expected[0].as_i64().unwrap() as i32,
                    expected[1].as_i64().unwrap() as i32
                ),
                "{name}"
            );
        }
    }
}

#[test]
fn idle_drive_keeps_supplied_raw_height_and_headless_setter_preserves_it() {
    let terrain = terrain();
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Drive);
    entity.movement_target = None;
    // A completed TubeMovement can leave an arbitrary raw Z without an active
    // tube state. An ordinary idle visit has no native SetHeight call.
    entity.position.exact_z_leptons = Some(-347);
    insert(&mut sim, entity);
    tick(&mut sim, &terrain, &grid, 0);
    let position = &mut sim.substrate.entities.get_mut(1).unwrap().position;
    assert_eq!(position.exact_z_leptons, Some(-347));
    assert!(!ground_pose::commit_ground_height(
        position, false, None, None
    ));
    assert_eq!(position.exact_z_leptons, Some(-347));
}

#[test]
fn forced_track_terminal_samples_full_head_xy_before_relink() {
    let mut terrain = terrain();
    terrain.cell_mut(3, 4).unwrap().slope_type = 7;
    terrain.cell_mut(3, 4).unwrap().level = 2;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Drive);
    entity.movement_target = None;
    insert(&mut sim, entity);
    sim.resolved_terrain = Some(terrain.clone());
    assert!(sim.force_drive_track(
        1,
        0x47,
        DriveCoord {
            x: 3 * 256,
            y: 4 * 256,
            z: -347
        }
    ));
    for frame in 0..64 {
        sim.session.binary_frame = frame;
        sim.run_track_points(
            super::track_process::TrackInvocation {
                entity_id: 1,
                family: super::track_process::TrackFamily::Drive,
                apply_fresh_occupation: false,
            },
            128,
            None,
            Some(&grid),
            None,
        );
        if super::track_head::committed_track_head(sim.substrate.entities.get(1).unwrap()).is_none()
        {
            break;
        }
    }
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(super::track_head::committed_track_head(entity).is_none());
    assert_eq!((entity.position.rx, entity.position.ry), (3, 4));
    assert_eq!(
        (entity.position.sub_x, entity.position.sub_y),
        (SIM_ZERO, SIM_ZERO)
    );
    let expected = crate::util::lepton::ground_height_leptons(2, 7, 3 * 256, 4 * 256).unwrap();
    assert_eq!(entity.position.exact_z_leptons, Some(expected));
    assert!(!sim.substrate.occupancy.contains_entity(3, 3, 1));
    assert!(sim.substrate.occupancy.contains_entity(3, 4, 1));
}

#[test]
fn ordinary_drive_ship_command_keeps_subcell_origin_through_terminal_cleanup() {
    let terrain = terrain();
    let grid = PathGrid::from_resolved_terrain(&terrain);
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for (sub_x, sub_y, facing, destination) in [
            (85, 153, 0, (3, 2)),
            (0, 153, 64, (4, 3)),
            (85, 0, 128, (3, 4)),
        ] {
            let mut sim = Simulation::new();
            let mut entity = mover(&mut sim, kind);
            entity.movement_target = None;
            entity.position.sub_x = SimFixed::from_num(sub_x);
            entity.position.sub_y = SimFixed::from_num(sub_y);
            entity.facing = facing;
            insert(&mut sim, entity);
            assert!(crate::sim::movement::issue_move_command(
                &mut sim.substrate.entities,
                &grid,
                1,
                destination,
                SimFixed::from_num(128),
                false,
                None,
                None,
                None,
                false,
                crate::sim::movement::DestinationTiming::new(0, 60),
            ));
            for frame in 0..128 {
                tick(&mut sim, &terrain, &grid, frame);
                if sim
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .movement_target
                    .is_none()
                {
                    break;
                }
            }
            let entity = sim.substrate.entities.get(1).unwrap();
            assert!(entity.movement_target.is_none(), "{kind:?} must arrive");
            assert_eq!(
                ground_pose::position_world_xy(&entity.position),
                [
                    i32::from(destination.0) * 256 + sub_x,
                    i32::from(destination.1) * 256 + sub_y
                ],
                "{kind:?}"
            );
            assert!(super::track_head::committed_track_head(entity).is_none());
            assert_eq!(
                entity.drive_locomotion.as_ref().and_then(|d| d.head_to),
                None
            );
            assert_eq!(
                entity.ship_locomotion.as_ref().and_then(|s| s.head_to),
                None
            );
        }
    }
}

fn cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
        speed_costs: Default::default(),
        is_water: false,
        is_cliff_like: false,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        height_in_pixels: 0,
        variant: 0,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: Default::default(),
        base_speed_costs: Default::default(),
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

fn chained_mover(sim: &mut Simulation, kind: LocomotorKind) -> (GameEntity, DriveCoord) {
    let mut entity = mover(sim, kind);
    entity.position.sub_x = SimFixed::from_num(85);
    entity.position.sub_y = SimFixed::from_num(153);
    let path = vec![(3, 3), (3, 2), (4, 1), (5, 1)];
    let drive_track::DriveTrackDecision::Select(plan) = drive_track::plan_drive_track_from_path(
        0,
        (0, -1),
        Some((1, -1)),
        kind == LocomotorKind::Ship,
    ) else {
        panic!("native N -> NE curve");
    };
    assert_eq!(plan.nodes, 2);
    let head = super::track_head::begin_fresh(&plan, &entity.position).unwrap();
    super::track_head::accept_fresh_progress(
        kind,
        &mut entity.drive_locomotion,
        &mut entity.ship_locomotion,
        plan.selection.turn_track_index,
    );
    let mut replay = crate::sim::components::FootPathQueue::default();
    super::path_markers::install_path_replay(&mut replay, (3, 3), &path, 1);
    super::path_markers::accept_path_replay(&mut replay, (4, 1), 2);
    entity.navigation.path_replay = replay;
    match kind {
        LocomotorKind::Drive => {
            let d = entity.drive_locomotion.as_mut().unwrap();
            d.head_to = Some(head);
            d.occupation_head_to = Some(crate::sim::components::DriveOccupationFootprint {
                rx: (head.x / 256) as u16,
                ry: (head.y / 256) as u16,
                layer: MovementLayer::Ground,
            });
        }
        LocomotorKind::Ship => {
            let s = entity.ship_locomotion.as_mut().unwrap();
            s.head_to = Some(head);
        }
        _ => unreachable!(),
    }
    entity.movement_target = Some(MovementTarget {
        path,
        path_layers: vec![MovementLayer::Ground; 4],
        next_index: 1,
        speed: SimFixed::from_num(128),
        current_speed: SimFixed::from_num(128),
        final_goal: Some((5, 1)),
        ..Default::default()
    });
    (entity, head)
}

#[test]
fn admitted_tick_chain_uses_remaining_queue_and_retains_old_head_z() {
    // Unit+2C746E20 returns1; Process_Track then admits only Passive types.
    // This supplied type exercises accepted head publication. The stock false
    // gate and code0/code2 dispatch matrix live in track_chain_migration_tests.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MOVER\n[MOVER]\nSpeed=4\nPassive=yes\n",
    ))
    .unwrap();
    let terrain = terrain();
    let grid = PathGrid::from_resolved_terrain(&terrain);
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut sim = Simulation::new();
        let (entity, head) = chained_mover(&mut sim, kind);
        insert(&mut sim, entity);
        let expected = super::track_head::offset_head(head, 2);
        let mut chained = false;
        for frame in 0..128 {
            tick_with_rules(&mut sim, &terrain, &grid, frame, Some(&rules));
            let entity = sim.substrate.entities.get(1).unwrap();
            let (stored, queue) = match kind {
                LocomotorKind::Drive => {
                    let d = entity.drive_locomotion.as_ref().unwrap();
                    (d.head_to, &entity.navigation.path_replay)
                }
                LocomotorKind::Ship => {
                    let s = entity.ship_locomotion.as_ref().unwrap();
                    (s.head_to, &entity.navigation.path_replay)
                }
                _ => unreachable!(),
            };
            if stored == Some(expected) {
                assert_eq!(queue.cursor, 3);
                assert_eq!(queue.reference_cell, Some((4, 1)));
                assert_eq!(
                    super::track_head::committed_track_head(entity),
                    Some(expected)
                );
                assert!(track(entity).cursor > 0);
                assert_ne!(entity.position.exact_z_leptons, Some(expected.z));
                chained = true;
                break;
            }
        }
        assert!(
            chained,
            "{kind:?} must chain from retained head rather than changed Foot Z"
        );
    }
}

#[test]
fn stop_before_chain_keeps_committed_head_and_discards_abandoned_turn() {
    let terrain = terrain();
    let grid = PathGrid::from_resolved_terrain(&terrain);
    for (kind, teleport_identity) in [
        (LocomotorKind::Drive, false),
        (LocomotorKind::Ship, false),
        (LocomotorKind::Drive, true),
    ] {
        let mut sim = Simulation::new();
        let (mut entity, head) = chained_mover(&mut sim, kind);
        if teleport_identity {
            let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Teleport);
            assert!(locomotor.begin_drive_piggyback_for_teleporter(0));
            entity.locomotor = Some(locomotor);
        }
        // The N->NE segment has accepted two directions; E is still queued.
        // Stop is the production helper also used by the MCV deploy handoff.
        super::movement_commands::stop_navigation_at_committed_head(&mut entity);
        let (stored, queue) = match kind {
            LocomotorKind::Drive => {
                let d = entity.drive_locomotion.as_ref().unwrap();
                (d.head_to, &entity.navigation.path_replay)
            }
            LocomotorKind::Ship => {
                let s = entity.ship_locomotion.as_ref().unwrap();
                (s.head_to, &entity.navigation.path_replay)
            }
            _ => unreachable!(),
        };
        assert_eq!(stored, Some(head));
        assert_eq!(queue.cursor as usize, queue.directions.len());
        assert_eq!(queue.reference_cell, Some((4, 1)));
        assert_eq!(
            entity.movement_target.as_ref().unwrap().final_goal,
            Some((4, 1))
        );
        insert(&mut sim, entity);
        let mut finished = false;
        for frame in 0..512 {
            tick(&mut sim, &terrain, &grid, frame);
            let entity = sim.substrate.entities.get(1).unwrap();
            let stored = match kind {
                LocomotorKind::Drive => entity
                    .drive_locomotion
                    .as_ref()
                    .and_then(|drive| drive.head_to),
                LocomotorKind::Ship => entity.ship_locomotion.as_ref().unwrap().head_to,
                _ => unreachable!(),
            };
            assert!(
                stored.is_none() || stored == Some(head),
                "{kind:?}: abandoned E turn"
            );
            if entity.movement_target.is_none() {
                assert!(super::track_head::committed_track_head(entity).is_none());
                assert_eq!(stored, None);
                assert_eq!(
                    ground_pose::position_world_xy(&entity.position),
                    [head.x, head.y]
                );
                finished = true;
                break;
            }
        }
        assert!(finished, "{kind:?}: Stop must finish the committed segment");
    }
}

#[test]
fn destination_cell_height_keeps_receiver_before_structural_lookup() {
    let mut terrain = terrain();
    terrain.cell_mut(0, 0).unwrap().level = 7;
    terrain.test_set_dummy_cell_level_slope(-3, 0);
    let real = terrain.native_cell_identity((0, 0));
    terrain.write_native_cell_flags(real, 0x100);
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 3, 3);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    super::navcom::set_destination_internal_cell(&mut entity, (u16::MAX, u16::MAX), Some(&terrain));
    // Original486840/47B3A0 returns(-128,-128,-311): native adds0.5 before
    // truncation, including negative heights. Setter4AFD40 then looks up real
    // cell(0,0), because signed division truncates toward zero, and adds416.
    assert_eq!(
        entity.drive_locomotion.as_ref().unwrap().destination,
        Some(crate::sim::components::DriveCoord {
            x: -128,
            y: -128,
            z: 105
        })
    );
    assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (-1, -1));
}
