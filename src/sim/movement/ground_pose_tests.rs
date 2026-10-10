//! Production movement regressions for Object Z write cadence.

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
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
    entity.body_facing.snap(0x0000, 0);
    entity.position.exact_z_leptons = Some(731);
    entity.movement_target = Some(MovementTarget {
        // Gives budget4 in both Drive's integer division and Ship's existing
        // fixed-point frame product (60 * fixed(1/15) truncates to3).
        speed: SimFixed::from_num(61),
        final_goal: Some((3, 1)),
        ..Default::default()
    });
    match kind {
        LocomotorKind::Drive => {
            entity.foot_speed.set_speed_fraction(SIM_ONE);
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .install_drive_state_for_test(Some(
                        DriveLocomotionRuntime::default()
                            .with_target_speed_fraction_for_test(SIM_ONE)
                    ))
            )
        }
        LocomotorKind::Ship => {
            entity.foot_speed.set_speed_fraction(SIM_ONE);
            assert!(
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .install_ship_state_for_test(Some(
                        ShipLocomotionRuntime::default()
                            .with_target_speed_fraction_for_test(SIM_ONE)
                    ))
            )
        }
        LocomotorKind::Walk => {
            entity.category = EntityCategory::Infantry;
            entity.is_voxel = false;
            entity.sub_cell = Some(0);
            // Walk follows its Foot+5E0 route toward its destination; its
            // adapter keeps no route cells.
            entity.movement_target = Some(MovementTarget {
                speed: SimFixed::from_num(61),
                ..Default::default()
            });
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .set_walk_destination(Some(DriveCoord::cell(3, 1, 0)));
            entity.navigation.path_replay = crate::sim::components::FootPathQueue {
                directions: vec![0, 0],
                cursor: 0,
                reference_cell: Some((3, 3)),
            };
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
            let state = entity.locomotor.as_mut().unwrap();
            assert!(state.ensure_installed_track_state());
            assert!(state.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                Some(head)
            ));
            assert!(state.store_track_progress(
                crate::sim::movement::track_process::TrackFamily::Drive,
                track
            ));
            assert!(state.store_track_valid(
                crate::sim::movement::track_process::TrackFamily::Drive,
                true
            ));
        }
        LocomotorKind::Ship => {
            let state = entity.locomotor.as_mut().unwrap();
            assert!(state.ensure_installed_track_state());
            assert!(state.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Ship,
                Some(head)
            ));
            assert!(state.store_track_progress(
                crate::sim::movement::track_process::TrackFamily::Ship,
                track
            ));
            assert!(
                state.store_track_valid(
                    crate::sim::movement::track_process::TrackFamily::Ship,
                    true
                )
            );
        }
        _ => unreachable!(),
    }
}

fn track(entity: &GameEntity) -> crate::sim::components::TrackProgress {
    match entity.locomotor.as_ref().unwrap().kind {
        LocomotorKind::Drive => entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track(),
        LocomotorKind::Ship => entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track(),
        _ => unreachable!(),
    }
}

fn insert(sim: &mut Simulation, mut entity: GameEntity) {
    // This fixture inserts a live object-list entry, so its lifecycle mark
    // must agree. Forced terminal relinking intentionally respects this flag.
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
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

/// A supplied Passive type: Unit+2C746E20 admits Process_Track for it.
fn mover_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MOVER\n[MOVER]\nSpeed=4\nPassive=yes\n",
    ))
    .unwrap()
}

/// A Walk infantryman's type: Walk admission and its path request read it.
fn walk_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=MOVER\n[MOVER]\nSpeed=4\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap()
}

/// A Walk object turn with the native map inputs its admission and path
/// request read: rules, zones with Map Size and generous LocalSize bounds.
fn walk_tick(sim: &mut Simulation, terrain: &ResolvedTerrainGrid, grid: &PathGrid, frame: u32) {
    let rules = walk_rules();
    let (width, height) = (terrain.width(), terrain.height());
    sim.resolved_terrain = Some(terrain.clone());
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_bridge_geometry(
            grid,
            terrain,
            &[],
            width,
            height,
            Some((i32::from(width), i32::from(height))),
        ),
    );
    crate::sim::arena_fixture::supply_native_map(sim);
    sim.install_fixture_path_grid(Some(grid));
    sim.session.tick = u64::from(frame);
    sim.session.binary_frame = frame;
    sim.process_ground_locomotor_stats_for_test(1, Some(&rules), None)
        .expect("Walk fixture reaches the production object turn");
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
    sim.resolved_terrain = Some(terrain.clone());
    sim.install_fixture_path_grid(Some(grid));
    sim.session.tick = u64::from(frame);
    sim.session.binary_frame = frame;
    sim.process_ground_locomotor_stats_for_test(1, rules, None)
        .expect("fixture reached an unsupported production movement receiver");
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
fn residual_bridge_crossing_preserves_z_and_projects_the_layer_from_on_bridge() {
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
        // This route continues beyond the first retained segment. Native
        // owner NavCom must survive that terminal; a route without it admits
        // EnterIdleMode and its Foot SetSpeedFraction(0) at that first end.
        super::navcom::set_destination_internal_cell(&mut entity, (3, 1), Some(&terrain), 0);
        assert_eq!(
            entity
                .locomotor
                .as_ref()
                .and_then(|l| l.selected_drive_runtime())
                .and_then(|r| r.retained())
                .unwrap()
                .destination(),
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
        assert_eq!(
            entity.locomotor.as_ref().unwrap().layer,
            end_layer,
            "the layer is the OnBridge projection written at the crossing"
        );
        assert_eq!(
            track(entity).cursor,
            11,
            "residual relinking does not pay a point"
        );
        assert_eq!(sim.substrate.occupancy.count_on_layer(3, 3, start_layer), 0);
        assert_eq!(sim.substrate.occupancy.count_on_layer(3, 2, end_layer), 1);
        let relinked_generation = sim.substrate.occupancy.generation();

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
        assert_eq!(
            sim.substrate.occupancy.generation(),
            relinked_generation,
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
        assert_eq!(entity.position.exact_z_leptons, Some(416));
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
    walk_tick(&mut sim, &terrain, &grid, 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(ground_pose::position_world_coord(&entity.position), before);
    assert!(entity.locomotor.as_ref().unwrap().step_head().is_some());
    for frame in 1..4 {
        walk_tick(&mut sim, &terrain, &grid, frame);
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
        sim.rebuild_caches_after_load(
            terrain.clone(),
            Default::default(),
            &crate::sim::runtime::SimResources::empty().rules,
        );
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
            &crate::sim::runtime::SimResources::empty().rules,
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
                    entity.navigation.path_replay.clone(),
                )
            };
            assert_eq!(
                pose(loaded),
                pose(original),
                "paid continuation {kind:?}, frame {frame}"
            );
            assert!(loaded.on_bridge);
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
    // Walk's admission and boundary read the Cell's structural bridge bit.
    terrain.cell_mut(3, 2).unwrap().bridge_facts.raw_flags |= 0x100;
    let mut grid = PathGrid::from_resolved_terrain(&terrain);
    grid.set_bridge_cell_decoupled_for_test(3, 2, 0, true, true, 4, true);
    let mut sim = Simulation::new();
    let mut entity = mover(&mut sim, LocomotorKind::Walk);
    entity.position.z = 4;
    entity.position.sub_y = SimFixed::from_num(2);
    // A paid head on the deck cell north; this Process's step crosses the
    // boundary (head admission is covered by the walk_prehead corpus).
    entity
        .locomotor
        .as_mut()
        .unwrap()
        .set_step_head(Some(DriveCoord {
            x: 3 * 256 + 128,
            y: 2 * 256 + 128,
            z: 4 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
        }));
    insert(&mut sim, entity);
    walk_tick(&mut sim, &terrain, &grid, 1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!((entity.position.rx, entity.position.ry), (3, 2));
    assert!(entity.on_bridge);
    // Walk's layer is the OnBridge projection its next Find_Path starts from.
    assert_eq!(
        entity.locomotor.as_ref().unwrap().layer,
        MovementLayer::Bridge
    );
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
        speed: SimFixed::from_num(61),
        final_goal: Some((4, 2)),
        ..Default::default()
    });
    // Moving: its Foot+5E0 head word steps east from (3,2).
    blocker.navigation.path_replay = crate::sim::movement::fixture_path_replay(&[(3, 2), (4, 2)]);
    sim.substrate.occupancy.add(
        3,
        2,
        2,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::from_category(blocker.category),
    );
    sim.substrate.entities.insert(blocker);
    walk_tick(&mut sim, &terrain, &grid, 0);
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
    // The refusal's response may select another head in the same Process;
    // walk_prehead_response_tests pins that response against the original.
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
    assert!(loco.begin_piggyback(LocomotorKind::Drive, 0));
    entity.locomotor = Some(loco);
    let target = entity.movement_target.as_mut().unwrap();
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
    target.final_goal = Some((4, 3));
    target.speed = SimFixed::from_num(120);
    seed_track(
        &mut entity,
        LocomotorKind::Drive,
        23,
        DriveCoord::cell(3, 3, 731),
    );
    entity.position.sub_y = SimFixed::from_num(131);
    super::navcom::set_destination_internal_cell(&mut entity, (4, 3), Some(&terrain), 0);
    super::path_markers::install_path_replay(
        &mut entity.navigation.path_replay,
        (3, 3),
        &[(3, 3), (4, 3)],
        1,
    );
    insert(&mut sim, entity);
    tick_with_rules(&mut sim, &terrain, &grid, 0, Some(&mover_rules()));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(
        entity.movement_target.is_some(),
        "unfinished route must not run finalizer"
    );
    assert!(super::track_head::committed_track_head(entity).is_none());
    // Process_Track(0) at 4B0576 retires the selector; with Foot+5E0 still
    // queued, the same Process runs Process_Movement (4B0647), whose fresh
    // selection turns toward (4,3).
    assert_eq!(entity.body_facing.destination(), 0x4000);
    assert_eq!(entity.position.sub_y, SimFixed::from_num(128));
    // The last real table point is Y131 (height53); the final snap is Y128.
    assert_eq!(entity.position.exact_z_leptons, Some(52));
    tick_with_rules(&mut sim, &terrain, &grid, 1, Some(&mover_rules()));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.body_facing.destination(), 0x4000);
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
        serde_json::from_str(crate::test_fixture::text("tools/ramp_height_vectors.json")).unwrap();
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
        let requested = case["requested_height"].as_i64().unwrap() as i32;
        ground_pose::set_height(&mut position, on_bridge, requested, Some(&terrain), None);
        assert_eq!(
            position.exact_z_leptons,
            Some(case["native"]["set_height_raw_z"].as_i64().unwrap() as i32),
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
fn idle_drive_keeps_supplied_raw_height() {
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
    let position = &sim.substrate.entities.get(1).unwrap().position;
    assert_eq!(position.exact_z_leptons, Some(-347));
}

/// SetHeight (`0x005F5FA0`) writes the ground under the Location plus the
/// height, plus the deck OnBridge. The ground comes from resolved terrain,
/// else a PathGrid, else the Dummy cell's flat level 0. An unsupported slope
/// leaves Z alone.
#[test]
fn set_height_samples_terrain_then_path_grid_then_the_dummy_ground() {
    use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
    let mut terrain = terrain();
    terrain.cell_mut(3, 3).unwrap().level = 2;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut position = GameEntity::test_default(1, "MOVER", "Americans", 3, 3).position;
    position.z = 1;
    ground_pose::set_height(&mut position, true, 10, Some(&terrain), None);
    assert_eq!(
        position.exact_z_leptons,
        Some(208 + BRIDGE_DECK_HEIGHT_LEPTONS + 10)
    );
    ground_pose::set_height(&mut position, false, 10, None, Some(&grid));
    assert_eq!(position.exact_z_leptons, Some(208 + 10));
    ground_pose::set_height(&mut position, true, 10, None, None);
    assert_eq!(
        position.exact_z_leptons,
        Some(BRIDGE_DECK_HEIGHT_LEPTONS + 10)
    );
    terrain.cell_mut(3, 3).unwrap().slope_type = 21;
    ground_pose::set_height(&mut position, false, 0, Some(&terrain), None);
    assert_eq!(
        position.exact_z_leptons,
        Some(BRIDGE_DECK_HEIGHT_LEPTONS + 10)
    );
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
    assert!(sim.force_track(
        1,
        0x47,
        DriveCoord {
            x: 3 * 256,
            y: 4 * 256,
            z: -347
        },
        None,
        None,
    ));
    if sim.path_grid.is_none() {
        sim.path_grid = Some(std::sync::Arc::new(grid.clone()));
    }
    for frame in 0..64 {
        sim.session.binary_frame = frame;
        sim.run_track_points(
            super::track_process::TrackInvocation {
                entity_id: 1,
                family: super::track_process::TrackFamily::Drive,
                apply_fresh_occupation: false,
                active_gate: false,
                retry: false,
            },
            128,
            None,
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
    let rules = mover_rules();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        for (sub_x, sub_y, facing, destination) in [
            (85, 153, 0u8, (3, 2)),
            (0, 153, 64, (4, 3)),
            (85, 0, 128, (3, 4)),
        ] {
            let mut sim = Simulation::new();
            let grid = crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
            let mut entity = mover(&mut sim, kind);
            entity.movement_target = None;
            entity.position.sub_x = SimFixed::from_num(sub_x);
            entity.position.sub_y = SimFixed::from_num(sub_y);
            entity.body_facing.snap(u16::from(facing) << 8, 0);
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
                crate::sim::movement::DestinationTiming::new(0, 60),
            ));
            for frame in 0..128 {
                sim.session.binary_frame = frame;
                sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
                    .unwrap();
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
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_drive_runtime())
                    .and_then(|r| r.retained())
                    .and_then(|d| d.head_to()),
                None
            );
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .and_then(|s| s.head_to()),
                None
            );
        }
    }
}

fn cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    crate::map::resolved_terrain::ResolvedTerrainCell {
        // Every row the Drive/Ship fixtures use admits, so the native
        // Unit+1AC (the chain query) answers from occupancy alone.
        speed_costs: crate::rules::terrain_rules::SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            ..Default::default()
        },
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

fn chained_mover(sim: &mut Simulation, kind: LocomotorKind) -> (GameEntity, DriveCoord) {
    let mut entity = mover(sim, kind);
    entity.position.sub_x = SimFixed::from_num(85);
    entity.position.sub_y = SimFixed::from_num(153);
    let path = vec![(3, 3), (3, 2), (4, 1), (5, 1)];
    // Path N then NE: the native N -> NE curve, a two-node turn.
    let turn = drive_track::fresh_turn_index(0, 1);
    assert_ne!(
        drive_track::TURN_TRACKS[turn].flags & drive_track::TURN_TRACK_TURNS_FLAG,
        0
    );
    let offset = super::track_head::offset_head;
    let current = super::ground_pose::position_world_coord(&entity.position);
    let head = offset(offset(current, (turn / 8) as u8), (turn % 8) as u8);
    super::track_head::accept_fresh_progress(entity.locomotor.as_mut().unwrap(), turn);
    let mut replay = crate::sim::components::FootPathQueue::default();
    super::path_markers::install_path_replay(&mut replay, (3, 3), &path, 1);
    super::path_markers::accept_path_replay(&mut replay, (4, 1), 2);
    entity.navigation.path_replay = replay;
    match kind {
        LocomotorKind::Drive => {
            let d = entity.locomotor.as_mut().unwrap();
            assert!(d.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                Some(head)
            ));
            assert!(
                d.publish_track_occupation(
                    crate::sim::movement::track_process::TrackFamily::Drive,
                    Some(crate::sim::components::DriveOccupationFootprint {
                        rx: (head.x / 256) as u16,
                        ry: (head.y / 256) as u16,
                        layer: MovementLayer::Ground,
                    }),
                    d.selected_drive_runtime()
                        .unwrap()
                        .retained()
                        .unwrap()
                        .occupation_handoff()
                )
            );
        }
        LocomotorKind::Ship => {
            let s = entity.locomotor.as_mut().unwrap();
            assert!(s.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Ship,
                Some(head)
            ));
        }
        _ => unreachable!(),
    }
    entity.movement_target = Some(MovementTarget {
        speed: SimFixed::from_num(128),
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
    let rules = mover_rules();
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
                    let d = entity
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_drive_runtime())
                        .and_then(|r| r.retained())
                        .unwrap();
                    (d.head_to(), &entity.navigation.path_replay)
                }
                LocomotorKind::Ship => {
                    let s = entity
                        .locomotor
                        .as_ref()
                        .and_then(|l| l.selected_ship_runtime())
                        .and_then(|r| r.retained())
                        .unwrap();
                    (s.head_to(), &entity.navigation.path_replay)
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
fn destination_cell_height_keeps_receiver_before_structural_lookup() {
    let mut terrain = terrain();
    terrain.cell_mut(0, 0).unwrap().level = 7;
    terrain.test_set_dummy_cell_level_slope(-3, 0);
    let real = terrain.native_cell_identity((0, 0));
    terrain.write_native_cell_flags(real, 0x100);
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 3, 3);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    super::navcom::set_destination_internal_cell(
        &mut entity,
        (u16::MAX, u16::MAX),
        Some(&terrain),
        0,
    );
    // Original486840/47B3A0 returns(-128,-128,-311): native adds0.5 before
    // truncation, including negative heights. Setter4AFD40 then looks up real
    // cell(0,0), because signed division truncates toward zero, and adds416.
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .destination(),
        Some(crate::sim::components::DriveCoord {
            x: -128,
            y: -128,
            z: 105
        })
    );
    assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (-1, -1));
}

/// GetCoords' XY: a mobile's Location (`0x005F65A0`); a building's Location
/// plus `(dimension - 1) * 128` on each axis (`0x00447AC0`: `SHL 7; SUB
/// 0x80`), from the foundation its type stamped on it. A name the foundation
/// table lacks reads as its default 1x1.
#[test]
fn getcoords_xy_is_the_location_or_a_buildings_foundation_centre() {
    let mut entity = GameEntity::test_default(1, "BLDG", "Enemy", 10, 20);
    entity.position.sub_x = SimFixed::from_num(200);
    entity.position.sub_y = SimFixed::from_num(33);
    let raw = [10 * 256 + 200, 20 * 256 + 33];
    assert_eq!(
        ground_pose::object_center_xy(&entity),
        raw,
        "mobile GetCoords is raw"
    );
    entity.category = crate::map::entities::EntityCategory::Structure;
    for (foundation, expected) in [
        ("1x1", raw),
        ("2x2", [raw[0] + 128, raw[1] + 128]),
        ("4x3", [raw[0] + 384, raw[1] + 256]),
        ("", raw),
        ("not-a-native-foundation", raw),
    ] {
        entity.foundation = foundation.to_string();
        assert_eq!(
            ground_pose::object_center_xy(&entity),
            expected,
            "foundation {foundation:?}"
        );
    }
}

/// A building's GetCoords keeps its Location's Z (`0x00447B04`), which its
/// Unlimbo (Reveal) wrote as the floor there: `BuildingTypeClass` virtual
/// +0x6C (`0x00464A70`) replaces the coordinate's Z with `0x00578080`'s ground
/// height. An order to a plain building (`0x00447E90` returns +0x48) targets
/// that floor, not the cell's flat level.
#[test]
fn a_building_on_a_ramp_is_targeted_at_its_floor() {
    let mut sim = Simulation::new();
    let mut terrain = terrain();
    // Native `signed_level_5_bridge_0` (tools/ramp_height_vectors.json):
    // `0x00578080` at (640, 640) on a level-5 ramp-1 cell answers 572.
    let cell = terrain.cell_mut(2, 2).unwrap();
    cell.level = 5;
    cell.slope_type = 1;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=TEST\n[TEST]\nFoundation=1x1\n",
    ))
    .unwrap();
    sim.resolved_terrain = Some(terrain);
    let mut building = GameEntity::test_default_of_category(
        1,
        "TEST",
        "Americans",
        2,
        2,
        crate::map::entities::EntityCategory::Structure,
    );
    building.owner = sim.intern("Americans");
    building.type_ref = sim.intern("TEST");
    let (sub_x, sub_y) = (building.position.sub_x, building.position.sub_y);
    sim.substrate.entities.insert(building);
    assert!(matches!(
        sim.try_reveal_entity(
            1,
            crate::sim::world::RevealRequest {
                position: crate::sim::world::RevealPosition {
                    exact_z_leptons: None,
                    rx: 2,
                    ry: 2,
                    z: 5,
                    sub_x,
                    sub_y,
                },
                placement: crate::sim::world::PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
        ),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    let coord = super::navcom::nav_target_coordinate(
        crate::sim::components::NavTargetRef::Building { id: 1 },
        None,
        &sim.substrate.entities,
        sim.resolved_terrain.as_ref(),
        Some((&rules, &sim.interner)),
    )
    .unwrap();
    assert_eq!(
        coord,
        DriveCoord {
            x: 640,
            y: 640,
            z: 572
        }
    );
}

/// Two Drive units marked into cell (4, 4), 1 before 2, so 2 heads the list.
fn two_marked_units() -> Simulation {
    let mut sim = Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    for id in [1, 2] {
        let mut entity = GameEntity::test_default(id, "ACTOR", "Americans", 4, 4);
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = false;
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.position.exact_z_leptons = Some(0);
        sim.substrate.entities.insert(entity);
        assert!(sim.foot_mark_put(id, None, None));
    }
    sim
}

fn next_in_cell(sim: &Simulation, id: u64) -> Option<crate::sim::occupancy::CellObjectMember> {
    sim.next_cell_object(crate::sim::occupancy::CellObjectMember::Entity(id))
}

/// SetHeight (`0x005F5FA0`) on a marked object runs its Mark(UP) and
/// Mark(DOWN) around the Z write (`0x005F5FC8`, `0x005F6009`), so
/// `FootClass::Mark` prepends it to its cell's list again.
#[test]
fn set_height_on_a_marked_object_re_marks_it_at_the_head_of_its_cell() {
    use crate::sim::occupancy::CellObjectMember::Entity;
    let mut sim = two_marked_units();
    assert_eq!(next_in_cell(&sim, 2), Some(Entity(1)));
    sim.set_object_height(1, 30, None, None);
    assert_eq!(next_in_cell(&sim, 1), Some(Entity(2)));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.lifecycle.cell_marked);
    assert_eq!(entity.position.exact_z_leptons, Some(30));
}

/// An unmarked object takes SetHeight's Z write alone (`0x005F6017..`), as
/// Hover's SetHeight does with `+0x74` cleared (`0x00513E74..0x00513E8C`).
#[test]
fn set_height_on_an_unmarked_object_writes_z_without_marking_it() {
    use crate::sim::occupancy::CellObjectMember::Entity;
    let mut sim = two_marked_units();
    sim.set_object_height_unmarked(1, 30);
    assert_eq!(next_in_cell(&sim, 2), Some(Entity(1)));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .position
            .exact_z_leptons,
        Some(30)
    );

    assert!(sim.foot_mark_remove(1, None, None));
    sim.set_object_height(1, 60, None, None);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(!entity.lifecycle.cell_marked);
    assert_eq!(entity.position.exact_z_leptons, Some(60));
    assert!(!sim.substrate.occupancy.contains_entity(4, 4, 1));
}

/// A Location on the map keeps the cell `0x0041BEA0` reads (each axis divided
/// by 256 toward zero) and the remainder. Off it, the unsigned cell clamps and
/// the sub-cell keeps the rest, so negative and 16-bit-aliased coordinates,
/// which the Walk completion oracle feeds, read back unchanged.
#[test]
fn a_location_keeps_its_cell_and_reads_back_unchanged() {
    use super::ground_pose::{position_world_xy, set_position_world_xy};
    let mut position = GameEntity::test_default(1, "ACTOR", "Americans", 0, 0).position;
    let alias = (16 + 65536) * 256 + 192;
    for (world, cell, sub) in [
        (0, 0, 0),
        (255, 0, 255),
        (256, 1, 0),
        (511 * 256 + 255, 511, 255),
        (-1, 0, -1),
        (-256, 0, -256),
        (-32768, 0, -32768),
        (alias, 65535, alias - 65535 * 256),
    ] {
        set_position_world_xy(&mut position, [world, world]);
        assert_eq!([position.rx, position.ry], [cell, cell], "{world}");
        assert_eq!(position.sub_x, SimFixed::from_num(sub), "{world}");
        assert_eq!(position_world_xy(&position), [world, world]);
    }
}

/// `FootClass::SetLocation` (`0x004DB810`) writes the Location on both of its
/// arms (`0x004DB855`, `0x004DB86B`), changed or not, which settles a staged
/// sub-cell into its cell. Only the `OpenTopped=` rider tail waits for a
/// changed Location (`0x004DB819..0x004DB83D`, `0x004DB870`).
#[test]
fn foot_set_location_always_writes_but_moves_riders_only_on_a_change() {
    use super::ground_pose::{foot_set_location, position_world_coord};
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=BFRT\n[BFRT]\nOpenTopped=yes\nPassengers=5\n",
    ))
    .expect("rules");
    let mut entities = crate::sim::entity_store::EntityStore::new();
    let mut transport = GameEntity::test_default(1, "BFRT", "Americans", 1, 0);
    // A non-canonical Location: the world XY has left cell 1, the cell has not.
    transport.position.sub_x = SimFixed::from_num(300);
    transport.position.sub_y = SimFixed::from_num(128);
    transport.position.exact_z_leptons = Some(0);
    let mut cargo = PassengerCargo::new(5, 2);
    assert!(cargo.board(2, 1));
    transport.passenger_role = PassengerRole::Transport { cargo };
    let mut rider = GameEntity::test_default(2, "E1", "Americans", 3, 0);
    rider.passenger_role = PassengerRole::Inside {
        transport_id: 1,
        open_topped: true,
    };
    entities.insert(transport);
    entities.insert(rider);
    let interner = crate::sim::intern::test_interner();

    let staged = DriveCoord {
        x: 556,
        y: 128,
        z: 0,
    };
    foot_set_location(&mut entities, 1, staged, Some(&rules), &interner);
    let transport = &entities.get(1).unwrap().position;
    assert_eq!((transport.rx, transport.sub_x), (2, SimFixed::from_num(44)));
    assert_eq!(
        entities.get(2).unwrap().position.rx,
        3,
        "no change, no tail"
    );

    let moved = DriveCoord { x: 600, ..staged };
    foot_set_location(&mut entities, 1, moved, Some(&rules), &interner);
    let [transport, rider] =
        [1, 2].map(|id| position_world_coord(&entities.get(id).unwrap().position));
    assert_eq!(transport, moved);
    assert_eq!(rider, moved, "the rider took the changed Location");

    // The native change test includes Z (4DB834). A caller that only
    // changes height still reaches the same rider tail as an XY move.
    let lowered = DriveCoord { z: -5, ..moved };
    foot_set_location(&mut entities, 1, lowered, Some(&rules), &interner);
    let [transport, rider] =
        [1, 2].map(|id| position_world_coord(&entities.get(id).unwrap().position));
    assert_eq!(transport, lowered);
    assert_eq!(rider, lowered, "the rider took the Z-only Location change");
}
