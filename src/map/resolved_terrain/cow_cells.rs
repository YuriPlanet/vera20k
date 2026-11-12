//! Terrain cell storage the presentation copy of the world keeps in step by
//! copying only the chunks a write changed (docs/sim-thread.md,
//! [`ChunkedVec`]).
//!
//! `CellClass::RecalcAttributes @ 0x0047D2B0` runs whenever an object marks
//! or leaves a cell, and almost always derives the attributes the cell
//! already holds. [`CowCells::begin_shadow`] gives that recalc a private copy
//! of its one cell: reads and writes of that index use the copy, and
//! [`CowCells::end_shadow`] writes it back only if it differs, so an
//! unchanged recalc marks nothing for the copy to take.

use std::ops::{Index, IndexMut};

use super::ResolvedTerrainCell;
use crate::util::synced::ChunkedVec;

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

    /// Bring `mirror`, the presentation copy, up to these cells.
    pub(super) fn sync_into(&mut self, mirror: &mut Self) {
        debug_assert!(self.shadow.is_none(), "a sync inside a recalc");
        self.cells.sync_into(&mut mirror.cells);
        mirror.shadow = None;
    }
}

/// Equal cells.
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

    /// A copy synced, then changed where `live` was not written, keeps that
    /// change after the next sync only if `live` left the cell alone.
    fn kept_after_sync(live: &mut CowCells, index: usize) -> bool {
        let mut copy = CowCells::default();
        live.sync_into(&mut copy);
        copy[index].level = copy[index].level.wrapping_add(7);
        let marked = copy[index].level;
        live.sync_into(&mut copy);
        copy[index].level == marked
    }

    #[test]
    fn an_unchanged_recalc_marks_nothing_for_the_copy() {
        let mut live = cells();
        let mut copy = CowCells::default();
        live.sync_into(&mut copy);
        live.begin_shadow(2);
        let level = live[2].level;
        live[2].level = level.wrapping_add(1);
        live[2].level = level;
        live.end_shadow();
        copy[2].level = 9;
        live.sync_into(&mut copy);
        assert_eq!(copy[2].level, 9, "the unchanged chunk was not copied");
    }

    #[test]
    fn a_changed_cell_reaches_the_copy_with_its_chunk_only() {
        let mut live = cells();
        let mut copy = CowCells::default();
        live.sync_into(&mut copy);
        live.begin_shadow(1);
        live[1].level = 3;
        // The recalc reads its own write.
        assert_eq!(live.get(1).map(|cell| cell.level), Some(3));
        live.end_shadow();
        copy[599].level = 9;
        live.sync_into(&mut copy);
        assert_eq!(copy[1].level, 3);
        assert_eq!(copy[599].level, 9, "another chunk was not copied");
        assert!(kept_after_sync(&mut live, 300));
    }
}
