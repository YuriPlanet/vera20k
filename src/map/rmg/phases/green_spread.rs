//! Green-LAT spread: green terrain grows into adjacent clear cells.
//!
//! Runs after region partition and before the first attribute recalc. The
//! candidate list is collected once, then up to `min(len / 3, 1000)` random
//! entries are converted to green, each conversion re-feeding the list with
//! the converted cell's clear cardinal neighbors.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::tiles::TileIds;

use super::CellRef;

/// Conversion cap per map.
const SPREAD_CAP: i32 = 1000;
/// The four even directions (N, E, S, W) the spread steps in.
const EVEN_DIRS: [usize; 4] = [0, 2, 4, 6];

pub fn run(grid: &mut RmgGrid, ids: &TileIds, rng: &mut RmgRng) {
    // Collect: every green cell's clear cardinal neighbors, in native scan
    // order. Duplicates are kept — the original appends without dedup.
    let mut list: Vec<CellRef> = Vec::new();
    let coords: Vec<(i32, i32)> = grid.native_cells().collect();
    for (x, y) in coords {
        let tile = grid
            .get(x, y)
            .expect("native_cells yields valid cells only")
            .tile;
        if ids.is_green_lat(tile) {
            append_clear_neighbors(grid, ids, x, y, &mut list);
        }
    }

    let count = (list.len() as i32 / 3).min(SPREAD_CAP);
    for _ in 0..count {
        // The draw scales by the LIVE list length. The list can only shrink
        // net-zero or grow here, never empty (each iteration removes one and
        // the initial length is at least 3 * count).
        let index = rng.uniform(0, list.len() as i32 - 1) as usize;
        let entry = list.remove(index);

        // Paint through the held reference: a border entry writes the shared
        // border cell no matter what coordinate it was appended from.
        match entry {
            CellRef::Cell(x, y) => {
                grid.get_mut(i32::from(x), i32::from(y))
                    .expect("list entries reference existing cells")
                    .tile = ids.green;
            }
            CellRef::Border => grid.border_cell_mut().tile = ids.green,
        }

        // Re-scan the converted cell's cardinal neighbors. The base
        // coordinate is re-read from the cell each step — for the border
        // cell that is its live coordinate slot, which an out-of-band step
        // in this very loop can move.
        for dir in EVEN_DIRS {
            let (bx, by) = match entry {
                CellRef::Cell(x, y) => (i32::from(x), i32::from(y)),
                CellRef::Border => grid.border_coord(),
            };
            let (nx, ny) = RmgGrid::step(bx, by, dir);
            let clear = ids.is_clear(grid.cell_native(nx, ny).tile);
            if clear {
                list.push(CellRef::at(grid, nx, ny));
            }
        }
    }
}

fn append_clear_neighbors(
    grid: &mut RmgGrid,
    ids: &TileIds,
    x: i32,
    y: i32,
    list: &mut Vec<CellRef>,
) {
    for dir in EVEN_DIRS {
        let (nx, ny) = RmgGrid::step(x, y, dir);
        if ids.is_clear(grid.cell_native(nx, ny).tile) {
            list.push(CellRef::at(grid, nx, ny));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::map::rmg::tiles::SpecialTerrain;

    /// Minimal identity table: green base 100, green LAT 110, everything else
    /// far away so no accidental range overlaps.
    fn test_ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: -1,
            ramp_smooth: -1,
            slope_set_pieces: -1,
            rough: -1,
            sand: 200,
            green: 100,
            rough_lat: -1,
            sand_lat: 210,
            green_lat: 110,
            pave_lat: -1,
            pave: -1,
            water_base: 300,
            shore: -1,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            special: SpecialTerrain::default(),
        }
    }

    fn grid_with_water() -> RmgGrid {
        let mut grid = RmgGrid::new(32, 8, 24);
        let coords: Vec<(i32, i32)> = grid.native_cells().collect();
        for (x, y) in coords {
            grid.get_mut(x, y).unwrap().tile = 300;
        }
        grid
    }

    fn paint(grid: &mut RmgGrid, cells: &[(i32, i32)], tile: i32) {
        for &(x, y) in cells {
            grid.get_mut(x, y).unwrap().tile = tile;
        }
    }

    #[test]
    fn spread_is_deterministic_per_seed() {
        let make = || {
            let mut grid = grid_with_water();
            paint(&mut grid, &[(10, 8), (12, 10)], 100);
            let clears: Vec<(i32, i32)> =
                (8..15).flat_map(|x| (7..12).map(move |y| (x, y))).collect();
            let clears: Vec<(i32, i32)> = clears
                .into_iter()
                .filter(|&(x, y)| grid.is_valid(x, y) && grid.get(x, y).unwrap().tile == 300)
                .collect();
            paint(&mut grid, &clears, 0);
            grid
        };
        let ids = test_ids();

        let mut first = make();
        run(&mut first, &ids, &mut RmgRng::new(99));
        let mut second = make();
        run(&mut second, &ids, &mut RmgRng::new(99));

        let tiles = |grid: &RmgGrid| -> Vec<i32> {
            grid.native_cells()
                .collect::<Vec<_>>()
                .iter()
                .map(|&(x, y)| grid.get(x, y).unwrap().tile)
                .collect()
        };
        assert_eq!(tiles(&first), tiles(&second));
    }
}
