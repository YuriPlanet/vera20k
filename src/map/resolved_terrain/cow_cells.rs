//! Copy-on-write cell storage (docs/sim-thread.md, phase 2).
//!
//! A published presentation frame shares the live grid's cells in fixed
//! chunks ([`ChunkedVec`]). A write copies only its own chunk, and only
//! while a frame still shares it.
//!
//! `CellClass::RecalcAttributes @ 0x0047D2B0` runs whenever an object marks
//! or leaves a cell, and almost always derives the attributes the cell
//! already holds. [`CowCells::begin_shadow`] gives that recalc a private copy
//! of its one cell: reads and writes of that index use the copy, and
//! [`CowCells::end_shadow`] writes it back only if it differs, so an
//! unchanged recalc copies nothing.

use std::ops::{Index, IndexMut};

use super::ResolvedTerrainCell;
use crate::util::shared::ChunkedVec;

#[derive(Debug, Clone)]
pub(super) struct CowCells {
    cells: ChunkedVec<ResolvedTerrainCell>,
    /// The recalc's private copy of one cell, by index.
    shadow: Option<(usize, ResolvedTerrainCell)>,
}

impl Default for CowCells {
    fn default() -> Self {
        Self {
            cells: ChunkedVec::new(),
            shadow: None,
        }
    }
}

impl CowCells {
    pub(super) fn new(cells: Vec<ResolvedTerrainCell>) -> Self {
        Self {
            cells: cells.into(),
            shadow: None,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.cells.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub(super) fn get(&self, index: usize) -> Option<&ResolvedTerrainCell> {
        match &self.shadow {
            Some((shadowed, cell)) if *shadowed == index => Some(cell),
            _ => self.cells.get(index),
        }
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut ResolvedTerrainCell> {
        if let Some((shadowed, cell)) = &mut self.shadow
            && *shadowed == index
        {
            return Some(cell);
        }
        self.cells.get_mut(index)
    }

    /// Every cell, for whole-grid readers; never inside a shadowed recalc.
    pub(super) fn all(&self) -> &ChunkedVec<ResolvedTerrainCell> {
        debug_assert!(self.shadow.is_none(), "whole-grid read inside a recalc");
        &self.cells
    }

    /// Every cell, for whole-grid writers; never inside a shadowed recalc.
    /// A write still copies only the chunk it lands in.
    pub(super) fn all_mut(&mut self) -> &mut ChunkedVec<ResolvedTerrainCell> {
        debug_assert!(self.shadow.is_none(), "whole-grid write inside a recalc");
        &mut self.cells
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = &ResolvedTerrainCell> + '_ {
        self.all().iter()
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut ResolvedTerrainCell> + '_ {
        self.all_mut().iter_mut()
    }

    /// Give a recalc of `index` a private copy of that cell.
    pub(super) fn begin_shadow(&mut self, index: usize) {
        debug_assert!(self.shadow.is_none(), "nested cell recalc");
        self.shadow = self.cells.get(index).cloned().map(|cell| (index, cell));
    }

    /// Write the recalc's copy back if it changed; an unchanged cell leaves
    /// shared storage shared.
    pub(super) fn end_shadow(&mut self) {
        if let Some((index, cell)) = self.shadow.take()
            && self.cells[index] != cell
        {
            self.cells[index] = cell;
        }
    }

    /// Whether `other` shares the chunk holding `index` (a published
    /// frame's copy).
    #[cfg(test)]
    pub(super) fn shares_storage_with(&self, other: &Self, index: usize) -> bool {
        self.cells
            .shares_chunk_with(&other.cells, index / crate::util::shared::CHUNK)
    }
}

/// Equal cells, whether or not the storage is shared.
impl PartialEq for CowCells {
    fn eq(&self, other: &Self) -> bool {
        self.all() == other.all()
    }
}

impl Index<usize> for CowCells {
    type Output = ResolvedTerrainCell;

    fn index(&self, index: usize) -> &ResolvedTerrainCell {
        self.get(index).expect("terrain cell index in range")
    }
}

impl IndexMut<usize> for CowCells {
    fn index_mut(&mut self, index: usize) -> &mut ResolvedTerrainCell {
        self.get_mut(index).expect("terrain cell index in range")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells() -> CowCells {
        CowCells::new(
            (0..600)
                .map(|x| super::super::test_flat_cell(x % 300, x / 300))
                .collect(),
        )
    }

    #[test]
    fn an_unchanged_recalc_keeps_a_published_copy_shared() {
        let mut live = cells();
        let published = live.clone();
        live.begin_shadow(2);
        let level = live[2].level;
        live[2].level = level.wrapping_add(1);
        live[2].level = level;
        live.end_shadow();
        assert!(live.shares_storage_with(&published, 2));
    }

    #[test]
    fn a_changed_cell_copies_only_its_chunk() {
        let mut live = cells();
        let published = live.clone();
        live.begin_shadow(1);
        live[1].level = 3;
        // The recalc reads its own write; other cells read shared storage.
        assert_eq!(live.get(1).map(|cell| cell.level), Some(3));
        assert_eq!(live[0], published[0]);
        live.end_shadow();
        assert!(!live.shares_storage_with(&published, 1));
        assert!(live.shares_storage_with(&published, 599));
        assert_eq!(live[1].level, 3);
        assert_eq!(published[1].level, super::super::test_flat_cell(1, 0).level);
    }
}
