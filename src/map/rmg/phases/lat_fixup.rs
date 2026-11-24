//! LAT auto-tiling fixup over the generated grid (RNG-free).
//!
//! `CellClass::ApplyLAT_and_SlopeFixup @ 0x0047CA80` on every in-band cell,
//! in place: its LAT half ([`lat_fixed_tile_live`]: Rough, Sand, Green, then
//! Pave over the four cardinal neighbours), then its slope half
//! ([`slope_fixed_tile`]) from the raw `GridCell.slope` id. Off-band
//! neighbours read as the map-edge sentinel (tile 0), so a base cell at the
//! diamond edge always gets its edge variant. RMG has populated all slope ids
//! before these recalc passes, so one live-grid walk is sufficient for each
//! scheduled recalc.

use crate::map::lat::{LatConfig, SlopeFixupConfig, lat_fixed_tile_live, slope_fixed_tile};
use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::tiles::TileIds;

/// The four cardinal directions as `DIRECTION_DELTAS` indices (N, E, S, W), the
/// LAT mask's bit order.
const CARDINAL_DIRS: [usize; 4] = [0, 2, 4, 6];
/// Off-band / off-map neighbour tile (`Get_CellClass(off_map)` → tile 0).
const EDGE_SENTINEL: i32 = 0;

/// Apply the LAT and slope fixup to every in-band grid cell in place.
pub fn run(grid: &mut RmgGrid, ids: &TileIds) {
    let lat = LatConfig::from_tile_ids(ids);
    let slope = SlopeFixupConfig::from_tile_ids(ids);
    for (x, y) in grid.native_cells().collect::<Vec<_>>() {
        apply_cell(grid, &lat, slope, x, y);
    }
}

fn apply_cell(grid: &mut RmgGrid, lat: &LatConfig, slope: SlopeFixupConfig, x: i32, y: i32) {
    let Some(cell) = grid.get(x, y).copied() else {
        return;
    };
    let tile = lat_fixed_tile_live(cell.tile, lat, || {
        CARDINAL_DIRS.map(|dir| {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            grid.get(nx, ny)
                .map_or(EDGE_SENTINEL, |neighbor| neighbor.tile)
        })
    });
    let cardinal_slopes = CARDINAL_DIRS.map(|dir| {
        let (nx, ny) = RmgGrid::step(x, y, dir);
        grid.get(nx, ny).map_or(0, |neighbor| neighbor.slope)
    });
    let tile = slope_fixed_tile(tile, cell.slope, cardinal_slopes, slope);
    if let Some(cell) = grid.get_mut(x, y) {
        cell.tile = tile;
    }
}
