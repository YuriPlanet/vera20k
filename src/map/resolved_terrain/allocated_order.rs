//! The cells a complete `CellIterator_Init @ 0x00578350` /
//! `Next @ 0x00578290` walk yields ([`NativeAllocatedCellCursor`]), numbered
//! in walk order. A walk that visits only some of them uses it to keep their
//! order and to leave the shared cursor as the complete walk does.

use super::ResolvedTerrainGrid;
use super::native_cell::NativeAllocatedCellCursor;
use std::sync::{Arc, OnceLock};

#[cfg(test)]
#[path = "allocated_order_tests.rs"]
mod tests;

/// A cell's eight neighbours in row order: the order of
/// `Tactical::6D8700`'s lookups, and the bit order of
/// [`AllocatedCellOrder::missing_neighbors`].
pub(crate) const ROW_NEIGHBORS: [(i16, i16); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The cells a walk from reset yields on one terrain, by rank in walk order.
/// The walk runs along anti-diagonal rows: row `k` holds the cells with
/// `x + y == size_width + 1 + k`, `size_width` of them in an even row and
/// one fewer in an odd one, and the walk ends at its first unallocated cell.
///
/// Only a regular terrain has one: its cells stored row-major, narrower than
/// the 512 stride (so no neighbour lookup wraps onto a real cell), and each
/// cursor position yielding its own cell. [`Self::build`] gives `None` for
/// any other, and its walks visit every cell.
#[derive(Debug)]
pub(crate) struct AllocatedCellOrder {
    size_width: i32,
    len: u32,
    /// The cursor a complete walk leaves: one step past its first null.
    end: NativeAllocatedCellCursor,
    /// The walk's cells with an unallocated neighbour, by rank, with a mask
    /// of those neighbours (bit `i` for [`ROW_NEIGHBORS`]`[i]`).
    missing_neighbors: Vec<(u32, u8)>,
}

impl AllocatedCellOrder {
    pub(crate) fn build(terrain: &ResolvedTerrainGrid, size_width: i32) -> Option<Self> {
        if size_width < 2 || !terrain.is_regular() {
            return None;
        }
        let mut order = Self {
            size_width,
            len: 0,
            end: NativeAllocatedCellCursor::reset(size_width),
            missing_neighbors: Vec::new(),
        };
        let mut cursor = order.end;
        loop {
            let at = cursor;
            let Some(identity) = cursor.next_in_terrain(terrain, size_width) else {
                break;
            };
            let ((x, y), remaining) = order.position(order.len);
            if at != NativeAllocatedCellCursor::at(i32::from(x), i32::from(y), remaining)
                || terrain.native_cell_coord(identity) != (x as i16, y as i16)
            {
                return None;
            }
            let mut missing = 0u8;
            for (bit, (dx, dy)) in ROW_NEIGHBORS.into_iter().enumerate() {
                let neighbor = ((x as i16).wrapping_add(dx), (y as i16).wrapping_add(dy));
                if terrain
                    .native_fixed_cell_index(neighbor.0, neighbor.1)
                    .is_none()
                {
                    missing |= 1 << bit;
                }
            }
            if missing != 0 {
                order.missing_neighbors.push((order.len, missing));
            }
            order.len += 1;
        }
        order.end = cursor;
        Some(order)
    }

    /// How many cells the walk yields.
    pub(crate) fn len(&self) -> u32 {
        self.len
    }

    /// The cursor a complete walk leaves.
    pub(crate) fn end(&self) -> NativeAllocatedCellCursor {
        self.end
    }

    /// The rank of `cell`, when the walk yields it.
    pub(crate) fn rank(&self, cell: (i32, i32)) -> Option<u32> {
        let row = u32::try_from(cell.0 + cell.1 - (self.size_width + 1)).ok()?;
        let ((start_x, _), len) = self.row(row);
        let column = u32::try_from(cell.0 - start_x)
            .ok()
            .filter(|&column| column < len)?;
        let rank = u64::from(self.row_start(row)) + u64::from(column);
        (rank < u64::from(self.len)).then_some(rank as u32)
    }

    /// The cell at `rank`.
    pub(crate) fn cell(&self, rank: u32) -> (u16, u16) {
        self.position(rank).0
    }

    /// The shared cursor once a walk from reset has yielded `rank`.
    pub(crate) fn cursor_after(&self, rank: u32) -> NativeAllocatedCellCursor {
        let ((x, y), remaining) = self.position(rank);
        let mut cursor = NativeAllocatedCellCursor::at(i32::from(x), i32::from(y), remaining);
        cursor.advance(self.size_width);
        cursor
    }

    /// The walk's cells with an unallocated neighbour, ranked at most
    /// `through`, and which neighbours.
    pub(crate) fn missing_neighbors_through(&self, through: u32) -> &[(u32, u8)] {
        let end = self
            .missing_neighbors
            .partition_point(|&(rank, _)| rank <= through);
        &self.missing_neighbors[..end]
    }

    /// Row `row`'s first cell and length.
    fn row(&self, row: u32) -> ((i32, i32), u32) {
        let half = (row / 2) as i32;
        let width = self.size_width;
        if row % 2 == 0 {
            ((1 + half, width + half), width as u32)
        } else {
            ((2 + half, width + half), (width - 1) as u32)
        }
    }

    /// How many cells the rows before `row` hold.
    fn row_start(&self, row: u32) -> u32 {
        let width = self.size_width as u32;
        row.div_ceil(2) * width + row / 2 * (width - 1)
    }

    /// The cell at `rank`, and how many cells its row has left after it: the
    /// cursor's state as it yields the cell.
    fn position(&self, rank: u32) -> ((u16, u16), i32) {
        let width = self.size_width as u32;
        let (pair, offset) = (rank / (2 * width - 1), rank % (2 * width - 1));
        let (row, column) = if offset < width {
            (2 * pair, offset)
        } else {
            (2 * pair + 1, offset - width)
        };
        let ((start_x, start_y), len) = self.row(row);
        (
            (
                (start_x + column as i32) as u16,
                (start_y - column as i32) as u16,
            ),
            (len - 1 - column) as i32,
        )
    }
}

/// The cells of a regular terrain (see [`AllocatedCellOrder`]) whose eight
/// [`ROW_NEIGHBORS`] are allocated. A fixed-grid lookup of such a cell or of
/// any neighbour returns that very cell, so it neither misses nor stamps the
/// shared Dummy, and its storage index is `y * width + x`.
#[derive(Debug)]
pub(crate) struct InteriorCells {
    width: u16,
    bits: Vec<u64>,
}

impl InteriorCells {
    fn build(terrain: &ResolvedTerrainGrid) -> Option<Self> {
        if !terrain.is_regular() {
            return None;
        }
        let (width, height) = (usize::from(terrain.width), usize::from(terrain.height));
        let allocated = |x: usize, y: usize| {
            terrain
                .native_fixed_cell_index(x as i16, y as i16)
                .is_some()
        };
        let mut bits = vec![0u64; (width * height).div_ceil(64)];
        for y in 1..height.saturating_sub(1) {
            for x in 1..width.saturating_sub(1) {
                if (y - 1..=y + 1).all(|y| (x - 1..=x + 1).all(|x| allocated(x, y))) {
                    let index = y * width + x;
                    bits[index / 64] |= 1 << (index % 64);
                }
            }
        }
        Some(Self {
            width: terrain.width,
            bits,
        })
    }

    /// Whether `cell` and its eight neighbours are allocated.
    pub(crate) fn contains(&self, (x, y): (u16, u16)) -> bool {
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        x < self.width
            && self
                .bits
                .get(index / 64)
                .is_some_and(|word| word >> (index % 64) & 1 != 0)
    }
}

/// What a terrain's allocation gives, built at the first ask. The allocation
/// never changes once the terrain exists.
#[derive(Debug, Clone, Default)]
pub(super) struct AllocationCaches {
    order: OnceLock<(i32, Option<Arc<AllocatedCellOrder>>)>,
    interior: OnceLock<Option<Arc<InteriorCells>>>,
}

impl ResolvedTerrainGrid {
    /// Cells stored row-major, narrower than the 512 stride, so that no
    /// neighbour lookup wraps onto a real cell and a real cell's lookup
    /// index is its storage index.
    fn is_regular(&self) -> bool {
        let width = usize::from(self.width);
        width < 512
            && self.cells.iter().enumerate().all(|(index, cell)| {
                usize::from(cell.rx) == index % width && usize::from(cell.ry) == index / width
            })
    }

    /// [`InteriorCells`], on a regular terrain.
    pub(crate) fn interior_cells(&self) -> Option<&InteriorCells> {
        self.allocation_caches
            .interior
            .get_or_init(|| InteriorCells::build(self).map(Arc::new))
            .as_deref()
    }

    /// [`AllocatedCellOrder`] for walks with Size width `size_width`; the
    /// first width asked is kept.
    pub(crate) fn allocated_cell_order(&self, size_width: i32) -> Option<Arc<AllocatedCellOrder>> {
        let (cached_width, order) = self.allocation_caches.order.get_or_init(|| {
            (
                size_width,
                AllocatedCellOrder::build(self, size_width).map(Arc::new),
            )
        });
        if *cached_width == size_width {
            order.clone()
        } else {
            AllocatedCellOrder::build(self, size_width).map(Arc::new)
        }
    }
}
