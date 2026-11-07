//! `OverlayRenderIndex` — the app's explicit presentation index over dynamic
//! overlay cells (F08).
//!
//! Frozen contract:
//! - source-map coordinates retain source order;
//! - an update to an existing coordinate retains its slot;
//! - the first dynamic appearance appends in `SimFrameOutput.overlay_updates`
//!   order;
//! - clearing leaves a coordinate tombstone that draws nothing because the
//!   live `OverlayGrid` is authoritative and empty there;
//! - reoccupation with a different ID reuses the slot and reads the new live
//!   value;
//! - full restore appends only missing occupied coordinates in restore-output
//!   order (an existing coordinate additionally refreshes its source-seed
//!   identity, which is invisible in-match because the live grid wins);
//!
//! Residual (recorded, not silent): entries still carry the source-seed
//! `overlay_id`/`frame` as the pre-runtime display fallback (shell/loading
//! render without a simulation); in-match, live `OverlayGrid` identity always
//! wins in `overlay_render_identity`.

use std::collections::BTreeSet;

use crate::map::overlay::OverlayEntry;
use crate::render::cell_window::CellWindow;

#[derive(Default)]
pub(crate) struct OverlayRenderIndex {
    entries: Vec<OverlayEntry>,
    /// Derived `(ry, rx, slot)` for every entry, ordered by cell row-major
    /// and then slot. `entries` is the source of truth: `replace_from_source`
    /// rebuilds this, `upsert_occupied` adds each appended slot, and
    /// `remove_cells` rebuilds it when `retain` shifts slots. Debug builds
    /// check it against `entries` on every window query.
    by_cell: BTreeSet<(u16, u16, usize)>,
}

impl OverlayRenderIndex {
    /// Install the source-map overlay list, in source order.
    pub(crate) fn replace_from_source(&mut self, entries: Vec<OverlayEntry>) {
        self.entries = entries;
        self.rebuild_by_cell();
    }

    /// Upsert authoritative occupied cells by coordinate: existing
    /// coordinates keep their slot (only the seed identity refreshes); new
    /// coordinates append in candidate order. Returns inserted-or-changed.
    pub(crate) fn upsert_occupied(&mut self, candidates: Vec<OverlayEntry>) -> usize {
        let mut synced = 0;
        for candidate in candidates {
            // A repeated coordinate resolves to its last slot.
            let slot = self
                .by_cell
                .range((candidate.ry, candidate.rx, 0)..=(candidate.ry, candidate.rx, usize::MAX))
                .next_back()
                .map(|&(_, _, slot)| slot);
            if let Some(index) = slot {
                let entry = &mut self.entries[index];
                if entry.overlay_id != candidate.overlay_id || entry.frame != candidate.frame {
                    entry.overlay_id = candidate.overlay_id;
                    entry.frame = candidate.frame;
                    synced += 1;
                }
            } else {
                self.by_cell
                    .insert((candidate.ry, candidate.rx, self.entries.len()));
                self.entries.push(candidate);
                synced += 1;
            }
        }
        synced
    }

    /// Drop the render entries for cells whose overlay identity was erased.
    ///
    /// [`OverlayRenderIndex::upsert_occupied`] only ever adds or rewrites an
    /// occupied cell, so a removal has to arrive on its own channel. Returns the
    /// number of entries actually dropped.
    pub(crate) fn remove_cells(&mut self, cells: &[(u16, u16)]) -> usize {
        if cells.is_empty() {
            return 0;
        }
        let removed: std::collections::HashSet<(u16, u16)> = cells.iter().copied().collect();
        let before = self.entries.len();
        self.entries
            .retain(|entry| !removed.contains(&(entry.rx, entry.ry)));
        let dropped = before - self.entries.len();
        if dropped != 0 {
            self.rebuild_by_cell();
        }
        dropped
    }

    fn rebuild_by_cell(&mut self) {
        self.by_cell = cell_slots(&self.entries);
    }

    /// The entries whose cell lies in `window`, in index order: one range
    /// query per window row, then the slots sorted.
    pub(crate) fn entries_in(&self, window: CellWindow) -> impl Iterator<Item = &OverlayEntry> {
        debug_assert!(
            self.by_cell == cell_slots(&self.entries),
            "the overlay cell index disagrees with its entries"
        );
        let mut slots: Vec<usize> = match window {
            CellWindow::Whole => (0..self.entries.len()).collect(),
            CellWindow::Bands { .. } => window
                .rows()
                .into_iter()
                .flat_map(|(first, last)| first..=last)
                .filter_map(|ry| window.row(ry).map(|row| (ry, row)))
                .flat_map(|(ry, (first, last))| {
                    self.by_cell
                        .range((ry, first, 0)..=(ry, last, usize::MAX))
                        .map(|&(_, _, slot)| slot)
                })
                .collect(),
        };
        slots.sort_unstable();
        slots.into_iter().map(|slot| &self.entries[slot])
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, OverlayEntry> {
        self.entries.iter()
    }

    pub(crate) fn as_slice(&self) -> &[OverlayEntry] {
        &self.entries
    }
}

/// `(ry, rx, slot)` for every entry.
fn cell_slots(entries: &[OverlayEntry]) -> BTreeSet<(u16, u16, usize)> {
    entries
        .iter()
        .enumerate()
        .map(|(slot, entry)| (entry.ry, entry.rx, slot))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(rx: u16, ry: u16, id: u8, frame: u8) -> OverlayEntry {
        OverlayEntry {
            rx,
            ry,
            overlay_id: id,
            frame,
        }
    }

    /// F08 matrix: source order, dynamic-append order, slot retention on
    /// update and reoccupation, and restore appending only missing
    /// coordinates in restore order.
    #[test]
    fn overlay_render_index_preserves_source_dynamic_tombstone_and_restore_order() {
        let mut index = OverlayRenderIndex::default();
        // Source install keeps source order.
        index.replace_from_source(vec![entry(5, 5, 10, 0), entry(3, 3, 11, 0)]);
        let coords: Vec<_> = index.iter().map(|e| (e.rx, e.ry)).collect();
        assert_eq!(coords, vec![(5, 5), (3, 3)]);

        // First dynamic appearances append in update order.
        assert_eq!(
            index.upsert_occupied(vec![entry(9, 9, 20, 1), entry(1, 1, 21, 2)]),
            2
        );
        let coords: Vec<_> = index.iter().map(|e| (e.rx, e.ry)).collect();
        assert_eq!(coords, vec![(5, 5), (3, 3), (9, 9), (1, 1)]);

        // Updating an existing coordinate retains its slot.
        assert_eq!(index.upsert_occupied(vec![entry(3, 3, 12, 4)]), 1);
        let third = &index.as_slice()[1];
        assert_eq!(
            (third.rx, third.ry, third.overlay_id, third.frame),
            (3, 3, 12, 4)
        );
        assert_eq!(index.as_slice().len(), 4);

        // Reoccupation with a different ID reuses the slot (tombstoned
        // coordinates draw nothing in-match because the live grid is empty;
        // the index itself never removes the coordinate).
        assert_eq!(index.upsert_occupied(vec![entry(9, 9, 30, 0)]), 1);
        assert_eq!(index.as_slice()[2].overlay_id, 30);

        // Restore appends only missing occupied coordinates, in restore order.
        let restored = vec![entry(5, 5, 10, 0), entry(7, 7, 40, 0), entry(2, 2, 41, 0)];
        index.upsert_occupied(restored);
        let coords: Vec<_> = index.iter().map(|e| (e.rx, e.ry)).collect();
        assert_eq!(coords, vec![(5, 5), (3, 3), (9, 9), (1, 1), (7, 7), (2, 2)]);
    }

    /// A window query returns index order, not cell order, after `retain`
    /// shifts the slots, and an upsert finds the shifted slot.
    #[test]
    fn window_query_keeps_index_order_after_removal() {
        let mut index = OverlayRenderIndex::default();
        index.replace_from_source(vec![
            entry(6, 2, 1, 0),
            entry(2, 6, 1, 0),
            entry(4, 4, 1, 0),
            entry(40, 40, 1, 0),
        ]);
        assert_eq!(index.upsert_occupied(vec![entry(3, 3, 1, 0)]), 1);
        assert_eq!(index.remove_cells(&[(2, 6)]), 1);
        assert_eq!(index.upsert_occupied(vec![entry(4, 4, 2, 0)]), 1);
        let window = CellWindow::Bands {
            diff: (-5, 5),
            sum: (0, 12),
        };
        let found: Vec<_> = index
            .entries_in(window)
            .map(|e| (e.rx, e.ry, e.overlay_id))
            .collect();
        assert_eq!(found, [(6, 2, 1), (4, 4, 2), (3, 3, 1)]);
    }
}
