//! Selected ordinary CellClass knowledge state, distinct from the legacy cache
//! projection. Original487630/487690/4876F0,6FB170/6FB470 and578100.
//! Native counter arithmetic wraps; pending is not equivalent to current sight.
use crate::sim::intern::InternedId;
use crate::sim::state_hasher::hash_records;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ShroudKnowledge {
    pub counter: i32,
    pub gap_counter: i32,
    pub open: bool,
    pub pending: bool,
    pub local_sources: u32,
    pub allied_sources: u32,
    pub transient_visible: bool,
}
impl Default for ShroudKnowledge {
    fn default() -> Self {
        Self {
            counter: 1,
            gap_counter: 0,
            open: false,
            pending: false,
            local_sources: 0,
            allied_sources: 0,
            transient_visible: false,
        }
    }
}
impl ShroudKnowledge {
    /// This cell's state hash record: the four counters little-endian, then
    /// the flags.
    pub(super) fn hash_record(&self) -> [u8; 17] {
        let Self {
            counter,
            gap_counter,
            open,
            pending,
            local_sources,
            allied_sources,
            transient_visible,
        } = *self;
        let mut record = [0u8; 17];
        record[..4].copy_from_slice(&counter.to_le_bytes());
        record[4..8].copy_from_slice(&gap_counter.to_le_bytes());
        record[8..12].copy_from_slice(&local_sources.to_le_bytes());
        record[12..16].copy_from_slice(&allied_sources.to_le_bytes());
        record[16] = u8::from(open) | u8::from(pending) << 1 | u8::from(transient_visible) << 2;
        record
    }

    pub fn reveal(&mut self) {
        if self.counter == 1 {
            self.counter = 0;
        }
        self.counter = self.counter.wrapping_sub(1);
        if self.counter <= 0 {
            //487630 opens missing18 and returns. Only the already-open branch
            //cancels20: a second gap followed by return must retain pending.
            if self.open {
                self.pending = false;
            } else {
                self.open = true;
            }
        }
    }
    pub fn leave(&mut self) {
        let old = self.counter;
        if self.counter == -1 {
            self.counter = 0;
        }
        self.counter = self.counter.wrapping_add(1).min(self.gap_counter);
        if old <= 0 && self.counter > 0 {
            self.pending = true;
        }
    }
    pub fn fire_unshroud(&mut self) {
        self.open = true;
        if self.counter > 0 {
            self.pending = true;
        }
    }
    pub fn add_gap(&mut self) {
        if self.counter != 1 && self.counter >= 0 {
            self.counter = self.counter.wrapping_add(1);
        }
        self.gap_counter = self.gap_counter.wrapping_add(1);
        if self.counter > 0 {
            self.open = false;
        }
    }
    pub fn remove_gap(&mut self, spy_sat_active: bool) {
        self.gap_counter = self.gap_counter.wrapping_sub(1);
        if spy_sat_active && self.gap_counter <= 0 {
            self.counter = self.counter.wrapping_sub(1);
            if self.counter <= 0 {
                self.open = true;
            }
        }
    }
    pub fn sweep(&mut self) {
        if self.pending {
            self.open = false;
            self.pending = false;
        }
    }
}

/// The existing source producer supplies geometry. Native70AF50 admits one
/// ordinary contribution under latch250 and stores254XYZ/260radius;70B1D0
/// clears the admission latch then releases that stored footprint, retaining
/// the stored geometry. Reconciliation must not
/// turn an unchanged source into another reveal event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SightAdmission {
    pub owner: InternedId,
    pub origin: (u16, u16, i32),
    pub radius: u16,
    pub cells: Vec<(u16, u16)>,
}

impl Hash for SightAdmission {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        let Self {
            owner,
            origin,
            radius,
            cells,
        } = self;
        owner.hash(hasher);
        origin.hash(hasher);
        radius.hash(hasher);
        // The footprint lists every cell of the sight circle.
        hash_records(cells, hasher, |&(rx, ry)| {
            let [x0, x1] = rx.to_le_bytes();
            let [y0, y1] = ry.to_le_bytes();
            [x0, x1, y0, y1]
        });
    }
}

/// Native Foot65C/664 countdown, one logical native client per viewer. The
/// constructor initial value is retained so an unseen/nonallied viewer never
/// borrows another viewer's timer history. Admission release does not reset it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SightRefreshTimers {
    initial: crate::sim::timer::CdTimer,
    pub(crate) by_viewer: std::collections::BTreeMap<InternedId, crate::sim::timer::CdTimer>,
}
impl SightRefreshTimers {
    pub(crate) fn at_construction(frame: u32) -> Self {
        Self {
            initial: crate::sim::timer::CdTimer::started(frame as i32, 0),
            by_viewer: Default::default(),
        }
    }
    pub(crate) fn timer(&self, viewer: InternedId) -> crate::sim::timer::CdTimer {
        self.by_viewer.get(&viewer).copied().unwrap_or(self.initial)
    }
    pub(crate) fn reload(&mut self, viewer: InternedId, frame: u32) {
        self.by_viewer.insert(
            viewer,
            crate::sim::timer::CdTimer::started(frame as i32, 15),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ShroudKnowledge;
    use crate::sim::state_hasher::{StateHasher, hash_records};
    use std::hash::Hasher;

    fn grid_hash(cells: &[ShroudKnowledge]) -> u64 {
        let mut hasher = StateHasher::new();
        hash_records(cells, &mut hasher, ShroudKnowledge::hash_record);
        hasher.finish()
    }

    #[test]
    fn every_field_of_every_cell_moves_the_grid_hash() {
        // 300 cells span two 240-record batches.
        let grid = vec![ShroudKnowledge::default(); 300];
        let base = grid_hash(&grid);
        let edits: [fn(&mut ShroudKnowledge); 7] = [
            |cell| cell.counter = 2,
            |cell| cell.gap_counter = 1,
            |cell| cell.open = true,
            |cell| cell.pending = true,
            |cell| cell.local_sources = 1,
            |cell| cell.allied_sources = 1,
            |cell| cell.transient_visible = true,
        ];
        for index in [0, 239, 240, 299] {
            for edit in edits {
                let mut changed = grid.clone();
                edit(&mut changed[index]);
                assert_ne!(grid_hash(&changed), base, "cell {index}");
            }
        }
        assert_ne!(grid_hash(&grid[..299]), base);
    }
}
