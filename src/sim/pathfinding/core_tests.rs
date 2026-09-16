//! Tests for A* pathfinding and PathGrid walkability.
//!
//! Extracted from pathfinding.rs to stay under the 400-line limit.

use super::*;
use crate::map::map_file::MapCell;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, YR_CELL_LAND_TUNNEL};
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::zone_hierarchy::{ZoneLevelGraph, ZoneRecord};
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

#[test]
fn test_path_grid_new_all_walkable() {
    let grid: PathGrid = PathGrid::new(10, 10);
    assert!(grid.is_walkable(0, 0));
    assert!(grid.is_walkable(9, 9));
    assert!(grid.is_walkable(5, 5));
}

#[test]
fn test_path_grid_out_of_bounds() {
    let grid: PathGrid = PathGrid::new(10, 10);
    assert!(!grid.is_walkable(10, 0));
    assert!(!grid.is_walkable(0, 10));
    assert!(!grid.is_walkable(255, 255));
}

#[test]
fn test_path_grid_set_blocked() {
    let mut grid: PathGrid = PathGrid::new(10, 10);
    grid.set_blocked(3, 4, true);
    assert!(!grid.is_walkable(3, 4));
    // Unblock it again.
    grid.set_blocked(3, 4, false);
    assert!(grid.is_walkable(3, 4));
}

#[test]
fn astar_trace_sink_records_rejected_and_accepted_candidates() {
    let mut grid = PathGrid::test_all_passable(3, 1);
    grid.set_blocked(1, 0, true);
    let collector = AStarTraceCollector::new();
    let path = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (2, 0),
        &AStarOptions {
            trace_sink: Some(&collector),
            trace_search_id: 42,
            ..Default::default()
        },
    );

    assert!(path.is_none());
    let steps = collector.steps();
    assert!(steps.iter().any(|step| step.search_id == 42));
    assert!(
        steps
            .iter()
            .any(|step| step.rejected_reason == Some("walkability_blocked"))
    );
}

#[test]
fn test_euclidean_heuristic_cardinal() {
    // Pure cardinal: sqrt(25) * 1000 = 5000.
    let h: i32 = euclidean_heuristic(0, 0, 5, 0);
    assert_eq!(h, 5000);
}

#[test]
fn test_euclidean_heuristic_diagonal() {
    // Pure diagonal (3,3): sqrt(18) * 1000 ≈ 4242.64;
    // isqrt(18_000_000) = 4242 (4242² = 17_994_564, 4243² = 18_003_049).
    let h: i32 = euclidean_heuristic(0, 0, 3, 3);
    assert_eq!(h, 4242);
}

#[test]
fn test_euclidean_heuristic_mixed() {
    // dx=5, dy=3: sqrt(34) * 1000 ≈ 5830.95;
    // isqrt(34_000_000) = 5830 (5830² = 33_988_900, 5831² = 34_000_561).
    let h: i32 = euclidean_heuristic(0, 0, 5, 3);
    assert_eq!(h, 5830);
}

#[test]
fn test_euclidean_heuristic_zero() {
    // Heuristic at goal cell must be 0 — guarantees A* can recognize the
    // goal as the lowest-f node when popped.
    let h: i32 = euclidean_heuristic(7, 7, 7, 7);
    assert_eq!(h, 0);
}

#[test]
fn bridge_traversal_direction_minus_one_seeds_candidate_bridge_height_without_bridgehead() {
    let candidate = bridge_test_cell(2, true, false, 0);
    let grid = PathGrid::from_cells(vec![candidate], 1, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(0, 0).unwrap(),
            candidate_coord: (0, 0),
            direction: -1,
            path_height: -1,
            parent: None,
        },
    );

    assert!(result.allowed);
    assert_eq!(result.path_height, 6);
    assert!(!result.force_bridge_list);
}

#[test]
fn bridge_traversal_explicit_parent_unknown_height_requires_candidate_transition() {
    let parent = bridge_test_cell(0, true, false, 0);
    let candidate = bridge_test_cell(0, true, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: -1,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );

    assert!(!result.allowed);
    assert_eq!(result.path_height, 4);
}

/// Despite the old name, the diff here is **4**, not 0: the parent carries no
/// structural bridge, so `parent_selected` is the path height (4) and the
/// candidate is at level 0. `0x004D9D8F-0x004D9D94` allows that case — the
/// parent's own level is neither `candidate ± 4`, so `JNZ 0x004D9E5E` takes
/// the `XOR EAX,EAX` exit. The assertion below used to demand a block.
#[test]
fn bridge_traversal_allows_a_diff_four_edge_with_no_matching_parent_level() {
    let parent = bridge_test_cell(0, false, false, 0);
    let candidate = bridge_test_cell(0, false, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 4,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );

    assert!(result.allowed);
    assert!(!result.force_bridge_list);
}

#[test]
fn bridge_traversal_diff_zero_blocks_forward2_at_deck_height() {
    let parent = bridge_test_cell(0, true, true, 0);
    let forward2 = bridge_test_cell(0, true, false, 0);
    let grid = PathGrid::from_cells(vec![parent, forward2], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 4,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );

    assert!(
        !result.allowed,
        "Forward2-style structural cells lack the transition flag and must not be deck destinations"
    );
}

#[test]
fn bridge_traversal_diff_zero_allows_transition_at_deck_height() {
    let parent = bridge_test_cell(0, true, true, 0);
    let transition = bridge_test_cell(0, true, true, 0);
    let grid = PathGrid::from_cells(vec![parent, transition], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 4,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );

    assert!(
        result.allowed,
        "Stamped transition cells remain valid deck destinations"
    );
}

#[test]
fn bridge_traversal_diff_one_lower_parent_requires_parent_slope() {
    let parent = bridge_test_cell(0, false, false, 0);
    let candidate = bridge_test_cell(1, false, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let blocked = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 0,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );
    assert!(!blocked.allowed);

    let parent = bridge_test_cell(0, false, false, 1);
    let candidate = bridge_test_cell(1, false, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);
    let allowed = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 0,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );
    assert!(allowed.allowed);
}

#[test]
fn bridge_traversal_diff_one_higher_parent_requires_candidate_slope() {
    let parent = bridge_test_cell(1, false, false, 0);
    let candidate = bridge_test_cell(0, false, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let blocked = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 1,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );
    assert!(!blocked.allowed);

    let parent = bridge_test_cell(1, false, false, 0);
    let candidate = bridge_test_cell(0, false, false, 1);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);
    let allowed = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 1,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );
    assert!(allowed.allowed);
}

#[test]
fn bridge_traversal_diff_four_candidate_low_parent_high_forces_bridge_list() {
    let parent = bridge_test_cell(4, true, false, 0);
    let candidate = bridge_test_cell(0, true, true, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 4,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );

    assert!(result.allowed);
    assert!(result.force_bridge_list);
}

#[test]
fn bridge_traversal_diff_four_candidate_high_parent_low_requires_parent_structural() {
    let parent = bridge_test_cell(0, false, false, 0);
    let candidate = bridge_test_cell(4, true, true, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let blocked = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 0,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );
    assert!(!blocked.allowed);

    let parent = bridge_test_cell(0, true, false, 0);
    let candidate = bridge_test_cell(4, true, true, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);
    let allowed = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 4,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );
    assert!(allowed.allowed);
}

#[test]
fn bridge_traversal_invalid_diff_blocks() {
    let parent = bridge_test_cell(0, false, false, 0);
    let candidate = bridge_test_cell(2, false, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 0,
            parent: Some((grid.cell(0, 0).unwrap(), (0, 0))),
        },
    );

    assert!(!result.allowed);
}

#[test]
fn bridge_traversal_null_parent_valid_direction_reconstructs_predecessor() {
    let parent = bridge_test_cell(0, false, false, 1);
    let candidate = bridge_test_cell(1, false, false, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(1, 0).unwrap(),
            candidate_coord: (1, 0),
            direction: 2,
            path_height: 0,
            parent: None,
        },
    );

    assert!(result.allowed);
}

#[test]
fn can_enter_layers_keep_bridge_object_list_with_ground_occupancy_bits() {
    let candidate = bridge_test_cell(0, true, true, 0);

    let layers = can_enter_layer_context(
        MovementLayer::Bridge,
        MovementLayer::Bridge,
        &candidate,
        candidate.signed_level(),
    );

    assert_eq!(layers.terrain_layer, MovementLayer::Bridge);
    assert_eq!(layers.object_list_layer, MovementLayer::Bridge);
    assert_eq!(layers.occupancy_bits_layer, MovementLayer::Ground);
}

#[test]
fn can_enter_layers_resnapshot_bridge_occupancy_bits_at_deck_height() {
    let candidate = bridge_test_cell(0, true, true, 0);

    let layers = can_enter_layer_context(
        MovementLayer::Bridge,
        MovementLayer::Bridge,
        &candidate,
        candidate.signed_level() + 4,
    );

    assert_eq!(layers.object_list_layer, MovementLayer::Bridge);
    assert_eq!(layers.occupancy_bits_layer, MovementLayer::Bridge);
}

#[test]
fn can_enter_layers_non_bridge_cells_retain_single_ground_layer() {
    let candidate = bridge_test_cell(0, false, false, 0);

    let layers = can_enter_layer_context(
        MovementLayer::Ground,
        MovementLayer::Ground,
        &candidate,
        candidate.signed_level(),
    );

    assert_eq!(layers.terrain_layer, MovementLayer::Ground);
    assert_eq!(layers.object_list_layer, MovementLayer::Ground);
    assert_eq!(layers.occupancy_bits_layer, MovementLayer::Ground);
}

#[test]
fn test_find_path_trivial_same_cell() {
    let grid: PathGrid = PathGrid::new(10, 10);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (5, 5), (5, 5));
    assert_eq!(path, Some(vec![(5, 5)]));
}

#[test]
fn test_find_path_straight_line() {
    let grid: PathGrid = PathGrid::new(10, 10);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (0, 0), (4, 0));
    let path: Vec<(u16, u16)> = path.expect("Should find a path on open grid");
    // Path should start at (0,0) and end at (4,0).
    assert_eq!(*path.first().expect("non-empty"), (0, 0));
    assert_eq!(*path.last().expect("non-empty"), (4, 0));
    // Should be 5 cells (0,0) through (4,0).
    assert_eq!(path.len(), 5);
}

#[test]
fn test_find_path_diagonal() {
    let grid: PathGrid = PathGrid::new(10, 10);
    let path: Option<Vec<(u16, u16)>> = find_path(&grid, (0, 0), (3, 3));
    let path: Vec<(u16, u16)> = path.expect("Should find diagonal path");
    assert_eq!(*path.first().expect("non-empty"), (0, 0));
    assert_eq!(*path.last().expect("non-empty"), (3, 3));
    // Pure diagonal: exactly 4 cells.
    assert_eq!(path.len(), 4);
}

#[test]
fn test_path_step_count_chebyshev_open_grid() {
    // With uniform edge cost and a Euclidean heuristic, the optimal step
    // count from (sx,sy) to (gx,gy) on an open grid is max(|dx|, |dy|).
    // Path length includes the start cell, so it equals chebyshev + 1.
    let grid: PathGrid = PathGrid::new(20, 20);
    let cases: &[((u16, u16), (u16, u16), usize)] = &[
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
fn test_cliff_cost_detours_under_uniform_base() {
    // 3x3 grid. Direct row 0 has a height-step at (1,0): ground=4 between
    // ground=0 cells. Both edges (0,0)→(1,0) and (1,0)→(2,0) have diff=4,
    // which the height-diff legality gate hard-blocks (diff>=2 always blocks
    // in astar_search). A* must detour via row 1 (all flat).
    // (Pre-gate, the path was permitted at ~10000 g_cost vs ~4000 for the
    // alt; the alt won on cost. The new gate makes the detour the only
    // option, but the visible outcome is identical.)
    let cells = vec![
        // Row 0 (y=0): 0, [cliff height=4], 0
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 4,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        // Row 1 (y=1): all flat 0
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        // Row 2 (y=2): all flat 0 (filler so 3x3)
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 3, 3);
    let path = find_path(&grid, (0, 0), (2, 0)).expect("path should exist over flat alt route");
    // Direct path through cliff would visit (1,0). Alt route avoids it.
    assert!(
        !path.contains(&(1, 0)),
        "path should detour around cliff cell (1,0): {:?}",
        path
    );
    assert_eq!(path.first().copied(), Some((0, 0)));
    assert_eq!(path.last().copied(), Some((2, 0)));
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
    assert!(bridge_cell.is_bridge_transition_cell());
    assert!(bridge_cell.is_elevated_bridge_cell());
    assert_eq!(bridge_cell.bridge_deck_level_if_any(), Some(4));
    assert_eq!(
        bridge_cell.effective_cell_z_for_layer(MovementLayer::Bridge),
        4
    );
    assert_eq!(
        bridge_cell.effective_cell_z_for_layer(MovementLayer::Ground),
        1
    );
    assert!(bridge_cell.can_enter_bridge_layer_from_ground());

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
    assert!(!low_bridge.is_bridge_transition_cell());
    assert!(!low_bridge.is_elevated_bridge_cell());
    assert_eq!(low_bridge.bridge_deck_level_if_any(), Some(2));
    assert_eq!(
        low_bridge.effective_cell_z_for_layer(MovementLayer::Bridge),
        2
    );
    assert!(!low_bridge.can_enter_bridge_layer_from_ground());
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
        &AStarOptions::default(),
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
fn test_path_grid_dimensions() {
    let grid: PathGrid = PathGrid::new(80, 70);
    assert_eq!(grid.width(), 80);
    assert_eq!(grid.height(), 70);
}

#[test]
fn test_from_map_data_marks_terrain_walkable() {
    let cells: Vec<MapCell> = vec![
        MapCell {
            rx: 2,
            ry: 3,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        },
        MapCell {
            rx: 4,
            ry: 5,
            tile_index: 1,
            sub_tile: 0,
            z: 0,
        },
    ];
    let grid: PathGrid = PathGrid::from_map_data(&cells, None, 10, 10);
    // Cells with terrain should be walkable.
    assert!(grid.is_walkable(2, 3));
    assert!(grid.is_walkable(4, 5));
    // Cells without terrain should be blocked (all start blocked).
    assert!(!grid.is_walkable(0, 0));
    assert!(!grid.is_walkable(9, 9));
}

#[test]
fn test_from_map_data_skips_no_tile() {
    let cells: Vec<MapCell> = vec![
        MapCell {
            rx: 1,
            ry: 1,
            tile_index: -1,
            sub_tile: 0,
            z: 0,
        },
        MapCell {
            rx: 2,
            ry: 2,
            tile_index: 5,
            sub_tile: 0,
            z: 0,
        },
    ];
    let grid: PathGrid = PathGrid::from_map_data(&cells, None, 10, 10);
    assert!(!grid.is_walkable(1, 1), "No-tile cells should be blocked");
    assert!(
        grid.is_walkable(2, 2),
        "Valid tile cells should be walkable"
    );
}

#[test]
fn test_block_building_footprint() {
    let cells: Vec<MapCell> = (0..10u16)
        .flat_map(|rx| {
            (0..10u16).map(move |ry| MapCell {
                rx,
                ry,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            })
        })
        .collect();
    let mut grid: PathGrid = PathGrid::from_map_data(&cells, None, 10, 10);
    // All cells should be walkable initially.
    assert!(grid.is_walkable(3, 3));
    assert!(grid.is_walkable(4, 4));
    // Block a 2x2 building at (3, 3).
    grid.block_building_footprint(3, 3, "2x2", &[], &[], false);
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
    let cells: Vec<MapCell> = (0..32u16)
        .flat_map(|rx| {
            (0..32u16).map(move |ry| MapCell {
                rx,
                ry,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            })
        })
        .collect();
    let mut grid: PathGrid = PathGrid::from_map_data(&cells, None, 32, 32);
    grid.block_building_movement_cells(10, 10, "4x3", false);
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
    let cells: Vec<MapCell> = (0..32u16)
        .flat_map(|rx| {
            (0..32u16).map(move |ry| MapCell {
                rx,
                ry,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            })
        })
        .collect();
    let mut grid: PathGrid = PathGrid::from_map_data(&cells, None, 32, 32);
    grid.block_building_movement_cells(10, 10, "4x3", true);
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
                is_water: true,
                ground_walk_blocked: true,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                terrain_object_blocks: true,
                terrain_object_occupation: Some(1),
                build_blocked: true,
                ..make_resolved_cell(0, 1)
            },
            ResolvedTerrainCell {
                overlay_blocks: true,
                overlay_zone_type: Some(crate::map::resolved_terrain::zone_class::IMPASSABLE),
                build_blocked: true,
                ..make_resolved_cell(1, 1)
            },
        ],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    assert!(grid.is_walkable(0, 0), "Clear land is walkable");
    // Water cells are PathGrid-walkable — SpeedType-dependent blocking
    // is handled by TerrainCostGrid (cost=0 blocks ground units in A*).
    assert!(grid.is_walkable(1, 0), "Water is PathGrid-walkable");
    assert!(!grid.is_walkable(0, 1), "Terrain object blocks all units");
    assert!(!grid.is_walkable(1, 1), "Overlay blocks all units");
}

#[test]
fn test_parse_foundation() {
    assert_eq!(parse_foundation("2x2"), (2, 2));
    assert_eq!(parse_foundation("3x3"), (3, 3));
    assert_eq!(parse_foundation("1x1"), (1, 1));
    assert_eq!(parse_foundation("4x2"), (4, 2));
    assert_eq!(parse_foundation(""), (1, 1));
    assert_eq!(parse_foundation("custom"), (1, 1));
}

#[test]
fn test_layered_path_transitions_onto_bridge_and_stays_on_deck() {
    // Height-based bridge routing requires deck_level - ground_level >= 2
    // for the pathfinder to recognize bridge cells as "at bridge level".
    // Use realistic heights: ground=0, deck=4.
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
                has_bridge_deck: true,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                ground_walk_blocked: true,
                build_blocked: true,
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                is_water: true,
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
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let path = find_layered_path(
        &grid,
        None,
        None,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        None,
        None,
        None,
        0,
        false,
        false,
    )
    .expect("bridge path should exist");

    assert_eq!(path.first().map(|step| (step.rx, step.ry)), Some((0, 0)));
    assert_eq!(path.last().map(|step| (step.rx, step.ry)), Some((4, 0)));
    assert!(path.len() >= 2, "path should have at least start and goal");
}

#[test]
fn test_layered_path_stays_on_ground_when_bridge_not_needed() {
    let terrain = ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            make_resolved_cell(0, 0),
            make_resolved_cell(1, 0),
            make_resolved_cell(2, 0),
        ],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let path = find_layered_path(
        &grid,
        None,
        None,
        (0, 0),
        MovementLayer::Ground,
        (2, 0),
        None,
        None,
        None,
        0,
        false,
        false,
    )
    .expect("ground path should exist");
    assert!(path.iter().all(|step| step.layer == MovementLayer::Ground));
}

#[test]
fn test_layered_path_rebuild_blocks_destroyed_bridge_deck() {
    // Height-based routing needs deck - ground >= 2 to recognize bridge level.
    // Use 5 cells: land(h=4) → transition → water+bridge → transition → land(h=4)
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
                build_blocked: true,
                base_build_blocked: true,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                is_water: true,
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
    let mut bridge_state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 10);
    let intact_grid = PathGrid::from_resolved_terrain_with_bridges(&terrain, Some(&bridge_state));
    assert!(
        find_layered_path(
            &intact_grid,
            None,
            None,
            (0, 0),
            MovementLayer::Ground,
            (4, 0),
            None,
            None,
            None,
            0,
            false,
            false,
        )
        .is_some(),
        "intact bridge should be traversable"
    );

    // Destroy only the body cell (rx=2). The bridgeheads at rx=1 / rx=3 must
    // stay Healthy — pass 4 of `from_resolved_terrain` creates them with
    // `damage_state = Healthy { variant: 0 }` permanently, and the contract
    // for `BridgeCellRole::Bridgehead` is that no code mutates that field.
    if let Some(c) = bridge_state.cell_mut(2, 0) {
        c.damage_state = crate::sim::bridge_state::DamageState::Destroyed;
    }

    let destroyed_grid =
        PathGrid::from_resolved_terrain_with_bridges(&terrain, Some(&bridge_state));
    assert!(
        find_layered_path(
            &destroyed_grid,
            None,
            None,
            (0, 0),
            MovementLayer::Ground,
            (4, 0),
            None,
            None,
            None,
            0,
            false,
            false,
        )
        .is_none(),
        "destroyed bridge should invalidate the layered route"
    );
}

#[test]
fn test_pathcell_bridge_walkable_preserved_for_bridgeheads_across_rebuild() {
    // After the G7 fix, PathCell.bridge_walkable for pass-4 bridgeheads
    // (rx=1, rx=3) must stay true across every PathGrid rebuild driven by
    // from_resolved_terrain_with_bridges. Pre-fix this would flip to false
    // because the bridgeheads aren't registered in BridgeRuntimeState.
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
                build_blocked: true,
                base_build_blocked: true,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                bridge_walkable: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                is_water: true,
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
    let bridge_state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 10);

    // Multiple rebuilds: in production each fires from a non-bridge event
    // (unit spawn, ownership change, destroyed structure).
    for _ in 0..3 {
        let grid = PathGrid::from_resolved_terrain_with_bridges(&terrain, Some(&bridge_state));
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

// --- Bridge height helper tests ---

#[test]
fn test_is_at_bridge_level_no_bridge() {
    let cell = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Non-bridge cell is never "at bridge level"
    assert!(!is_at_bridge_level(0, &cell));
    assert!(!is_at_bridge_level(4, &cell));
}

#[test]
fn test_is_at_bridge_level_ground_near() {
    let cell = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // path_height=0, ground=0 -> diff=0 < 2 -> ground list
    assert!(!is_at_bridge_level(0, &cell));
    // path_height=1, ground=0 -> diff=1 < 2 -> ground list
    assert!(!is_at_bridge_level(1, &cell));
}

#[test]
fn test_is_at_bridge_level_bridge_far() {
    let cell = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // path_height=4, ground=0 -> diff=4 >= 2 -> bridge list
    assert!(is_at_bridge_level(4, &cell));
    // path_height=2, ground=0 -> diff=2 >= 2 -> bridge list
    assert!(is_at_bridge_level(2, &cell));
}

#[test]
fn test_compute_neighbor_height_no_bridge() {
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 2,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 3,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Case 1: neighbor not bridge -> ground_level
    assert_eq!(compute_neighbor_height(2, &parent, &neighbor), 3);
}

#[test]
fn test_compute_neighbor_height_parent_on_bridge_deck() {
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Case 2a: parent on bridge at deck level -> stay on bridge
    assert_eq!(compute_neighbor_height(4, &parent, &neighbor), 4);
}

#[test]
fn test_compute_neighbor_height_parent_under_bridge() {
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Case 2b: parent on bridge cell but at ground level -> stay under
    assert_eq!(compute_neighbor_height(0, &parent, &neighbor), 0);
}

#[test]
fn test_compute_neighbor_height_ramp_up() {
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 4,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: true,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Case 3: parent not bridge, neighbor is bridge,
    // diff = 4 - 0 = 4, in [2,4] -> ramp up to bridge deck
    assert_eq!(compute_neighbor_height(4, &parent, &neighbor), 4);

    // `AStar_create_node` @ 0x0042A460 accepts a drop of 2 or 3 as well —
    // `abs((neighbor.Level - parent_height) + 3) <= 1` — and reads no
    // bridgehead flag. Both cases used to carry the ground level instead.
    assert_eq!(
        compute_neighbor_height(2, &parent, &neighbor),
        4,
        "drop of 2"
    );
    assert_eq!(
        compute_neighbor_height(3, &parent, &neighbor),
        4,
        "drop of 3"
    );

    let unflagged = PathCell {
        transition: false,
        ..neighbor
    };
    assert_eq!(
        compute_neighbor_height(4, &parent, &unflagged),
        4,
        "a deck cell with no bridgehead flag still promotes",
    );

    // Drops outside 2..=4 stay on the ground plane.
    assert_eq!(
        compute_neighbor_height(1, &parent, &neighbor),
        0,
        "drop of 1"
    );
    assert_eq!(
        compute_neighbor_height(5, &parent, &neighbor),
        0,
        "drop of 5"
    );
}

#[test]
fn bridge_traversal_allows_a_null_parent() {
    // 0x004D9CC5 `TEST ESI,ESI; JZ 0x004D9E5E` reaches `XOR EAX,EAX`, so an
    // unresolvable predecessor — the outer map ring — is allowed, not refused.
    let candidate = bridge_test_cell(0, false, false, 0);
    let grid = PathGrid::from_cells(vec![candidate], 1, 1);

    let result = check_bridge_traversal(
        &grid,
        BridgeTraversalInput {
            candidate: grid.cell(0, 0).unwrap(),
            candidate_coord: (0, 0),
            // West from column 0: the predecessor is off-map.
            direction: 2,
            path_height: 0,
            parent: None,
        },
    );

    assert!(result.allowed);
    assert!(!result.force_bridge_list);
}

#[test]
fn test_compute_neighbor_height_pass_under() {
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Case 3: parent not bridge, neighbor is bridge,
    // diff = 0 - 0 = 0, NOT in [2,4] -> pass under
    assert_eq!(compute_neighbor_height(0, &parent, &neighbor), 0);
}

#[test]
fn test_compute_neighbor_height_extreme_diff_no_ramp() {
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 8,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: true,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    // Case 3: diff = 8 - 0 = 8, NOT in [2,4] -> stays at ground (no ramp)
    assert_eq!(compute_neighbor_height(8, &parent, &neighbor), 0);
}

#[test]
fn test_encode_decode_from_ground() {
    let encoded = encode_from(1234, false);
    let (idx, bridge) = decode_from(encoded);
    assert_eq!(idx, 1234);
    assert!(!bridge);
}

#[test]
fn test_encode_decode_from_bridge() {
    let encoded = encode_from(1234, true);
    let (idx, bridge) = decode_from(encoded);
    assert_eq!(idx, 1234);
    assert!(bridge);
}

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
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        height_in_pixels: 0,
        variant: 0,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
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
fn infantry_under_span_admission_reads_ground_speed_with_deck_cost_grid() {
    use crate::sim::pathfinding::cell_entry::{
        CanEnterCellContext, TerrainEntryMode, evaluate_can_enter_cell,
    };

    for transition in [false, true] {
        for foot_cost in [0, 100] {
            let terrain = ResolvedTerrainGrid::from_cells(
                1,
                1,
                vec![infantry_under_span_cell(0, foot_cost, transition)],
            );
            let grid = PathGrid::from_cells(vec![bridge_test_cell(2, true, transition, 0)], 1, 1);
            let costs = TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Foot);
            assert_eq!(costs.cost_at(0, 0), 100, "coarse grid describes the deck");
            for mode in [
                TerrainEntryMode::AStarNeighbor,
                TerrainEntryMode::RuntimeTransition,
            ] {
                for speed_type in [None, Some(SpeedType::Foot)] {
                    assert_eq!(
                        evaluate_can_enter_cell(CanEnterCellContext {
                            wall: None,
                            target: (0, 0),
                            terrain_layer: MovementLayer::Ground,
                            movement_zone: Some(MovementZone::Infantry),
                            speed_type,
                            path_grid: Some(&grid),
                            resolved_terrain: Some(&terrain),
                            terrain_costs: Some(&costs),
                            bypass_grid: false,
                            mode,
                            is_infantry: true,
                            mover_is_crusher: false,
                        })
                        .is_clear(),
                        foot_cost != 0,
                        "transition={transition}, mode={mode:?}, speed={speed_type:?}"
                    );
                }
            }
        }
    }
}

/// `UnitClass::Can_Enter_Cell` @ `0x0073F0A0` admits the ground plane beneath a
/// span exactly as Infantry does: the deck flag clears for a path height within
/// one of the cell level (`0x0073F0B7..F0E8`), the ground list is walked and the
/// terrain's own land row is read at `0x0073FAB5`. It never reaches the
/// `CheckCellPassability 0x004834A0` level rejection. Track over land is
/// admitted, Track over water is refused, and an amphibious hover mover swims.
#[test]
fn unit_under_span_admission_reads_ground_row_beneath_deck() {
    use crate::sim::pathfinding::cell_entry::{
        CanEnterCellContext, TerrainEntryMode, evaluate_can_enter_cell,
    };

    let cases: [(MovementZone, SpeedType, SpeedCostProfile, bool); 4] = [
        (
            MovementZone::Normal,
            SpeedType::Track,
            SpeedCostProfile {
                track: Some(100),
                ..SpeedCostProfile::default()
            },
            true,
        ),
        (
            MovementZone::Normal,
            SpeedType::Track,
            SpeedCostProfile {
                track: Some(0),
                ..SpeedCostProfile::default()
            },
            false,
        ),
        (
            MovementZone::AmphibiousDestroyer,
            SpeedType::Hover,
            SpeedCostProfile {
                track: Some(0),
                hover: Some(100),
                amphibious: Some(100),
                ..SpeedCostProfile::default()
            },
            true,
        ),
        (
            MovementZone::Crusher,
            SpeedType::Track,
            SpeedCostProfile {
                track: Some(0),
                hover: Some(100),
                ..SpeedCostProfile::default()
            },
            false,
        ),
    ];
    for (zone, planner_speed, speed_costs, expected) in cases {
        for transition in [false, true] {
            let mut cell = infantry_under_span_cell(0, 100, transition);
            cell.speed_costs = speed_costs;
            let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);
            let grid = PathGrid::from_cells(vec![bridge_test_cell(2, true, transition, 0)], 1, 1);
            let costs = TerrainCostGrid::from_resolved_terrain(&terrain, planner_speed);
            assert_eq!(costs.cost_at(0, 0), 100, "coarse grid describes the deck");
            for mode in [
                TerrainEntryMode::AStarNeighbor,
                TerrainEntryMode::RuntimeTransition,
            ] {
                assert_eq!(
                    evaluate_can_enter_cell(CanEnterCellContext {
                        wall: None,
                        target: (0, 0),
                        terrain_layer: MovementLayer::Ground,
                        movement_zone: Some(zone),
                        speed_type: None,
                        path_grid: Some(&grid),
                        resolved_terrain: Some(&terrain),
                        terrain_costs: Some(&costs),
                        bypass_grid: false,
                        mode,
                        is_infantry: false,
                        mover_is_crusher: false,
                    })
                    .is_clear(),
                    expected,
                    "zone={zone:?}, transition={transition}, mode={mode:?}"
                );
            }
            // The deck itself stays open to the same mover regardless of the
            // row beneath it.
            assert!(
                evaluate_can_enter_cell(CanEnterCellContext {
                    wall: None,
                    target: (0, 0),
                    terrain_layer: MovementLayer::Bridge,
                    movement_zone: Some(zone),
                    speed_type: None,
                    path_grid: Some(&grid),
                    resolved_terrain: Some(&terrain),
                    terrain_costs: Some(&costs),
                    bypass_grid: false,
                    mode: TerrainEntryMode::AStarNeighbor,
                    is_infantry: false,
                    mover_is_crusher: false,
                })
                .is_clear(),
                "deck entry for zone={zone:?}, transition={transition}"
            );
        }
    }
}

#[test]
fn infantry_under_span_admission_preserves_wall_and_grid_blocks() {
    use crate::sim::pathfinding::cell_entry::{
        CanEnterCellContext, TerrainEntryMode, evaluate_can_enter_cell,
    };

    for wall in [false, true] {
        let mut cell = infantry_under_span_cell(0, 100, false);
        if wall {
            cell.zone_type = crate::map::resolved_terrain::zone_class::WALL;
        }
        let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);
        let mut grid = PathGrid::from_cells(vec![bridge_test_cell(2, true, false, 0)], 1, 1);
        if !wall {
            grid.set_blocked(0, 0, true);
        }
        let costs = TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Foot);
        for mode in [
            TerrainEntryMode::AStarNeighbor,
            TerrainEntryMode::RuntimeTransition,
        ] {
            assert!(
                !evaluate_can_enter_cell(CanEnterCellContext {
                    wall: None,
                    target: (0, 0),
                    terrain_layer: MovementLayer::Ground,
                    movement_zone: Some(MovementZone::Infantry),
                    speed_type: Some(SpeedType::Foot),
                    path_grid: Some(&grid),
                    resolved_terrain: Some(&terrain),
                    terrain_costs: Some(&costs),
                    bypass_grid: false,
                    mode,
                    is_infantry: true,
                    mover_is_crusher: false,
                })
                .is_clear(),
                "wall={wall}, mode={mode:?}"
            );
        }
    }
}

#[test]
fn infantry_under_span_admission_preserves_missing_target_rejection() {
    use crate::sim::pathfinding::cell_entry::{
        CanEnterCellContext, TerrainEntryMode, evaluate_can_enter_cell,
    };

    let grid = PathGrid::new(1, 1);
    for path_grid in [None, Some(&grid)] {
        for mode in [
            TerrainEntryMode::AStarNeighbor,
            TerrainEntryMode::RuntimeTransition,
        ] {
            assert!(
                !evaluate_can_enter_cell(CanEnterCellContext {
                    wall: None,
                    target: (1, 0),
                    terrain_layer: MovementLayer::Ground,
                    movement_zone: Some(MovementZone::Infantry),
                    speed_type: Some(SpeedType::Foot),
                    path_grid,
                    resolved_terrain: None,
                    terrain_costs: None,
                    bypass_grid: true,
                    mode,
                    is_infantry: true,
                    mover_is_crusher: false,
                })
                .is_clear(),
                "bypassing grid blockers must not admit an absent target"
            );
        }
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
    let costs = TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Foot);
    let zones = ZoneLevelGraph::new(2).with_cell_zone_ids(vec![1, 1, 2, 2, 2, 2, 1, 1], 8, 1);
    let counts = BlockerNeighborCounts::new(8, 1);
    for include_under_span in [false, true] {
        let marked = if include_under_span {
            std::collections::BTreeSet::from([1, 2])
        } else {
            std::collections::BTreeSet::from([1])
        };
        let path = astar_search(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (7, 0),
            &AStarOptions {
                terrain_costs: Some(&costs),
                resolved_terrain: Some(&terrain),
                movement_zone: Some(MovementZone::Infantry),
                is_infantry: true,
                hierarchy_gate: Some(HierarchyGate {
                    level0_zones: &zones,
                    marked_level0: &marked,
                    blocker_neighbor_counts: &counts,
                }),
                ..Default::default()
            },
        );
        if include_under_span {
            let path = path.expect("marked ground corridor must cross all four span lanes");
            assert_eq!(path.len(), 8);
            assert!(path.iter().all(|step| step.layer == MovementLayer::Ground));
            assert_eq!((path.last().unwrap().rx, path.last().unwrap().ry), (7, 0));
        } else {
            assert!(
                path.is_none(),
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
        find_path_with_costs(
            &grid,
            (0, 1),
            (4, 1),
            None,
            None,
            None,
            Some(terrain),
            None,
            0,
            false,
            false,
        )
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
    cell.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
    cell.tube_index = Some(TubeId(7));
    let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![cell]);

    let grid = PathGrid::from_resolved_terrain(&terrain);
    let path_cell = grid.cell(0, 0).expect("path cell");
    assert_eq!(path_cell.tube_index, Some(TubeId(7)));
    assert!(path_cell.is_low_bridge_tube_cell());
}

#[test]
fn astar_uses_explicit_nonzero_tube_edge() {
    let mut cells: Vec<_> = (0..5).map(|x| make_resolved_cell(x, 0)).collect();
    cells[0].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
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

    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (4, 0),
        None,
        None,
        None,
        Some(&terrain),
        None,
        0,
        false,
        false,
    )
    .expect("explicit nonzero tube should bridge blocked intermediate cells");

    assert_eq!(path, vec![(0, 0), (4, 0)]);
}

#[test]
fn astar_rejects_zero_step_auto_shell_tube_edge() {
    let mut cells: Vec<_> = (0..5).map(|x| make_resolved_cell(x, 0)).collect();
    cells[0].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
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

    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (4, 0),
        None,
        None,
        None,
        Some(&terrain),
        None,
        0,
        false,
        false,
    );

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
        cell.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        cell.tube_index = Some(TubeId(tubes.len() as u16));
        tubes.push(TubeFact::auto_low_bridge((cell.rx, cell.ry), 2));
    }
    let terrain = ResolvedTerrainGrid::from_cells_with_tubes(5, 1, cells, tubes);
    assert!(terrain.tube_facts().iter().all(|tube| tube.source
        == crate::map::tube_facts::TubeSource::AutoLowBridge
        && tube.path_len() == 0));
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (4, 0),
        None,
        None,
        None,
        Some(&terrain),
        None,
        0,
        false,
        false,
    )
    .expect("stock auto low-bridge shell cells should remain normal ground-walkable cells");

    assert_eq!(path, vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
}

// ---------------------------------------------------------------------------
// Entity-block (friendly-passable) pathfinding tests
// ---------------------------------------------------------------------------

#[test]
fn test_entity_blocks_routes_around_blocked_cell() {
    let grid = PathGrid::new(10, 10);
    let mut blocks: BTreeSet<(u16, u16)> = BTreeSet::new();
    blocks.insert((3, 0)); // Block cell (3,0) — directly on the straight path.

    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (5, 0),
        None,
        Some(&blocks),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some(), "Should find a path around the entity block");
    let path = path.unwrap();
    assert!(
        !path.contains(&(3, 0)),
        "Path should not go through entity-blocked cell"
    );
    assert_eq!(path.last(), Some(&(5, 0)), "Path should reach goal");
}

#[test]
fn test_entity_blocks_goal_cell_still_reachable() {
    // Goal cell is in entity_blocks — path should still reach it.
    let grid = PathGrid::new(10, 10);
    let mut blocks: BTreeSet<(u16, u16)> = BTreeSet::new();
    blocks.insert((5, 0)); // Block the GOAL cell.

    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (5, 0),
        None,
        Some(&blocks),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(
        path.is_some(),
        "Goal cell should always be reachable even if entity-blocked"
    );
    assert_eq!(path.unwrap().last(), Some(&(5, 0)));
}

#[test]
fn test_entity_blocks_empty_set_same_as_none() {
    let grid = PathGrid::new(10, 10);
    let empty: BTreeSet<(u16, u16)> = BTreeSet::new();

    let path_none = find_path_with_costs(
        &grid,
        (0, 0),
        (5, 5),
        None,
        None,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    let path_empty = find_path_with_costs(
        &grid,
        (0, 0),
        (5, 5),
        None,
        Some(&empty),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert_eq!(path_none, path_empty);
}

#[test]
fn test_entity_blocks_fully_surrounded_no_path() {
    let grid = PathGrid::new(10, 10);
    let mut blocks: BTreeSet<(u16, u16)> = BTreeSet::new();
    // Surround (5,5) on all 8 sides.
    for dx in -1..=1i32 {
        for dy in -1..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            blocks.insert(((5i32 + dx) as u16, (5i32 + dy) as u16));
        }
    }
    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (5, 5),
        None,
        Some(&blocks),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    // Goal itself is reachable, but all approaches blocked → no path.
    assert!(
        path.is_none(),
        "Should not find path when all approaches to goal are entity-blocked"
    );
}

// ---------------------------------------------------------------------------
// Code-2 (friendly-moving blocker) dynamic cost tests
// Matches gamemd.exe AStar_compute_edge_cost @ 0x00429830.
// ---------------------------------------------------------------------------

#[test]
fn code2_urgency_2_routes_around_blocker() {
    // Urgency=2 → 1000x multiplier, so A* should detour one cell off the direct row.
    let grid = PathGrid::new(10, 3);
    let mut ebm = LayeredEntityBlockMap::new();
    // Blocker sitting at (3,1) with next cell (4,1).
    ebm.insert(
        MovementLayer::Ground,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    let path = find_path_with_costs(
        &grid,
        (0, 1),
        (6, 1),
        None,
        None,
        None,
        None,
        Some(&ebm),
        2,
        false,
        false,
    )
    .expect("urgency=2 should still find a path");
    assert!(
        !path.contains(&(3, 1)),
        "urgency=2 should route around the blocker cell at (3,1): {:?}",
        path
    );
    assert_eq!(path.last(), Some(&(6, 1)));
}

#[test]
fn code2_urgency_1_picks_alt_when_available() {
    // Urgency=1 → 4x multiplier. With a parallel alt row of equal terrain
    // cost, A* should prefer the alt row over paying 4x on the blocker cell.
    let grid = PathGrid::new(10, 3);
    let mut ebm = LayeredEntityBlockMap::new();
    ebm.insert(
        MovementLayer::Ground,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    let path = find_path_with_costs(
        &grid,
        (0, 1),
        (6, 1),
        None,
        None,
        None,
        None,
        Some(&ebm),
        1,
        false,
        false,
    )
    .expect("urgency=1 should find a path");
    assert!(
        !path.contains(&(3, 1)),
        "urgency=1 with a viable alt row should detour: {:?}",
        path
    );
}

#[test]
fn code2_urgency_0_chain_clears_uses_baseline() {
    // Three-blocker chain whose final next cell is empty. Urgency=0 chain walk
    // should detect the cleared tail within 10 hops and return the baseline
    // cost (1x), so the path may run straight through the blocker cell.
    let grid = PathGrid::new(10, 3);
    let mut ebm = LayeredEntityBlockMap::new();
    // Chain: (3,1)→(4,1), (4,1)→(5,1), (5,1)→(6,1). (6,1) has no blocker.
    ebm.insert(
        MovementLayer::Ground,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    ebm.insert(
        MovementLayer::Ground,
        (4, 1),
        EntityBlockEntry {
            next_cell: Some((5, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    ebm.insert(
        MovementLayer::Ground,
        (5, 1),
        EntityBlockEntry {
            next_cell: Some((6, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    // Block the alt rows so the only route is through the chain.
    let mut hard: BTreeSet<(u16, u16)> = BTreeSet::new();
    for x in 0..10u16 {
        hard.insert((x, 0));
        hard.insert((x, 2));
    }
    let path = find_path_with_costs(
        &grid,
        (0, 1),
        (6, 1),
        None,
        Some(&hard),
        None,
        None,
        Some(&ebm),
        0,
        false,
        false,
    )
    .expect("urgency=0 clearing chain should not block routing");
    assert!(path.contains(&(3, 1)));
    assert_eq!(path.last(), Some(&(6, 1)));
}

#[test]
fn code2_urgency_0_ten_step_jam_uses_4x() {
    // A 10-step chain that never clears within 10 hops. compute_code2_multiplier
    // should return 4x (jam). The path should still go through if no alt,
    // but we verify the multiplier doesn't explode to 1000.
    let grid = PathGrid::new(12, 3);
    let mut ebm = LayeredEntityBlockMap::new();
    // 11-link chain (>10 hops): (0,1)→(1,1)→...→(10,1). Each link maps to the
    // next cell and the chain runs 11 deep — chain walk exhausts at 10.
    for x in 0..11u16 {
        ebm.insert(
            MovementLayer::Ground,
            (x, 1),
            EntityBlockEntry {
                next_cell: Some((x + 1, 1)),
                cost_code: 2,
                blocker_is_infantry: false,
            },
        );
    }
    // Hard-block both alt rows to force the path through the jam.
    let mut hard: BTreeSet<(u16, u16)> = BTreeSet::new();
    for x in 0..12u16 {
        hard.insert((x, 0));
        hard.insert((x, 2));
    }
    let path = find_path_with_costs(
        &grid,
        (0, 1),
        (11, 1),
        None,
        Some(&hard),
        None,
        None,
        Some(&ebm),
        0,
        false,
        false,
    )
    .expect("jam path should still exist — 4x penalty doesn't make it infeasible");
    // Path does go through the chain cells.
    assert!(path.contains(&(5, 1)));
    assert_eq!(path.last(), Some(&(11, 1)));
}

#[test]
fn code2_goal_cell_exempt_from_multiplier() {
    // A blocker at the goal cell must not increase cost — the mover is
    // entitled to reach its goal regardless.
    let grid = PathGrid::new(10, 3);
    let mut ebm = LayeredEntityBlockMap::new();
    ebm.insert(
        MovementLayer::Ground,
        (5, 1),
        EntityBlockEntry {
            next_cell: Some((6, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    let path = find_path_with_costs(
        &grid,
        (0, 1),
        (5, 1),
        None,
        None,
        None,
        None,
        Some(&ebm),
        2,
        false,
        false,
    )
    .expect("goal cell should be reachable even with urgency=2 blocker on it");
    assert_eq!(path.last(), Some(&(5, 1)));
}

#[test]
fn soft_blocker_cost_uses_selected_ground_object_list_layer() {
    let grid = PathGrid::new(7, 3);
    let mut ebm = LayeredEntityBlockMap::new();
    ebm.insert(
        MovementLayer::Bridge,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );

    let bridge_only_path = find_path_with_costs(
        &grid,
        (0, 1),
        (6, 1),
        None,
        None,
        None,
        None,
        Some(&ebm),
        2,
        false,
        false,
    )
    .expect("bridge-layer soft blocker must not affect ground path");
    assert!(
        bridge_only_path.contains(&(3, 1)),
        "ground A* should ignore bridge object-list blocker at the same cell: {:?}",
        bridge_only_path
    );

    ebm.insert(
        MovementLayer::Ground,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    let ground_path = find_path_with_costs(
        &grid,
        (0, 1),
        (6, 1),
        None,
        None,
        None,
        None,
        Some(&ebm),
        2,
        false,
        false,
    )
    .expect("ground path should still exist by detouring");
    assert!(
        !ground_path.contains(&(3, 1)),
        "ground A* should apply ground object-list blocker cost: {:?}",
        ground_path
    );
}

#[test]
fn soft_blocker_cost_uses_selected_bridge_object_list_layer() {
    let mut grid = PathGrid::new(7, 3);
    for y in 0..3 {
        for x in 0..7 {
            grid.set_cell_for_test(x, y, 0, true, true);
        }
    }

    let mut ebm = LayeredEntityBlockMap::new();
    ebm.insert(
        MovementLayer::Ground,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    let ground_only_path = find_layered_path(
        &grid,
        None,
        None,
        (0, 1),
        MovementLayer::Bridge,
        (6, 1),
        None,
        None,
        Some(&ebm),
        2,
        false,
        false,
    )
    .expect("ground-layer soft blocker must not affect bridge path");
    assert!(
        ground_only_path
            .iter()
            .any(|step| (step.rx, step.ry) == (3, 1)),
        "bridge A* should ignore ground object-list blocker at the same cell: {:?}",
        ground_only_path
    );

    ebm.insert(
        MovementLayer::Bridge,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    let bridge_path = find_layered_path(
        &grid,
        None,
        None,
        (0, 1),
        MovementLayer::Bridge,
        (6, 1),
        None,
        None,
        Some(&ebm),
        2,
        false,
        false,
    )
    .expect("bridge path should still exist by detouring");
    assert!(
        !bridge_path.iter().any(|step| (step.rx, step.ry) == (3, 1)),
        "bridge A* should apply bridge object-list blocker cost: {:?}",
        bridge_path
    );
}

#[test]
fn code2_chain_lookup_stays_on_selected_layer() {
    let mut ebm = LayeredEntityBlockMap::new();
    ebm.insert(
        MovementLayer::Bridge,
        (3, 1),
        EntityBlockEntry {
            next_cell: Some((4, 1)),
            cost_code: 2,
            blocker_is_infantry: false,
        },
    );
    for x in 4..15u16 {
        ebm.insert(
            MovementLayer::Ground,
            (x, 1),
            EntityBlockEntry {
                next_cell: Some((x + 1, 1)),
                cost_code: 2,
                blocker_is_infantry: false,
            },
        );
    }

    assert_eq!(
        compute_code2_multiplier(0, (3, 1), MovementLayer::Bridge, &ebm),
        CODE2_MULT_CLEARING,
        "bridge chain must not continue through ground-layer blockers"
    );
    assert_eq!(
        compute_code2_multiplier(0, (4, 1), MovementLayer::Ground, &ebm),
        CODE2_MULT_JAM,
        "ground-only chain behavior remains unchanged"
    );
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
}

#[test]
fn astar_edge_cost_marker_stacks_after_code2_before_tiebreak() {
    let mut overlay = SearchMarkerOverlay::new();
    overlay.toggle((3, 1));

    let code2_cost = STEP_COST * CODE2_MULT_JAM;
    let marked_cost = apply_search_marker_cost(code2_cost, Some(&overlay), (3, 1));
    assert_eq!(
        marked_cost,
        STEP_COST * CODE2_MULT_JAM * SEARCH_MARKER_COST_MULTIPLIER
    );

    let tentative = marked_cost + DIR_TIEBREAK[2];
    assert_eq!(
        tentative,
        STEP_COST * CODE2_MULT_JAM * SEARCH_MARKER_COST_MULTIPLIER + DIR_TIEBREAK[2],
        "marker must multiply the code-2 step cost, with direction tiebreak added afterward"
    );
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
            ..Default::default()
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
    cells[start_idx].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
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
            ..Default::default()
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
            ..Default::default()
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
    let marked_zones = BTreeSet::from([1, 3]);
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
            }),
            ..Default::default()
        },
    );

    assert!(
        path.is_none(),
        "unmarked zone 2 should be rejected when it has no blocker-neighbor exception"
    );
}

#[test]
fn astar_hierarchy_allows_off_marker_cell_with_blocker_neighbor_count() {
    let grid = PathGrid::new(3, 1);
    let level0_zones = row_level0_graph(&[1, 2, 3]);
    let marked_zones = BTreeSet::from([1, 3]);
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
            }),
            ..Default::default()
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
fn astar_hierarchy_progress_tracks_last_accepted_next_path_zone() {
    let grid = PathGrid::new(4, 1);
    let level0_zones = row_level0_graph(&[1, 2, 3, 4]);
    let marked_zones = BTreeSet::from([1, 2, 3, 4]);
    let blocker_counts = BlockerNeighborCounts::new(4, 1);
    let level0_path = vec![1, 2, 3, 4];

    let result = find_path_with_costs_hierarchy_marker_progress(
        &grid,
        (0, 0),
        (3, 0),
        None,
        None,
        &level0_zones,
        &marked_zones,
        &blocker_counts,
        &level0_path,
        Some(MovementZone::Normal),
        None,
        None,
        None,
        0,
        false,
        false,
        None,
    )
    .expect("marked straight path should succeed");

    assert_eq!(result.path, vec![(0, 0), (1, 0), (2, 0), (3, 0)]);
    assert_eq!(result.progress_index, 3);
    assert_eq!(result.progress_cell, (3, 0));
}

#[test]
fn astar_hierarchy_progress_remains_start_when_no_next_zone_accepted() {
    let mut grid = PathGrid::new(3, 1);
    grid.set_blocked(1, 0, true);
    let level0_zones = row_level0_graph(&[1, 2, 3]);
    let marked_zones = BTreeSet::from([1, 2, 3]);
    let blocker_counts = BlockerNeighborCounts::new(3, 1);
    let level0_path = vec![1, 2, 3];
    let progress = HierarchyProgressTracker::new((0, 0), &level0_path);

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
            }),
            hierarchy_progress: Some(&progress),
            movement_zone: Some(MovementZone::Normal),
            ..Default::default()
        },
    );

    assert!(path.is_none());
    assert_eq!(progress.progress_index(), 0);
    assert_eq!(progress.progress_cell(), (0, 0));
}

// ---------------------------------------------------------------------------
// Path truncation tests (RA2 24-step segment system)
// ---------------------------------------------------------------------------

#[test]
fn test_truncate_path_shorter_than_limit_unchanged() {
    let path: Vec<(u16, u16)> = vec![(0, 0), (1, 0), (2, 0)]; // 2 steps
    let result = truncate_path(path.clone(), 24);
    assert_eq!(result, path, "Short path should be returned unchanged");
}

#[test]
fn test_truncate_path_exactly_at_limit() {
    // 24 steps = 25 entries (start + 24 moves).
    let path: Vec<(u16, u16)> = (0..25).map(|i| (i as u16, 0)).collect();
    let result = truncate_path(path.clone(), 24);
    assert_eq!(result, path, "Path at exact limit should be unchanged");
}

#[test]
fn test_truncate_path_longer_than_limit() {
    // 30 steps = 31 entries → should be truncated to 25 entries (24 steps).
    let path: Vec<(u16, u16)> = (0..31).map(|i| (i as u16, 0)).collect();
    let result = truncate_path(path, 24);
    assert_eq!(
        result.len(),
        25,
        "Should truncate to 25 entries (24 steps + start)"
    );
    assert_eq!(result[0], (0, 0));
    assert_eq!(result[24], (24, 0));
}

#[test]
fn test_truncate_layered_path_truncates_both_vecs() {
    let path: Vec<(u16, u16)> = (0..31).map(|i| (i as u16, 0)).collect();
    let layers: Vec<MovementLayer> = vec![MovementLayer::Ground; 31];
    let (rp, rl) = truncate_layered_path(path, layers, 24);
    assert_eq!(rp.len(), 25);
    assert_eq!(rl.len(), 25);
    assert_eq!(rp[24], (24, 0));
}

#[test]
fn test_truncate_path_single_cell() {
    let path: Vec<(u16, u16)> = vec![(5, 5)]; // 0 steps
    let result = truncate_path(path.clone(), 24);
    assert_eq!(result, path);
}

#[test]
fn test_find_path_long_gets_full_result() {
    // A* itself returns the full path — truncation happens at the caller level.
    let grid = PathGrid::new(50, 1);
    let path = find_path(&grid, (0, 0), (40, 0)).expect("should find path on open grid");
    assert_eq!(path.len(), 41, "A* should return the complete 40-step path");
}

// ---------------------------------------------------------------------------
// Water / naval pathfinding tests
// ---------------------------------------------------------------------------

/// Helper: build a resolved terrain grid with a water channel across the middle.
/// Layout (7 wide × 3 tall):
///   Row 0: land  land  land  land  land  land  land
///   Row 1: water water water water water water water
///   Row 2: land  land  land  land  land  land  land
fn make_water_channel_terrain() -> ResolvedTerrainGrid {
    let width: u16 = 7;
    let height: u16 = 3;
    let mut cells: Vec<ResolvedTerrainCell> = Vec::new();
    for ry in 0..height {
        for rx in 0..width {
            let is_water: bool = ry == 1;
            cells.push(ResolvedTerrainCell {
                is_water,
                ground_walk_blocked: is_water,
                terrain_class: if is_water {
                    TerrainClass::Water
                } else {
                    TerrainClass::Clear
                },
                speed_costs: if is_water {
                    // Water: ground blocked, Float/Hover/Amphibious passable.
                    SpeedCostProfile {
                        foot: Some(0),
                        track: Some(0),
                        wheel: Some(0),
                        float: Some(100),
                        amphibious: Some(100),
                        hover: Some(100),
                        float_beach: Some(100),
                    }
                } else {
                    // Land: ground passable, Float blocked.
                    SpeedCostProfile {
                        foot: Some(100),
                        track: Some(100),
                        wheel: Some(100),
                        float: Some(0),
                        amphibious: Some(80),
                        hover: Some(50),
                        float_beach: Some(0),
                    }
                },
                ..make_resolved_cell(rx, ry)
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

#[test]
fn test_water_cells_are_pathgrid_walkable() {
    let terrain: ResolvedTerrainGrid = make_water_channel_terrain();
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);
    // Water cells should be walkable in PathGrid (passability is SpeedType-dependent).
    assert!(
        grid.is_walkable(0, 1),
        "Water cell should be PathGrid-walkable"
    );
    assert!(
        grid.is_walkable(3, 1),
        "Water cell should be PathGrid-walkable"
    );
    // Land cells still walkable.
    assert!(grid.is_walkable(0, 0), "Land cell should be walkable");
}

#[test]
fn ground_mover_rejects_pathgrid_walkable_water_without_cost_grid() {
    let terrain: ResolvedTerrainGrid = make_water_channel_terrain();
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);

    assert!(
        grid.is_walkable(0, 1),
        "fixture keeps water PathGrid-walkable"
    );
    assert!(
        !is_cell_passable_for_mover(&grid, 0, 1, Some(MovementZone::Normal), Some(&terrain)),
        "Normal ground entry must reject true water through the terrain evaluator"
    );
    assert!(
        is_cell_passable_for_mover(&grid, 0, 0, Some(MovementZone::Normal), Some(&terrain)),
        "Normal ground entry still accepts ordinary land"
    );
}

#[test]
fn runtime_and_search_share_known_water_cell_admission() {
    let terrain = make_water_channel_terrain();
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let costs = TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Track);

    // Ground movement uses RuntimeTransition at the cell boundary; A* uses
    // AStarNeighbor. Both must enter through the same known-input predicate.
    let runtime_admission = is_cell_passable_for_mover_with_speed(
        &grid,
        0,
        1,
        Some(MovementZone::Normal),
        Some(SpeedType::Track),
        Some(&terrain),
        Some(&costs),
        false,
        TerrainEntryMode::RuntimeTransition,
    );
    let search_admission = is_cell_passable_for_mover_with_speed(
        &grid,
        0,
        1,
        Some(MovementZone::Normal),
        Some(SpeedType::Track),
        Some(&terrain),
        Some(&costs),
        false,
        TerrainEntryMode::AStarNeighbor,
    );
    assert!(!runtime_admission);
    assert_eq!(search_admission, runtime_admission);

    let trace = AStarTraceCollector::new();
    let path = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Ground,
        (0, 2),
        &AStarOptions {
            terrain_costs: Some(&costs),
            movement_zone: Some(MovementZone::Normal),
            resolved_terrain: Some(&terrain),
            trace_sink: Some(&trace),
            ..Default::default()
        },
    );
    assert!(path.is_none(), "A* must reject the same water boundary");
    assert!(trace.steps().iter().any(|step| {
        step.candidate_cell == (0, 1) && step.rejected_reason == Some("walkability_blocked")
    }));
}

#[test]
fn test_cliff_cells_remain_pathgrid_blocked() {
    let terrain: ResolvedTerrainGrid = ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            make_resolved_cell(0, 0),
            ResolvedTerrainCell {
                is_cliff_like: true,
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
fn test_float_unit_pathfinds_through_water() {
    let terrain: ResolvedTerrainGrid = make_water_channel_terrain();
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);
    let float_costs: TerrainCostGrid =
        TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Float);

    // Float unit paths along the water channel (row 1).
    let path = find_path_with_costs(
        &grid,
        (0, 1),
        (6, 1),
        Some(&float_costs),
        None,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some(), "Float unit should pathfind through water");
    let path: Vec<(u16, u16)> = path.unwrap();
    assert_eq!(path.first(), Some(&(0, 1)));
    assert_eq!(path.last(), Some(&(6, 1)));
    // All cells should be on the water row.
    for &(_, ry) in &path {
        assert_eq!(ry, 1, "Float path should stay on water row");
    }
}

#[test]
fn test_track_unit_cannot_pathfind_through_water() {
    let terrain: ResolvedTerrainGrid = make_water_channel_terrain();
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);
    let track_costs: TerrainCostGrid =
        TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Track);

    // Track unit trying to cross from land (0,0) to land (6,2) — must go around water.
    // But with a full water channel blocking, there is no path.
    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (6, 2),
        Some(&track_costs),
        None,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(
        path.is_none(),
        "Track unit cannot cross water channel — no path should exist"
    );
}

#[test]
fn test_amphibious_unit_crosses_land_water_land() {
    let terrain: ResolvedTerrainGrid = make_water_channel_terrain();
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);
    let amphi_costs: TerrainCostGrid =
        TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Amphibious);

    // Amphibious unit crosses from land (0,0) through water (row 1) to land (0,2).
    let path = find_path_with_costs(
        &grid,
        (0, 0),
        (0, 2),
        Some(&amphi_costs),
        None,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some(), "Amphibious unit should cross water channel");
    let path: Vec<(u16, u16)> = path.unwrap();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(0, 2)));
    // Path should go through water row 1.
    assert!(
        path.contains(&(0, 1)),
        "Amphibious path should traverse the water row"
    );
}

#[test]
fn test_ground_unit_diagonal_clips_water_corner() {
    // gamemd parity: AStar_main_loop calls Can_Enter_Cell only on the
    // diagonal neighbor; flanking cardinals are not consulted. A foot
    // unit at (1,0) heading to a land cell at (2,1) may cut the diagonal
    // even though both flanks (2,0) and (1,1) are water.
    //
    // Layout:
    //   (0,0)L  (1,0)L  (2,0)W  (3,0)L
    //   (0,1)L  (1,1)W  (2,1)L  (3,1)L
    //   (0,2)L  (1,2)L  (2,2)L  (3,2)L
    let water_cost: SpeedCostProfile = SpeedCostProfile {
        foot: Some(0),
        track: Some(0),
        wheel: Some(0),
        float: Some(100),
        ..SpeedCostProfile::default()
    };
    let terrain: ResolvedTerrainGrid = ResolvedTerrainGrid::from_cells(
        4,
        3,
        vec![
            make_resolved_cell(0, 0),
            make_resolved_cell(1, 0), // start
            ResolvedTerrainCell {
                is_water: true,
                ground_walk_blocked: true,
                speed_costs: water_cost,
                ..make_resolved_cell(2, 0)
            },
            make_resolved_cell(3, 0),
            make_resolved_cell(0, 1),
            ResolvedTerrainCell {
                is_water: true,
                ground_walk_blocked: true,
                speed_costs: water_cost,
                ..make_resolved_cell(1, 1)
            },
            make_resolved_cell(2, 1), // goal
            make_resolved_cell(3, 1),
            make_resolved_cell(0, 2),
            make_resolved_cell(1, 2),
            make_resolved_cell(2, 2),
            make_resolved_cell(3, 2),
        ],
    );
    let grid: PathGrid = PathGrid::from_resolved_terrain(&terrain);
    let foot_costs: TerrainCostGrid =
        TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Foot);

    let path = find_path_with_costs(
        &grid,
        (1, 0),
        (2, 1),
        Some(&foot_costs),
        None,
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(path.is_some(), "Foot unit should find a path to (2,1)");
    let path: Vec<(u16, u16)> = path.unwrap();
    assert_eq!(path.first(), Some(&(1, 0)));
    assert_eq!(path.last(), Some(&(2, 1)));
    // gamemd parity: the direct diagonal (1,0)->(2,1) is the cheapest
    // route, so the search picks it even with water flanking. A 1-step
    // path with no detour is the expected gamemd result.
    assert_eq!(
        path.len(),
        2,
        "Foot unit should take the direct diagonal (1,0)->(2,1) — flanking water cells do not block"
    );
}

#[test]
fn test_ground_units_on_land_no_regression() {
    // Basic sanity: ground pathfinding on land still works correctly.
    let grid: PathGrid = PathGrid::new(10, 10);
    let path = find_path(&grid, (0, 0), (5, 5));
    assert!(
        path.is_some(),
        "Ground pathfinding on open land must still work"
    );
    let path: Vec<(u16, u16)> = path.unwrap();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(5, 5)));
}

// --- nearest_walkable tests ---

#[test]
fn test_nearest_walkable_already_walkable() {
    let grid: PathGrid = PathGrid::new(10, 10);
    let result = grid.nearest_walkable(5, 5, 3, None, None);
    assert_eq!(result, Some((5, 5)));
}

#[test]
fn test_nearest_walkable_blocked_center() {
    let mut grid: PathGrid = PathGrid::new(10, 10);
    grid.set_blocked(5, 5, true);
    let result = grid.nearest_walkable(5, 5, 3, None, None);
    assert!(result.is_some());
    let (rx, ry) = result.unwrap();
    // Must be adjacent (distance 1) since surrounding cells are walkable.
    let dx = (rx as i32 - 5).abs();
    let dy = (ry as i32 - 5).abs();
    assert!(
        dx <= 1 && dy <= 1,
        "Expected adjacent cell, got ({},{})",
        rx,
        ry
    );
    assert!(grid.is_walkable(rx, ry));
}

#[test]
fn test_nearest_walkable_building_footprint() {
    // 3x3 building at (4,4) — block cells (4,4) through (6,6).
    let mut grid: PathGrid = PathGrid::new(10, 10);
    for dy in 0..3u16 {
        for dx in 0..3u16 {
            grid.set_blocked(4 + dx, 4 + dy, true);
        }
    }
    // Searching from center of building (5,5) should find a cell outside.
    let result = grid.nearest_walkable(5, 5, 5, None, None);
    assert!(result.is_some());
    let (rx, ry) = result.unwrap();
    assert!(grid.is_walkable(rx, ry));
}

#[test]
fn test_nearest_walkable_with_entity_blocks() {
    let grid: PathGrid = PathGrid::new(10, 10);
    let mut blocks: BTreeSet<(u16, u16)> = BTreeSet::new();
    blocks.insert((5, 5));
    blocks.insert((5, 4)); // block north neighbor too
    let result = grid.nearest_walkable(5, 5, 3, Some(&blocks), None);
    assert!(result.is_some());
    let (rx, ry) = result.unwrap();
    assert_ne!((rx, ry), (5, 5));
    assert_ne!((rx, ry), (5, 4));
    assert!(grid.is_walkable(rx, ry));
}

#[test]
fn test_nearest_walkable_no_walkable_in_radius() {
    // Tiny grid entirely blocked.
    let mut grid: PathGrid = PathGrid::new(3, 3);
    for y in 0..3u16 {
        for x in 0..3u16 {
            grid.set_blocked(x, y, true);
        }
    }
    let result = grid.nearest_walkable(1, 1, 5, None, None);
    assert_eq!(result, None);
}

#[test]
fn test_height_based_bridge_routing_deck_at_4() {
    // 5x1 grid: land(h=4) → transition(g=0,d=4) → bridge(g=0,d=4) → transition(g=0,d=4) → land(h=4)
    let terrain = ResolvedTerrainGrid::from_cells(
        5,
        1,
        vec![
            ResolvedTerrainCell {
                level: 4,
                ..make_resolved_cell(0, 0)
            },
            ResolvedTerrainCell {
                level: 0,
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                level: 0,
                ground_walk_blocked: true,
                is_water: true,
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                ..make_resolved_cell(2, 0)
            },
            ResolvedTerrainCell {
                level: 0,
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
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let path = find_layered_path(
        &grid,
        None,
        None,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        None,
        None,
        None,
        0,
        false,
        false,
    )
    .expect("path across bridge should exist");

    assert_eq!(path.first().map(|s| (s.rx, s.ry)), Some((0, 0)));
    assert_eq!(path.last().map(|s| (s.rx, s.ry)), Some((4, 0)));

    // Middle cells (bridge span) should be on Bridge layer since
    // start height=4, bridge ground_level=0, diff=4 >= 2 → bridge list
    let bridge_steps: Vec<_> = path
        .iter()
        .filter(|s| s.layer == MovementLayer::Bridge)
        .collect();
    assert!(
        !bridge_steps.is_empty(),
        "Bridge cells should route on Bridge layer with height diff >= 2"
    );
}

#[test]
fn test_cliff_cost_uses_effective_height_not_ground_level() {
    // Verify: bridge deck (effective 4) → land (ground 4) has NO cliff penalty.
    // Old behavior: skipped cliff on bridge layer. New: compares node heights.
    // node.height=4 (deck), neighbor_height=4 (land ground_level) → equal → no penalty.
    let terrain = ResolvedTerrainGrid::from_cells(
        3,
        1,
        vec![
            ResolvedTerrainCell {
                level: 0,
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                ..make_resolved_cell(0, 0)
            },
            ResolvedTerrainCell {
                level: 0,
                bridge_walkable: true,
                bridge_transition: true,
                bridge_deck_level: 4,
                has_bridge_deck: true,
                ..make_resolved_cell(1, 0)
            },
            ResolvedTerrainCell {
                level: 4,
                ..make_resolved_cell(2, 0)
            },
        ],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    // Path from bridge start to land at same effective height
    let path = find_layered_path(
        &grid,
        None,
        None,
        (0, 0),
        MovementLayer::Bridge,
        (2, 0),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    // Should find a path (no false cliff penalty blocking it)
    assert!(
        path.is_some(),
        "Bridge(deck=4) to land(ground=4) should not have false cliff penalty"
    );
}

// ============================================================================
// Bridge-locomotor A* regression tests (Plan: 2026-05-11 G3/G4 fixes).
// Pin: diff-2 and diff-3 blocked, diff-4 with bridgehead allowed, diff-4
// without bridgehead rejected, stamped transition cells remain usable, and
// Forward2-style non-transition structural deck destinations are rejected.
// ============================================================================

fn make_grid_for_bridge_test() -> PathGrid {
    // 10x10 grid. Default cells are ground_walkable at height 0.
    PathGrid::new(10, 10)
}

fn direction6_bridge_row_grid() -> PathGrid {
    let mut g = make_grid_for_bridge_test();
    g.set_cell_for_test(0, 1, 0, true, false); // Forward2/min-X stamped edge lane.
    g.set_cell_for_test(1, 1, 0, true, true); // Forward1.
    g.set_cell_for_test(2, 1, 0, true, true); // Raw BRIDGE2 anchor.
    g.set_cell_for_test(3, 1, 0, true, true); // Opposite.
    g.set_cell_for_test(4, 1, 0, false, false); // ExtraDir6 side marker, not deck.
    g
}

#[test]
fn astar_blocks_height_diff_2() {
    // Adjacent ground cells at heights 0 and 2, no bridge. Should NOT path through.
    let mut g = make_grid_for_bridge_test();
    g.set_cell_for_test(2, 1, 2, false, false);
    let result = astar_search(
        &g,
        (1, 1),
        MovementLayer::Ground,
        (3, 1),
        &AStarOptions::default(),
    );
    if let Some(path) = result {
        assert!(
            !path.iter().any(|s| (s.rx, s.ry) == (2, 1)),
            "A* must not route through diff-2 ground step"
        );
    }
}

#[test]
fn astar_blocks_height_diff_3() {
    let mut g = make_grid_for_bridge_test();
    g.set_cell_for_test(2, 1, 3, false, false);
    let result = astar_search(
        &g,
        (1, 1),
        MovementLayer::Ground,
        (3, 1),
        &AStarOptions::default(),
    );
    if let Some(path) = result {
        assert!(
            !path.iter().any(|s| (s.rx, s.ry) == (2, 1)),
            "A* must not route through diff-3 ground step"
        );
    }
}

#[test]
fn astar_allows_height_diff_4_with_bridgehead() {
    // Ground at (1,1) raw h=4 → bridgehead (2,1) raw h=0 (bridge_walkable+transition)
    // → body (3,1) raw h=0 (bridge_walkable only).
    let mut g = make_grid_for_bridge_test();
    g.set_cell_for_test(1, 1, 4, false, false);
    g.set_cell_for_test(2, 1, 0, true, true); // bridgehead
    g.set_cell_for_test(3, 1, 0, true, true); // stamped deck
    let result = astar_search(
        &g,
        (1, 1),
        MovementLayer::Ground,
        (3, 1),
        &AStarOptions::default(),
    );
    assert!(result.is_some(), "A* must find a path through bridgehead");
    let path = result.unwrap();
    let step_2_1 = path.iter().find(|s| (s.rx, s.ry) == (2, 1));
    assert!(step_2_1.is_some(), "Path must include (2,1)");
    assert_eq!(
        step_2_1.unwrap().layer,
        MovementLayer::Bridge,
        "Bridgehead step must be on Bridge layer (G3)"
    );
}

#[test]
fn astar_blocks_height_diff_4_without_bridgehead() {
    // Same as above but (2,1) has transition=false (body cell, not bridgehead).
    // A* must NOT route Ground→Bridge through this cell.
    let mut g = make_grid_for_bridge_test();
    g.set_cell_for_test(1, 1, 4, false, false);
    g.set_cell_for_test(2, 1, 0, true, false); // body, no transition
    g.set_cell_for_test(3, 1, 0, true, false);
    let result = astar_search(
        &g,
        (1, 1),
        MovementLayer::Ground,
        (3, 1),
        &AStarOptions::default(),
    );
    if let Some(path) = result {
        let through_body = path
            .iter()
            .find(|s| (s.rx, s.ry) == (2, 1) && s.layer == MovementLayer::Bridge);
        assert!(
            through_body.is_none(),
            "A* must not route Ground→Bridge through body cell without bridgehead (G3)"
        );
    }
}

#[test]
fn bridge_traversal_predicate_uses_height_for_structural_deck_gate() {
    let current = bridge_test_cell(0, true, true, 0);
    let forward2 = bridge_test_cell(0, true, false, 0);

    assert!(
        needs_bridge_traversal_for_edge(4, &current, &forward2),
        "Deck-height structural moves must run CheckBridgeTraversal"
    );
    assert!(
        !needs_bridge_traversal_for_edge(0, &current, &forward2),
        "Ground-height movement under a structural bridge is not the deck-only Forward2 gate"
    );
}

#[test]
fn astar_direction6_uses_non_anchor_transition_cells() {
    let g = direction6_bridge_row_grid();

    assert!(g.cell(0, 1).unwrap().bridge_walkable);
    assert!(
        !g.cell(0, 1).unwrap().transition,
        "Forward2 is stamped structural but not a transition cell"
    );
    assert!(
        g.cell(1, 1).unwrap().transition,
        "Forward1 remains a stamped transition cell"
    );
    assert!(
        !g.is_walkable_on_layer(4, 1, MovementLayer::Bridge),
        "ExtraDir6 side marker must not become a raw-anchor-only bridge lane"
    );

    let result = astar_search(
        &g,
        (3, 1),
        MovementLayer::Bridge,
        (1, 1),
        &AStarOptions::default(),
    );
    let path = result.expect("A* must path across stamped transition cells");
    let destination = path.last().unwrap();
    assert_eq!((destination.rx, destination.ry), (1, 1));
    assert_eq!(destination.layer, MovementLayer::Bridge);
}

#[test]
fn astar_direction6_rejects_forward2_bridge_deck_destination() {
    let g = direction6_bridge_row_grid();

    let result = astar_search(
        &g,
        (2, 1),
        MovementLayer::Bridge,
        (0, 1),
        &AStarOptions::default(),
    );

    if let Some(path) = result {
        assert!(
            !path
                .iter()
                .any(|step| (step.rx, step.ry) == (0, 1) && step.layer == MovementLayer::Bridge),
            "A* must not route deck vehicles into Forward2/min-X structural edge lane"
        );
    }
}

#[test]
fn astar_blocks_structural_body_to_body_bad_height_jump() {
    let parent = bridge_test_cell(0, true, true, 0);
    let candidate = bridge_test_cell(2, true, true, 0);
    let grid = PathGrid::from_cells(vec![parent, candidate], 2, 1);

    let result = astar_search(
        &grid,
        (0, 0),
        MovementLayer::Bridge,
        (1, 0),
        &AStarOptions::default(),
    );

    assert!(
        result.is_none(),
        "A* must not bypass height legality for structural bridge cells"
    );
}

// ============================================================================
// Height-diff legality gate (CheckBridgeTraversal port).
// Pins: diff-1 transitions require the LOWER cell's slope_type != 0;
// diff ∈ {±2, ±3, ±5+} always block; diff-4 reaching the gate is non-bridge
// cliff and blocks too (legitimate bridge entries carry through as diff-0 via
// compute_neighbor_height Case 3).
// ============================================================================

#[test]
fn gsi_04_03a_signed_minus_one_height_uses_lower_raw_slope_in_both_directions() {
    let lower = bridge_test_cell(0xFF, false, false, 0);
    let upper = bridge_test_cell(0, false, false, 0xFE);
    let grid = PathGrid::from_cells(vec![lower, upper], 2, 1);

    assert!(
        find_path(&grid, (0, 0), (1, 0)).is_none(),
        "-1 to 0 must test the lower -1 cell's zero raw slope byte"
    );
    assert!(
        find_path(&grid, (1, 0), (0, 0)).is_none(),
        "0 to -1 must test the lower -1 cell's zero raw slope byte"
    );
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

#[test]
fn gsi_04_03a_signed_minus_one_height_accepts_high_nonzero_raw_slope() {
    let lower = bridge_test_cell(0xFF, false, false, 0xFE);
    let upper = bridge_test_cell(0, false, false, 0);
    let grid = PathGrid::from_cells(vec![lower, upper], 2, 1);

    assert!(
        find_path(&grid, (0, 0), (1, 0)).is_some(),
        "-1 to 0 must accept any nonzero lower raw slope byte, including 0xFE"
    );
    assert!(
        find_path(&grid, (1, 0), (0, 0)).is_some(),
        "0 to -1 must accept any nonzero lower raw slope byte, including 0xFE"
    );
}

#[test]
fn diff_1_slope_zero_lower_blocks_going_up() {
    // 3x1 grid: level 0 (slope 0) — level 1 (slope 0) — level 0 (slope 0).
    // Both diff-1 edges have a slope=0 lower cell — gate must block both.
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 1,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 3, 1);
    let path = find_path(&grid, (0, 0), (2, 0));
    assert!(
        path.is_none(),
        "diff-1 cliff with slope=0 lower cell must block A* — got {:?}",
        path
    );
}

#[test]
fn diff_1_slope_only_on_upper_cell_still_blocks() {
    // 3x1 grid: level 0 (slope 0) — level 1 (slope 2, ramp) — level 0 (slope 0).
    // The UPPER cell has slope=2, but the LOWER cell on each edge is the
    // level-0 flat with slope=0 — and the gate reads the LOWER cell's slope.
    // So both edges must still block.
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 1,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 3, 1);
    let path = find_path(&grid, (0, 0), (2, 0));
    assert!(
        path.is_none(),
        "lower cell (level-0 flat, slope=0) gates the edge — upper-side slope is irrelevant"
    );
}

#[test]
fn diff_1_slope_nonzero_lower_permits() {
    // 3x1 grid: level 0 (slope 2) — level 1 (slope 0) — level 0 (slope 2).
    // The LOWER cell on each edge has slope=2, so the gate permits and A*
    // finds the path 0→1→2.
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 1,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 3, 1);
    let path = find_path(&grid, (0, 0), (2, 0))
        .expect("lower-cell slope=2 must permit diff-1 transitions");
    assert_eq!(path.first().copied(), Some((0, 0)));
    assert_eq!(path.last().copied(), Some((2, 0)));
}

#[test]
fn diff_1_slope_zero_lower_blocks_going_down() {
    // 2x1 grid stepping DOWN: level 1 → level 0. Lower cell has slope=0.
    // Symmetry check that the gate fires regardless of step direction.
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 1,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 2, 1);
    let path = find_path(&grid, (0, 0), (1, 0));
    assert!(
        path.is_none(),
        "going-down diff-1 with slope=0 lower must block — got {:?}",
        path
    );
}

#[test]
fn l6_canary_unit_at_deck_height_stepping_to_non_bridge_produces_nonzero_diff() {
    // Design ledger L6: the divergent diff-0 case (unit Z mismatched with
    // src ground level on a non-bridge transition) cannot arise in our model
    // because compute_neighbor_height keeps unit Z synced with cell state.
    //
    // This canary fixes the invariant in a unit test. A unit on a bridge deck
    // (height=4) stepping into a non-bridge cell takes Case 1, which returns
    // neighbor.ground_level (0), giving diff=-4. The legality gate then
    // blocks via `_ => false`.
    //
    // If this test ever fails, compute_neighbor_height has been refactored
    // in a way that lets the divergent diff-0 state arise — at which point
    // the legality gate needs an explicit diff-0 guard.
    let parent = PathCell {
        ground_walkable: true,
        bridge_walkable: true,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 4,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let neighbor = PathCell {
        ground_walkable: true,
        bridge_walkable: false,
        bridge_structural: false,
        bridge_marker_0x80: false,
        transition: false,
        ground_level: 0,
        bridge_deck_level: 0,
        slope_type: 0,
        tube_index: None,
        low_bridge_tube_cell: false,
    };
    let h = compute_neighbor_height(4, &parent, &neighbor);
    assert_eq!(h, 0, "Case 1: non-bridge neighbor returns its ground_level");
    let diff = h as i16 - 4i16;
    assert_eq!(
        diff.abs(),
        4,
        "L6 invariant: unit at deck height stepping to non-bridge produces |diff|=4, not 0"
    );
}

#[test]
fn diff_2_blocks_regardless_of_slope() {
    // 2x1 grid: level 0 — level 2. Both cells have slope=2 (would permit
    // diff-1 either way). diff>=2 always hard-blocks even with ramps.
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 2,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 2, 1);
    let path = find_path(&grid, (0, 0), (1, 0));
    assert!(
        path.is_none(),
        "diff-2 must always block, even with slope=2 on both cells — got {:?}",
        path
    );
}

#[test]
fn diff_3_blocks_regardless_of_slope() {
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 3,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 2, 1);
    let path = find_path(&grid, (0, 0), (1, 0));
    assert!(path.is_none(), "diff-3 must block — got {:?}", path);
}

#[test]
fn diff_5_blocks_regardless_of_slope() {
    let cells = vec![
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
        PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 5,
            bridge_deck_level: 0,
            slope_type: 2,
            tube_index: None,
            low_bridge_tube_cell: false,
        },
    ];
    let grid = PathGrid::from_cells(cells, 2, 1);
    let path = find_path(&grid, (0, 0), (1, 0));
    assert!(path.is_none(), "diff-5 must block — got {:?}", path);
}
