//! The hills corner-morph engine: a `(map_w+map_h)²` corner-height grid the
//! driver builds, morphs by adjusting corners ±1 level with recursive slope
//! propagation and all-or-nothing rollback, then finalizes into ramp tiles.
//!
//! Consumes NO RNG — a pure deterministic function of the pre-morph map plus
//! the walk's scratch heights. Geometry: origin `(1, 1)`, so a cell at map
//! coord `(mx, my)` maps to grid-local `(mx-1, my-1)`; `W = H = map_w+map_h-1`
//! and the grid is `(W+1)×(W+1)`. See `RMG_HILLS_CORNER_ENGINE_GHIDRA_REPORT`.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::{TILE_UNASSIGNED, TileIds};
use crate::map::rmg::x87;

/// One level of corner height (deltas in the ramp table are 0/15/30).
const LEVEL: i32 = 15;
/// Corner-height clamp: 12 levels.
const MAX_HEIGHT: i32 = 180;
/// Ramp-tile slope patterns, order `[NW, NE, SE, SW]`, units 1/15 level.
/// Verified byte-exact against `0x0083FF18` / `0x0083FDD8`.
const RAMP: [[i32; 4]; 19] = [
    [0, 0, 0, 0],
    [0, 15, 15, 0],
    [0, 0, 15, 15],
    [15, 0, 0, 15],
    [15, 15, 0, 0],
    [0, 0, 15, 0],
    [0, 0, 0, 15],
    [15, 0, 0, 0],
    [0, 15, 0, 0],
    [0, 15, 15, 15],
    [15, 0, 15, 15],
    [15, 15, 0, 15],
    [15, 15, 15, 0],
    [0, 15, 30, 15],
    [15, 0, 15, 30],
    [30, 15, 0, 15],
    [15, 30, 15, 0],
    [0, 15, 0, 15],
    [15, 0, 15, 0],
];

/// Whether a tile permits morphing: the unassigned sentinel or a
/// `Morphable=yes` tileset (`IsoTileType+0x2E0 != 0`).
pub type Morphable<'a> = &'a dyn Fn(i32) -> bool;

#[derive(Clone, Copy, Default)]
struct Corner {
    height: i32,
    locked: bool,
    visited: bool,
}

pub struct CornerGrid {
    /// Corners per row (`W+1`).
    stride: i32,
    corners: Vec<Corner>,
}

/// Undo entry: a corner index and its pre-morph height.
type Undo = Vec<(usize, i32)>;

impl CornerGrid {
    fn in_grid(&self, gx: i32, gy: i32) -> bool {
        gx >= 0 && gy >= 0 && gx < self.stride && gy < self.stride
    }

    fn idx(&self, gx: i32, gy: i32) -> usize {
        (gy * self.stride + gx) as usize
    }

    /// The 4 corner indices of the cell at map coord `(mx, my)`, ordered
    /// `[NW, NE, SE, SW]`.
    fn cell_corners(&self, mx: i32, my: i32) -> [usize; 4] {
        let (gx, gy) = (mx - 1, my - 1);
        let nw = self.idx(gx, gy);
        let s = self.stride as usize;
        [nw, nw + 1, nw + s + 1, nw + s]
    }

    /// Build the grid: seed every corner's height from its owner cell and lock
    /// corners that border un-morphable terrain.
    pub fn build(
        grid: &RmgGrid,
        scratch: &RmgScratch,
        span: i32,
        ids: &TileIds,
        morphable: Morphable<'_>,
    ) -> Self {
        let stride = span + 1;
        let mut cg = CornerGrid {
            stride,
            corners: vec![Corner::default(); (stride * stride) as usize],
        };
        for gy in 0..stride {
            for gx in 0..stride {
                // Grid-local (gx,gy) -> map coord (origin (1,1)).
                let (mx, my) = (gx + 1, gy + 1);
                // Owner: first in-diamond of own, N, NW, W.
                let candidates = [(mx, my), (mx, my - 1), (mx - 1, my - 1), (mx - 1, my)];
                let owner = candidates
                    .iter()
                    .copied()
                    .find(|&(cx, cy)| scratch.in_diamond(cx, cy));

                let index = cg.idx(gx, gy);
                let mut locked = owner.is_none();
                if let Some((ox, oy)) = owner {
                    let cell = grid.get(ox, oy).expect("in-diamond owner");
                    let slope = usize::from(cell.slope).min(RAMP.len() - 1);
                    cg.corners[index].height = RAMP[slope][0] + i32::from(cell.level) * LEVEL;
                }
                // Lock if any of N, NW, W, own is in-diamond and blocks morph.
                for &(cx, cy) in &[(mx, my - 1), (mx - 1, my - 1), (mx - 1, my), (mx, my)] {
                    if scratch.in_diamond(cx, cy)
                        && blocks_morph(grid, scratch, cx, cy, ids, morphable)
                    {
                        locked = true;
                    }
                }
                cg.corners[index].locked = locked;
            }
        }
        cg
    }

    /// The cell's current level: floor of the minimum corner height / 15 for
    /// editable cells, else the persisted level byte.
    pub fn current_level(
        &self,
        grid: &RmgGrid,
        scratch: &RmgScratch,
        mx: i32,
        my: i32,
        ids: &TileIds,
        morphable: Morphable<'_>,
    ) -> i32 {
        let corners = self.cell_corners(mx, my);
        let min = corners
            .iter()
            .map(|&c| self.corners[c].height)
            .min()
            .unwrap_or(0);
        if eligible(grid, scratch, mx, my, ids, morphable) {
            min / LEVEL
        } else {
            i32::from(grid.get(mx, my).map_or(0, |cell| cell.level))
        }
    }

    /// The corner mask (bits `NW=0, NE=1, SE=2, SW=3`): all-equal picks every
    /// unlocked corner, raise picks unlocked corners below the max, lower picks
    /// unlocked corners above the min.
    pub fn pick_mask(&self, mx: i32, my: i32, direction: i32) -> u32 {
        let corners = self.cell_corners(mx, my);
        let heights: [i32; 4] = std::array::from_fn(|i| self.corners[corners[i]].height);
        let locked: [bool; 4] = std::array::from_fn(|i| self.corners[corners[i]].locked);
        let min = *heights.iter().min().unwrap();
        let max = *heights.iter().max().unwrap();
        let mut mask = 0u32;
        for i in 0..4 {
            if locked[i] {
                continue;
            }
            let set = if max == min {
                true
            } else if direction > 0 {
                heights[i] < max
            } else {
                heights[i] > min
            };
            if set {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Apply one level of adjustment to the masked corners of a cell, with
    /// recursive propagation and all-or-nothing rollback. Returns whether it
    /// committed.
    pub fn apply(&mut self, direction: i32, mx: i32, my: i32, mask: u32) -> bool {
        // Remap the picker mask (NW,NE,SE,SW) to the 2x2 row-major iteration
        // order (NW,NE,SW,SE) by swapping the SE/SW bits.
        let m = (mask >> 1 & 4) | ((mask & 4) << 1) | (mask & 3);
        let (gx, gy) = (mx - 1, my - 1);
        let block = [(gx, gy), (gx + 1, gy), (gx, gy + 1), (gx + 1, gy + 1)];

        let mut undo: Undo = Vec::new();
        for (i, &(cgx, cgy)) in block.iter().enumerate() {
            if m & (1 << i) == 0 {
                continue;
            }
            let index = self.idx(cgx, cgy);
            if self.corners[index].locked {
                self.rollback(&undo);
                return false;
            }
            let new = self.corners[index].height + direction * LEVEL;
            self.corners[index].height = new;
            if !(0..=MAX_HEIGHT).contains(&new) {
                self.corners[index].height = new - direction * LEVEL;
            } else {
                undo.push((index, new - direction * LEVEL));
            }
            self.corners[index].visited = true;
            if !self.propagate(direction, cgx, cgy, &mut undo) {
                self.rollback(&undo);
                return false;
            }
        }
        true
    }

    /// Recursive slope propagation: any 8-neighbour corner differing from the
    /// just-moved corner by more than one level is pulled along by one level;
    /// a locked corner or grid edge that would need to move fails the whole op.
    fn propagate(&mut self, direction: i32, cgx: i32, cgy: i32, undo: &mut Undo) -> bool {
        let center = self.corners[self.idx(cgx, cgy)].height;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (cgx + dx, cgy + dy);
                if !self.in_grid(nx, ny) {
                    return false;
                }
                let nindex = self.idx(nx, ny);
                let diff = (self.corners[nindex].height - center).abs();
                if self.corners[nindex].locked {
                    if diff > LEVEL {
                        return false;
                    }
                    continue;
                }
                if diff <= LEVEL {
                    continue;
                }
                if direction == 1 && self.corners[nindex].height < center {
                    undo.push((nindex, self.corners[nindex].height));
                    self.corners[nindex].height = center - LEVEL;
                    self.corners[nindex].visited = true;
                } else if direction != 1 && self.corners[nindex].height > center {
                    undo.push((nindex, self.corners[nindex].height));
                    self.corners[nindex].height = center + LEVEL;
                    self.corners[nindex].visited = true;
                }
                if !self.propagate(direction, nx, ny, undo) {
                    return false;
                }
            }
        }
        true
    }

    /// Restore heights LIFO (visited marks stay set; finalize tolerates that).
    fn rollback(&mut self, undo: &Undo) {
        for &(index, old) in undo.iter().rev() {
            self.corners[index].height = old;
        }
    }

    /// Write levels, slopes, and ramp tiles for every cell whose corners were
    /// touched and whose spread is a single level.
    pub fn finalize(
        &self,
        grid: &mut RmgGrid,
        scratch: &RmgScratch,
        ids: &TileIds,
        morphable: Morphable<'_>,
    ) {
        for (mx, my) in grid.native_cells().collect::<Vec<_>>() {
            let corners = self.cell_corners(mx, my);
            if !corners.iter().any(|&c| self.corners[c].visited) {
                continue;
            }
            let heights: [i32; 4] = std::array::from_fn(|i| self.corners[corners[i]].height);
            let min = *heights.iter().min().unwrap();
            let max = *heights.iter().max().unwrap();
            if max - min >= 16 {
                continue;
            }
            if !eligible(grid, scratch, mx, my, ids, morphable) {
                continue;
            }
            let deltas: [i32; 4] = std::array::from_fn(|i| heights[i] - min);
            let cell = grid.get_mut(mx, my).expect("native cell");
            cell.level = (min / LEVEL) as u8;
            if let Some(slope) = RAMP.iter().position(|pattern| *pattern == deltas) {
                cell.slope = slope as u8;
                cell.tile = if slope == 0 {
                    ids.clear
                } else {
                    ids.ramp_base + slope as i32 - 1
                };
            }
        }
    }
}

/// A cell blocks morphing of an adjacent corner if it carries an overlay, an
/// occupier, the protected flag, or an un-morphable real tile.
fn blocks_morph(
    grid: &RmgGrid,
    scratch: &RmgScratch,
    x: i32,
    y: i32,
    _ids: &TileIds,
    morphable: Morphable<'_>,
) -> bool {
    let Some(cell) = grid.get(x, y) else {
        return false;
    };
    cell.overlay != -1
        || cell.occupied
        || scratch.get(x, y).water_lock
        || (cell.tile != TILE_UNASSIGNED && !morphable(cell.tile))
}

/// A cell is editable (its level/tile may be rewritten) when it is in-diamond,
/// unoccupied, overlay-free, unprotected, and holds a morphable/unassigned tile.
fn eligible(
    grid: &RmgGrid,
    scratch: &RmgScratch,
    mx: i32,
    my: i32,
    _ids: &TileIds,
    morphable: Morphable<'_>,
) -> bool {
    if !scratch.in_diamond(mx, my) {
        return false;
    }
    let Some(cell) = grid.get(mx, my) else {
        return false;
    };
    cell.overlay == -1
        && !cell.occupied
        && !scratch.get(mx, my).water_lock
        && (cell.tile == TILE_UNASSIGNED || morphable(cell.tile))
}

/// The driver morph loop: for every scratch cell push its corner heights toward
/// `level + walk_height` one level at a time.
pub fn morph(
    grid: &mut RmgGrid,
    scratch: &RmgScratch,
    cg: &mut CornerGrid,
    ids: &TileIds,
    morphable: Morphable<'_>,
) {
    let width = scratch.width();
    for k in 0..width * width {
        let (mx, my) = {
            let cell = scratch.cells()[k];
            (i32::from(cell.x), i32::from(cell.y))
        };
        if (mx, my) == (0, 0) {
            continue;
        }
        let Some(cell) = grid.get(mx, my) else {
            continue;
        };
        let level = i32::from(cell.level);
        let height = scratch.cells()[k].height;
        let current = cg.current_level(grid, scratch, mx, my, ids, morphable);
        let delta = x87::ftol(f64::from(level) + height - f64::from(current));
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
    use crate::map::theater::TheaterCliffRanges;

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: 600,
            ramp_smooth: 620,
            slope_set_pieces: -1,
            rough: -1,
            sand: -1,
            green: 100,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: 110,
            pave_lat: -1,
            pave: -1,
            water_base: 500,
            shore: 400,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            cliffs: TheaterCliffRanges::default(),
        }
    }

    fn geometry() -> (i32, i32, usize, i32) {
        let (map_w, map_h) = (24, 20);
        let stride = (map_w + map_h + 1) as usize;
        let span = map_w + map_h - 1;
        (map_w, map_h, stride, span)
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
    fn ramp_table_matches_the_verified_bytes() {
        assert_eq!(RAMP[0], [0, 0, 0, 0]);
        assert_eq!(RAMP[1], [0, 15, 15, 0]);
        assert_eq!(RAMP[13], [0, 15, 30, 15]);
        assert_eq!(RAMP[18], [15, 0, 15, 0]);
        assert_eq!(RAMP.len(), 19);
    }

    #[test]
    fn corner_indexing_shares_edges_between_cells() {
        let (_grid, _scratch, span) = world();
        let cg = CornerGrid {
            stride: span + 1,
            corners: vec![Corner::default(); ((span + 1) * (span + 1)) as usize],
        };
        // A cell's SE corner is its eastern neighbour's SW corner.
        let a = cg.cell_corners(10, 10);
        let b = cg.cell_corners(11, 10);
        assert_eq!(a[2], b[3], "SE of (10,10) == SW of (11,10)");
        // and its southern neighbour's NW corner is its own SW corner.
        let c = cg.cell_corners(10, 11);
        assert_eq!(a[3], c[0], "SW of (10,10) == NW of (10,11)");
    }

    #[test]
    fn flat_land_builds_a_flat_unlocked_grid() {
        let (mut grid, scratch, span) = world();
        // All cells clear (unassigned) at level 4; morphable-permitting.
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().level = 4;
        }
        let morphable = |_tile: i32| true;
        let cg = CornerGrid::build(&grid, &scratch, span, &ids(), &morphable);
        // A cell in the diamond centre (x+y=44 between min 24 and max 64):
        // all four corners are owned by a level-4 cell -> 4*15 = 60.
        assert!(grid.is_valid(22, 22), "centre cell is in the diamond");
        let interior = cg.cell_corners(22, 22);
        for &c in &interior {
            assert_eq!(cg.corners[c].height, 60, "level 4 -> height 60");
            assert!(!cg.corners[c].locked, "flat morphable land is unlocked");
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
        let cg = CornerGrid::build(&grid, &scratch, span, &ids(), &morphable);
        let corners = cg.cell_corners(20, 15);
        assert!(
            corners.iter().all(|&c| cg.corners[c].locked),
            "water cell corners are locked"
        );
    }

    #[test]
    fn a_raised_cell_becomes_a_ramp_or_plateau() {
        let (mut grid, mut scratch, span) = world();
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().level = 0;
        }
        // Deposit a height of 2 on one interior cell; the walk would have
        // truncated it to a whole number.
        scratch.get_mut(16, 12).height = 2.0;
        let identity = ids();
        let morphable = |_tile: i32| true;
        let mut cg = CornerGrid::build(&grid, &scratch, span, &identity, &morphable);
        morph(&mut grid, &scratch, &mut cg, &identity, &morphable);
        cg.finalize(&mut grid, &scratch, &identity, &morphable);
        // The raised cell (and its surroundings) gained level / ramp tiles.
        let touched = grid
            .native_cells()
            .filter(|&(x, y)| {
                let cell = grid.get(x, y).unwrap();
                cell.level > 0 || cell.slope > 0
            })
            .count();
        assert!(touched > 0, "raising a cell reshapes the terrain");
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
        let mut cg = CornerGrid::build(&grid, &scratch, span, &ids(), &morphable);
        let before: Vec<i32> = cg.corners.iter().map(|c| c.height).collect();
        // Any raise attempt must roll back (locked neighbours block it).
        let mask = cg.pick_mask(16, 12, 1);
        cg.apply(1, 16, 12, mask);
        let after: Vec<i32> = cg.corners.iter().map(|c| c.height).collect();
        assert_eq!(before, after, "a blocked adjustment leaves heights intact");
    }

    #[test]
    fn engine_is_deterministic() {
        let snapshot = || {
            let (mut grid, mut scratch, span) = world();
            for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                grid.get_mut(x, y).unwrap().level = 1;
            }
            for &(x, y) in &[(14, 12), (18, 14), (16, 18)] {
                scratch.get_mut(x, y).height = 2.0;
            }
            let identity = ids();
            let morphable = |_tile: i32| true;
            let mut cg = CornerGrid::build(&grid, &scratch, span, &identity, &morphable);
            morph(&mut grid, &scratch, &mut cg, &identity, &morphable);
            cg.finalize(&mut grid, &scratch, &identity, &morphable);
            quad_cleanup(&mut grid);
            grid.native_cells()
                .map(|(x, y)| {
                    let cell = grid.get(x, y).unwrap();
                    (cell.tile, cell.level, cell.slope)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(snapshot(), snapshot());
    }
}
