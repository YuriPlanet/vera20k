//! The generator's working cell grid and the native cell-iteration order.
//!
//! Phases mutate `GridCell`s (the subset of cell state the generator reads or
//! writes); `emit` projects the grid into a `MapFile` at the end. Iteration
//! order is load-bearing: every per-cell RNG draw happens in `DiamondScan`
//! order, so a different traversal silently reorders the whole draw stream.

use super::tiles::TILE_UNASSIGNED;

/// Clockwise-from-north neighbor offsets, index = direction code 0..7.
pub const DIRECTION_OFFSETS: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// One generated cell — the fields the generation phases touch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridCell {
    /// Flat tile index; 0 = clear ground, `TILE_UNASSIGNED` = untouched.
    pub tile: i32,
    /// Sub-tile index within a multi-cell tile block (0 = anchor/unset).
    pub sub_tile: u8,
    /// Ground level.
    pub level: u8,
    /// Native raw slope id (0 flat; generated ramps use values through at least 16).
    pub slope: u8,
    /// Overlay type index, -1 = none.
    pub overlay: i32,
    /// Overlay density/frame byte.
    pub density: u8,
    /// An object occupies this cell (tree, tech building).
    pub occupied: bool,
    /// Start-cell marker set when a waypoint lands here.
    pub start_marker: bool,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            tile: TILE_UNASSIGNED,
            sub_tile: 0,
            // The map-prep stage initialises generated cells to level 4.
            level: 4,
            slope: 0,
            overlay: -1,
            density: 0,
            occupied: false,
            start_marker: false,
        }
    }
}

/// Working grid, indexed by the same linear (x, y) scheme as the scratch
/// array. Cells outside the map's valid band read as `None`, which is what
/// terminates the native iteration.
#[derive(Debug, Clone)]
pub struct RmgGrid {
    width: usize,
    diamond_min: i32,
    diamond_max: i32,
    cells: Vec<GridCell>,
    /// The shared out-of-band fallback cell. The original returns one static
    /// cell for every invalid lookup, writing the requested coordinate into
    /// it first — so out-of-band accesses alias each other, and a phase that
    /// paints the fallback affects every later out-of-band read. Its tile
    /// starts at 0 (zero-initialised static), which the clear-tile test
    /// accepts.
    border: GridCell,
    border_coord: (i16, i16),
}

impl RmgGrid {
    pub fn new(width: usize, diamond_min: i32, diamond_max: i32) -> Self {
        Self {
            width,
            diamond_min,
            diamond_max,
            cells: vec![GridCell::default(); width * width],
            border: GridCell {
                tile: 0,
                level: 0,
                ..GridCell::default()
            },
            border_coord: (0, 0),
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    #[cfg(test)]
    pub fn diamond_bounds(&self) -> (i32, i32) {
        (self.diamond_min, self.diamond_max)
    }

    /// Whether a cell exists at this coordinate — the allocation predicate
    /// the original uses when building the map (asymmetric diamond band).
    pub fn is_valid(&self, x: i32, y: i32) -> bool {
        self.diamond_min < x + y
            && x - y < self.diamond_min
            && y - x < self.diamond_min
            && x + y <= self.diamond_max
            && self.in_bounds(x, y)
    }

    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.width
    }

    pub fn get(&self, x: i32, y: i32) -> Option<&GridCell> {
        self.is_valid(x, y)
            .then(|| &self.cells[y as usize * self.width + x as usize])
    }

    pub fn get_mut(&mut self, x: i32, y: i32) -> Option<&mut GridCell> {
        self.is_valid(x, y)
            .then(move || &mut self.cells[y as usize * self.width + x as usize])
    }

    /// Cell lookup with the original's fallback semantics: an invalid
    /// coordinate yields the shared border cell after stamping the requested
    /// coordinate into it.
    pub fn cell_native(&mut self, x: i32, y: i32) -> &GridCell {
        if self.is_valid(x, y) {
            &self.cells[y as usize * self.width + x as usize]
        } else {
            self.border_coord = (x as i16, y as i16);
            &self.border
        }
    }

    /// Mutable variant of [`Self::cell_native`].
    pub fn cell_native_mut(&mut self, x: i32, y: i32) -> &mut GridCell {
        if self.is_valid(x, y) {
            &mut self.cells[y as usize * self.width + x as usize]
        } else {
            self.border_coord = (x as i16, y as i16);
            &mut self.border
        }
    }

    /// The coordinate most recently stamped into the border cell — what the
    /// original reads back from the fallback's coordinate field.
    pub fn border_coord(&self) -> (i32, i32) {
        (
            i32::from(self.border_coord.0),
            i32::from(self.border_coord.1),
        )
    }

    /// Direct access to the border cell WITHOUT stamping a coordinate — for
    /// writes made through a retained pointer rather than a fresh lookup.
    pub fn border_cell_mut(&mut self) -> &mut GridCell {
        &mut self.border
    }

    /// Neighbor coordinate one step in direction `dir` (0..7, masked like the
    /// original's `& 7`).
    pub fn step(x: i32, y: i32, dir: usize) -> (i32, i32) {
        let (dx, dy) = DIRECTION_OFFSETS[dir & 7];
        (x + i32::from(dx), y + i32::from(dy))
    }

    /// All existing cells in native scan order. The scan's rows always stay
    /// inside the off-axis bounds, so the first coordinate past the far
    /// diagonal is where the original's null-cell check stops it.
    pub fn native_cells(&self) -> NativeCells {
        NativeCells {
            scan: DiamondScan::new(self.diamond_min),
            diamond_max: self.diamond_max,
        }
    }
}

/// Iterator over valid cell coordinates in native order.
pub struct NativeCells {
    scan: DiamondScan,
    diamond_max: i32,
}

impl Iterator for NativeCells {
    type Item = (i32, i32);

    fn next(&mut self) -> Option<(i32, i32)> {
        let (x, y) = self.scan.next_coord();
        (x + y <= self.diamond_max).then_some((x, y))
    }
}

/// The native cell-iteration order: anti-diagonal rows of the isometric map.
///
/// State machine transcribed from the original iterator: start at
/// `(x=1, y=d)`, walk each row by `(x+1, y-1)`, and when a row of `rem + 1`
/// cells is exhausted start the next row from the swapped coordinate with a
/// parity rule choosing between `x = old_y + 1` (row length `d`) and
/// `y = old_x + 1` (row length `d - 1`... `d`). The machine itself never
/// stops; the caller stops at the first coordinate that has no cell.
#[derive(Debug, Clone)]
pub struct DiamondScan {
    d: i32,
    x: i32,
    y: i32,
    rem: i32,
}

impl DiamondScan {
    pub fn new(d: i32) -> Self {
        Self {
            d,
            x: 1,
            y: d,
            rem: d - 1,
        }
    }

    /// The next coordinate in native order. Infinite by design — the caller
    /// owns termination (the original stops on the first null cell).
    pub fn next_coord(&mut self) -> (i32, i32) {
        let current = (self.x, self.y);
        if self.rem != 0 {
            self.x += 1;
            self.y -= 1;
            self.rem -= 1;
        } else {
            let (old_x, old_y) = (self.x, self.y);
            // Swap, then nudge one axis by parity of (x + y - d - 1).
            self.x = old_y;
            self.y = old_x;
            if (old_y + old_x - self.d - 1) & 1 == 0 {
                self.rem = self.d - 2;
                self.x = old_y + 1;
            } else {
                self.rem = self.d - 1;
                self.y = old_x + 1;
            }
        }
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_offsets_are_clockwise_from_north() {
        assert_eq!(DIRECTION_OFFSETS[0], (0, -1), "N");
        assert_eq!(DIRECTION_OFFSETS[2], (1, 0), "E");
        assert_eq!(DIRECTION_OFFSETS[4], (0, 1), "S");
        assert_eq!(DIRECTION_OFFSETS[6], (-1, 0), "W");
        assert_eq!(DIRECTION_OFFSETS[3], (1, 1), "SE");
        assert_eq!(DIRECTION_OFFSETS[7], (-1, -1), "NW");
    }

    #[test]
    fn step_masks_the_direction_like_the_original() {
        assert_eq!(RmgGrid::step(5, 5, 2), (6, 5));
        assert_eq!(RmgGrid::step(5, 5, 8 + 2), (6, 5), "dir & 7 wraps");
    }

    /// Hand-walked from the original iterator's state machine with d = 4.
    /// Rows alternate d and d-1 cells; anti-diagonal sums increase by one per
    /// row. Any deviation here reorders every per-cell draw in the pipeline.
    #[test]
    fn scan_order_matches_the_native_state_machine() {
        let mut scan = DiamondScan::new(4);
        let seq: Vec<(i32, i32)> = (0..14).map(|_| scan.next_coord()).collect();
        assert_eq!(
            seq,
            vec![
                (1, 4),
                (2, 3),
                (3, 2),
                (4, 1), // row sum 5, 4 cells
                (2, 4),
                (3, 3),
                (4, 2), // row sum 6, 3 cells
                (2, 5),
                (3, 4),
                (4, 3),
                (5, 2), // row sum 7, 4 cells
                (3, 5),
                (4, 4),
                (5, 3), // row sum 8, 3 cells
            ]
        );
    }

    #[test]
    fn scan_rows_alternate_between_full_and_short() {
        let mut scan = DiamondScan::new(6);
        let mut rows: Vec<Vec<(i32, i32)>> = Vec::new();
        let mut row: Vec<(i32, i32)> = Vec::new();
        for _ in 0..40 {
            let coord = scan.next_coord();
            if let Some(&(px, py)) = row.last()
                && coord.0 + coord.1 != px + py
            {
                rows.push(std::mem::take(&mut row));
            }
            row.push(coord);
        }
        for (index, row) in rows.iter().enumerate() {
            let expected = if index % 2 == 0 { 6 } else { 5 };
            assert_eq!(row.len(), expected, "row {index} length");
            let sum = row[0].0 + row[0].1;
            assert!(row.iter().all(|(x, y)| x + y == sum), "row {index} sum");
        }
    }

    #[test]
    fn default_cell_matches_map_prep_initial_state() {
        let cell = GridCell::default();
        assert_eq!(cell.tile, TILE_UNASSIGNED);
        assert_eq!(cell.level, 4, "generated cells initialise to level 4");
        assert_eq!(cell.overlay, -1);
        assert_eq!(cell.sub_tile, 0);
        assert!(!cell.occupied);
    }

    #[test]
    fn grid_validity_is_the_diamond_band() {
        let mut grid = RmgGrid::new(16, 4, 12);
        assert!(grid.get(3, 2).is_some(), "sum 5 is inside");
        assert!(grid.get(2, 2).is_none(), "sum == min is outside");
        assert!(grid.get(6, 6).is_some(), "sum == max is inside");
        assert!(grid.get(7, 6).is_none(), "sum > max is outside");
        assert!(grid.get(6, 1).is_none(), "x - y >= min is outside");
        grid.get_mut(3, 2).unwrap().tile = 42;
        assert_eq!(grid.get(3, 2).unwrap().tile, 42);
        assert_eq!(grid.get(2, 3).unwrap().tile, TILE_UNASSIGNED);
    }

    #[test]
    fn out_of_band_lookups_share_the_border_cell() {
        let mut grid = RmgGrid::new(16, 4, 12);
        assert_eq!(grid.cell_native(0, 0).tile, 0, "border starts clear");
        assert_eq!(grid.border_coord(), (0, 0));
        grid.cell_native_mut(15, 15).tile = 99;
        assert_eq!(grid.border_coord(), (15, 15), "lookup stamps the coord");
        assert_eq!(
            grid.cell_native(1, 0).tile,
            99,
            "every invalid coordinate aliases the same cell"
        );
        assert_eq!(grid.border_coord(), (1, 0));
        assert_eq!(
            grid.get(6, 6).unwrap().tile,
            TILE_UNASSIGNED,
            "valid cells are untouched by border writes"
        );
    }

    #[test]
    fn native_cells_walk_the_band_and_stop_at_the_far_diagonal() {
        let grid = RmgGrid::new(16, 4, 8);
        let cells: Vec<(i32, i32)> = grid.native_cells().collect();
        assert_eq!(cells.first(), Some(&(1, 4)));
        assert!(cells.iter().all(|&(x, y)| grid.is_valid(x, y)));
        assert!(cells.iter().all(|&(x, y)| x + y <= 8));
        // Every valid cell appears exactly once.
        let mut sorted = cells.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), cells.len(), "no duplicates");
        let expected: usize = (0..16i32)
            .flat_map(|y| (0..16i32).map(move |x| (x, y)))
            .filter(|&(x, y)| grid.is_valid(x, y))
            .count();
        assert_eq!(cells.len(), expected, "full coverage of the band");
    }
}
