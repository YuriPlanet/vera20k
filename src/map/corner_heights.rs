//! The hills corner-height engine: a grid of cell-corner heights that moves
//! corners one level at a time, pulls neighbouring corners along so no slope
//! is steeper than one level per cell, rolls a whole adjustment back when a
//! corner it needs cannot move, and finalizes the corners into each cell's
//! Level, slope and clear or ramp tile.
//!
//! Native: `RandomMapGenerator::CreateHills @ 0x005A35F0` is the only caller
//! of its build (`0x006B2A70`), level (`0x006B4100`), mask (`0x006B4240`),
//! adjust (`0x006B3E60`, propagating through `0x006B3A80`) and finalize
//! (`0x006B3850`) helpers, which read and write `CellClass` data. The random
//! map generator runs them over its whole diamond (`rmg::phases::hills`);
//! terrain deformation runs them over a window of a live map.
//! [`CornerCells`] is the cell data each reads.
//!
//! Consumes no RNG. Geometry: corner `(gx, gy)` is the NW corner of map cell
//! `origin + (gx, gy)`.

/// One level of corner height (deltas in the ramp table are 0/15/30).
pub const LEVEL: i32 = 15;
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

/// The cell data the engine reads.
pub trait CornerCells {
    /// `RandomMapGenerator::CellIsInsideDiamond @ 0x005AC230`: the cell is
    /// on the map.
    fn in_diamond(&self, x: i32, y: i32) -> bool;
    /// An in-diamond cell's signed Level (`CellClass+0x11B`) and slope
    /// (`+0x11C`).
    fn level_slope(&self, x: i32, y: i32) -> (i32, u8);
    /// `0x006B2520`: the cell may be reshaped. It is in the diamond, holds no
    /// overlay (`+0x44 == -1`) and no object (`+0xE4` FirstObject null), is
    /// not protected, and its tile is unassigned or of a `Morphable=` set.
    fn editable(&self, x: i32, y: i32) -> bool;
}

/// One cell [`CornerGrid::finalize`] rewrites: its level and, when its
/// corners form a ramp pattern, that slope and its tile (the clear tile or
/// `RampBase + slope - 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalCell {
    pub x: i32,
    pub y: i32,
    pub level: i32,
    pub slope_tile: Option<(u8, i32)>,
}

/// The tiles [`CornerGrid::finalize`] picks between: the theater's clear tile
/// and its first ramp tile (`RampBase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalTiles {
    pub clear: i32,
    pub ramp_base: i32,
}

#[derive(Clone, Copy, Default)]
struct Corner {
    height: i32,
    locked: bool,
    visited: bool,
}

pub struct CornerGrid {
    /// The map cell whose NW corner is grid corner `(0, 0)`.
    origin: (i32, i32),
    /// Corners per row and per column.
    stride: i32,
    /// The lowest and highest height the adjuster writes.
    heights: (i32, i32),
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
        let (gx, gy) = (mx - self.origin.0, my - self.origin.1);
        let nw = self.idx(gx, gy);
        let s = self.stride as usize;
        [nw, nw + 1, nw + s + 1, nw + s]
    }

    /// `0x006B2A70`: build the grid, seeding every corner's height from its
    /// owner cell and locking corners that border cells the engine may not
    /// reshape. `stride` corners per side start at `origin`'s NW corner;
    /// `heights` bounds what [`Self::apply`] writes.
    pub fn build(
        cells: &impl CornerCells,
        origin: (i32, i32),
        stride: i32,
        heights: (i32, i32),
    ) -> Self {
        let mut cg = CornerGrid {
            origin,
            stride,
            heights,
            corners: vec![Corner::default(); (stride * stride) as usize],
        };
        for gy in 0..stride {
            for gx in 0..stride {
                let (mx, my) = (gx + origin.0, gy + origin.1);
                // Owner: first in-diamond of own, N, NW, W.
                let candidates = [(mx, my), (mx, my - 1), (mx - 1, my - 1), (mx - 1, my)];
                let owner = candidates
                    .iter()
                    .copied()
                    .find(|&(cx, cy)| cells.in_diamond(cx, cy));

                let index = cg.idx(gx, gy);
                let mut locked = owner.is_none();
                if let Some((ox, oy)) = owner {
                    let (level, slope) = cells.level_slope(ox, oy);
                    let slope = usize::from(slope).min(RAMP.len() - 1);
                    cg.corners[index].height = RAMP[slope][0] + level * LEVEL;
                }
                // Lock if any of N, NW, W, own is in-diamond and not editable.
                for &(cx, cy) in &[(mx, my - 1), (mx - 1, my - 1), (mx - 1, my), (mx, my)] {
                    if cells.in_diamond(cx, cy) && !cells.editable(cx, cy) {
                        locked = true;
                    }
                }
                cg.corners[index].locked = locked;
            }
        }
        cg
    }

    /// `0x006B4100`: the cell's current level, the floor of its minimum
    /// corner height / 15 for editable cells, else its Level byte.
    pub fn current_level(&self, cells: &impl CornerCells, mx: i32, my: i32) -> i32 {
        let corners = self.cell_corners(mx, my);
        let min = corners
            .iter()
            .map(|&c| self.corners[c].height)
            .min()
            .unwrap_or(0);
        if cells.editable(mx, my) {
            min / LEVEL
        } else if cells.in_diamond(mx, my) {
            cells.level_slope(mx, my).0
        } else {
            0
        }
    }

    /// `0x006B4240`: the corner mask (bits `NW=0, NE=1, SE=2, SW=3`): all-equal picks every
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

    /// `0x006B3E60`: apply one level of adjustment to the masked corners of a
    /// cell, with recursive propagation and all-or-nothing rollback. A corner
    /// the move would take outside the grid's height range stays put. Returns
    /// whether it committed.
    pub fn apply(&mut self, direction: i32, mx: i32, my: i32, mask: u32) -> bool {
        // Remap the picker mask (NW,NE,SE,SW) to the 2x2 row-major iteration
        // order (NW,NE,SW,SE) by swapping the SE/SW bits.
        let m = (mask >> 1 & 4) | ((mask & 4) << 1) | (mask & 3);
        let (gx, gy) = (mx - self.origin.0, my - self.origin.1);
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
            if !(self.heights.0..=self.heights.1).contains(&new) {
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

    /// `0x006B3A80`, recursive slope propagation: any 8-neighbour corner differing from the
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

    /// `0x006B3850`: the level, slope and ramp tile of every editable cell
    /// of `domain` whose corners were touched and whose spread is a single
    /// level. Each result depends only on the corners and its own cell, so
    /// the caller writes them afterwards in any order.
    pub fn finalize(
        &self,
        cells: &impl CornerCells,
        domain: impl IntoIterator<Item = (i32, i32)>,
        tiles: FinalTiles,
    ) -> Vec<FinalCell> {
        let mut finals = Vec::new();
        for (mx, my) in domain {
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
            if !cells.editable(mx, my) {
                continue;
            }
            let deltas: [i32; 4] = std::array::from_fn(|i| heights[i] - min);
            let slope_tile = RAMP
                .iter()
                .position(|pattern| *pattern == deltas)
                .map(|slope| {
                    let tile = if slope == 0 {
                        tiles.clear
                    } else {
                        tiles.ramp_base + slope as i32 - 1
                    };
                    (slope as u8, tile)
                });
            finals.push(FinalCell {
                x: mx,
                y: my,
                level: min / LEVEL,
                slope_tile,
            });
        }
        finals
    }
}

#[cfg(test)]
impl CornerGrid {
    /// The corners of the cell at `(mx, my)`, `[NW, NE, SE, SW]`, as their
    /// height and whether they are locked.
    pub(crate) fn cell_corner_states(&self, mx: i32, my: i32) -> [(i32, bool); 4] {
        self.cell_corners(mx, my)
            .map(|c| (self.corners[c].height, self.corners[c].locked))
    }

    /// Every corner's height, row by row.
    pub(crate) fn heights(&self) -> Vec<i32> {
        self.corners.iter().map(|c| c.height).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILES: FinalTiles = FinalTiles {
        clear: 0,
        ramp_base: 600,
    };

    #[test]
    fn ramp_table_matches_the_verified_bytes() {
        assert_eq!(RAMP[0], [0, 0, 0, 0]);
        assert_eq!(RAMP[1], [0, 15, 15, 0]);
        assert_eq!(RAMP[13], [0, 15, 30, 15]);
        assert_eq!(RAMP[18], [15, 0, 15, 0]);
        assert_eq!(RAMP.len(), 19);
    }

    /// Flat editable ground at `level`, `size` cells square.
    struct Field {
        size: i32,
        level: i32,
    }

    impl CornerCells for Field {
        fn in_diamond(&self, x: i32, y: i32) -> bool {
            (0..self.size).contains(&x) && (0..self.size).contains(&y)
        }

        fn level_slope(&self, _x: i32, _y: i32) -> (i32, u8) {
            (self.level, 0)
        }

        fn editable(&self, x: i32, y: i32) -> bool {
            self.in_diamond(x, y)
        }
    }

    #[test]
    fn lowering_level_zero_ground_needs_a_height_range_below_zero() {
        let field = Field { size: 9, level: 0 };
        let lower = |heights: (i32, i32)| {
            let mut cg = CornerGrid::build(&field, (0, 0), 10, heights);
            let mask = cg.pick_mask(4, 4, -1);
            assert!(cg.apply(-1, 4, 4, mask));
            let domain = (0..9).flat_map(|y| (0..9).map(move |x| (x, y)));
            cg.finalize(&field, domain, TILES)
        };
        // The random map generator's range, 0 to 12 levels, keeps every
        // corner at 0: the 3x3 stays flat.
        let flat = lower((0, 12 * LEVEL));
        assert_eq!(flat.len(), 9);
        assert!(
            flat.iter()
                .all(|f| (f.level, f.slope_tile) == (0, Some((0, TILES.clear))))
        );
        // Below zero the cell sinks a level and its neighbours ramp to it.
        let pit = lower((-4 * LEVEL, 12 * LEVEL));
        let at = |x: i32, y: i32| pit.iter().find(|f| (f.x, f.y) == (x, y)).copied();
        assert_eq!(pit.len(), 9);
        assert_eq!(
            at(4, 4).map(|f| (f.level, f.slope_tile)),
            Some((-1, Some((0, TILES.clear))))
        );
        // North side: its SE and SW corners sank, [15, 15, 0, 0] is slope 4.
        assert_eq!(
            at(4, 3).map(|f| (f.level, f.slope_tile)),
            Some((-1, Some((4, TILES.ramp_base + 3))))
        );
        // North-east corner: only its SW corner sank, slope 12.
        assert_eq!(
            at(5, 3).map(|f| (f.level, f.slope_tile)),
            Some((-1, Some((12, TILES.ramp_base + 11))))
        );
    }
}
