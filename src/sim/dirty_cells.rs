//! Cells whose presentation must refresh after a runtime map change.

use std::collections::BTreeSet;

/// Radar and tactical redraw marks for runtime cell changes.
///
/// Radar: cells whose minimap terrain pixel must refresh. Presentation reads
/// the pending batch with its generation and acknowledges it once its radar
/// update completes, so a stale acknowledgement cannot clear a newer batch.
/// Active `RadarClass` dirty processing at `0x00655250` drains and clears its
/// retained cell list the same way.
///
/// Tactical: the cells native marks for a tactical redraw. VERA's tactical
/// view rebuilds every frame and never reads them, so only test builds keep
/// them, as evidence of a path's native marks.
///
/// Both are presentation handoff state: not saved and not in `state_hash`.
#[derive(Debug, Default, Clone)]
pub struct DirtyCells {
    radar: Vec<(u16, u16)>,
    radar_generation: u64,
    /// Membership of `radar`, so a whole-map reveal needs no list scan.
    radar_index: BTreeSet<(u16, u16)>,
    #[cfg(any(test, feature = "test-support"))]
    tactical: Vec<(u16, u16)>,
}

impl DirtyCells {
    /// The pending radar batch.
    pub fn radar(&self) -> &[(u16, u16)] {
        &self.radar
    }

    /// Advances whenever a cell joins the pending radar batch.
    pub fn radar_generation(&self) -> u64 {
        self.radar_generation
    }

    /// Appends each cell not yet in the pending radar batch. The generation
    /// advances once if any cell was new.
    pub(crate) fn mark_radar(&mut self, cells: impl IntoIterator<Item = (u16, u16)>) {
        let mut changed = false;
        for cell in cells {
            if self.radar_index.insert(cell) {
                self.radar.push(cell);
                changed = true;
            }
        }
        if changed {
            self.radar_generation = self.radar_generation.wrapping_add(1);
        }
    }

    /// Acknowledges one completed radar update. A stale generation or an
    /// empty batch is refused.
    pub(crate) fn acknowledge_radar(&mut self, generation: u64) -> bool {
        if generation != self.radar_generation || self.radar.is_empty() {
            return false;
        }
        self.radar.clear();
        self.radar_index.clear();
        true
    }

    /// Native's tactical redraw mark for `cell`. Every call is kept in order,
    /// so wall paths can be compared with native visit histories.
    pub(crate) fn mark_tactical(&mut self, cell: (u16, u16)) {
        #[cfg(any(test, feature = "test-support"))]
        self.tactical.push(cell);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = cell;
    }

    /// A tactical mark for the paths whose evidence records each cell once:
    /// tiberium placement and reduction, the ore anim arm and a placement
    /// footprint.
    pub(crate) fn mark_tactical_once(&mut self, cell: (u16, u16)) {
        #[cfg(any(test, feature = "test-support"))]
        if !self.tactical.contains(&cell) {
            self.tactical.push(cell);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = cell;
    }

    /// Marks `cell` for both radar and tactical redraw.
    pub(crate) fn mark_cell(&mut self, cell: (u16, u16)) {
        self.mark_tactical(cell);
        self.mark_radar([cell]);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn tactical_for_test(&self) -> &[(u16, u16)] {
        &self.tactical
    }

    /// Fixture reset between test phases; the generation stays.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_radar_for_test(&mut self) {
        self.radar.clear();
        self.radar_index.clear();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_tactical_for_test(&mut self) {
        self.tactical.clear();
    }
}
