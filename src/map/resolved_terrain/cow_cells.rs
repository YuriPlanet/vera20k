//! Copy-on-write cell storage (docs/sim-thread.md, phase 2).
//!
//! The presentation frame each tick publishes shares the live grid's cells.
//! A write copies them only while a frame still shares them, so the
//! terrain is copied only on ticks that really change it.
//!
//! `CellClass::RecalcAttributes @ 0x0047D2B0` runs whenever an object marks
//! or leaves a cell, and almost always derives the attributes the cell
//! already holds. [`CowCells::begin_shadow`] gives that recalc a private copy
//! of its one cell: reads and writes of that index use the copy, and
//! [`CowCells::end_shadow`] writes it back only if it differs, so an
//! unchanged recalc copies nothing.

use std::ops::{Index, IndexMut};
use std::sync::Arc;

use super::ResolvedTerrainCell;

#[derive(Debug, Clone, Default)]
pub(super) struct CowCells {
    cells: Arc<Vec<ResolvedTerrainCell>>,
    /// The recalc's private copy of one cell, by index.
    shadow: Option<(usize, ResolvedTerrainCell)>,
}

impl CowCells {
    pub(super) fn new(cells: Vec<ResolvedTerrainCell>) -> Self {
        Self {
            cells: Arc::new(cells),
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
        Arc::make_mut(&mut self.cells).get_mut(index)
    }

    /// Every cell, for whole-grid readers; never inside a shadowed recalc.
    pub(super) fn as_slice(&self) -> &[ResolvedTerrainCell] {
        debug_assert!(self.shadow.is_none(), "whole-grid read inside a recalc");
        &self.cells
    }

    /// Every cell, for whole-grid writers; never inside a shadowed recalc.
    pub(super) fn as_mut_slice(&mut self) -> &mut [ResolvedTerrainCell] {
        debug_assert!(self.shadow.is_none(), "whole-grid write inside a recalc");
        Arc::make_mut(&mut self.cells).as_mut_slice()
    }

    /// The storage itself, for fixtures that reshape a grid.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn vec_mut(&mut self) -> &mut Vec<ResolvedTerrainCell> {
        debug_assert!(self.shadow.is_none());
        Arc::make_mut(&mut self.cells)
    }

    pub(super) fn iter(&self) -> std::slice::Iter<'_, ResolvedTerrainCell> {
        self.as_slice().iter()
    }

    pub(super) fn iter_mut(&mut self) -> std::slice::IterMut<'_, ResolvedTerrainCell> {
        self.as_mut_slice().iter_mut()
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
            Arc::make_mut(&mut self.cells)[index] = cell;
        }
    }

    /// Whether `other` shares this storage (a published frame's copy).
    #[cfg(test)]
    pub(super) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cells, &other.cells)
    }
}

/// Equal cells, whether or not the storage is shared.
impl PartialEq for CowCells {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
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

impl<'a> IntoIterator for &'a CowCells {
    type Item = &'a ResolvedTerrainCell;
    type IntoIter = std::slice::Iter<'a, ResolvedTerrainCell>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut CowCells {
    type Item = &'a mut ResolvedTerrainCell;
    type IntoIter = std::slice::IterMut<'a, ResolvedTerrainCell>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells() -> CowCells {
        CowCells::new((0..4).map(|x| super::super::test_flat_cell(x, 0)).collect())
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
        assert!(live.shares_storage_with(&published));
    }

    #[test]
    fn a_changed_recalc_copies_once_and_leaves_the_published_copy_as_it_was() {
        let mut live = cells();
        let published = live.clone();
        live.begin_shadow(1);
        live[1].level = 3;
        // The recalc reads its own write; other cells read shared storage.
        assert_eq!(live.get(1).map(|cell| cell.level), Some(3));
        assert_eq!(live[0], published[0]);
        live.end_shadow();
        assert!(!live.shares_storage_with(&published));
        assert_eq!(live[1].level, 3);
        assert_eq!(published[1].level, super::super::test_flat_cell(1, 0).level);
    }
}
