//! Tests for A* pathfinding and PathGrid walkability.
//!
//! Extracted from pathfinding.rs to stay under the 400-line limit.

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::zone_hierarchy::{MarkedZones, ZoneLevelGraph, ZoneRecord};
use crate::sim::pathfinding::zone_map::ZoneId;

fn bridge_test_cell(level: u8, structural: bool, transition: bool, slope_type: u8) -> PathCell {
    PathCell {
        ground_walkable: true,
        bridge_walkable: structural,
        bridge_structural: structural,
        bridge_marker_0x80: false,
        transition,
        ground_level: level,
        bridge_deck_level: level.saturating_add(4),
        slope_type,
        tube_index: None,
        low_bridge_tube_cell: false,
    }
}

/// An expansion's cheapest child is held out of the open list; on a tie the
/// open list's root is expanded first and the held child takes its place
/// (`0x0042A1B3..0x0042A1E2`).
#[test]
fn the_held_child_gives_way_to_an_equal_open_root() {
    let node = |f_cost: f32, x: u16| AStarNode {
        f_cost,
        g_cost: 0.0,
        x,
        y: 0,
        height: 0,
        on_bridge: false,
    };
    let mut open = OpenList::new(OPEN_LIST_SIZE);
    let mut held = None;
    for (x, f_cost) in [(1, 5.0), (2, 3.0), (3, 3.0)] {
        offer_child(&mut held, &mut open, node(f_cost, x));
    }
    // The first 3.0 replaced 5.0 as the held child; the second went to the
    // open list.
    assert_eq!(held.map(|n| n.x), Some(2));
    assert_eq!(next_node(held, &mut open).map(|n| n.x), Some(3));
    assert_eq!(open.pop(cheaper).map(|n| n.x), Some(2));
    // A held child cheaper than the root goes next.
    assert_eq!(
        next_node(Some(node(1.0, 4)), &mut open).map(|n| n.x),
        Some(4)
    );
}

/// A route found after exactly 10,000 expansions is refused (`0x0042A3E2`).
/// In a corridor each expansion reaches one new cell, so the goal 10,000
/// cells away is found at that count.
#[test]
fn a_route_found_after_exactly_ten_thousand_expansions_is_refused() {
    let grid = PathGrid::new(10_002, 1);
    assert!(find_path(&grid, (0, 0), (9_999, 0)).is_some());
    assert!(find_path(&grid, (0, 0), (10_000, 0)).is_none());
    assert!(find_path(&grid, (0, 0), (10_001, 0)).is_some());
}

/// Admits the goal only from one neighbour, at class 5 (base 20), and refuses
/// it from every other.
struct OneDoorGoal {
    goal: (u16, u16),
    door: (u16, u16),
}

impl SearchFootEntry for OneDoorGoal {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
        let SearchEntryCandidate::CopiedCoord(candidate) = query.candidate else {
            panic!("A* asks copied coordinates");
        };
        Ok(if candidate != self.goal {
            0
        } else if query.from == Some(self.door) {
            5
        } else {
            7
        })
    }
}

/// A closed goal is asked about again from a neighbour whose g is at least
/// 1.009 below the goal node's (`0x00429EE2..0x00429F31`). Refused there, the
/// search ends at that neighbour (`0x0042A17D`) and never expands the goal's
/// node.
#[test]
fn a_closed_goal_refused_from_a_cheaper_neighbor_ends_the_route_there() {
    let grid = PathGrid::new(3, 2);
    let entry = OneDoorGoal {
        goal: (2, 0),
        door: (1, 0),
    };
    let path = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (2, 0),
        &AStarOptions::new(&entry),
    )
    .unwrap();
    // The goal's node came from the door at g about 21; the diagonal (1, 1)
    // is expanded first at g about 1 and finds it refused.
    assert_eq!(
        path.iter()
            .map(|step| (step.rx, step.ry))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 1)]
    );
}

/// Refuses only the goal, as a Unit whose type may be `Passive=`.
struct RefusedGoal {
    goal: (u16, u16),
    passive_unit: bool,
}

impl SearchFootEntry for RefusedGoal {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
        let SearchEntryCandidate::CopiedCoord(candidate) = query.candidate else {
            panic!("A* asks copied coordinates");
        };
        Ok(if candidate == self.goal { 7 } else { 0 })
    }
    fn is_passive_unit(&self) -> bool {
        self.passive_unit
    }
}

/// A `Passive=` Unit never takes the blocked-goal tail (`0x0042A183`): its
/// search for a refused goal finds no route instead of ending beside it.
#[test]
fn a_passive_unit_does_not_end_its_route_beside_a_refused_goal() {
    let grid = PathGrid::new(4, 1);
    let route = |passive_unit| {
        let entry = RefusedGoal {
            goal: (3, 0),
            passive_unit,
        };
        astar_search(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (3, 0),
            &AStarOptions::new(&entry),
        )
        .map(|path| {
            path.iter()
                .map(|step| (step.rx, step.ry))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(route(false), Ok(vec![(0, 0), (1, 0), (2, 0)]));
    assert_eq!(route(true), Err(PathSearchFailure::CellSearchExhausted));
}

/// A tube edge costs the cells from the current cell to the tube's exit,
/// Chebyshev (`0x00429FA3..0x00429FE6`), however long the tube's path. This
/// one winds ten steps to an exit three cells away, so going through it
/// (3.0, then 1.002 to the goal) beats walking (about 4.008).
#[test]
fn a_tube_edge_costs_the_cells_to_its_exit_not_its_path_length() {
    let mut cells = Vec::new();
    for y in 0..3 {
        for x in 0..5 {
            cells.push(make_resolved_cell(x, y));
        }
    }
    let start_idx = 5; // (0, 1)
    cells[start_idx].land_type = LandType::Tunnel.as_index();
    cells[start_idx].tube_index = Some(TubeId(0));
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        5,
        3,
        cells,
        vec![TubeFact::explicit((0, 1), (3, 1), 2, vec![2; 10])],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let path = astar_search(
        &grid,
        (0, 1),
        MovementLayer::Ground,
        (4, 1),
        &AStarOptions {
            resolved_terrain: Some(&terrain),
            ..AStarOptions::new(&WalkableEntry::new(&grid))
        },
    )
    .expect("route through the tube");
    assert_eq!(
        path.iter()
            .map(|step| (step.rx, step.ry))
            .collect::<Vec<_>>(),
        [(0, 1), (3, 1), (4, 1)]
    );
}

/// 0x00429BF3..0x00429C0A: AStar has no route for a goal in its start cell.
#[test]
fn test_find_path_same_cell_has_no_route() {
    let grid: PathGrid = PathGrid::new(10, 10);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (5, 5), (5, 5));
    assert_eq!(path, None);
}

#[test]
fn test_path_step_count_chebyshev_open_grid() {
    // With uniform edge cost and a Euclidean heuristic, the optimal step
    // count from (sx,sy) to (gx,gy) on an open grid is max(|dx|, |dy|).
    // Path length includes the start cell, so it equals chebyshev + 1.
    let grid: PathGrid = PathGrid::new(20, 20);
    type Case = ((u16, u16), (u16, u16), usize);
    let cases: &[Case] = &[
        ((0, 0), (5, 0), 6),    // pure cardinal: 5 E steps -> 6 cells
        ((0, 0), (0, 7), 8),    // pure cardinal: 7 S steps -> 8 cells
        ((0, 0), (4, 4), 5),    // pure diagonal: 4 SE steps -> 5 cells
        ((0, 0), (5, 3), 6),    // mixed: max(5,3) = 5 -> 6 cells
        ((0, 0), (7, 2), 8),    // mixed: max(7,2) = 7 -> 8 cells
        ((10, 10), (3, 15), 8), // both axes nonzero, dx=7 dy=5 -> 8 cells
    ];
    for &(start, goal, expected_len) in cases {
        let path = find_path(&grid, start, goal)
            .unwrap_or_else(|| panic!("no path for {:?}->{:?}", start, goal));
        assert_eq!(
            path.len(),
            expected_len,
            "path {:?}->{:?} expected {} cells, got {}: {:?}",
            start,
            goal,
            expected_len,
            path.len(),
            path
        );
        assert_eq!(path.first().copied(), Some(start));
        assert_eq!(path.last().copied(), Some(goal));
    }
}

#[test]
fn test_layered_path_cell_bridge_helpers() {
    let bridge_cell = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: true,
        ground_level: 1,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    assert_eq!(bridge_cell.bridge_deck_level_if_any(), Some(4));
    assert_eq!(
        bridge_cell.effective_cell_z_for_layer(MovementLayer::Bridge),
        4
    );
    assert_eq!(
        bridge_cell.effective_cell_z_for_layer(MovementLayer::Ground),
        1
    );

    let low_bridge = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 2,
        bridge_deck_level: 2,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    assert_eq!(low_bridge.bridge_deck_level_if_any(), Some(2));
    assert_eq!(
        low_bridge.effective_cell_z_for_layer(MovementLayer::Bridge),
        2
    );
}

#[test]
fn test_find_path_around_obstacle() {
    let mut grid: PathGrid = PathGrid::new(10, 10);
    // Build a wall from (3,0) to (3,8), leaving a gap at (3,9).
    for y in 0..9 {
        grid.set_blocked(3, y, true);
    }
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (1, 5), (5, 5));
    let path: Vec<(u16, u16)> = path.expect("Should find path around wall");
    assert_eq!(*path.first().expect("non-empty"), (1, 5));
    assert_eq!(*path.last().expect("non-empty"), (5, 5));
    // Path must avoid column 3, rows 0-8.
    for &(x, y) in &path {
        if x == 3 {
            assert!(y >= 9, "Path must not cross blocked cells at (3,{})", y);
        }
    }
}

#[test]
fn test_find_path_after_another_search_ignores_its_closed_cells() {
    // The thread's A* workspace keeps its buffers between searches. A search
    // on a bigger map leaves them bigger, and the way back runs through cells
    // the way there closed; neither may reach a later search.
    find_path(&PathGrid::new(40, 40), (0, 0), (39, 39)).expect("open-map route");
    let mut grid: PathGrid = PathGrid::new(10, 10);
    for y in 0..9 {
        grid.set_blocked(3, y, true);
    }
    find_path(&grid, (1, 5), (5, 5)).expect("Should find path around wall");
    let back = find_path(&grid, (5, 5), (1, 5));
    let fresh = std::thread::scope(|scope| {
        scope
            .spawn(|| find_path(&grid, (5, 5), (1, 5)))
            .join()
            .expect("fresh-workspace search")
    });
    assert!(back.is_some(), "Should find the way back");
    assert_eq!(back, fresh);
}

#[test]
fn test_find_path_no_path_exists() {
    let mut grid: PathGrid = PathGrid::new(10, 10);
    // Completely wall off the right side.
    for y in 0..10 {
        grid.set_blocked(5, y, true);
    }
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (0, 0), (9, 9));
    assert!(path.is_none(), "Should find no path through complete wall");
}

#[test]
fn test_find_path_blocked_start_finds_path_through_walkable_neighbors() {
    // Matches the original engine's A*: the start cell may be blocked (e.g.
    // a unit standing inside a building footprint after undock). A* expands
    // neighbors normally; if any neighbor is walkable, a path can be found.
    let mut grid: PathGrid = PathGrid::new(10, 10);
    grid.set_blocked(5, 5, true);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (5, 5), (8, 5));
    assert!(
        path.is_some(),
        "Blocked start with walkable neighbors should find a path"
    );
    let path = path.unwrap();
    assert_eq!(
        path.first().copied(),
        Some((5, 5)),
        "path starts at start cell"
    );
    assert_eq!(path.last().copied(), Some((8, 5)), "path ends at goal");
    assert!(path.len() >= 2, "path has at least start + goal");
}

#[test]
fn test_find_path_blocked_start_all_neighbors_blocked_returns_none() {
    // Negative case: if the start cell is blocked AND all 8 neighbors are
    // blocked, A* exhausts its open set and returns None.
    let mut grid: PathGrid = PathGrid::new(10, 10);
    grid.set_blocked(5, 5, true);
    grid.set_blocked(4, 4, true);
    grid.set_blocked(5, 4, true);
    grid.set_blocked(6, 4, true);
    grid.set_blocked(4, 5, true);
    grid.set_blocked(6, 5, true);
    grid.set_blocked(4, 6, true);
    grid.set_blocked(5, 6, true);
    grid.set_blocked(6, 6, true);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (5, 5), (8, 5));
    assert!(
        path.is_none(),
        "Blocked start with all neighbors blocked should return None"
    );
}

#[test]
fn test_find_path_blocked_goal_returns_path_to_adjacent_cell() {
    // An impassable destination does not abort the search. gamemd runs it, and
    // the first time a reached cell has the blocked goal as a neighbour it
    // leaves the loop and returns the path to that adjacent cell.
    let mut grid: PathGrid = PathGrid::new(10, 10);
    grid.set_blocked(5, 5, true);
    let path: Vec<(u16, u16)> = find_path(&grid, (0, 0), (5, 5))
        .expect("blocked goal must still produce the path to a cell adjacent to it");
    assert!(path.len() >= 2, "path must contain at least one real step");
    assert_eq!(path[0], (0, 0));
    let last = *path.last().unwrap();
    assert_ne!(last, (5, 5), "the blocked goal itself is never entered");
    assert!(
        last.0.abs_diff(5) <= 1 && last.1.abs_diff(5) <= 1,
        "path must end adjacent to the blocked goal, ended at {last:?}"
    );
}

#[test]
fn test_find_path_blocked_goal_adjacent_to_start_fails() {
    // The success tail requires the aborting node to be at least one real step
    // from the start (node depth >= 2). A mover already standing next to the
    // blocked cell therefore gets no path at all.
    let mut grid: PathGrid = PathGrid::new(10, 10);
    grid.set_blocked(5, 5, true);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (4, 4), (5, 5));
    assert!(
        path.is_none(),
        "start-adjacent blocked goal fails the search outright"
    );
}

#[test]
fn test_find_path_diagonal_corner_cutting_allowed() {
    // gamemd parity: AStar_main_loop calls Can_Enter_Cell only on the
    // diagonal neighbor; it does NOT validate the two flanking cardinal
    // cells. Units may "clip" between two impassable cells at a corner.
    // See PATHFINDING_ASTAR_GHIDRA_REPORT.md §4.3 +
    // PATHFINDING_CELL_ENTRY_VERIFICATION_REPORT.md §6.2.
    let mut grid: PathGrid = PathGrid::new(5, 5);
    grid.set_blocked(1, 0, true);
    grid.set_blocked(0, 1, true);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (0, 0), (1, 1));
    assert!(
        path.is_some(),
        "Diagonal (0,0)->(1,1) must be reachable even when both flanking cardinals are blocked"
    );
    let path = path.unwrap();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(1, 1)));
}

#[test]
fn gsi_04_12_astar_allows_destination_legal_bridge_diagonal_with_missing_flank() {
    // Retail PathfinderClass+0x01 is constructor-zero and never written, so
    // AStar_compute_edge_cost never consults the two cardinal bridge flanks.
    // BayOPigs has this stock-shaped entry at (112,134)->(113,135): the start
    // and east flank are level-5 ground, while the south flank and diagonal
    // destination are level-1 structural transition cells stamped by BRIDGE2.
    let mut grid = PathGrid::new(2, 2);
    grid.set_cell_for_test(0, 0, 5, false, false);
    grid.set_cell_for_test(1, 0, 5, false, false);
    grid.set_cell_for_test(0, 1, 1, true, true);
    grid.set_cell_for_test(1, 1, 1, true, true);

    let path = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (1, 1),
        &AStarOptions::new(&WalkableEntry::new(&grid)),
    )
    .expect("destination-legal bridge diagonal must remain traversable");

    assert_eq!(
        path.iter()
            .map(|step| (step.rx, step.ry, step.layer))
            .collect::<Vec<_>>(),
        vec![(0, 0, MovementLayer::Ground), (1, 1, MovementLayer::Bridge),],
        "missing cardinal bridge flank must not force a detour"
    );
}

#[test]
fn test_block_building_movement_cells() {
    let mut grid = PathGrid::new(10, 10);
    // All cells should be walkable initially.
    assert!(grid.is_walkable(3, 3));
    assert!(grid.is_walkable(4, 4));
    // Block a 2x2 building at (3, 3).
    grid.block_building_movement_cells(3, 3, &"2x2".into(), false);
    assert!(!grid.is_walkable(3, 3));
    assert!(!grid.is_walkable(4, 3));
    assert!(!grid.is_walkable(3, 4));
    assert!(!grid.is_walkable(4, 4));
    // Adjacent cells remain walkable.
    assert!(grid.is_walkable(2, 3));
    assert!(grid.is_walkable(5, 3));
}

#[test]
fn garefn_footprint_leaves_dock_pad_walkable() {
    let mut grid = PathGrid::new(32, 32);
    grid.block_building_movement_cells(10, 10, &"4x3".into(), false);
    assert!(
        !grid.is_walkable(13, 11),
        "base foundation should block the dock pad"
    );
    assert!(
        grid.is_walkable(9, 10),
        "AddOccupy1=-1,0 is hidden occupancy only"
    );
    assert!(
        grid.is_walkable(9, 9),
        "AddOccupy2=-1,-1 is hidden occupancy only"
    );
    assert!(!grid.is_walkable(10, 10));
    assert!(!grid.is_walkable(13, 12));
}

#[test]
fn garefn_bib_static_blockers_only_relax_east_edge() {
    let mut grid = PathGrid::new(32, 32);
    grid.block_building_movement_cells(10, 10, &"4x3".into(), true);
    assert!(!grid.is_walkable(10, 11));
    assert!(!grid.is_walkable(12, 11));
    assert!(grid.is_walkable(13, 11));
    assert!(grid.is_walkable(9, 10));
}

#[test]
fn test_from_resolved_terrain_uses_resolved_blocking_flags() {
    let terrain = ResolvedTerrainGrid::from_cells(
        2,
        2,
        vec![
            make_resolved_cell(0, 0),
            ResolvedTerrainCell {
                land_type: LandType::Water.as_index(),
                ground_walk_blocked: true,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                terrain_object_blocks: true,
                terrain_object_occupation: Some(1),
                ..make_resolved_cell(0, 1)
            },
            ResolvedTerrainCell {
                overlay_blocks: true,
                overlay_zone_type: Some(crate::map::resolved_terrain::zone_class::IMPASSABLE),
                ..make_resolved_cell(1, 1)
            },
        ],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    assert!(grid.is_walkable(0, 0), "Clear land is walkable");
    // Water cells are PathGrid-walkable: the mover's Can_Enter_Cell reads the
    // land row.
    assert!(grid.is_walkable(1, 0), "Water is PathGrid-walkable");
    assert!(!grid.is_walkable(0, 1), "Terrain object blocks all units");
    assert!(!grid.is_walkable(1, 1), "Overlay blocks all units");
}

#[test]
fn test_path_grid_rebuild_closes_destroyed_bridge_deck() {
    // Five cells: land(h=4) → bridgehead → deck over water → bridgehead → land(h=4).
    let terrain = ResolvedTerrainGrid::from_cells(
        5,
        1,
        vec![
            ResolvedTerrainCell {
                level: 4,
                ..make_resolved_cell(0, 0)
            },
            ResolvedTerrainCell {
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                // Binary high-bridge bridgeheads carry both 0x100 and 0x200:
                // they are structural transition cells, not ramp metadata.
                has_bridge_deck: true,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                ground_walk_blocked: true,
                base_build_blocked: true,
                base_land_type: 0,
                base_speed_costs: Default::default(),
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                land_type: LandType::Water.as_index(),
                ..make_resolved_cell(2, 0)
            },
            ResolvedTerrainCell {
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                ..make_resolved_cell(3, 0)
            },
            ResolvedTerrainCell {
                level: 4,
                ..make_resolved_cell(4, 0)
            },
        ],
    );
    let mut terrain = terrain;
    // Legacy decks (no bit0x100) stay intact while their +44 identity decodes.
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(5, 1);
    for rx in 1..=3 {
        overlays.cell_mut_for_test(rx, 0).overlay_id = Some(0x18);
    }
    let intact_grid = PathGrid::from_resolved_terrain_with_bridges(&terrain, Some(&overlays));
    assert!(
        intact_grid.is_walkable_on_layer(2, 0, MovementLayer::Bridge),
        "intact deck stays open on the bridge layer"
    );

    // Destroy only the body cell (rx=2) with 47E040's destroyed stamp: state
    // 0, bit0x100 clear and bit0x400 set. The bridgeheads keep their state.
    terrain
        .cell_mut_for_test(2, 0)
        .unwrap()
        .bridge_facts
        .raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_DESTROYED_OR_RAMP;
    overlays.cell_mut_for_test(2, 0).overlay_data = 0;

    let destroyed_grid = PathGrid::from_resolved_terrain_with_bridges(&terrain, Some(&overlays));
    assert!(
        !destroyed_grid.is_walkable_on_layer(2, 0, MovementLayer::Bridge),
        "the rebuild closes the destroyed deck"
    );
}

#[test]
fn test_pathcell_bridge_walkable_preserved_for_bridgeheads_across_rebuild() {
    // After the G7 fix, PathCell.bridge_walkable for pass-4 bridgeheads
    // (rx=1, rx=3) must stay true across every PathGrid rebuild driven by
    // from_resolved_terrain_with_bridges.
    let terrain = ResolvedTerrainGrid::from_cells(
        5,
        1,
        vec![
            ResolvedTerrainCell {
                level: 4,
                ..make_resolved_cell(0, 0)
            },
            ResolvedTerrainCell {
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: false,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                ground_walk_blocked: true,
                base_build_blocked: true,
                base_land_type: 0,
                base_speed_costs: Default::default(),
                bridge_walkable: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                land_type: LandType::Water.as_index(),
                ..make_resolved_cell(2, 0)
            },
            ResolvedTerrainCell {
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: false,
                ..make_resolved_cell(3, 0)
            },
            ResolvedTerrainCell {
                level: 4,
                ..make_resolved_cell(4, 0)
            },
        ],
    );
    // Multiple rebuilds: in production each fires from a non-bridge event
    // (unit spawn, ownership change, destroyed structure).
    for _ in 0..3 {
        let grid = PathGrid::from_resolved_terrain_with_bridges(&terrain, None);
        let pc1 = grid.cell(1, 0).expect("bridgehead cell exists");
        let pc3 = grid.cell(3, 0).expect("bridgehead cell exists");
        assert!(
            pc1.bridge_walkable,
            "rx=1 bridgehead must remain bridge_walkable"
        );
        assert!(
            pc3.bridge_walkable,
            "rx=3 bridgehead must remain bridge_walkable"
        );
        assert!(
            pc1.transition,
            "rx=1 bridgehead must remain a transition cell"
        );
        assert!(
            pc3.transition,
            "rx=3 bridgehead must remain a transition cell"
        );
    }
}

// Native height and list-selection goldens live in astar_entry_tests.rs; the
// class's height, bridge and tube traversal (Foot `0x004D9C60`) is compared
// with the original rows in `world::object_entry`'s `unit_entry_tests`.

#[test]
fn test_encode_decode_from_max_map() {
    // 512x512 = 262144 cells — must not collide with bridge bit (1 << 20 = 1048576)
    let encoded = encode_from(262143, true);
    let (idx, bridge) = decode_from(encoded);
    assert_eq!(idx, 262143);
    assert!(bridge);
}

fn make_resolved_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

/// [`find_path`] with the map's cells, for the allocated-Cell test and Tube
/// edges.
fn find_path_over(
    grid: &PathGrid,
    terrain: &ResolvedTerrainGrid,
    start: (u16, u16),
    goal: (u16, u16),
) -> Option<Vec<(u16, u16)>> {
    let steps = astar_search(
        grid,
        start,
        MovementLayer::Ground,
        goal,
        &AStarOptions {
            resolved_terrain: Some(terrain),
            ..AStarOptions::new(&WalkableEntry::new(grid))
        },
    )
    .ok()?;
    Some(steps.into_iter().map(|s| (s.rx, s.ry)).collect())
}

fn infantry_under_span_cell(rx: u16, foot_cost: u8, transition: bool) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        level: 2,
        speed_costs: SpeedCostProfile {
            foot: Some(foot_cost),
            ..Default::default()
        },
        has_bridge_deck: true,
        bridge_walkable: true,
        bridge_transition: transition,
        bridge_deck_level: 6,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts {
            raw_flags: 0x100 | if transition { 0x200 } else { 0 },
            ..Default::default()
        },
        ..make_resolved_cell(rx, 0)
    }
}

#[test]
fn infantry_under_span_astar_keeps_ground_hierarchy_gate() {
    let terrain = ResolvedTerrainGrid::from_cells(
        8,
        1,
        (0..8)
            .map(|x| {
                if (2..=5).contains(&x) {
                    infantry_under_span_cell(x, 100, x != 2)
                } else {
                    ResolvedTerrainCell {
                        level: 2,
                        ..make_resolved_cell(x, 0)
                    }
                }
            })
            .collect(),
    );
    let grid = PathGrid::from_cells(
        (0..8)
            .map(|x| bridge_test_cell(2, (2..=5).contains(&x), (3..=5).contains(&x), 0))
            .collect(),
        8,
        1,
    );
    let zones = ZoneLevelGraph::new(2).with_cell_zone_ids(vec![1, 1, 2, 2, 2, 2, 1, 1], 8, 1);
    let counts = BlockerNeighborCounts::new(8, 1);
    for include_under_span in [false, true] {
        let marked = if include_under_span {
            MarkedZones::from_iter([1, 2])
        } else {
            MarkedZones::from_iter([1])
        };
        let path = astar_search(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (7, 0),
            &AStarOptions {
                resolved_terrain: Some(&terrain),
                hierarchy_gate: Some(HierarchyGate {
                    level0_zones: &zones,
                    marked_level0: &marked,
                    blocker_neighbor_counts: &counts,
                    level0_route: &[],
                    progress: &Cell::new(RouteProgress {
                        index: 0,
                        cell: (0, 0),
                    }),
                }),
                ..AStarOptions::new(&WalkableEntry::new(&grid))
            },
        );
        if include_under_span {
            let path = path.expect("marked ground corridor must cross all four span lanes");
            assert_eq!(path.len(), 8);
            assert!(path.iter().all(|step| step.layer == MovementLayer::Ground));
            assert_eq!((path.last().unwrap().rx, path.last().unwrap().ry), (7, 0));
        } else {
            assert!(
                path.is_err(),
                "a span must not bypass the ground hierarchy filter"
            );
        }
    }
}

#[test]
fn a_height_change_costs_the_same_as_a_flat_step() {
    // `AStar_compute_edge_cost` @ 0x00429830 has no height, level, slope or ramp
    // term, and `AStar_main_loop` @ 0x00429A90's seven reads of the cell level
    // byte +0x11B all feed the bridge/layer selection. A ramp step must
    // therefore cost exactly one step.
    //
    // Row y=1 rises to level 1 in its middle three cells; row y=0 stays flat.
    // Straight along y=1 is four steps crossing four height changes (up, level,
    // level, down); the y=0 detour is also four steps but entirely flat. Under a
    // ×4 height multiplier the straight run cost 13 steps against the detour's
    // 4, so the search took the detour.
    let build = |raised: bool| {
        let mut cells = Vec::with_capacity(10);
        for ry in 0..2u16 {
            for rx in 0..5u16 {
                let mut cell = make_resolved_cell(rx, ry);
                if raised && ry == 1 {
                    if (1..=3).contains(&rx) {
                        cell.level = 1;
                    } else {
                        // The ±1 step is legal only when the *lower* cell is a
                        // ramp, so the two cells flanking the rise carry one.
                        cell.slope_type = 1;
                    }
                }
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(5, 2, cells)
    };

    let route = |terrain: &ResolvedTerrainGrid| {
        let grid = PathGrid::from_resolved_terrain(terrain);
        find_path_over(&grid, terrain, (0, 1), (4, 1))
    };

    let flat = build(false);
    let raised = build(true);
    assert_eq!(
        raised.cell(2, 1).expect("raised cell").level,
        1,
        "precondition: the strip really is raised",
    );
    let flat_path = route(&flat).expect("flat route exists");
    assert_eq!(
        route(&raised),
        Some(flat_path),
        "a height change must not change the route",
    );
}

#[test]
fn path_grid_preserves_low_bridge_tube_metadata() {
    let mut cell = make_resolved_cell(0, 0);
    cell.land_type = LandType::Tunnel.as_index();
    cell.tube_index = Some(TubeId(7));
    let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);

    let grid = PathGrid::from_resolved_terrain(&terrain);
    let path_cell = grid.cell(0, 0).expect("path cell");
    assert_eq!(path_cell.tube_index, Some(TubeId(7)));
    assert!(path_cell.low_bridge_tube_cell);
}

#[test]
fn astar_uses_explicit_nonzero_tube_edge() {
    let mut cells: Vec<_> = (0..5).map(|x| make_resolved_cell(x, 0)).collect();
    cells[0].land_type = LandType::Tunnel.as_index();
    cells[0].tube_index = Some(TubeId(0));
    for cell in &mut cells[1..4] {
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
    }
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        5,
        1,
        cells,
        vec![TubeFact::explicit((0, 0), (4, 0), 2, vec![2, 2, 2, 2])],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let path = find_path_over(&grid, &terrain, (0, 0), (4, 0))
        .expect("explicit nonzero tube should bridge blocked intermediate cells");

    assert_eq!(path, vec![(0, 0), (4, 0)]);
}

#[test]
fn astar_rejects_zero_step_auto_shell_tube_edge() {
    let mut cells: Vec<_> = (0..5).map(|x| make_resolved_cell(x, 0)).collect();
    cells[0].land_type = LandType::Tunnel.as_index();
    cells[0].tube_index = Some(TubeId(0));
    for cell in &mut cells[1..4] {
        cell.ground_walk_blocked = true;
        cell.base_ground_walk_blocked = true;
    }
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        5,
        1,
        cells,
        vec![TubeFact::auto_low_bridge((0, 0), 2)],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let path = find_path_over(&grid, &terrain, (0, 0), (4, 0));

    assert!(
        path.is_none(),
        "zero-step auto shell must not create a visible tube route"
    );
}

#[test]
fn astar_walks_stock_low_bridge_cells_without_explicit_tubes() {
    let mut cells: Vec<_> = (0..5).map(|x| make_resolved_cell(x, 0)).collect();
    let mut tubes = Vec::new();
    for cell in &mut cells[1..4] {
        cell.land_type = LandType::Tunnel.as_index();
        cell.tube_index = Some(TubeId(tubes.len() as u16));
        tubes.push(TubeFact::auto_low_bridge((cell.rx, cell.ry), 2));
    }
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(5, 1, cells, tubes);
    assert!(terrain.tube_facts().iter().all(|tube| tube.source
        == crate::map::tube_facts::TubeSource::AutoLowBridge
        && tube.path_len() == 0));
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let path = find_path_over(&grid, &terrain, (0, 0), (4, 0))
        .expect("stock auto low-bridge shell cells should remain normal ground-walkable cells");

    assert_eq!(path, vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
}

// ---------------------------------------------------------------------------
// Code-2 (friendly-moving blocker) costs. `code2_multiplier` is the urgency
// rule of AStar_compute_edge_cost @ 0x00429830. The live chain and the
// original executable's controls are tested in
// `movement::foot_path::code2_chain_tests`.
// ---------------------------------------------------------------------------

/// `0x00429878..0x004299A6`: only urgency 0 asks the chain; urgency 2 routes
/// around and any other nonzero urgency jams.
#[test]
fn code2_urgency_selects_the_multiplier_and_only_zero_asks_the_chain() {
    for (urgency, jams, expected) in [
        (0, false, CODE2_MULT_CLEARING),
        (0, true, CODE2_MULT_JAM),
        (1, false, CODE2_MULT_JAM),
        (2, true, CODE2_MULT_ROUTE_AROUND),
        (3, false, CODE2_MULT_JAM),
    ] {
        let mut asked = false;
        let multiplier = code2_multiplier(urgency, || {
            asked = true;
            Ok::<_, ()>(jams)
        });
        assert_eq!(multiplier, Ok(expected), "urgency {urgency}");
        assert_eq!(asked, urgency == 0, "urgency {urgency}");
    }
}

// ---------------------------------------------------------------------------
// Search-scoped 0x40000 marker overlay tests
// Matches gamemd.exe temporary A* marker cost consumed at 0x00429830.
// ---------------------------------------------------------------------------

#[test]
fn search_marker_overlay_uses_xor_parity() {
    let mut overlay = SearchMarkerOverlay::new();
    assert!(overlay.is_empty());

    overlay.toggle((3, 1));
    assert!(overlay.contains((3, 1)));
    assert!(!overlay.is_empty());

    overlay.toggle((3, 1));
    assert!(!overlay.contains((3, 1)));
    assert!(overlay.is_empty());

    // Cells toggled in no particular order stay marked until toggled again,
    // and no other cell is, inside their range or outside it.
    for cell in [(5, 2), (1, 9), (5, 0), (300, 4), (1, 8), (5, 0)] {
        overlay.toggle(cell);
    }
    for cell in [(5, 2), (1, 9), (300, 4), (1, 8)] {
        assert!(overlay.contains(cell), "{cell:?}");
    }
    for cell in [(5, 0), (5, 1), (1, 7), (0, 9), (300, 5), (301, 0), (0, 0)] {
        assert!(!overlay.contains(cell), "{cell:?}");
    }
}

#[test]
fn astar_marker_overlay_penalizes_normal_compass_edges() {
    let grid = PathGrid::new(7, 3);
    let mut overlay = SearchMarkerOverlay::new();
    overlay.toggle((3, 1));

    let path = astar_search(
        &grid,
        (0, 1),
        MovementLayer::Ground,
        (6, 1),
        &AStarOptions {
            marker_overlay: Some(&overlay),
            ..AStarOptions::new(&WalkableEntry::new(&grid))
        },
    )
    .expect("marked normal-edge route should still have a detour");

    assert!(
        !path.iter().any(|step| (step.rx, step.ry) == (3, 1)),
        "A* should avoid the marked destination cell when a comparable unmarked route exists: {:?}",
        path
    );
}

#[test]
fn astar_marker_overlay_does_not_apply_to_direction8_tube_edge() {
    let mut cells = Vec::new();
    for y in 0..3 {
        for x in 0..5 {
            cells.push(make_resolved_cell(x, y));
        }
    }
    let start_idx = 5; // (0, 1)
    cells[start_idx].land_type = LandType::Tunnel.as_index();
    cells[start_idx].tube_index = Some(TubeId(0));
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
        5,
        3,
        cells,
        vec![TubeFact::explicit((0, 1), (4, 1), 2, vec![2, 2])],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let mut overlay = SearchMarkerOverlay::new();
    overlay.toggle((4, 1));
    let path = astar_search(
        &grid,
        (0, 1),
        MovementLayer::Ground,
        (4, 1),
        &AStarOptions {
            resolved_terrain: Some(&terrain),
            marker_overlay: Some(&overlay),
            ..AStarOptions::new(&WalkableEntry::new(&grid))
        },
    )
    .expect("explicit tube edge should remain usable even when the exit is marked");

    assert_eq!(
        path.iter()
            .map(|step| (step.rx, step.ry))
            .collect::<Vec<_>>(),
        vec![(0, 1), (4, 1)],
        "direction-8 tube edge should bypass normal marker cost"
    );
}

#[test]
fn astar_bridge_marker_overlay_is_search_scoped_and_does_not_mutate_pathgrid() {
    let grid = PathGrid::new(7, 3);
    let before = grid.clone();
    let mut overlay = SearchMarkerOverlay::new();
    overlay.toggle((3, 1));

    let _ = astar_search(
        &grid,
        (0, 1),
        MovementLayer::Ground,
        (6, 1),
        &AStarOptions {
            marker_overlay: Some(&overlay),
            ..AStarOptions::new(&WalkableEntry::new(&grid))
        },
    )
    .expect("search with marker overlay should still find a path");

    assert_eq!(grid.cells, before.cells);
    assert_eq!(grid.width, before.width);
    assert_eq!(grid.height, before.height);
}

fn row_level0_graph(zones: &[ZoneId]) -> ZoneLevelGraph {
    let zone_count = zones.iter().copied().max().unwrap_or(0);
    let mut graph =
        ZoneLevelGraph::new(zone_count).with_cell_zone_ids(zones.to_vec(), zones.len() as u16, 1);
    for zone in 1..=zone_count {
        graph.set_record(ZoneRecord::new(zone, 0, 0));
    }
    graph
}

#[test]
fn astar_hierarchy_rejects_unmarked_one_ring_zone_without_blocker_exception() {
    let grid = PathGrid::new(3, 1);
    let level0_zones = row_level0_graph(&[1, 2, 3]);
    let marked_zones = MarkedZones::from_iter([1, 3]);
    let blocker_counts = BlockerNeighborCounts::new(3, 1);

    let path = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (2, 0),
        &AStarOptions {
            hierarchy_gate: Some(HierarchyGate {
                level0_zones: &level0_zones,
                marked_level0: &marked_zones,
                blocker_neighbor_counts: &blocker_counts,
                level0_route: &[],
                progress: &Cell::new(RouteProgress {
                    index: 0,
                    cell: (0, 0),
                }),
            }),
            ..AStarOptions::new(&WalkableEntry::new(&grid))
        },
    );

    assert!(
        path.is_err(),
        "unmarked zone 2 should be rejected when it has no blocker-neighbor exception"
    );
}

#[test]
fn astar_hierarchy_allows_off_marker_cell_with_blocker_neighbor_count() {
    let grid = PathGrid::new(3, 1);
    let level0_zones = row_level0_graph(&[1, 2, 3]);
    let marked_zones = MarkedZones::from_iter([1, 3]);
    let mut blocker_counts = BlockerNeighborCounts::new(3, 1);
    blocker_counts.set_count(1, 0, 1);

    let path = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (2, 0),
        &AStarOptions {
            hierarchy_gate: Some(HierarchyGate {
                level0_zones: &level0_zones,
                marked_level0: &marked_zones,
                blocker_neighbor_counts: &blocker_counts,
                level0_route: &[],
                progress: &Cell::new(RouteProgress {
                    index: 0,
                    cell: (0, 0),
                }),
            }),
            ..AStarOptions::new(&WalkableEntry::new(&grid))
        },
    )
    .expect("blocker-neighbor exception should permit the one unmarked middle zone");

    assert_eq!(
        path.iter()
            .map(|step| (step.rx, step.ry))
            .collect::<Vec<_>>(),
        vec![(0, 0), (1, 0), (2, 0)]
    );
}

#[test]
fn astar_hierarchy_marker_follows_the_marked_straight_path() {
    let grid = PathGrid::new(4, 1);
    let level0_zones = row_level0_graph(&[1, 2, 3, 4]);
    let marked_zones = MarkedZones::from_iter([1, 2, 3, 4]);
    let blocker_counts = BlockerNeighborCounts::new(4, 1);

    let path = find_layered_path_marker(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (3, 0),
        Some(HierarchyGate {
            level0_zones: &level0_zones,
            marked_level0: &marked_zones,
            blocker_neighbor_counts: &blocker_counts,
            level0_route: &[],
            progress: &Cell::new(RouteProgress {
                index: 0,
                cell: (0, 0),
            }),
        }),
        None,
        None,
        MoverSearchFacts {
            urgency: 0,
            foot_entry: &WalkableEntry::new(&grid),
        },
    )
    .expect("marked straight path should succeed");

    assert_eq!(
        path.into_iter()
            .map(|step| (step.rx, step.ry))
            .collect::<Vec<_>>(),
        vec![(0, 0), (1, 0), (2, 0), (3, 0)]
    );
}

// ---------------------------------------------------------------------------
// Water / naval pathfinding tests
// ---------------------------------------------------------------------------

#[test]
fn test_cliff_cells_remain_pathgrid_blocked() {
    let terrain: ResolvedTerrainGrid = ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            make_resolved_cell(0, 0),
            ResolvedTerrainCell {
                land_type: LandType::Rock.as_index(),
                ..make_resolved_cell(1, 0)
            },
            make_resolved_cell(2, 0),
        ],
    );
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);
    assert!(grid.is_walkable(0, 0));
    assert!(!grid.is_walkable(1, 0), "Cliff cell must remain blocked");
    assert!(grid.is_walkable(2, 0));
}

#[test]
fn gsi_04_10_resolved_zero_occupation_is_walkable_path_input() {
    let terrain = ResolvedTerrainGrid::from_cells(
        1,
        1,
        vec![ResolvedTerrainCell {
            terrain_object_occupation: Some(0),
            terrain_object_blocks: false,
            ..make_resolved_cell(0, 0)
        }],
    );

    let grid = PathGrid::from_resolved_terrain(&terrain);
    assert!(
        grid.is_walkable(0, 0),
        "resolved OccupationBits=0 is a walkable PathGrid input"
    );
}

/// A terrain object with `TemperateOccupationBits` short of the full
/// sub-cell mask blocks vehicles but not infantry. 56 of 60 stock temperate
/// terrain types are `=4`, i.e. a single sub-cell bit.
fn tree_cell(rx: u16, ry: u16, bits: u8) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        terrain_object_occupation: Some(i32::from(bits)),
        terrain_object_blocks: bits != 0,
        // Resolved terrain folds the terrain object into ground_walk_blocked;
        // base_ground_walk_blocked is the value without it.
        ground_walk_blocked: bits != 0,
        base_ground_walk_blocked: false,
        ..make_resolved_cell(rx, ry)
    }
}

#[test]
fn partial_terrain_occupation_blocks_vehicles_but_not_infantry() {
    let terrain =
        ResolvedTerrainGrid::from_cells(2, 1, vec![tree_cell(0, 0, 4), make_resolved_cell(1, 0)]);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    assert!(grid.is_walkable_for_infantry(0, 0));
    assert!(!grid.is_walkable(0, 0));
    assert_eq!(grid.terrain_object_cell_bits_at(0, 0), 0x10);
}

#[test]
fn full_terrain_occupation_blocks_infantry_too() {
    let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![tree_cell(0, 0, 7)]);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    assert!(!grid.is_walkable_for_infantry(0, 0));
    assert!(!grid.is_walkable(0, 0));
    assert_eq!(grid.terrain_object_cell_bits_at(0, 0), 0x1C);
}

#[test]
fn terrain_occupation_five_and_six_leave_one_subcell_for_infantry() {
    for bits in [5u8, 6u8] {
        let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![tree_cell(0, 0, bits)]);
        let grid = PathGrid::from_resolved_terrain(&terrain);
        assert!(grid.is_walkable_for_infantry(0, 0), "bits={bits}");
        assert!(!grid.is_walkable(0, 0), "bits={bits}");
    }
}

#[test]
fn terrain_relaxation_never_reopens_a_cliff_cell() {
    let cliff_with_tree = ResolvedTerrainCell {
        land_type: LandType::Rock.as_index(),
        base_ground_walk_blocked: true,
        ..tree_cell(0, 0, 4)
    };
    let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![cliff_with_tree]);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    assert!(!grid.is_walkable_for_infantry(0, 0));
}
