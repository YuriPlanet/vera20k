//! Diamond-ring exit-cell search for Mission_Harvest state 4 (off the
//! refinery cell) and the tank-bunker exit.
//!
//! VERA-internal approximation of `MapClass::Find_Nearby_Passable_Cell @
//! 0x0056DC20`: the native range and flag arguments are not modelled. The
//! native port is `sim::find_nearby_cell`; these two callers have not moved
//! onto it yet.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/pathfinding, sim/occupancy.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::PathGrid;

/// Maximum diamond-ring radius for the exit-cell spiral search. gamemd's
/// `FootClass::Find_Nearby_Passable_Cell` derives its cap from
/// `Speed + SightRange` (capped at 32). A miner-class unit lands at ~14.
/// 16 covers the same footprint with a small safety margin and still
/// terminates quickly when the area around the refinery is fully blocked.
pub(super) const EXIT_SEARCH_MAX_RADIUS: i32 = 16;

/// Whether `(x, y)` is in-bounds, passable on the ground layer, and not
/// occupied by any other ground-layer entity.
///
/// Bridge layer is intentionally ignored: refinery exit cells must drop the
/// miner on land. `occupancy` may be `None` when only path passability is
/// known (used by direct unit tests of the spiral algorithm).
fn is_exit_cell_passable(
    x: i32,
    y: i32,
    grid: &PathGrid,
    occupancy: Option<&OccupancyGrid>,
) -> bool {
    if x < 0 || y < 0 || x >= grid.width() as i32 || y >= grid.height() as i32 {
        return false;
    }
    let cx = x as u16;
    let cy = y as u16;
    if !grid.is_walkable(cx, cy) {
        return false;
    }
    occupancy.is_none_or(|occ| occ.is_empty_on_layer(cx, cy, MovementLayer::Ground))
}

/// Diamond-ring spiral that collects ALL passable cells in the first
/// non-empty ring, mirroring gamemd's `FootClass::Find_Nearby_Passable_Cell`
/// (0x56DC20) candidate-collection block. The original engine then picks
/// from the collected pool via `g_CurrentFrameCounter % count` — caller
/// supplies the equivalent index.
///
/// Why this matters: a deterministic "return first walkable" picks the
/// same cell every time, which is visually fine when the anchor itself is
/// passable, but produces "miner always exits at the same spot" drift when
/// a conditional release anchor is blocked and several ring-1 candidates
/// exist.
/// The modulo selection over the ring's candidates spreads exits across
/// the available cells the way gamemd does.
///
/// Returns the chosen cell, or `None` if no ring within `max_radius`
/// produces a passable cell. The selection is `candidates[index % count]`,
/// matching gamemd's modulo-based pick.
pub(crate) fn find_nearby_passable_cell_with_index(
    ox: i32,
    oy: i32,
    grid: &PathGrid,
    occupancy: Option<&OccupancyGrid>,
    max_radius: i32,
    index: u64,
) -> Option<(u16, u16)> {
    // Ring 0: the anchor itself. If passable, it's the sole candidate.
    if is_exit_cell_passable(ox, oy, grid, occupancy) {
        return Some((ox as u16, oy as u16));
    }
    let mut candidates: Vec<(u16, u16)> = Vec::with_capacity(24);
    for r in 1..=max_radius {
        candidates.clear();
        // Segment 1: top + bottom rows.
        for delta in -r..=r {
            if is_exit_cell_passable(ox + delta, oy - r, grid, occupancy) {
                candidates.push(((ox + delta) as u16, (oy - r) as u16));
            }
            if is_exit_cell_passable(ox + delta, oy + r, grid, occupancy) {
                candidates.push(((ox + delta) as u16, (oy + r) as u16));
            }
        }
        // Segment 2: left + right columns (corners already covered by segment 1).
        for delta in (1 - r)..=(r - 1) {
            if is_exit_cell_passable(ox - r, oy + delta, grid, occupancy) {
                candidates.push(((ox - r) as u16, (oy + delta) as u16));
            }
            if is_exit_cell_passable(ox + r, oy + delta, grid, occupancy) {
                candidates.push(((ox + r) as u16, (oy + delta) as u16));
            }
        }
        if !candidates.is_empty() {
            // gamemd's `local_60[g_CurrentFrameCounter % count]` selection.
            let pick = (index as usize) % candidates.len();
            return Some(candidates[pick]);
        }
    }
    None
}
