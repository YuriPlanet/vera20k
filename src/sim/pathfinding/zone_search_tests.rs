//! Tests for zone-aware pathfinding wrappers.

use super::super::zone_hierarchy::{ZoneEdgeRecord, ZoneHierarchy, ZoneLevelGraph, ZoneRecord};
use super::super::zone_map::{ZoneAdjacency, ZoneGrid, ZoneInfo, ZoneMap};
use super::*;
use crate::map::bridge_facts::{BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL};
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::combat::AttackTarget;
use crate::sim::components::{NavTargetRef, OrderIntent};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::test_interner;
use crate::sim::miner::miner_system::{issue_move_if_idle, issue_stock_miner_drive_move};
use crate::sim::miner::{CargoBale, MinerConfig, MinerState, RefineryDockPhase, ResourceType};
use crate::sim::movement::{issue_move_command_with_layered, tick_movement_with_grids};
use crate::sim::occupancy::{CellOccupationGrid, OccupancyGrid};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig;
use crate::sim::production::{ProductionCategory, STARTING_CREDITS, tick_production};
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use std::collections::{BTreeMap, BTreeSet};

fn grid_from_str(s: &str) -> PathGrid {
    let lines: Vec<&str> = s.trim().lines().map(|l| l.trim()).collect();
    let h = lines.len() as u16;
    let w = lines[0].len() as u16;
    let mut grid = PathGrid::new(w, h);
    for (ry, line) in lines.iter().enumerate() {
        for (rx, ch) in line.chars().enumerate() {
            if ch == '#' {
                grid.set_blocked(rx as u16, ry as u16, true);
            }
        }
    }
    grid
}

fn gsi_04_12_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let cells = (0..height)
        .flat_map(|ry| {
            (0..width).map(move |rx| ResolvedTerrainCell {
                rx,
                ry,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: None,
                land_type: LandType::Clear.as_index(),
                yr_cell_land_type: LandType::Clear.as_index(),
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs,
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
                zone_type: zone_class::GROUND,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: LandType::Clear.as_index(),
                base_yr_cell_land_type: LandType::Clear.as_index(),
                base_terrain_class: TerrainClass::Clear,
                base_speed_costs: speed_costs,
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: Default::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.test_set_high_bridge_set_starts(Some(100), None);
    terrain
}

fn gsi_04_12_cell_listed_entity(
    stable_id: u64,
    type_ref: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) -> GameEntity {
    let mut entity = GameEntity::test_default(stable_id, type_ref, owner, rx, ry);
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity
}

fn hierarchy_endpoint_bounds() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 10,
        off_fc: 2,
        off_100: 1,
        off_104: 10,
        off_108: 6,
    }
}

fn rectangular_spawn_bounds(span: i32) -> PlayfieldBounds {
    PlayfieldBounds {
        base: 0,
        off_fc: -span,
        off_100: -span,
        off_104: span * 2,
        off_108: span * 2,
    }
}

/// Drive/Ship orders accept without a route (Unit741970); the first Process
/// requests it with the pass's own blocker plane and zone context, whatever
/// the caller. Runs that Process through the production host and returns the
/// installed route.
fn first_track_process_route(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    grid: &PathGrid,
) -> Option<crate::sim::components::MovementTarget> {
    let request = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.movement_target.as_ref())
        .expect("the order scheduled a Process");
    assert!(request.path.is_empty(), "the order installs no route");
    sim.process_ground_locomotor_for_test(id, rules, Some(grid), None)
        .expect("the first Process completes");
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.movement_target.clone())
}

fn hierarchy_endpoint_zone_grid() -> ZoneGrid {
    let mut reduced = PathGrid::new(16, 16);
    for y in 0..16 {
        reduced.set_blocked(7, y, true);
    }
    ZoneGrid::build(&reduced, &BTreeMap::new(), 16, 16)
}

fn hierarchy_endpoint_path(
    start: (u16, u16),
    goal: (u16, u16),
    terrain: &ResolvedTerrainGrid,
) -> Option<Vec<(u16, u16)>> {
    let grid = PathGrid::new(16, 16);
    let zone_grid = hierarchy_endpoint_zone_grid();
    assert!(
        !zone_grid.can_reach(
            MovementZone::Normal,
            start,
            MovementLayer::Ground,
            goal,
            MovementLayer::Ground,
        ),
        "fixture must make hierarchy/reduced zones abort while raw cell A* stays open"
    );
    find_path_zoned_marker(
        &grid,
        start,
        goal,
        None,
        None,
        Some(&zone_grid),
        MovementZone::Normal,
        Some(MovementZone::Normal),
        Some(terrain),
        None,
        None,
        None,
        0,
        false,
        false,
        true,
        Some(hierarchy_endpoint_bounds()),
    )
}

#[test]
fn playfield_hierarchy_outside_resolved_source_falls_back_to_flat_astar() {
    let terrain = gsi_04_12_terrain(16, 16);
    let bounds = hierarchy_endpoint_bounds();
    assert!(!bounds.contains_height_aware_packed(6, 6, 0, 0));
    assert!(bounds.contains_height_aware_packed(8, 6, 0, 0));

    let path = hierarchy_endpoint_path((6, 6), (8, 6), &terrain)
        .expect("outside resolved source must disable hierarchy, not fail pathfinding");
    assert_eq!(path.first().copied(), Some((6, 6)));
    assert_eq!(path.last().copied(), Some((8, 6)));
}

#[test]
fn playfield_hierarchy_outside_resolved_goal_falls_back_to_flat_astar() {
    let terrain = gsi_04_12_terrain(16, 16);
    let bounds = hierarchy_endpoint_bounds();
    assert!(bounds.contains_height_aware_packed(8, 6, 0, 0));
    assert!(!bounds.contains_height_aware_packed(6, 6, 0, 0));

    let path = hierarchy_endpoint_path((8, 6), (6, 6), &terrain)
        .expect("outside resolved goal must disable hierarchy, not fail pathfinding");
    assert_eq!(path.first().copied(), Some((8, 6)));
    assert_eq!(path.last().copied(), Some((6, 6)));
}

#[test]
fn playfield_hierarchy_mode_one_level_and_slope_boundaries() {
    let mut terrain = gsi_04_12_terrain(16, 16);
    let bounds = hierarchy_endpoint_bounds();
    let (_, _, flat_inside) = resolve_hierarchy_endpoint_contract(
        None,
        Some(&terrain),
        Some(bounds),
        (7, 6),
        false,
        (8, 6),
        false,
    );
    assert!(flat_inside, "sum=13 is just inside mode one at level zero");

    terrain.cell_mut(7, 6).unwrap().level = 1;
    let (_, _, raised_inside) = resolve_hierarchy_endpoint_contract(
        None,
        Some(&terrain),
        Some(bounds),
        (7, 6),
        false,
        (8, 6),
        false,
    );
    assert!(
        !raised_inside,
        "signed level must move the strict mode-one edge"
    );

    let cell = terrain.cell_mut(7, 6).unwrap();
    cell.level = 0;
    cell.slope_type = 1;
    let (_, _, sloped_inside) = resolve_hierarchy_endpoint_contract(
        None,
        Some(&terrain),
        Some(bounds),
        (7, 6),
        false,
        (8, 6),
        false,
    );
    assert!(
        !sloped_inside,
        "nonzero slope below the native threshold adds one level"
    );

    terrain.cell_mut(10, 6).unwrap().slope_type = 1;
    let (_, _, threshold_equal_inside) = resolve_hierarchy_endpoint_contract(
        None,
        Some(&terrain),
        Some(bounds),
        (10, 6),
        false,
        (8, 6),
        false,
    );
    assert!(
        threshold_equal_inside,
        "sum equal to the native slope threshold must not add one level"
    );
}

#[test]
fn playfield_hierarchy_bridge_projection_tracks_intact_and_destroyed_records() {
    let mut terrain = gsi_04_12_terrain(16, 16);
    terrain.cell_mut(7, 6).unwrap().bridge_facts.raw_flags =
        BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let record = BridgeEndpointRecord {
        endpoint_a: (6, 6),
        endpoint_b: (8, 6),
        group_id: 1,
        active: true,
        bridge_kind: BridgeRecordKind::High,
    };
    let intact = ZoneGrid::build_with_terrain(
        &grid,
        &BTreeMap::new(),
        Some(&terrain),
        &[record.clone()],
        16,
        16,
    );
    let (intact_start, _, intact_inside) = resolve_hierarchy_endpoint_contract(
        Some(&intact),
        Some(&terrain),
        Some(hierarchy_endpoint_bounds()),
        (7, 6),
        true,
        (9, 6),
        false,
    );
    assert_eq!(
        intact_start,
        (8, 6),
        "native distance tie selects endpoint B"
    );
    assert!(intact_inside);

    let destroyed_record = BridgeEndpointRecord {
        active: false,
        ..record
    };
    let destroyed = ZoneGrid::build_with_terrain(
        &grid,
        &BTreeMap::new(),
        Some(&terrain),
        &[destroyed_record],
        16,
        16,
    );
    let (destroyed_start, _, destroyed_inside) = resolve_hierarchy_endpoint_contract(
        Some(&destroyed),
        Some(&terrain),
        Some(hierarchy_endpoint_bounds()),
        (7, 6),
        true,
        (9, 6),
        false,
    );
    assert_eq!(
        destroyed_start,
        (6, 6),
        "without a high-tile east exit, inactive native projection selects endpoint A"
    );
    assert!(!destroyed_inside);
}

#[test]
fn playfield_hierarchy_initial_order_outside_endpoint_uses_flat_astar() {
    let grid = PathGrid::new(16, 16);
    let terrain = gsi_04_12_terrain(16, 16);
    let zone_grid = hierarchy_endpoint_zone_grid();
    assert!(!zone_grid.can_reach(
        MovementZone::Normal,
        (6, 6),
        MovementLayer::Ground,
        (8, 6),
        MovementLayer::Ground,
    ));

    let mut entities = EntityStore::new();
    // Hover keeps the command-time search adapter; Drive/Ship reach the same
    // hierarchy/flat choice at their first Process search.
    let mut mover = gsi_04_12_cell_listed_entity(1, "LCRF", "Americans", 6, 6);
    mover.category = crate::map::entities::EntityCategory::Unit;
    let mut hover = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Hover,
    );
    hover.movement_zone = MovementZone::Normal;
    mover.locomotor = Some(hover);
    mover.in_playfield = true;
    entities.insert(mover);

    assert!(issue_move_command_with_layered(
        &mut entities,
        &grid,
        1,
        (8, 6),
        SimFixed::from_num(128),
        false,
        None,
        None,
        Some(&terrain),
        Some(&zone_grid),
        None,
        None,
        Some(hierarchy_endpoint_bounds()),
        None,
        crate::sim::movement::DestinationTiming::new(0, 60),
    ));
    let target = entities.get(1).unwrap().movement_target.as_ref().unwrap();
    assert_eq!(target.path.first().copied(), Some((6, 6)));
    assert_eq!(target.path.last().copied(), Some((8, 6)));
}

#[test]
fn zoned_path_reachable_returns_path() {
    let grid = grid_from_str(
        "
        .....
        .....
        .....
    ",
    );
    let zg = ZoneGrid::build(&grid, &BTreeMap::new(), 5, 3);
    let path = find_path_zoned(
        &grid,
        (0, 0),
        (4, 2),
        None,
        None,
        Some(&zg),
        MovementZone::Normal,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some());
    let path = path.unwrap();
    assert_eq!(*path.first().unwrap(), (0, 0));
    assert_eq!(*path.last().unwrap(), (4, 2));
}

#[test]
fn zoned_path_unreachable_returns_none_instantly() {
    let grid = grid_from_str(
        "
        ..#..
        ..#..
        ..#..
    ",
    );
    let zg = ZoneGrid::build(&grid, &BTreeMap::new(), 5, 3);
    // (0,0) and (4,0) are in different disconnected zones.
    let path = find_path_zoned(
        &grid,
        (0, 0),
        (4, 0),
        None,
        None,
        Some(&zg),
        MovementZone::Normal,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_none());
}

#[test]
fn zoned_path_no_zone_grid_falls_through() {
    let grid = grid_from_str(
        "
        .....
        .....
    ",
    );
    // Without zone grid, should just run normal A*.
    let path = find_path_zoned(
        &grid,
        (0, 0),
        (4, 1),
        None,
        None,
        None,
        MovementZone::Normal,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some());
}

#[test]
fn zoned_path_same_cell() {
    let grid = grid_from_str(
        "
        .....
    ",
    );
    let zg = ZoneGrid::build(&grid, &BTreeMap::new(), 5, 1);
    let path = find_path_zoned(
        &grid,
        (2, 0),
        (2, 0),
        None,
        None,
        Some(&zg),
        MovementZone::Normal,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some());
    assert_eq!(path.unwrap(), vec![(2, 0)]);
}

#[test]
fn zoned_path_entity_blocks_respected() {
    let grid = grid_from_str(
        "
        ...
        ...
        ...
    ",
    );
    let zg = ZoneGrid::build(&grid, &BTreeMap::new(), 3, 3);
    // Block the direct path with entities.
    let mut blocks = BTreeSet::new();
    blocks.insert((1, 0));
    blocks.insert((1, 1));
    blocks.insert((1, 2));
    // Zone says reachable (static terrain is connected), but entities block.
    // A* should still find no path since the wall of entities cuts off (2,x).
    let path = find_path_zoned(
        &grid,
        (0, 0),
        (2, 0),
        None,
        Some(&blocks),
        Some(&zg),
        MovementZone::Normal,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    // Path exists because goal cell is always reachable even if entity-blocked.
    // But the path would need to go around — with a 3x3 grid fully blocked
    // in column 1, there's no way around.
    assert!(path.is_none());
}

fn test_zone_map() -> (ZoneMap, ZoneAdjacency) {
    let zone_map = ZoneMap::new(
        vec![1, 2, 3, 4],
        None,
        4,
        1,
        4,
        vec![
            ZoneInfo {
                center: (0, 0),
                cell_count: 1,
            },
            ZoneInfo {
                center: (1, 0),
                cell_count: 1,
            },
            ZoneInfo {
                center: (0, 1),
                cell_count: 1,
            },
            ZoneInfo {
                center: (2, 0),
                cell_count: 1,
            },
        ],
    );
    let adjacency =
        ZoneAdjacency::new(vec![vec![], vec![2, 3], vec![1, 3, 4], vec![1, 2], vec![2]]);
    (zone_map, adjacency)
}

fn equal_cost_zone_map(adjacency_order: Vec<ZoneId>) -> (ZoneMap, ZoneAdjacency) {
    let zone_map = ZoneMap::new(
        vec![1, 2, 3, 4, 5],
        None,
        5,
        1,
        5,
        vec![
            ZoneInfo {
                center: (0, 0),
                cell_count: 1,
            },
            ZoneInfo {
                center: (1, 0),
                cell_count: 1,
            },
            ZoneInfo {
                center: (1, 0),
                cell_count: 1,
            },
            ZoneInfo {
                center: (0, 1),
                cell_count: 1,
            },
            ZoneInfo {
                center: (2, 0),
                cell_count: 1,
            },
        ],
    );
    let adjacency = ZoneAdjacency::new(vec![
        vec![],
        adjacency_order,
        vec![1, 5],
        vec![1, 5],
        vec![],
        vec![2, 3],
    ]);
    (zone_map, adjacency)
}

fn linear_level0_hierarchy(zones: Vec<ZoneId>, edges: &[(ZoneId, ZoneId)]) -> ZoneHierarchy {
    let width = zones.len() as u16;
    level0_hierarchy(zones, width, 1, edges)
}

fn level0_hierarchy(
    zones: Vec<ZoneId>,
    width: u16,
    height: u16,
    edges: &[(ZoneId, ZoneId)],
) -> ZoneHierarchy {
    debug_assert_eq!(zones.len(), width as usize * height as usize);
    let zone_count = zones.iter().copied().max().unwrap_or(0);
    let mut level2 = ZoneLevelGraph::new(1);
    level2.set_record(ZoneRecord::new(1, 0, 0));

    let mut level1 = ZoneLevelGraph::new(1);
    level1.set_record(ZoneRecord::new(1, 1, 0));

    let mut level0 = ZoneLevelGraph::new(zone_count).with_cell_zone_ids(zones, width, height);
    for zone in 1..=zone_count {
        level0.set_record(ZoneRecord::new(zone, 1, 0));
    }
    for &(a, b) in edges {
        level0.push_edge(a, ZoneEdgeRecord::new(b, 0));
        level0.push_edge(b, ZoneEdgeRecord::new(a, 0));
    }

    ZoneHierarchy::new(level0, level1, level2)
}

#[test]
fn tube_hierarchy_precheck_rejects_unequal_base_labels_before_connected_graph() {
    let astar_grid = PathGrid::new(3, 1);
    let mut reduced_grid = PathGrid::new(3, 1);
    reduced_grid.set_blocked(1, 0, true);
    let mut zg = ZoneGrid::build(&reduced_grid, &BTreeMap::new(), 3, 1);
    zg.set_hierarchy(linear_level0_hierarchy(vec![1, 2, 3], &[(1, 2), (2, 3)]));
    assert!(
        !zg.can_reach(
            MovementZone::Normal,
            (0, 0),
            MovementLayer::Ground,
            (2, 0),
            MovementLayer::Ground
        ),
        "fixture must prove the old reduced SuperZoneMap would abort"
    );

    let blocker_counts = BlockerNeighborCounts::new(3, 1);
    let path = find_path_zoned_marker_inner(
        &astar_grid,
        (0, 0),
        (2, 0),
        (0, 0),
        (2, 0),
        None, // no native raw-row receipt in this compatibility fixture
        None,
        None,
        Some(&zg),
        MovementZone::Normal,
        Some(MovementZone::Normal),
        None,
        None,
        None,
        0,
        false,
        false,
        Some(&blocker_counts),
    );

    assert!(
        path.is_none(),
        "42CB22 rejects unequal base labels before a successful graph precheck"
    );
}

#[test]
fn zone_precheck_failed_hierarchy_keeps_zone_map_same_zone_fallback() {
    let astar_grid = PathGrid::new(3, 1);
    let mut zg = ZoneGrid::build(&astar_grid, &BTreeMap::new(), 3, 1);
    zg.set_hierarchy(linear_level0_hierarchy(
        vec![ZONE_INVALID, ZONE_INVALID, ZONE_INVALID],
        &[],
    ));

    let blocker_counts = BlockerNeighborCounts::new(3, 1);
    let path = find_path_zoned_marker_inner(
        &astar_grid,
        (0, 0),
        (2, 0),
        (0, 0),
        (2, 0),
        None, // no native raw-row receipt in this compatibility fixture
        None,
        None,
        Some(&zg),
        MovementZone::Normal,
        Some(MovementZone::Normal),
        None,
        None,
        None,
        0,
        false,
        false,
        Some(&blocker_counts),
    )
    .expect("same-zone ZoneMap fallback should survive incomplete hierarchy cell IDs");

    assert_eq!(path, vec![(0, 0), (1, 0), (2, 0)]);
}

#[test]
fn gsi_04_12_layered_production_precheck_projects_only_hierarchy_coordinates() {
    let mut astar_grid = PathGrid::new(5, 1);
    for x in 1..=3 {
        astar_grid.set_cell_for_test(x, 0, 0, true, true);
    }

    let mut reduced_grid = PathGrid::new(5, 1);
    reduced_grid.set_blocked(2, 0, true);
    let mut zone_grid = ZoneGrid::build(&reduced_grid, &BTreeMap::new(), 5, 1);
    zone_grid.set_hierarchy(linear_level0_hierarchy(vec![1, 3, 1, 4, 2], &[(1, 2)]));
    assert!(
        !zone_grid.can_reach(
            MovementZone::Normal,
            (1, 0),
            MovementLayer::Bridge,
            (3, 0),
            MovementLayer::Bridge,
        ),
        "fixture must prove reduced reachability would abort the layered search"
    );

    let mut terrain = gsi_04_12_terrain(5, 1);
    for x in 1..=3 {
        let cell = terrain.cell_mut(x, 0).unwrap();
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    terrain.cell_mut(0, 0).unwrap().final_tile_index = 100;
    terrain.cell_mut(4, 0).unwrap().final_tile_index = 100;

    let make_entities = || {
        let mut entities = EntityStore::new();
        let mut mover = gsi_04_12_cell_listed_entity(1, "HTNK", "Americans", 1, 0);
        let mut locomotor = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        );
        locomotor.layer = MovementLayer::Bridge;
        mover.locomotor = Some(locomotor);
        mover.on_bridge = true;
        mover.drive_locomotion = Some(Default::default());
        entities.insert(mover);
        entities.insert(gsi_04_12_cell_listed_entity(2, "HTNK", "Russians", 2, 0));
        entities
    };

    let make_sim = |terrain: &ResolvedTerrainGrid| {
        // Entities intern their names first; the snapshot must contain them.
        let entities = make_entities();
        let mut sim = Simulation::new();
        sim.interner = test_interner();
        sim.substrate.entities = entities;
        sim.resolved_terrain = Some(terrain.clone());
        sim.zone_grid = Some(zone_grid.clone());
        sim
    };
    let order = |sim: &mut Simulation, terrain: &ResolvedTerrainGrid| {
        assert!(issue_move_command_with_layered(
            &mut sim.substrate.entities,
            &astar_grid,
            1,
            (3, 0),
            SimFixed::from_num(128),
            false,
            None,
            None,
            Some(terrain),
            Some(&zone_grid),
            None,
            None,
            None,
            None,
            crate::sim::movement::DestinationTiming::new(0, 60),
        ));
    };
    let mut sim = make_sim(&terrain);
    order(&mut sim, &terrain);
    let movement = first_track_process_route(&mut sim, 1, None, &astar_grid)
        .expect("the first Process should install the projected hierarchy route");

    assert_eq!(
        movement.path.first().copied(),
        Some((1, 0)),
        "projection must not mutate the A* start coordinate or layer"
    );
    assert_eq!(
        movement.path_layers.first().copied(),
        Some(MovementLayer::Bridge),
        "the Process search must keep the raw A* start layer"
    );
    assert_eq!(
        movement.path.last().copied(),
        Some((3, 0)),
        "projection must not mutate the A* goal or returned path"
    );

    terrain.cell_mut(3, 0).unwrap().bridge_facts.raw_flags = 0;
    let mut sim = make_sim(&terrain);
    order(&mut sim, &terrain);
    assert!(
        first_track_process_route(&mut sim, 1, None, &astar_grid)
            .is_none_or(|movement| movement.path.is_empty()),
        "destination projection must be selected by the destination structural bit"
    );
}

// Shared caller witness: the ordinary ground shortcut differs from the marked
// bridge detour. Its unmarked ground approach needs Cell+122; bridge-deck
// height exemption alone cannot admit that approach.
fn caller_count_bridge_detour(
    mz: MovementZone,
    reverse: bool,
) -> (PathGrid, ZoneGrid, ResolvedTerrainGrid) {
    let mut path = PathGrid::new(6, 4);
    let mut terrain = gsi_04_12_terrain(6, 4);
    for y in 0..4 {
        for x in 0..6 {
            let open = (y == 0 && x >= 1) || (y == 1 && (x == 1 || x == 5)) || (y == 2 && x >= 1);
            path.set_blocked(x, y, !open);
            if open {
                path.set_cell_for_test(x, y, 4, false, false);
                terrain.cell_mut(x, y).unwrap().level = 4;
            }
        }
    }
    for x in 2..=4 {
        path.set_cell_for_test(x, 2, 0, true, true);
        let cell = terrain.cell_mut(x, 2).unwrap();
        cell.level = 0;
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    // Foot4D3810 -> Map56D100 needs the native base rows before the route
    // search. Build those from this fixture's real levels and Map Size=(6,4),
    // including the bridge's ground endpoints. The deliberately restricted
    // hierarchy below remains the separate Cell+122 caller-count witness.
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &path,
        &BTreeMap::new(),
        Some(&terrain),
        &[BridgeEndpointRecord {
            endpoint_a: (1, 2),
            endpoint_b: (5, 2),
            group_id: 0,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }],
        6,
        4,
        Some((6, 4)),
    );
    let mut labels = vec![3; 24];
    for (x, y) in [(1, 0), (5, 0), (5, 1), (5, 2), (1, 2)] {
        labels[y * 6 + x] = 1;
    }
    zones.set_hierarchy(level0_hierarchy(labels, 6, 4, &[]));
    let (start, goal) = if reverse {
        ((5, 0), (1, 0))
    } else {
        ((1, 0), (5, 0))
    };
    let native_start = zones
        .get_path_zone_id_native(&terrain, start, mz, false)
        .expect("caller fixture retains native topology");
    assert!(
        (2..u32::from(u16::MAX)).contains(&native_start),
        "the native precheck must use a valid ground row, not equal invalid labels"
    );
    assert_eq!(
        zones.get_path_zone_id_native(&terrain, goal, mz, false),
        Some(native_start),
        "the top-row shortcut connects the native source and destination"
    );
    assert_eq!(
        zones.get_path_zone_id_native(&terrain, (3, 2), mz, true),
        Some(native_start),
        "the bridge deck resolves through the same reachable ground component"
    );
    let route = |counts: Option<&BlockerNeighborCounts>| {
        find_layered_path_zoned_marker(
            &path,
            None,
            None,
            start,
            MovementLayer::Ground,
            goal,
            Some(&zones),
            mz,
            None,
            Some(mz),
            Some(&terrain),
            None,
            None,
            counts,
            0,
            false,
            mz == MovementZone::Infantry,
            true,
            None,
        )
    };
    let ordinary = route(None).expect("no-count route must take the available shortcut");
    assert!(
        ordinary
            .iter()
            .all(|step| step.ry == 0 && step.layer == MovementLayer::Ground)
    );
    assert!(
        route(Some(&BlockerNeighborCounts::new(6, 4))).is_none(),
        "zero counts cannot enter the unmarked ground approach"
    );
    let mut live_counts = BlockerNeighborCounts::new(6, 4);
    live_counts.add_single_cell_neighbor_source(0, 2);
    let marked = route(Some(&live_counts)).expect("neighbor escape admits marked bridge detour");
    assert!(
        marked
            .iter()
            .any(|step| step.layer == MovementLayer::Bridge)
    );
    (path, zones, terrain)
}

#[test]
fn gsi_04_12_completed_ground_unit_rally_threads_exact_blocker_counts() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n\
         [BuildingTypes]\n0=GAWEAP\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\n\
         [GAWEAP]\nStrength=1000\nFoundation=1x1\nFactory=UnitType\nExitCoord=256,0,0\n",
    ))
    .expect("ground-unit production rules should parse");

    // The real war-factory exit is ground cell (1,0); the rally is ground cell
    // (5,0). Exact counts admit the ground approach to the bridge detour;
    // omitted counts instead select the distinct ordinary ground shortcut.
    let (path_grid, zone_grid, terrain) = caller_count_bridge_detour(MovementZone::Normal, false);

    let mut height_map = BTreeMap::new();
    height_map.insert((1, 0), 4);
    let mut sim = Simulation::new();
    // Explicit fixture Map Size=(6,4) beside the generous LocalSize bounds;
    // Foot's production precheck consumes both header dimensions.
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: 6,
        ..rectangular_spawn_bounds(6)
    });
    sim.playfield_size_height = Some(4);
    sim.resolved_terrain = Some(terrain);
    sim.zone_grid = Some(zone_grid);
    sim.spawn_object("GAWEAP", "Americans", 0, 0, 0, &rules, &height_map)
        .expect("war factory should spawn");
    sim.spawn_object("MTNK", "Russians", 0, 2, 0, &rules, &height_map)
        .expect("dynamic blocker should spawn");

    let owner = sim.interner.intern("Americans");
    let produced_type = sim.interner.intern("MTNK");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, STARTING_CREDITS, 10),
    );
    sim.houses.get_mut(&owner).unwrap().rally_point = Some((5, 0));
    let started = sim.production.factory_shadow.test_enqueue_kernel(
        owner,
        ProductionCategory::Vehicle,
        produced_type,
        0,
        100,
        0,
    );
    assert!(started);
    crate::sim::production::construct_active_factory_fixture(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        produced_type,
    )
    .expect("production fixture constructs at StartProduction");
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, ProductionCategory::Vehicle)
    );

    assert!(
        tick_production(&mut sim, &rules, &height_map, Some(&path_grid)),
        "ready ground-unit production should deliver through the real completion entry"
    );

    let produced = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.owner == owner && entity.type_ref == produced_type)
        .expect("completed MTNK should exist");
    assert_eq!((produced.position.rx, produced.position.ry), (1, 0));
    let locomotor = produced.locomotor.as_ref().expect("produced locomotor");
    assert_eq!(
        locomotor.kind,
        crate::rules::locomotor_type::LocomotorKind::Drive
    );
    assert_eq!(locomotor.movement_zone, MovementZone::Normal);
    assert!(!produced.on_bridge);
    assert!(!produced.too_big_to_fit_under_bridge);
    let produced_id = produced.stable_id();
    let movement = first_track_process_route(&mut sim, produced_id, Some(&rules), &path_grid)
        .expect("completed MTNK should receive the hierarchy-backed rally route");
    assert_eq!(movement.path.first().copied(), Some((1, 0)));
    assert_eq!(movement.path_layers.first(), Some(&MovementLayer::Ground));
    assert!(
        movement
            .path_layers
            .iter()
            .any(|layer| *layer == MovementLayer::Bridge),
        "the rally route must actually traverse the high-bridge layer"
    );
    assert_eq!(movement.path.last().copied(), Some((5, 0)));
    assert!(sim.production.factory_shadow.is_empty());
    assert_eq!(sim.houses.get(&owner).unwrap().rally_point, Some((5, 0)));
}

#[test]
fn gsi_04_12_miner_dock_approach_threads_exact_blocker_counts() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n1=BLOCK\n\n\
         [BuildingTypes]\n0=REFN\n\n\
         [HARV]\nStrength=600\nSpeed=4\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\nHarvester=yes\nDock=REFN\nStorage=20\n\n\
         [BLOCK]\nStrength=300\nSpeed=4\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\n\
         [REFN]\nStrength=900\nFoundation=1x1\nRefinery=yes\n",
    ))
    .expect("dock-approach rules should parse");

    // The refinery's 1x1 geometric QueueingCell is (1,0). The live miner
    // starts at (5,0). The blocker at(0,2) admits the unmarked ground approach
    // to the bridge detour without occupying the route.
    let (path_grid, zone_grid, terrain) = caller_count_bridge_detour(MovementZone::Normal, true);

    let mut height_map = BTreeMap::new();
    height_map.insert((1, 0), 4);
    height_map.insert((5, 0), 4);
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: 6,
        ..rectangular_spawn_bounds(6)
    });
    sim.playfield_size_height = Some(4);
    sim.resolved_terrain = Some(terrain);
    sim.zone_grid = Some(zone_grid);
    let refinery_id = sim
        .spawn_object("REFN", "Americans", 0, 0, 0, &rules, &height_map)
        .expect("refinery should spawn");
    let miner_id = sim
        .spawn_object("HARV", "Americans", 5, 0, 0, &rules, &height_map)
        .expect("harvester should spawn");
    sim.spawn_object("BLOCK", "Russians", 0, 2, 0, &rules, &height_map)
        .expect("dynamic blocker should spawn");
    {
        let entity = sim.substrate.entities.get_mut(miner_id).unwrap();
        let miner = entity.miner.as_mut().expect("HARV should own miner state");
        miner.cargo.push(CargoBale {
            resource_type: ResourceType::Ore,
            value: 25,
        });
        miner.reserved_refinery = Some(refinery_id);
        miner.dock_phase = RefineryDockPhase::Approach;
        miner.approach_hello_timer = crate::sim::mission::timer::MissionTimer::armed(0, 10);
        entity.mission.set_handler_state(MinerState::Dock.cursor());
    }

    crate::sim::miner::miner_system::tick_miners(
        &mut sim,
        &rules,
        &MinerConfig::default(),
        Some(&path_grid),
    );

    let miner = sim.substrate.entities.get(miner_id).unwrap();
    assert_eq!(miner.miner_state(), Some(MinerState::Dock));
    let miner_state = miner.miner.as_ref().unwrap();
    assert_eq!(miner_state.dock_phase, RefineryDockPhase::Approach);
    assert_eq!(miner_state.reserved_refinery, Some(refinery_id));
    let movement = first_track_process_route(&mut sim, miner_id, Some(&rules), &path_grid)
        .expect("live Approach dispatch should install the hierarchy-backed queue route");
    assert_eq!(movement.path.first().copied(), Some((5, 0)));
    assert!(
        movement
            .path_layers
            .iter()
            .any(|layer| *layer == MovementLayer::Bridge),
        "dock approach must actually traverse the high-bridge layer"
    );
    assert_eq!(movement.path.last().copied(), Some((1, 0)));
}

#[test]
fn gsi_04_12_interaction_order_entry_threads_exact_blocker_counts() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=ENGINEER\n\n\
         [VehicleTypes]\n0=BLOCK\n\n\
         [BuildingTypes]\n0=TARGET\n\n\
         [ENGINEER]\nStrength=75\nSpeed=4\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Infantry\nEngineer=yes\n\n\
         [BLOCK]\nStrength=300\nSpeed=4\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\n\
         [TARGET]\nStrength=500\nFoundation=1x1\nCapturable=yes\n",
    ))
    .expect("interaction-order rules should parse");

    let (path_grid, zone_grid, terrain) = caller_count_bridge_detour(MovementZone::Infantry, false);

    let mut height_map = BTreeMap::new();
    height_map.insert((1, 0), 4);
    height_map.insert((5, 0), 4);
    let mut sim = Simulation::new();
    // Explicit fixture Map Size=(6,4), separate from the generous LocalSize
    // bounds. Foot's production precheck consumes both header dimensions.
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: 6,
        ..rectangular_spawn_bounds(6)
    });
    sim.playfield_size_height = Some(4);
    sim.resolved_terrain = Some(terrain);
    sim.zone_grid = Some(zone_grid);
    let engineer_id = sim
        .spawn_object("ENGINEER", "Americans", 1, 0, 0, &rules, &height_map)
        .expect("engineer should spawn");
    let target_id = sim
        .spawn_object("TARGET", "Russians", 5, 0, 0, &rules, &height_map)
        .expect("capture target should spawn");
    sim.spawn_object("BLOCK", "Russians", 0, 2, 0, &rules, &height_map)
        .expect("dynamic blocker should spawn");

    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::CaptureBuilding {
            engineer_id,
            target_building_id: target_id,
        },
        Some(&rules),
        Some(&path_grid),
        &height_map,
    ));

    let engineer = sim.substrate.entities.get(engineer_id).unwrap();
    assert_eq!(engineer.capture_target, Some(target_id));
    assert_eq!(
        engineer.navigation.nav_com,
        Some(NavTargetRef::building(target_id))
    );
    let request = engineer
        .movement_target
        .as_ref()
        .expect("accepted Capture request");
    assert_eq!(request.final_goal, Some((5, 0)));
    assert!(
        request.path.is_empty(),
        "Walk searches during Process, after order admission"
    );

    assert_eq!(
        engineer.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Capture)
    );

    // The direct Process seam omits Object/Techno AI. Run the production
    // Ready -> Commence checkpoint first: Infantry51C300 reads CURRENT
    // mission when admitting the Building NavCom, not its queued Capture.
    sim.mission_host_promote(engineer_id, sim.session.binary_frame, &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(engineer_id)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(crate::sim::mission::MissionType::Capture)
    );
    // Publish the same navigation used by order admission for shared world
    // receivers, which take their grid from Simulation rather than the
    // locomotor test seam's optional fallback.
    sim.path_grid = Some(std::sync::Arc::new(path_grid));
    let target_cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity((5, 0));
    assert_eq!(
        sim.infantry_can_enter(
            engineer_id,
            target_cell,
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            &rules,
            None,
        )
        .expect("Capture target admission uses the live Building NavCom"),
        crate::sim::movement::infantry_entry::InfantryEntryClass::Clear
    );

    // Execute the real no-head Process with live blockers and canonical grids.
    sim.process_ground_locomotor_for_test(engineer_id, Some(&rules), None, None)
        .expect("Capture Process retains the fixture's native map inputs");

    let engineer = sim.substrate.entities.get(engineer_id).unwrap();
    assert_eq!(
        engineer.navigation.nav_com,
        Some(NavTargetRef::building(target_id))
    );
    let movement = engineer
        .movement_target
        .as_ref()
        .expect("live Capture Process should install the hierarchy-backed target route");
    assert_eq!(movement.final_goal, Some((5, 0)));
    assert_eq!(movement.path.first().copied(), Some((1, 0)));
    assert!(
        movement
            .path_layers
            .iter()
            .any(|layer| *layer == MovementLayer::Bridge),
        "interaction approach must actually traverse the high-bridge layer"
    );
    // Native Infantry 51C300/51C37D/51C71B admits the matching Building
    // NavCom. Foot 4D3A92..4D3E0A passes its original Cell to the core;
    // tools/spatial_oracle/capture_core_goal.py covers that admission/goal
    // boundary. This Rust route check does not claim full native AStar parity.
    assert_eq!(
        movement.path.last().copied(),
        Some((5, 0)),
        "Capture must retain the admitted building Cell as its path goal"
    );
}

#[test]
fn gsi_04_12_attack_pursuit_entry_threads_exact_blocker_counts() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=1\nWarhead=AP\n\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("pursuit rules should parse");

    let mut path_grid = PathGrid::new(5, 1);
    for x in 1..=3 {
        path_grid.set_cell_for_test(x, 0, 0, true, true);
    }

    let mut reduced_grid = PathGrid::new(5, 1);
    reduced_grid.set_blocked(2, 0, true);
    let mut zone_grid = ZoneGrid::build(&reduced_grid, &BTreeMap::new(), 5, 1);
    zone_grid.set_hierarchy(linear_level0_hierarchy(vec![1, 3, 1, 4, 2], &[(1, 2)]));
    assert!(
        !zone_grid.can_reach(
            MovementZone::Normal,
            (1, 0),
            MovementLayer::Bridge,
            (3, 0),
            MovementLayer::Bridge,
        ),
        "fixture must make the no-count pursuit path fail reduced reachability"
    );

    let mut terrain = gsi_04_12_terrain(5, 1);
    for x in 1..=3 {
        let cell = terrain.cell_mut(x, 0).unwrap();
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    terrain.cell_mut(0, 0).unwrap().final_tile_index = 100;
    terrain.cell_mut(4, 0).unwrap().final_tile_index = 100;

    let mut attacker = gsi_04_12_cell_listed_entity(1, "MTNK", "Americans", 1, 0);
    let mut locomotor = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Drive,
    );
    locomotor.layer = MovementLayer::Bridge;
    attacker.locomotor = Some(locomotor);
    attacker.on_bridge = true;
    attacker.drive_locomotion = Some(Default::default());
    attacker.attack_target = Some(AttackTarget::for_cell(3, 0));
    let blocker = gsi_04_12_cell_listed_entity(2, "MTNK", "Russians", 2, 0);

    let mut sim = Simulation::new();
    sim.interner = test_interner();
    sim.substrate.entities.insert(attacker);
    sim.substrate.entities.insert(blocker);
    sim.resolved_terrain = Some(terrain);
    sim.zone_grid = Some(zone_grid);

    sim.tick_attack_pursuit(&rules, Some(&path_grid));

    let movement = first_track_process_route(&mut sim, 1, Some(&rules), &path_grid)
        .expect("real out-of-range pursuit should reach the projected hierarchy route");
    assert_eq!(movement.path.first().copied(), Some((1, 0)));
    assert_eq!(
        movement.path_layers.first().copied(),
        Some(MovementLayer::Bridge)
    );
    assert_eq!(movement.path.last().copied(), Some((3, 0)));
}

#[test]
fn gsi_04_12_phase_six_order_resume_threads_exact_blocker_counts() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n",
    ))
    .expect("phase-six movement rules should parse");

    let mut path_grid = PathGrid::new(5, 1);
    for x in 1..=3 {
        path_grid.set_cell_for_test(x, 0, 0, true, true);
    }

    let mut reduced_grid = PathGrid::new(5, 1);
    reduced_grid.set_blocked(2, 0, true);
    let mut zone_grid = ZoneGrid::build(&reduced_grid, &BTreeMap::new(), 5, 1);
    zone_grid.set_hierarchy(linear_level0_hierarchy(vec![1, 3, 1, 4, 2], &[(1, 2)]));
    assert!(
        !zone_grid.can_reach(
            MovementZone::Normal,
            (1, 0),
            MovementLayer::Bridge,
            (3, 0),
            MovementLayer::Bridge,
        ),
        "fixture must make Phase-6 resume fail when blocker counts are absent"
    );

    let mut terrain = gsi_04_12_terrain(5, 1);
    for x in 1..=3 {
        let cell = terrain.cell_mut(x, 0).unwrap();
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    terrain.cell_mut(0, 0).unwrap().final_tile_index = 100;
    terrain.cell_mut(4, 0).unwrap().final_tile_index = 100;

    let mut mover = gsi_04_12_cell_listed_entity(1, "MTNK", "Americans", 1, 0);
    let mut locomotor = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Drive,
    );
    locomotor.layer = MovementLayer::Bridge;
    mover.locomotor = Some(locomotor);
    mover.on_bridge = true;
    mover.drive_locomotion = Some(Default::default());
    mover.order_intent = Some(OrderIntent::AttackMove {
        goal_rx: 3,
        goal_ry: 0,
    });
    let blocker = gsi_04_12_cell_listed_entity(2, "MTNK", "Russians", 2, 0);

    let mut sim = Simulation::new();
    sim.interner = test_interner();
    sim.substrate.entities.insert(mover);
    sim.substrate.entities.insert(blocker);
    sim.resolved_terrain = Some(terrain);
    sim.zone_grid = Some(zone_grid);

    sim.tick_order_intents_post_combat(Some(&path_grid), Some(&rules));

    let movement = first_track_process_route(&mut sim, 1, Some(&rules), &path_grid)
        .expect("real Phase-6 resume should reach the projected hierarchy route");
    let resumed = sim.substrate.entities.get(1).expect("resumed mover");
    assert_eq!(movement.path.first().copied(), Some((1, 0)));
    assert_eq!(
        movement.path_layers.first().copied(),
        Some(MovementLayer::Bridge)
    );
    assert_eq!(movement.path.last().copied(), Some((3, 0)));
    assert_eq!(
        resumed.order_intent,
        Some(OrderIntent::AttackMove {
            goal_rx: 3,
            goal_ry: 0,
        }),
        "resume must preserve the continuing AttackMove order"
    );
}

#[test]
fn gsi_04_12_drive_pending_continuation_keeps_hierarchy_context_and_raw_route() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n",
    ))
    .expect("Drive continuation rules should parse");

    let mut path_grid = PathGrid::new(5, 1);
    for x in 1..=3 {
        path_grid.set_cell_for_test(x, 0, 0, true, true);
    }

    let mut reduced_grid = PathGrid::new(5, 1);
    reduced_grid.set_blocked(2, 0, true);
    let mut zone_grid = ZoneGrid::build(&reduced_grid, &BTreeMap::new(), 5, 1);
    zone_grid.set_hierarchy(linear_level0_hierarchy(vec![1, 3, 1, 4, 2], &[(1, 2)]));
    assert!(
        !zone_grid.can_reach(
            MovementZone::Normal,
            (1, 0),
            MovementLayer::Bridge,
            (3, 0),
            MovementLayer::Bridge,
        ),
        "fixture must reject a continuation that drops the hierarchy context"
    );

    let mut terrain = gsi_04_12_terrain(5, 1);
    for x in 1..=3 {
        let cell = terrain.cell_mut(x, 0).unwrap();
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    terrain.cell_mut(0, 0).unwrap().final_tile_index = 100;
    terrain.cell_mut(4, 0).unwrap().final_tile_index = 100;

    let mut mover = gsi_04_12_cell_listed_entity(1, "MTNK", "Americans", 1, 0);
    let mut locomotor = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
        crate::rules::locomotor_type::LocomotorKind::Drive,
    );
    locomotor.layer = MovementLayer::Bridge;
    mover.locomotor = Some(locomotor);
    mover.on_bridge = true;
    mover.drive_locomotion = Some(Default::default());
    mover.navigation.nav_com = Some(NavTargetRef::cell(3, 0));
    mover.navigation.pending_arrival_clear = true;
    let blocker = gsi_04_12_cell_listed_entity(2, "MTNK", "Russians", 2, 0);

    let mut entities = EntityStore::new();
    entities.insert(mover);
    entities.insert(blocker);
    let mut interner = test_interner();
    let mut occupancy = OccupancyGrid::new();
    let mut cell_occupation = CellOccupationGrid::new();
    let mut raw_cell_occupation = crate::sim::occupancy::RawCellOccupationGrid::new();
    let mut enter_order = crate::sim::world::EnterOrderCounter::new();
    let mut rng = SimRng::new(0);
    let terrain_speed_config = TerrainSpeedConfig::default();
    let terrain_costs = BTreeMap::new();
    let alliances = HouseAllianceMap::new();
    let mut sound_events = Vec::new();
    let mut lifecycle_requests = Vec::new();
    let live_order = [1];

    tick_movement_with_grids(
        &mut entities,
        Some(&live_order),
        Some(&path_grid),
        &terrain_costs,
        &alliances,
        &mut occupancy,
        &mut cell_occupation,
        &mut raw_cell_occupation,
        &mut enter_order,
        &mut rng,
        1,
        1,
        Some(&zone_grid),
        Some(&terrain),
        None,
        &terrain_speed_config,
        SimFixed::from_num(0),
        9,
        60,
        &mut interner,
        Some(&rules),
        &mut sound_events,
        &mut lifecycle_requests,
    );

    let continued = entities.get(1).expect("continued Drive mover");
    assert_eq!(continued.navigation.nav_com, Some(NavTargetRef::cell(3, 0)));
    assert!(!continued.navigation.pending_arrival_clear);
    let movement = continued
        .movement_target
        .as_ref()
        .expect("pending Drive continuation should rebuild the hierarchy route");
    assert_eq!(movement.path.first().copied(), Some((1, 0)));
    assert_eq!(
        movement.path_layers.first().copied(),
        Some(MovementLayer::Bridge)
    );
    assert_eq!(movement.path.last().copied(), Some((3, 0)));
    assert_eq!(movement.final_goal, Some((3, 0)));
}

#[test]
fn gsi_04_12_stock_miner_move_entries_thread_exact_world_context() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n\n\
         [HARV]\nStrength=1000\nArmor=heavy\nSpeed=5\nHarvester=yes\n",
    ))
    .expect("stock harvester rules should parse");

    let mut path_grid = PathGrid::new(5, 1);
    for x in 1..=3 {
        path_grid.set_cell_for_test(x, 0, 0, true, true);
    }

    let make_sim = || {
        let mut reduced_grid = PathGrid::new(5, 1);
        reduced_grid.set_blocked(2, 0, true);
        let mut zone_grid = ZoneGrid::build(&reduced_grid, &BTreeMap::new(), 5, 1);
        zone_grid.set_hierarchy(linear_level0_hierarchy(vec![1, 3, 1, 4, 2], &[(1, 2)]));
        assert!(
            !zone_grid.can_reach(
                MovementZone::Normal,
                (1, 0),
                MovementLayer::Bridge,
                (3, 0),
                MovementLayer::Bridge,
            ),
            "fixture must make either miner entry fail if it drops blocker counts"
        );

        let mut terrain = gsi_04_12_terrain(5, 1);
        for x in 1..=3 {
            let cell = terrain.cell_mut(x, 0).unwrap();
            cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO;
            cell.has_bridge_deck = true;
            cell.bridge_walkable = true;
            cell.bridge_transition = true;
            cell.bridge_deck_level = 4;
        }
        terrain.cell_mut(0, 0).unwrap().final_tile_index = 100;
        terrain.cell_mut(4, 0).unwrap().final_tile_index = 100;

        let mut miner = gsi_04_12_cell_listed_entity(1, "HARV", "Americans", 1, 0);
        let mut locomotor = crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        );
        locomotor.layer = MovementLayer::Bridge;
        miner.locomotor = Some(locomotor);
        miner.on_bridge = true;
        miner.drive_locomotion = Some(Default::default());
        let blocker = gsi_04_12_cell_listed_entity(2, "HARV", "Russians", 2, 0);

        let mut sim = Simulation::new();
        sim.interner = test_interner();
        sim.substrate.entities.insert(miner);
        sim.substrate.entities.insert(blocker);
        sim.resolved_terrain = Some(terrain);
        sim.zone_grid = Some(zone_grid);
        sim
    };

    let mut ore_trip = make_sim();
    assert!(issue_stock_miner_drive_move(
        &mut ore_trip,
        &rules,
        &path_grid,
        1,
        (3, 0),
    ));
    assert_eq!(
        first_track_process_route(&mut ore_trip, 1, Some(&rules), &path_grid)
            .and_then(|movement| movement.path.last().copied()),
        Some((3, 0)),
    );

    let mut refinery_return = make_sim();
    issue_move_if_idle(
        &mut refinery_return,
        Some(&rules),
        &path_grid,
        1,
        (3, 0),
        SimFixed::from_num(128),
        None,
    );
    assert_eq!(
        first_track_process_route(&mut refinery_return, 1, Some(&rules), &path_grid)
            .and_then(|movement| movement.path.last().copied()),
        Some((3, 0)),
    );
}

#[test]
fn zone_corridor_equal_cost_ties_keep_adjacency_order() {
    let (zone_map, adjacency) = equal_cost_zone_map(vec![3, 2]);
    let excluded_edges = BTreeSet::new();

    let corridor = find_zone_corridor(&zone_map, &adjacency, 1, 5, &excluded_edges)
        .expect("equal-cost corridor should exist");

    assert_eq!(
        corridor,
        vec![1, 3, 5],
        "equal-cost zone ties must keep adjacency discovery order, not lower ZoneId"
    );
}

#[test]
fn zone_corridor_equal_cost_ties_follow_reversed_adjacency_order() {
    let (zone_map, adjacency) = equal_cost_zone_map(vec![2, 3]);
    let excluded_edges = BTreeSet::new();

    let corridor = find_zone_corridor(&zone_map, &adjacency, 1, 5, &excluded_edges)
        .expect("equal-cost corridor should exist");

    assert_eq!(corridor, vec![1, 2, 5]);
}

#[test]
fn zone_corridor_retry_excludes_edges_not_zones() {
    let (zone_map, adjacency) = test_zone_map();
    let mut excluded_edges = BTreeSet::new();

    let first =
        find_zone_corridor(&zone_map, &adjacency, 1, 4, &excluded_edges).expect("initial corridor");
    assert_eq!(first, vec![1, 2, 4]);

    excluded_edges.insert(ZoneEdge::new(1, 2).unwrap());
    let second = find_zone_corridor(&zone_map, &adjacency, 1, 4, &excluded_edges)
        .expect("alternate corridor should reuse zone 2 through another edge");
    assert_eq!(second, vec![1, 3, 2, 4]);
}

#[test]
fn zone_edge_exclusions_are_undirected() {
    let zone_map = ZoneMap::new(
        vec![1, 2],
        None,
        2,
        1,
        2,
        vec![
            ZoneInfo {
                center: (0, 0),
                cell_count: 1,
            },
            ZoneInfo {
                center: (1, 0),
                cell_count: 1,
            },
        ],
    );
    let adjacency = ZoneAdjacency::new(vec![vec![], vec![2], vec![1]]);
    let mut excluded_edges = BTreeSet::new();
    excluded_edges.insert(ZoneEdge::new(1, 2).unwrap());

    assert!(find_zone_corridor(&zone_map, &adjacency, 2, 1, &excluded_edges).is_none());
}

#[test]
fn zone_cost_estimate_matches_precheck_and_alternate_margin() {
    let grid = grid_from_str(
        "
        .....
        .....
    ",
    );
    let zg = ZoneGrid::build(&grid, &BTreeMap::new(), 5, 2);

    let estimate = zone_cost_estimate(
        &zg,
        MovementZone::Normal,
        (0, 0),
        crate::sim::movement::locomotor::MovementLayer::Ground,
        (4, 1),
        crate::sim::movement::locomotor::MovementLayer::Ground,
    );
    assert_eq!(estimate, 4);
    assert!(accepts_blocked_destination_alternate(
        estimate,
        (4, 1),
        (0, 1)
    ));
    assert!(!accepts_blocked_destination_alternate(
        i32::MAX,
        (4, 1),
        (0, 1)
    ));

    let blocked_grid = grid_from_str(
        "
        ..#..
        ..#..
    ",
    );
    let blocked_zg = ZoneGrid::build(&blocked_grid, &BTreeMap::new(), 5, 2);
    assert_eq!(
        zone_cost_estimate(
            &blocked_zg,
            MovementZone::Normal,
            (0, 0),
            crate::sim::movement::locomotor::MovementLayer::Ground,
            (4, 0),
            crate::sim::movement::locomotor::MovementLayer::Ground,
        ),
        i32::MAX
    );
}

/// GSI-06.02 G2: gamemd gates every MovementZone row — `Can_Reach_Zone`
/// short-circuits only on `mzRow == -1`, and the A*-entry precheck reads
/// whatever row `MovementZone=` gives. Stock rulesmd puts every main battle tank
/// in `Destroyer`, every ore miner in `Crusher` and the Battle Fortress in
/// `CrusherAll`, so those rows must reach the reduced precheck.
#[test]
fn gsi_06_02_reduced_zone_precheck_covers_every_land_movement_zone() {
    for mz in [
        MovementZone::Normal,
        MovementZone::Crusher,
        MovementZone::Destroyer,
        MovementZone::AmphibiousDestroyer,
        MovementZone::AmphibiousCrusher,
        MovementZone::Amphibious,
        MovementZone::Infantry,
        MovementZone::InfantryDestroyer,
        MovementZone::Fly,
        MovementZone::CrusherAll,
    ] {
        assert!(
            can_use_reduced_zone_precheck(Some(mz)),
            "{mz:?} must be gated by the reduced zone precheck"
        );
    }

    // `mzRow == -1` returns "reachable" in gamemd, so the gate must not be
    // allowed to refuse the search for it.
    assert!(!can_use_reduced_zone_precheck(Some(MovementZone::Invalid)));

    // VERA-internal residual, gamemd equivalent UNCHECKED: naval surface
    // legality in the zone builder is still coarser than the runtime predicate,
    // so the two water rows stay outside the gate for now.
    assert!(!can_use_reduced_zone_precheck(Some(MovementZone::Water)));
    assert!(!can_use_reduced_zone_precheck(Some(
        MovementZone::WaterBeach
    )));
}

#[test]
fn tube_hierarchy_explicit_registry_keeps_flat_and_layered_corridor_active() {
    use crate::map::tube_facts::TubeFact;
    let (width, height) = (40, 3);
    let grid = PathGrid::new(width, height);
    let mut zones = ZoneGrid::build(&grid, &BTreeMap::new(), width, height);
    let labels = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                if y == 1 && x > 0 && x < width - 1 {
                    2
                } else {
                    1
                }
            })
        })
        .collect();
    zones.set_hierarchy(level0_hierarchy(labels, width, height, &[]));
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        width,
        height,
        gsi_04_12_terrain(width, height).cells,
        vec![TubeFact::explicit((50, 50), (60, 50), 2, vec![2; 10])],
    );
    let counts = BlockerNeighborCounts::new(width, height);
    let flat = find_path_zoned_marker(
        &grid,
        (0, 1),
        (39, 1),
        None,
        None,
        Some(&zones),
        MovementZone::Normal,
        Some(MovementZone::Normal),
        Some(&terrain),
        None,
        None,
        Some(&counts),
        0,
        false,
        false,
        true,
        None,
    )
    .expect("flat hierarchy route");
    assert!(
        !flat.contains(&(20, 1)),
        "an irrelevant explicit Tube must not disable the stamped corridor"
    );
    let flat_unrestricted = find_path_zoned_marker(
        &grid,
        (0, 1),
        (39, 1),
        None,
        None,
        Some(&zones),
        MovementZone::Normal,
        Some(MovementZone::Normal),
        Some(&terrain),
        None,
        None,
        Some(&counts),
        0,
        false,
        false,
        false,
        None,
    )
    .unwrap();
    assert!(
        flat_unrestricted.contains(&(20, 1)),
        "fixture distinguishes ordinary A* from hierarchy routing"
    );
    let layered = find_layered_path_zoned_marker(
        &grid,
        None,
        None,
        (0, 1),
        MovementLayer::Ground,
        (39, 1),
        Some(&zones),
        MovementZone::Normal,
        None,
        Some(MovementZone::Normal),
        Some(&terrain),
        None,
        None,
        Some(&counts),
        0,
        false,
        false,
        true,
        None,
    )
    .expect("layered hierarchy route");
    assert!(
        !layered.iter().any(|step| (step.rx, step.ry) == (20, 1)),
        "layered entry must retain the same hierarchy admission"
    );
}

#[test]
fn tube_hierarchy_gate_uses_raw_invalid_labels_and_flat_goal_bridge_flag() {
    let mut grid = PathGrid::new(2, 2);
    grid.set_cell_for_test(0, 0, 4, false, false);
    grid.set_cell_for_test(1, 0, 0, true, true);
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/tube_hierarchy.json"
    ))
    .unwrap();
    let expected_admission = |a, b| {
        corpus["path_gate"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["a"] == a && r["b"] == b && r["allow"] == 1)
            .unwrap()["action"]
            == "precheck"
    };
    let mut terrain = gsi_04_12_terrain(2, 2);
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &grid,
        &BTreeMap::new(),
        Some(&terrain),
        &[BridgeEndpointRecord {
            endpoint_a: (0, 0),
            endpoint_b: (1, 0),
            group_id: 0,
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }],
        2,
        2,
        Some((1, 1)),
    );
    let row = MovementZone::Normal.matrix_row().unwrap();
    {
        let base = zones.base_topology_mut().unwrap();
        base.zone_ids = vec![2, 3, 2, 3];
        base.zone_count = 3;
        base.raw_zone_ids_by_row[row] = vec![0, 0, 1, u16::MAX];
    }
    {
        let map = zones.map_mut(MovementZone::Normal).unwrap();
        for index in 0..4 {
            map.set_ground_zone_at_index(index, 0);
        }
    }
    zones.set_hierarchy(level0_hierarchy(vec![1; 4], 2, 2, &[]));
    let counts = BlockerNeighborCounts::new(2, 2);
    let route = |zones: &ZoneGrid, terrain: &ResolvedTerrainGrid| {
        find_path_zoned_marker(
            &grid,
            (0, 0),
            (1, 0),
            None,
            None,
            Some(zones),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            Some(terrain),
            None,
            None,
            Some(&counts),
            0,
            false,
            false,
            true,
            None,
        )
    };
    assert_eq!(
        zones.get_zone_id_native((0, 0), MovementZone::Normal, false),
        Some(1)
    );
    assert_eq!(
        zones.get_zone_id_native((1, 0), MovementZone::Normal, false),
        Some(u16::MAX)
    );
    assert_eq!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(0, 0, MovementLayer::Ground),
        0
    );
    assert_eq!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(1, 0, MovementLayer::Ground),
        0
    );
    assert_eq!(
        route(&zones, &terrain).is_some(),
        expected_admission(1, 65535),
        "distinct native invalid labels must reject before a connected graph"
    );
    {
        let base = zones.base_topology_mut().unwrap();
        base.raw_zone_ids_by_row[row][2] = 2;
        base.raw_zone_ids_by_row[row][3] = 3;
    }
    zones
        .map_mut(MovementZone::Normal)
        .unwrap()
        .set_bridge_redirect(Some(vec![None, Some((0, 0)), None, None]));
    zones.set_hierarchy(level0_hierarchy(vec![1; 4], 2, 2, &[]));
    terrain.cell_mut(0, 0).unwrap().level = 4;
    {
        let cell = terrain.cell_mut(1, 0).unwrap();
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    assert_eq!(
        zones.get_zone_id_native((1, 0), MovementZone::Normal, true),
        Some(2)
    );
    assert_eq!(
        route(&zones, &terrain).is_some(),
        expected_admission(2, 2),
        "flat goal Flags0x100 must select the redirected raw zone"
    );
    terrain.cell_mut(1, 0).unwrap().bridge_facts.raw_flags = 0;
    assert_eq!(
        route(&zones, &terrain).is_some(),
        expected_admission(2, 3),
        "ground goal reads its different underlying raw label"
    );
}

#[test]
fn tube_hierarchy_dword_zone_query_matches_original_executable() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/tube_hierarchy.json"
    ))
    .unwrap();
    for case in corpus["get_zone"].as_array().unwrap() {
        let coord =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let query = coord(&case["query"]);
        let linear = i32::from(query.1 as i16) * 512 + i32::from(query.0 as i16);
        let real = case["real"].as_bool().unwrap();
        let canonical = ((linear.rem_euclid(512)) as u16, (linear / 512) as u16);
        let extras = case["extra"].as_array().cloned().unwrap_or_default();
        let mut allocated = if real { vec![canonical] } else { vec![] };
        allocated.extend(extras.iter().map(|entry| coord(&entry[0])));
        let width = allocated.iter().map(|c| c.0).max().unwrap_or(0).max(15) + 1;
        let height = allocated.iter().map(|c| c.1).max().unwrap_or(0).max(15) + 1;
        let mut terrain = gsi_04_12_terrain(width, height);
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        for entry in &extras {
            let c = coord(&entry[0]);
            let cell = terrain.cell_mut(c.0, c.1).unwrap();
            cell.final_tile_index = entry[1].as_i64().unwrap() as i32;
            cell.yr_cell_land_type = entry[2].as_u64().unwrap() as u8;
            cell.bridge_facts.raw_flags = entry[3].as_u64().unwrap() as u32;
        }
        let flags = case["flags"].as_u64().unwrap() as u32;
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(1234, -2345);
        if real {
            terrain
                .cell_mut(canonical.0, canonical.1)
                .unwrap()
                .bridge_facts
                .raw_flags = flags;
        } else if flags & 0x100 != 0 {
            dummy.apply_bridge_flag_slot(crate::map::bridge_facts::BridgeStampSlot::Anchor, true);
        }
        let records = case["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| BridgeEndpointRecord {
                endpoint_a: coord(&r[0]),
                endpoint_b: coord(&r[1]),
                group_id: 0,
                active: r[2].as_bool().unwrap(),
                bridge_kind: if r[3] == 0 {
                    BridgeRecordKind::High
                } else {
                    BridgeRecordKind::Low
                },
            })
            .collect::<Vec<_>>();
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &PathGrid::new(width, height),
            &BTreeMap::new(),
            Some(&terrain),
            &records,
            width,
            height,
            Some((8, 8)),
        );
        {
            let base = zones.base_topology_mut().unwrap();
            base.zone_ids.fill(1);
            for record in &records {
                if !record.active {
                    let index = (i32::from(record.endpoint_b.1 as i16) * 17
                        + i32::from(record.endpoint_b.0 as i16))
                    .clamp(0, 288) as usize;
                    let (x, y) = (index % 17, index / 17);
                    assert!(
                        x < 16 && y < 16,
                        "fixture endpoint must project into real node storage"
                    );
                    base.zone_ids[y * usize::from(width) + x] = 2;
                }
            }
            base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()] =
                vec![0, case["raw"].as_u64().unwrap() as u16, 3];
        }
        dummy.stamp_coord(1234, -2345);
        assert_eq!(
            zones.get_path_zone_id_native(
                &terrain,
                query,
                MovementZone::Normal,
                case["check"] == 1
            ),
            Some(case["result"].as_u64().unwrap() as u32),
            "{}",
            case["name"]
        );
        assert_eq!(
            dummy.snapshot().coord,
            (
                case["dummy_coord"][0].as_i64().unwrap() as i32,
                case["dummy_coord"][1].as_i64().unwrap() as i32
            ),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn tube_hierarchy_missing_record_sentinel_controls_actual_route_gate() {
    let grid = PathGrid::new(16, 16);
    let mut terrain = gsi_04_12_terrain(16, 16);
    // Native producer corpus high_no_far: no far matching bridge tile, so the
    // real factory leaves the structural middle cell without a high record.
    {
        let start = terrain.cell_mut(4, 7).unwrap();
        start.final_tile_index = 106;
        start.final_sub_tile = 4;
    }
    for x in [5, 6] {
        let c = terrain.cell_mut(x, 7).unwrap();
        c.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        c.has_bridge_deck = true;
        c.bridge_walkable = true;
        c.bridge_transition = true;
        c.bridge_deck_level = 0;
    }
    terrain.test_set_high_bridge_set_starts(Some(100), None);
    let bridges = crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
        &terrain,
        true,
        100,
        (8, 8),
    );
    assert!(bridges.endpoint_records().is_empty());
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &grid,
        &BTreeMap::new(),
        Some(&terrain),
        bridges.endpoint_records(),
        16,
        16,
        Some((8, 8)),
    );
    zones.set_hierarchy(level0_hierarchy(vec![1; 256], 16, 16, &[]));
    let counts = BlockerNeighborCounts::new(16, 16);
    assert_eq!(
        zones.get_path_zone_id_native(&terrain, (5, 7), MovementZone::Normal, true),
        Some(u32::MAX)
    );
    assert_ne!(
        zones.get_path_zone_id_native(&terrain, (4, 7), MovementZone::Normal, false),
        Some(u32::MAX)
    );
    assert!(
        find_path_zoned_marker(
            &grid,
            (4, 7),
            (5, 7),
            None,
            None,
            Some(&zones),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            Some(&terrain),
            None,
            None,
            Some(&counts),
            0,
            false,
            false,
            true,
            None
        )
        .is_none(),
        "ordinary label differs from missing structural DWORD sentinel"
    );
    assert_eq!(
        native_path_zone_equality(
            &zones,
            Some(&terrain),
            MovementZone::Normal,
            (5, 7),
            true,
            (6, 7),
            true
        ),
        Some(true),
        "two missing structural records compare equal at DWORD width"
    );
    assert!(
        find_layered_path_zoned_marker(
            &grid,
            None,
            None,
            (5, 7),
            MovementLayer::Bridge,
            (6, 7),
            Some(&zones),
            MovementZone::Normal,
            None,
            Some(MovementZone::Normal),
            Some(&terrain),
            None,
            None,
            Some(&counts),
            0,
            false,
            false,
            true,
            None
        )
        .is_some(),
        "equal missing sentinels admit hierarchy; physical route remains independently passable"
    );
}

#[test]
fn tube_hierarchy_native_entry_prefix_matches_original_executable() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/path_entry.json"
    ))
    .unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 19);
    for case in cases.as_array().unwrap() {
        let coord =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let rows = case["cells"].as_array().unwrap();
        let allocated: Vec<_> = rows.iter().map(|r| coord(&r[0])).collect();
        let width = allocated.iter().map(|c| c.0).max().unwrap().max(15) + 1;
        let height = allocated.iter().map(|c| c.1).max().unwrap().max(15) + 1;
        let grid = PathGrid::new(width, height);
        let mut terrain = gsi_04_12_terrain(width, height);
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        for row in rows {
            let c = coord(&row[0]);
            let cell = terrain.cell_mut(c.0, c.1).unwrap();
            cell.bridge_facts.raw_flags = row[1].as_u64().unwrap() as u32;
            cell.final_tile_index = row[2].as_i64().unwrap() as i32;
            cell.yr_cell_land_type = row[3].as_u64().unwrap() as u8;
        }
        let records: Vec<_> = case["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| BridgeEndpointRecord {
                endpoint_a: coord(&r[0]),
                endpoint_b: coord(&r[1]),
                active: r[2].as_bool().unwrap(),
                group_id: 0,
                bridge_kind: BridgeRecordKind::High,
            })
            .collect();
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &grid,
            &BTreeMap::new(),
            Some(&terrain),
            &records,
            width,
            height,
            Some((8, 8)),
        );
        let base = zones.base_topology_mut().unwrap();
        base.zone_ids.fill(1);
        base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()] = vec![0, 2];
        let b = &case["bounds"];
        let bounds = PlayfieldBounds::from_normalized_local_size(
            b[0].as_i64().unwrap() as i32,
            b[1].as_i64().unwrap() as i32,
            b[2].as_i64().unwrap() as i32,
            b[3].as_i64().unwrap() as i32,
            b[4].as_i64().unwrap() as i32,
        );
        assert_eq!(
            case["dummy_flags"], 0,
            "fixture currently supplies constructor bridge flags"
        );
        let dummy = terrain.shared_cell_dummy();
        dummy.reconstruct_for_map_resize();
        dummy.stamp_coord(1234, -2345);
        let start = coord(&case["start"]);
        let goal = coord(&case["goal"]);
        let entry = prepare_native_path_entry(
            Some(&zones),
            Some(&terrain),
            MovementZone::Normal,
            start,
            case["start_bridge"].as_bool().unwrap(),
            goal,
            case["allow"].as_bool().unwrap(),
            Some(bounds),
        );
        assert_eq!(
            entry.raw_equal,
            Some(case["labels"][0] == case["labels"][1]),
            "{}",
            case["name"]
        );
        assert_eq!(
            entry.hierarchy_start,
            coord(&case["hierarchy_start"]),
            "{}",
            case["name"]
        );
        assert_eq!(
            entry.hierarchy_goal,
            coord(&case["hierarchy_goal"]),
            "{}",
            case["name"]
        );
        assert_eq!(
            entry.endpoints_in_playfield,
            case["endpoints_inside"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        let terminal = coord(&case["dummy_coord"]);
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(terminal.0 as i16), i32::from(terminal.1 as i16)),
            "{}",
            case["name"]
        );
        if case["name"] == "initial_source_miss_False" {
            // Physical backing admits this same-cell request, while the native
            // allocation table has a hole. With no playfield configured and
            // hierarchy disabled, only the live entry lookup publishes it.
            let hole = (4, 4);
            assert!(terrain.cell(hole.0, hole.1).is_none());
            dummy.stamp_coord(1234, -2345);
            assert!(
                find_path_zoned_marker(
                    &grid,
                    hole,
                    hole,
                    None,
                    None,
                    Some(&zones),
                    MovementZone::Normal,
                    Some(MovementZone::Normal),
                    Some(&terrain),
                    None,
                    None,
                    None,
                    0,
                    false,
                    false,
                    false,
                    None
                )
                .is_some()
            );
            assert_eq!(dummy.snapshot().coord, (4, 4));
            dummy.stamp_coord(1234, -2345);
            assert!(
                find_layered_path_zoned_marker(
                    &grid,
                    None,
                    None,
                    hole,
                    MovementLayer::Ground,
                    hole,
                    Some(&zones),
                    MovementZone::Normal,
                    None,
                    Some(MovementZone::Normal),
                    Some(&terrain),
                    None,
                    None,
                    None,
                    0,
                    false,
                    false,
                    false,
                    None
                )
                .is_some()
            );
            assert_eq!(dummy.snapshot().coord, (4, 4));
        }
    }
}

// Preserve the existing height/projection assertions through the live entry owner.
#[allow(clippy::too_many_arguments)]
fn resolve_hierarchy_endpoint_contract(
    zones: Option<&ZoneGrid>,
    terrain: Option<&ResolvedTerrainGrid>,
    bounds: Option<PlayfieldBounds>,
    start: (u16, u16),
    start_bridge: bool,
    goal: (u16, u16),
    _goal_bridge: bool,
) -> ((u16, u16), (u16, u16), bool) {
    let entry = prepare_native_path_entry(
        zones,
        terrain,
        MovementZone::Normal,
        start,
        start_bridge,
        goal,
        true,
        bounds,
    );
    (
        entry.hierarchy_start,
        entry.hierarchy_goal,
        entry.endpoints_in_playfield,
    )
}
