//! Map-edge cell finders.
//!
//! The legacy ground helper picks a walkable rectangular edge cell biased
//! toward a target. Paradrop carrier spawn and exit instead share active
//! `FUN_004AA440`'s sentinel/sentinel, criterion-4 MapClass path: randomized
//! LocalSize scans select a cell just outside the isometric playfield without
//! consulting ordinary ground passability.
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on sim/pathfinding.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::playfield::{local_to_packed_cell, PlayfieldBounds};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::cell_rect::cell_is_in_playfield_height_aware;
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    North,
    East,
    South,
    West,
}

impl Edge {
    /// `HouseClass @ 0x0050DA80`: the house's own edge, `+0x577C`
    /// (`HouseState::waypoint_edge`) when it is 0..3, else North.
    pub(crate) fn own_edge(waypoint_edge: u8) -> Self {
        Self::from_index(waypoint_edge).unwrap_or(Edge::North)
    }

    pub fn from_index(i: u8) -> Option<Self> {
        match i {
            0 => Some(Edge::North),
            1 => Some(Edge::East),
            2 => Some(Edge::South),
            3 => Some(Edge::West),
            _ => None,
        }
    }
}

/// Find a passable cell along the given map edge, biased toward `target`.
/// Returns `None` if no passable cell exists along that edge.
pub fn find_passable_at_edge(
    path_grid: &PathGrid,
    map_width: u16,
    map_height: u16,
    edge: Edge,
    target: (u16, u16),
) -> Option<(u16, u16)> {
    match edge {
        Edge::North | Edge::East | Edge::West => {
            scan_linear(path_grid, edge, map_width, map_height, target)
        }
        Edge::South => scan_candidates_closest(path_grid, map_width, map_height, target),
    }
}

/// Find the paradrop carrier spawn/exit cell along a MapClass edge.
///
/// Active spawner and Approach/Overfly callers pass sentinel references and
/// criterion `4`, as does Aircraft Mission_Attack's state 10 (`0x00418C43`).
/// `FUN_004AA440 @ 0x004AA440` first rejects cells that are inside mode-one
/// playfield geometry, then criterion 4 accepts the first outside candidate
/// unconditionally. Every edge spends its initial Scenario RandomRanged call;
/// North scans local row 0 (the sentinel arm leaves `param_4` 0), East and
/// West the local columns `LocalWidth` and 0; South gathers one outside
/// candidate per local X and spends a second draw to choose among the full
/// vector. Original rows: `tools/spatial_oracle/aircraft_states.py`
/// (`state10`, `state10_seeds`: all four edges over a flat map).
pub fn find_paradrop_edge_cell(
    playfield_bounds: Option<PlayfieldBounds>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    edge: Edge,
    scenario_rng: &mut SimRng,
) -> Option<(u16, u16)> {
    let bounds = playfield_bounds?;
    let width = bounds.off_104;
    let twice_height = bounds.off_108.wrapping_mul(2);

    let start = match edge {
        Edge::North | Edge::South => random_ranged_i32(scenario_rng, 1, width).wrapping_sub(1),
        Edge::East => random_ranged_i32(scenario_rng, 1, twice_height).wrapping_sub(1),
        Edge::West => random_ranged_i32(scenario_rng, 0, twice_height).wrapping_sub(1),
    };
    let fallback = local_to_packed_cell(bounds, 1, width / 2);

    match edge {
        Edge::North => {
            for n in 0..width {
                let local_u = n.wrapping_add(start) % width;
                let candidate = local_to_packed_cell(bounds, local_u, 0);
                if candidate_is_outside(candidate, bounds, resolved_terrain) {
                    return Some(pack_cell(candidate));
                }
            }
            Some(pack_cell(fallback))
        }
        Edge::East | Edge::West => {
            let local_u = if edge == Edge::East { width } else { 0 };
            for n in 0..twice_height {
                let local_v = n.wrapping_add(start) % twice_height;
                let candidate = local_to_packed_cell(bounds, local_u, local_v);
                if candidate_is_outside(candidate, bounds, resolved_terrain) {
                    return Some(pack_cell(candidate));
                }
            }
            Some(pack_cell(fallback))
        }
        Edge::South => {
            let mut candidates = Vec::with_capacity(10);
            for local_u in 0..width {
                for offset in 0..15 {
                    let local_v = twice_height.wrapping_add(offset);
                    let candidate = local_to_packed_cell(bounds, local_u, local_v);
                    if candidate_is_outside(candidate, bounds, resolved_terrain) {
                        candidates.push(pack_cell(candidate));
                        break;
                    }
                }
            }
            if candidates.is_empty() {
                return Some((0, 0));
            }
            let index = random_ranged_i32(
                scenario_rng,
                0,
                i32::try_from(candidates.len() - 1).expect("native candidate count fits i32"),
            );
            Some(candidates[index as usize])
        }
    }
}

fn candidate_is_outside(
    candidate: (i32, i32),
    bounds: PlayfieldBounds,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    !cell_is_in_playfield_height_aware(candidate, Some(bounds), resolved_terrain)
}

const fn pack_cell(cell: (i32, i32)) -> (u16, u16) {
    (cell.0 as i16 as u16, cell.1 as i16 as u16)
}

fn random_ranged_i32(rng: &mut SimRng, low: i32, high: i32) -> i32 {
    let (lo, hi) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let span = i64::from(hi) - i64::from(lo);
    if span == 0 {
        return lo;
    }
    let span = u32::try_from(span).expect("MapClass ranged span fits native u32");
    lo.wrapping_add(rng.next_range_u32_inclusive(0, span) as i32)
}

fn scan_linear(
    path_grid: &PathGrid,
    edge: Edge,
    map_width: u16,
    map_height: u16,
    target: (u16, u16),
) -> Option<(u16, u16)> {
    let cells: Vec<(u16, u16)> = match edge {
        Edge::North => (0..map_width).map(|x| (x, 0)).collect(),
        Edge::East => (0..map_height)
            .map(|y| (map_width.saturating_sub(1), y))
            .collect(),
        Edge::West => (0..map_height).map(|y| (0, y)).collect(),
        Edge::South => unreachable!("south uses scan_candidates_closest"),
    };

    cells
        .into_iter()
        .filter(|&(rx, ry)| path_grid.is_walkable(rx, ry))
        .min_by_key(|&(rx, ry)| {
            let dx = rx as i32 - target.0 as i32;
            let dy = ry as i32 - target.1 as i32;
            dx * dx + dy * dy
        })
}

fn scan_candidates_closest(
    path_grid: &PathGrid,
    map_width: u16,
    map_height: u16,
    target: (u16, u16),
) -> Option<(u16, u16)> {
    let south_y = map_height.saturating_sub(1);
    let mut candidates: Vec<(u16, u16)> = Vec::with_capacity(10);
    for x in 0..map_width {
        if candidates.len() >= 10 {
            break;
        }
        if path_grid.is_walkable(x, south_y) {
            candidates.push((x, south_y));
        }
    }
    candidates.into_iter().min_by_key(|&(rx, ry)| {
        let dx = rx as i32 - target.0 as i32;
        let dy = ry as i32 - target.1 as i32;
        dx * dx + dy * dy
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_north_edge_picks_closest_to_target_x() {
        let grid = PathGrid::test_all_passable(100, 100);
        let cell = find_passable_at_edge(&grid, 100, 100, Edge::North, (42, 50)).unwrap();
        assert_eq!(cell.1, 0);
        assert_eq!(cell.0, 42);
    }

    #[test]
    fn test_west_edge_picks_closest_to_target_y() {
        let grid = PathGrid::test_all_passable(100, 100);
        let cell = find_passable_at_edge(&grid, 100, 100, Edge::West, (50, 70)).unwrap();
        assert_eq!(cell.0, 0);
        assert_eq!(cell.1, 70);
    }

    #[test]
    fn test_east_edge_picks_closest_to_target_y() {
        let grid = PathGrid::test_all_passable(100, 100);
        let cell = find_passable_at_edge(&grid, 100, 100, Edge::East, (50, 30)).unwrap();
        assert_eq!(cell.0, 99);
        assert_eq!(cell.1, 30);
    }

    #[test]
    fn test_south_edge_picks_closest_to_target_x_within_first_10() {
        // Mode 2 only collects the first 10 walkable cells (x=0..10),
        // then picks closest to target.x. Target x=5 → cell x=5.
        let grid = PathGrid::test_all_passable(100, 100);
        let cell = find_passable_at_edge(&grid, 100, 100, Edge::South, (5, 50)).unwrap();
        assert_eq!(cell, (5, 99));
    }

    #[test]
    fn test_south_edge_target_outside_candidate_window_picks_nearest_candidate() {
        // Target x=80 — outside the 0..10 candidate window. Closest candidate is x=9.
        let grid = PathGrid::test_all_passable(100, 100);
        let cell = find_passable_at_edge(&grid, 100, 100, Edge::South, (80, 50)).unwrap();
        assert_eq!(cell, (9, 99));
    }

    #[test]
    fn test_no_passable_returns_none() {
        let grid = PathGrid::test_all_blocked(100, 100);
        assert_eq!(
            find_passable_at_edge(&grid, 100, 100, Edge::North, (50, 50)),
            None
        );
        assert_eq!(
            find_passable_at_edge(&grid, 100, 100, Edge::South, (50, 50)),
            None
        );
    }

    fn square_bounds() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 100,
            off_fc: 0,
            off_100: 0,
            off_104: 100,
            off_108: 100,
        }
    }

    #[test]
    fn paradrop_north_spends_initial_draw_and_returns_first_outside_cell() {
        let grid = PathGrid::test_all_blocked(200, 200);
        assert_eq!(
            find_passable_at_edge(&grid, 200, 200, Edge::North, (42, 50)),
            None
        );

        let bounds = square_bounds();
        let mut expected_rng = SimRng::new(0xAA44_0000);
        let start = expected_rng.next_range_u32_inclusive(1, 100) as i32 - 1;
        assert_eq!(start, 36);
        let expected = pack_cell(local_to_packed_cell(bounds, start, 0));
        assert_eq!(expected, (36, 64));
        let mut actual_rng = SimRng::new(0xAA44_0000);
        assert_eq!(
            find_paradrop_edge_cell(Some(bounds), None, Edge::North, &mut actual_rng),
            Some(expected)
        );
        assert_eq!(actual_rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn paradrop_vertical_modes_keep_native_random_start_and_west_negative_remainder() {
        let bounds = square_bounds();
        for (edge, seed) in [(Edge::East, 0xAA44_0001), (Edge::West, 0xAA44_0001)] {
            let mut expected_rng = SimRng::new(seed);
            let start = match edge {
                Edge::East => expected_rng.next_range_u32_inclusive(1, 200) as i32 - 1,
                Edge::West => expected_rng.next_range_u32_inclusive(0, 200) as i32 - 1,
                _ => unreachable!(),
            };
            assert_eq!(
                start,
                if edge == Edge::East { 0 } else { -1 },
                "fixed witness pins West's negative x86 remainder"
            );
            let local_u = if edge == Edge::East { 100 } else { 0 };
            let expected = (0..200)
                .map(|n| local_to_packed_cell(bounds, local_u, (n + start) % 200))
                .find(|&candidate| candidate_is_outside(candidate, bounds, None))
                .map(pack_cell)
                .expect("square edge scan has an outside candidate");
            let mut actual_rng = SimRng::new(seed);
            assert_eq!(
                find_paradrop_edge_cell(Some(bounds), None, edge, &mut actual_rng),
                Some(expected),
                "{edge:?}"
            );
            assert_eq!(
                actual_rng.logical_state(),
                expected_rng.logical_state(),
                "{edge:?}"
            );
        }
    }

    #[test]
    fn paradrop_south_grows_past_ten_and_randomly_selects_full_vector() {
        let bounds = square_bounds();
        let mut expected_rng = SimRng::new(0xAA44_0002);
        let _unused_start_draw = expected_rng.next_range_u32_inclusive(1, 100);
        assert_eq!(_unused_start_draw, 61);
        let mut candidates = Vec::new();
        for local_u in 0..100 {
            let candidate = (0..15)
                .map(|offset| local_to_packed_cell(bounds, local_u, 200 + offset))
                .find(|&candidate| candidate_is_outside(candidate, bounds, None))
                .expect("each square south column reaches outside within fifteen rows");
            candidates.push(pack_cell(candidate));
        }
        assert_eq!(candidates.len(), 100, "ten is capacity, not a cap");
        let selected = expected_rng.next_range_u32_inclusive(0, 99) as usize;
        assert_eq!(selected, 29, "fixed witness exercises grown storage");
        assert_eq!(candidates[selected], (131, 172));
        let mut actual_rng = SimRng::new(0xAA44_0002);
        assert_eq!(
            find_paradrop_edge_cell(Some(bounds), None, Edge::South, &mut actual_rng),
            Some(candidates[selected])
        );
        assert_eq!(actual_rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn paradrop_edge_requires_mapclass_authority_without_spending_rng() {
        let mut rng = SimRng::new(0xAA44_FFFF);
        let before = rng.logical_state();
        assert_eq!(
            find_paradrop_edge_cell(None, None, Edge::North, &mut rng),
            None
        );
        assert_eq!(rng.logical_state(), before);
    }

    #[test]
    fn test_edge_from_index() {
        assert_eq!(Edge::from_index(0), Some(Edge::North));
        assert_eq!(Edge::from_index(1), Some(Edge::East));
        assert_eq!(Edge::from_index(2), Some(Edge::South));
        assert_eq!(Edge::from_index(3), Some(Edge::West));
        assert_eq!(Edge::from_index(4), None);
    }
}
