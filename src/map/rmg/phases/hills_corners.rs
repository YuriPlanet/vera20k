//! The random map generator's side of the hills corner engine
//! ([`crate::map::corner_heights`]): its cells, the morph driver that pushes
//! each cell toward its walked height, the write-back of the finalized cells
//! and the 2x2 ramp-quad cleanup.

use crate::map::corner_heights::{CornerCells, CornerGrid, FinalCell};
use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TILE_UNASSIGNED;
use crate::util::native_x87::X87Chop53;

/// The RMG's corner-height clamp: 0 to 12 levels.
const MAX_HEIGHT: i32 = 180;
/// The corner heights the RMG's adjuster may write, inclusive.
pub const RMG_HEIGHTS: (i32, i32) = (0, MAX_HEIGHT);

/// Whether a tile permits morphing: the unassigned sentinel or a
/// `Morphable=yes` tileset (`IsoTileType+0x2E0 != 0`).
pub type Morphable<'a> = &'a dyn Fn(i32) -> bool;

/// The RMG's cells: its work grid and the height walk's protection flags.
pub struct RmgCornerCells<'a> {
    pub grid: &'a RmgGrid,
    pub scratch: &'a RmgScratch,
    pub morphable: Morphable<'a>,
}

impl CornerCells for RmgCornerCells<'_> {
    fn in_diamond(&self, x: i32, y: i32) -> bool {
        self.scratch.in_diamond(x, y)
    }

    fn level_slope(&self, x: i32, y: i32) -> (i32, u8) {
        self.grid
            .get(x, y)
            .map_or((0, 0), |cell| (i32::from(cell.level), cell.slope))
    }

    fn editable(&self, x: i32, y: i32) -> bool {
        if !self.scratch.in_diamond(x, y) {
            return false;
        }
        let Some(cell) = self.grid.get(x, y) else {
            return false;
        };
        cell.overlay == -1
            && !cell.occupied
            && !self.scratch.get(x, y).water_lock
            && (cell.tile == TILE_UNASSIGNED || (self.morphable)(cell.tile))
    }
}

/// Write [`CornerGrid::finalize`]'s results into the RMG work grid.
pub fn write_finals(grid: &mut RmgGrid, finals: &[FinalCell]) {
    for f in finals {
        let cell = grid.get_mut(f.x, f.y).expect("native cell");
        cell.level = f.level as u8;
        if let Some((slope, tile)) = f.slope_tile {
            cell.slope = slope;
            cell.tile = tile;
        }
    }
}

/// The driver morph loop: for every scratch cell push its corner heights toward
/// `level + walk_height` one level at a time.
pub fn morph(cells: &RmgCornerCells<'_>, cg: &mut CornerGrid) {
    let scratch = cells.scratch;
    let width = scratch.width();
    for k in 0..width * width {
        let (mx, my) = {
            let cell = scratch.cells()[k];
            (i32::from(cell.x), i32::from(cell.y))
        };
        if (mx, my) == (0, 0) {
            continue;
        }
        let Some(cell) = cells.grid.get(mx, my) else {
            continue;
        };
        let level = i32::from(cell.level);
        let height = scratch.cells()[k].height;
        let current = cg.current_level(cells, mx, my);
        let delta = X87Chop53::ftol_f64_low_masked(f64::from(level) + height - f64::from(current));
        let direction = if delta >= 0 { 1 } else { -1 };
        for _ in 0..delta.abs() {
            let mask = cg.pick_mask(mx, my, direction);
            cg.apply(direction, mx, my, mask);
        }
    }
}

/// The 2x2 ramp-quad cleanup: flatten quads that form the four halves of a
/// two-level step. Slope-5 quads `{5,6,7,8}` flatten in place; slope-11 quads
/// `{11,12,9,10}` flatten and raise a level.
pub fn quad_cleanup(grid: &mut RmgGrid) {
    const QUAD: [(i32, i32); 4] = [(0, 0), (1, 0), (1, 1), (0, 1)];
    for (x, y) in grid.native_cells().collect::<Vec<_>>() {
        let slope = grid.get(x, y).map_or(0, |c| c.slope);
        let (pattern, level_bump): ([u8; 4], u8) = if slope == 5 {
            ([5, 6, 7, 8], 0)
        } else if slope == 11 {
            ([11, 12, 9, 10], 1)
        } else {
            continue;
        };
        let matches = QUAD.iter().enumerate().all(|(i, &(dx, dy))| {
            grid.get(x + dx, y + dy)
                .is_some_and(|c| c.slope == pattern[i])
        });
        if !matches {
            continue;
        }
        for &(dx, dy) in &QUAD {
            if let Some(cell) = grid.get_mut(x + dx, y + dy) {
                cell.tile = TILE_UNASSIGNED;
                cell.sub_tile = 0;
                cell.slope = 0;
                cell.level = cell.level.wrapping_add(level_bump);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> (i32, i32, usize, i32) {
        let (map_w, map_h) = (24, 20);
        let stride = (map_w + map_h + 1) as usize;
        let span = map_w + map_h - 1;
        (map_w, map_h, stride, span)
    }

    fn build(
        grid: &RmgGrid,
        scratch: &RmgScratch,
        span: i32,
        morphable: Morphable<'_>,
    ) -> CornerGrid {
        let cells = RmgCornerCells {
            grid,
            scratch,
            morphable,
        };
        CornerGrid::build(&cells, (1, 1), span + 1, RMG_HEIGHTS)
    }

    fn world() -> (RmgGrid, RmgScratch, i32) {
        let (map_w, map_h, stride, span) = geometry();
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = TILE_UNASSIGNED;
        }
        (grid, scratch, span)
    }

    #[test]
    fn flat_land_builds_a_flat_unlocked_grid() {
        let (mut grid, scratch, span) = world();
        // All cells clear (unassigned) at level 4; morphable-permitting.
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().level = 4;
        }
        let morphable = |_tile: i32| true;
        let cg = build(&grid, &scratch, span, &morphable);
        // A cell in the diamond centre (x+y=44 between min 24 and max 64):
        // all four corners are owned by a level-4 cell -> 4*15 = 60.
        assert!(grid.is_valid(22, 22), "centre cell is in the diamond");
        for (height, locked) in cg.cell_corner_states(22, 22) {
            assert_eq!(height, 60, "level 4 -> height 60");
            assert!(!locked, "flat morphable land is unlocked");
        }
    }

    #[test]
    fn water_cells_lock_neighbouring_corners() {
        let (mut grid, scratch, span) = world();
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        // A water tile makes its corners lock (water is not morphable).
        grid.get_mut(20, 15).unwrap().tile = 500;
        let morphable = |tile: i32| tile == 0; // only clear morphs
        let cg = build(&grid, &scratch, span, &morphable);
        assert!(
            cg.cell_corner_states(20, 15)
                .iter()
                .all(|&(_, locked)| locked),
            "water cell corners are locked"
        );
    }

    #[test]
    fn locked_corners_roll_back_the_whole_adjustment() {
        let (mut grid, scratch, span) = world();
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        // Ring the target cell with water so every corner is locked.
        for (dx, dy) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
        ] {
            grid.get_mut(16 + dx, 12 + dy).unwrap().tile = 500;
        }
        let morphable = |tile: i32| tile == 0;
        let mut cg = build(&grid, &scratch, span, &morphable);
        let before = cg.heights();
        // Any raise attempt must roll back (locked neighbours block it).
        let mask = cg.pick_mask(16, 12, 1);
        cg.apply(1, 16, 12, mask);
        let after = cg.heights();
        assert_eq!(before, after, "a blocked adjustment leaves heights intact");
    }
}
