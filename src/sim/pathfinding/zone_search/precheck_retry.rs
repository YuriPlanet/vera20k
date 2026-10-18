//! What `AStar_pathfind_search` (`0x0042C900`) does between a failed corridor
//! search and its next try: `PathfinderClass::UpdateHierarchicalEdges`
//! (`0x0042CCD0`, called at `0x0042CC79`) floods from the cell the search got
//! furthest to with `MapClass::FloodFillReachableZones` (`0x005840C0`, called
//! at `0x0042CD80`), then either excludes the zone pairs the flood could not
//! cross or calls `PathfinderClass::InvalidateZoneEdge` (`0x0042CF80`, called
//! at `0x0042CDAC`). Those are their only callers.
//!
//! Evidence: instruction reading of the three bodies; no executed comparison.
//! None of them draws a random number, writes a timer or detaches anything;
//! the flood asks the Foot's `+0x1AC`, the receiver the cell search asks.

use super::super::passability::is_passable_for_zone;
use super::super::zone_hierarchy::{
    ZONE_PRECHECK_LEVELS, ZoneHierarchy, ZoneLevelGraph, ZonePrecheckExclusions,
};
use super::super::zone_map::{ZONE_INVALID, ZoneGrid, ZoneId};
use super::super::{SearchEntryCandidate, SearchEntryQuery, SearchFootEntry};
use super::PathSearchFailure;
use super::native_path_entry::live_mode_one;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::util::direction_tables::CELL_DELTAS;

/// What one retry reads, all fixed for the path search.
pub(super) struct RetryInputs<'a> {
    pub(super) zone_grid: &'a ZoneGrid,
    pub(super) hierarchy: &'a ZoneHierarchy,
    pub(super) terrain: &'a ResolvedTerrainGrid,
    pub(super) bounds: Option<PlayfieldBounds>,
    /// The type's `MovementZone=` (`+0x5B4`), which the flood reads for
    /// itself (`0x005841B4..0x005841C9`).
    pub(super) movement_zone: MovementZone,
    pub(super) foot_entry: &'a dyn SearchFootEntry,
}

impl RetryInputs<'_> {
    /// A cell's zone at `level`: Map+0x70's word at the packed, clamped
    /// index (`0x0056D3F0`, and the flood's `0x0056D430` with its own clamp).
    fn zone_at(&self, level: usize, (x, y): (i16, i16)) -> Result<ZoneId, PathSearchFailure> {
        self.zone_grid
            .hierarchy_zone_at_native(level, (x as u16, y as u16))
            .ok_or(PathSearchFailure::MissingHierarchyCell)
    }

    fn level(&self, level: usize) -> Result<&ZoneLevelGraph, PathSearchFailure> {
        self.hierarchy
            .level(level)
            .ok_or(PathSearchFailure::MissingHierarchyCell)
    }
}

/// `PathfinderClass::UpdateHierarchicalEdges @ 0x0042CCD0` after a corridor
/// search failed with its progress (`+0x70`) at `cell`: at each level, low to
/// high, flood from the cell; a split zone invalidates its route edge, and
/// any other excludes its neighbours the flood did not reach. Answers whether
/// the next try still runs the hierarchy (`PathfinderClass+0x38`, which only
/// [`invalidate_zone_edge`] clears).
pub(super) fn update_hierarchical_edges(
    inputs: &RetryInputs<'_>,
    routes: &[Vec<ZoneId>; ZONE_PRECHECK_LEVELS],
    cell: (u16, u16),
    exclusions: &mut ZonePrecheckExclusions,
) -> Result<bool, PathSearchFailure> {
    let cell = (cell.0 as i16, cell.1 as i16);
    let mut hierarchy_usable = true;
    for (level, route) in routes.iter().enumerate() {
        let zone = inputs.zone_at(level, cell)?;
        match flood_fill_reachable_zones(inputs, cell, level, zone)? {
            // 0x0042CDD8..0x0042CE5A, from the vector's last entry.
            ZoneFlood::Unreached(neighbors) => {
                for &neighbor in neighbors.iter().rev() {
                    exclusions.insert(level, zone, neighbor);
                }
            }
            // 0x0042CD89..0x0042CDAC: the zone is read again, unchanged.
            ZoneFlood::Split => {
                hierarchy_usable &=
                    invalidate_zone_edge(inputs.level(level)?, route, zone, level, exclusions);
            }
        }
    }
    Ok(hierarchy_usable)
}

/// What `FloodFillReachableZones` found from one cell at one level.
#[derive(Debug, PartialEq, Eq)]
enum ZoneFlood {
    /// It returned 1: the zone holds a cell the flood did not reach.
    Split,
    /// It returned 0 with these: the zone's neighbours, its edges from the
    /// last, that no cell beside the flood belongs to.
    Unreached(Vec<ZoneId>),
}

/// `MapClass::FloodFillReachableZones @ 0x005840C0` from `start`, whose zone
/// at `level` is `zone`. A level-`n` zone lies in one aligned block
/// `2^(n+1)` cells wide, and the flood marks the cells it reaches by their
/// place in that block (`0x00ABDE48`, rows eight bytes apart). It steps
/// through the eight neighbours of each cell it pops (`0x00481810`): one the
/// Foot's `+0x1AC` refuses (`(cell, direction, cell level, NULL, 1)`, any
/// nonzero code) on a cell its row may cross is passed over; one in the zone
/// is flooded; one in another zone, not zero, is kept as reached.
fn flood_fill_reachable_zones(
    inputs: &RetryInputs<'_>,
    start: (i16, i16),
    level: usize,
    zone: ZoneId,
) -> Result<ZoneFlood, PathSearchFailure> {
    let terrain = inputs.terrain;
    let side: i16 = 1 << (level + 1);
    let mask = side - 1;
    let slot = |(x, y): (i16, i16)| ((x & mask) as usize, (y & mask) as usize);
    let mut visited = [[false; 8]; 8];
    let (x, y) = slot(start);
    visited[x][y] = true;
    // The cell stack at 0x00ABDA68 holds Cell pointers: a popped shared
    // Dummy steps from whatever coordinate it holds then.
    let mut stack = vec![terrain.native_cell_identity(start)];
    let mut reached: Vec<ZoneId> = Vec::new();
    // 0x005842C2..0x005842D2: a run of one zone is looked up once.
    let mut last_reached = ZONE_INVALID;
    while let Some(cell) = stack.pop() {
        for (direction, &(dx, dy)) in CELL_DELTAS.iter().enumerate() {
            let (x, y) = terrain.native_cell_coord(cell);
            let neighbor = terrain
                .native_cell_identity((x.wrapping_add(dx as i16), y.wrapping_add(dy as i16)));
            let coord = terrain.native_cell_coord(neighbor);
            let neighbor_zone = inputs.zone_at(level, coord)?;
            let code = inputs
                .foot_entry
                .classify(SearchEntryQuery {
                    from: None,
                    candidate: SearchEntryCandidate::CopiedCoord((coord.0 as u16, coord.1 as u16)),
                    direction: direction as i32,
                    path_height: i32::from(terrain.native_cell_ground_fields(neighbor).0 as i8),
                })
                .map_err(PathSearchFailure::CellEntryUnavailable)?;
            // 0x00584277..0x00584286
            let zone_type = terrain.native_cell_zone_type(neighbor);
            if code != 0 && is_passable_for_zone(zone_type, inputs.movement_zone) {
                continue;
            }
            if neighbor_zone == zone {
                let (x, y) = slot(coord);
                if !visited[x][y] {
                    visited[x][y] = true;
                    stack.push(neighbor);
                }
            } else if neighbor_zone != last_reached && neighbor_zone != ZONE_INVALID {
                last_reached = neighbor_zone;
                if !reached.contains(&neighbor_zone) {
                    reached.push(neighbor_zone);
                }
            }
        }
    }
    // 0x00584357..0x0058441C means to find a cell of the zone the flood
    // missed, but each round tests one cell: the start's coordinate plus its
    // own place in the block. So the zone counts as split when that cell is
    // in the playfield (mode 1) and in the zone, and the flood left any place
    // in the block unmarked.
    let probe = (
        start.0.wrapping_add(start.0 & mask),
        start.1.wrapping_add(start.1 & mask),
    );
    let side = side as usize;
    let unmarked = visited[..side]
        .iter()
        .any(|row| row[..side].contains(&false));
    if live_mode_one(terrain, inputs.bounds, (probe.0 as u16, probe.1 as u16))
        && inputs.zone_at(level, probe)? == zone
        && unmarked
    {
        return Ok(ZoneFlood::Split);
    }
    // 0x00584453..0x005844EF: the zone's record edges, from the last.
    Ok(ZoneFlood::Unreached(
        inputs
            .level(level)?
            .edges(zone)
            .iter()
            .rev()
            .map(|edge| edge.neighbor)
            .filter(|neighbor| !reached.contains(neighbor))
            .collect(),
    ))
}

/// `PathfinderClass::InvalidateZoneEdge @ 0x0042CF80` for `zone` at `level`:
/// exclude the precheck route's edge from `zone` to the next zone, or from
/// the zone before it when `zone` is the route's last, and the edges from
/// that edge's first zone to every zone beside both of its ends. Answers
/// false, clearing `PathfinderClass+0x38`, when the route has fewer than two
/// zones (`0x0042CF9A`) or does not hold `zone` (`0x0042CFD3`).
fn invalidate_zone_edge(
    graph: &ZoneLevelGraph,
    route: &[ZoneId],
    zone: ZoneId,
    level: usize,
    exclusions: &mut ZonePrecheckExclusions,
) -> bool {
    if route.len() < 2 {
        return false;
    }
    let Some(index) = route.iter().position(|&route_zone| route_zone == zone) else {
        return false;
    };
    let (from, to) = if index == route.len() - 1 {
        (route[index - 1], route[index])
    } else {
        (route[index], route[index + 1])
    };
    exclusions.insert(level, from, to);
    // 0x0042D096..0x0042D157, both edge lists from the last.
    for beside_to in graph.edges(to).iter().rev() {
        let beside = beside_to.neighbor;
        if beside == from {
            continue;
        }
        for beside_from in graph.edges(from).iter().rev() {
            if beside_from.neighbor == beside {
                exclusions.insert(level, from, beside);
            }
        }
    }
    true
}

#[cfg(test)]
#[path = "precheck_retry_tests.rs"]
mod precheck_retry_tests;
