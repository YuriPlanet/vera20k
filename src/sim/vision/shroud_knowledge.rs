//! Selected ordinary CellClass knowledge state, distinct from the legacy cache
//! projection. Original487630/487690/4876F0,6FB170/6FB470 and578100.
//! Native counter arithmetic wraps; pending is not equivalent to current sight.
use crate::sim::intern::InternedId;
use crate::sim::state_hasher::hash_records;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ShroudKnowledge {
    counter: i32,
    gap_counter: i32,
    /// Cell+13C, initialized to zero by CellClass47BD50. Source: gamemd.exe.
    /// Unlike +134, map reveal/reset stores preserve this signed counter.
    friendly_gap_counter: i32,
    open: bool,
    pending: bool,
    local_sources: u32,
    allied_sources: u32,
    transient_visible: bool,
}
impl Default for ShroudKnowledge {
    fn default() -> Self {
        Self {
            counter: 1,
            gap_counter: 0,
            friendly_gap_counter: 0,
            open: false,
            pending: false,
            local_sources: 0,
            allied_sources: 0,
            transient_visible: false,
        }
    }
}
impl ShroudKnowledge {
    /// The shroud counter: positive is shrouded, zero or negative mapped.
    pub(crate) fn counter(&self) -> i32 {
        self.counter
    }

    /// Hostile gap generators covering the cell.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn gap_counter(&self) -> i32 {
        self.gap_counter
    }

    pub(crate) fn friendly_gap_counter(&self) -> i32 {
        self.friendly_gap_counter
    }

    /// CreateGap6FB408..414 / DestroyGap6FB6E5..6F1 use wrapping INC/DEC,
    /// including removal after a hostile activation and a later defeat.
    /// Source: original gamemd.exe instructions; no saturation or receipt gate.
    pub(super) fn count_friendly_gap(&mut self, active: bool) {
        self.friendly_gap_counter = if active {
            self.friendly_gap_counter.wrapping_add(1)
        } else {
            self.friendly_gap_counter.wrapping_sub(1)
        };
    }

    /// Mapped.
    pub(crate) fn open(&self) -> bool {
        self.open
    }

    /// Waiting for the reshroud sweep to conceal it.
    pub(crate) fn pending(&self) -> bool {
        self.pending
    }

    pub(crate) fn local_sources(&self) -> u32 {
        self.local_sources
    }

    pub(crate) fn allied_sources(&self) -> u32 {
        self.allied_sources
    }

    /// Sees for this frame only ([`Self::see_this_frame`]).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn transient_visible(&self) -> bool {
        self.transient_visible
    }

    /// The published flags of this cell (the projection's `FLAG_*` bits).
    pub(super) fn published_flags(&self) -> u8 {
        use super::{
            FLAG_EFFECTIVE_GAP_SIGHT, FLAG_GAP_COVERED, FLAG_GAP_FOG, FLAG_HOSTILE_GAP_PRESENT,
            FLAG_PENDING_GAP_CONCEAL, FLAG_REVEALED, FLAG_SUSTAINED_SIGHT, FLAG_VISIBLE,
        };
        let mut flags = 0;
        if self.open {
            flags |= FLAG_REVEALED;
        }
        if self.local_sources > 0 {
            flags |= FLAG_SUSTAINED_SIGHT;
        }
        if self.allied_sources > 0 {
            flags |= FLAG_EFFECTIVE_GAP_SIGHT;
        }
        if self.open
            && (self.local_sources > 0 || self.allied_sources > 0 || self.transient_visible)
        {
            flags |= FLAG_VISIBLE;
        }
        if self.pending {
            flags |= FLAG_PENDING_GAP_CONCEAL;
        }
        if self.gap_counter > 0 {
            flags |= FLAG_HOSTILE_GAP_PRESENT;
            if !self.open {
                flags |= FLAG_GAP_COVERED;
            }
        }
        // IsFogged58652E..586536 takes JLE for Cell+13C <= 0.
        if self.friendly_gap_counter > 0 {
            flags |= FLAG_GAP_FOG;
        }
        flags
    }

    /// A sight source's footprint count on this cell: the local one for the
    /// source owner's own plane, else the allied one. A release takes back
    /// only what a retained admission added.
    pub(super) fn count_source(&mut self, local: bool, add: bool) {
        let count = if local {
            &mut self.local_sources
        } else {
            &mut self.allied_sources
        };
        *count = if add {
            count.wrapping_add(1)
        } else {
            count
                .checked_sub(1)
                .expect("retained sight admission is balanced")
        };
    }

    /// This frame's transient sight, which [`Self::end_frame_sight`] ends.
    pub(super) fn see_this_frame(&mut self) {
        self.transient_visible = true;
    }

    /// The frame clear takes the transient sight off.
    pub(super) fn end_frame_sight(&mut self) {
        self.transient_visible = false;
    }

    /// A map reveal's projection (not a source's): a real cell sees for this
    /// frame, the shared Dummy does not.
    pub(super) fn project_reveal(&mut self, real_cell: bool) {
        self.transient_visible = real_cell;
    }

    /// Map577C90's receiver577D1F..577D47: the [`Self::leave`] arithmetic,
    /// then the knowledge clears only when the counter is not negative.
    /// Returns whether it cleared, for the caller's ground and cache bits.
    pub(super) fn leave_reshrouded(&mut self) -> bool {
        self.leave();
        if self.counter < 0 {
            return false;
        }
        self.open = false;
        self.transient_visible = false;
        true
    }

    /// The bulk577D90/577AB0 store: shrouded (`reset`) or mapped, with no
    /// gap count. Pending and the sources stay.
    pub(super) fn store_bulk(&mut self, reset: bool) {
        self.counter = i32::from(reset);
        self.gap_counter = 0;
        self.open = !reset;
    }

    /// The reshroud sweep's first pass marks a visible, unmapped cell.
    pub(super) fn mark_pending(&mut self) {
        self.pending = true;
    }

    /// The reshroud sweep's second pass (4ACB68) clears the pending byte,
    /// returning whether it was set.
    pub(super) fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }

    /// The sweep hides a cell that was visible: it is no longer mapped.
    pub(super) fn hide(&mut self) {
        self.open = false;
    }

    /// Map567F70's receiver: only a counter of exactly zero is concealed,
    /// back to one. Returns whether it was.
    pub(super) fn conceal(&mut self) -> bool {
        if self.counter != 0 {
            return false;
        }
        self.counter = 1;
        self.open = false;
        true
    }

    /// A map cell reveal maps the cell.
    pub(super) fn open_mapped(&mut self) {
        self.open = true;
    }

    /// A fixture's visible, mapped cell.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn mark_visible_for_test(&mut self) {
        self.counter = 0;
        self.open = true;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn set_open_for_test(&mut self, value: bool) {
        self.open = value;
    }

    #[cfg(test)]
    pub(super) fn set_counter_for_test(&mut self, value: i32) {
        self.counter = value;
    }

    #[cfg(test)]
    pub(super) fn set_gap_counter_for_test(&mut self, value: i32) {
        self.gap_counter = value;
    }

    #[cfg(test)]
    pub(super) fn set_pending_for_test(&mut self, value: bool) {
        self.pending = value;
    }

    #[cfg(test)]
    pub(super) fn set_local_sources_for_test(&mut self, value: u32) {
        self.local_sources = value;
    }

    #[cfg(test)]
    pub(super) fn set_transient_visible_for_test(&mut self, value: bool) {
        self.transient_visible = value;
    }

    /// The established sight/shroud hash prefix: four counters and flags.
    /// OwnerVisibility hashes the retained friendly gap plane separately.
    pub(super) fn hash_record(&self) -> [u8; 17] {
        let Self {
            counter,
            gap_counter,
            friendly_gap_counter: _,
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

    pub fn reveal(&mut self, ground_was_fully_open: bool) {
        if self.counter == 1 {
            self.counter = 0;
        }
        self.counter = self.counter.wrapping_sub(1);
        if self.counter <= 0 {
            // CellClass::Reveal @ 0x00487630 tests prior Cell+12C bits18.
            // Opening missing bits returns before the pending20 cancellation;
            // the projected `open` also admits explored-only ground08.
            if ground_was_fully_open {
                self.pending = false;
            }
            self.open = true;
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
    /// Whether6FB377..6FB3BB also clears the cell's ground bits18.
    pub fn add_gap(&mut self) -> bool {
        if self.counter != 1 && self.counter >= 0 {
            self.counter = self.counter.wrapping_add(1);
        }
        self.gap_counter = self.gap_counter.wrapping_add(1);
        if self.counter > 0 {
            self.open = false;
            return true;
        }
        false
    }
    /// Whether6FB603..6FB69C also restores the cell's ground bits18.
    pub fn remove_gap(&mut self, spy_sat_active: bool) -> bool {
        self.gap_counter = self.gap_counter.wrapping_sub(1);
        if spy_sat_active && self.gap_counter <= 0 {
            self.counter = self.counter.wrapping_sub(1);
            if self.counter <= 0 {
                self.open = true;
                return true;
            }
        }
        false
    }
    pub fn sweep(&mut self) {
        if self.pending {
            self.open = false;
            self.pending = false;
        }
    }
}

/// One viewer's accepted map contribution for a source event. Its `latched`
/// bit tracks whether the derived cell-count index is active; that logical
/// client's native +0x250 latch and cached +0x254/+0x260 geometry live in
/// [`SightSource`]. Reconciliation must not turn an unchanged source into
/// another reveal event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SightAdmission {
    /// Whether this viewer's derived map contribution is still active.
    /// Older snapshots stored only active contributions.
    #[serde(default = "sight_latched_by_default")]
    pub latched: bool,
    pub owner: InternedId,
    pub origin: (u16, u16, i32),
    pub radius: i32,
    /// Derived active source-count footprint of real native Cell identities.
    /// Missing slots still receive native calls through the shared Dummy, but
    /// never acquire rectangular source-count slots. Cleared with the latch;
    /// a native release recomputes geometry from cached origin/radius.
    pub cells: Vec<(u16, u16)>,
}

/// One logical client's `TechnoClass` sight cache: native +0x250, +0x254 and
/// +0x260. Ordinary per-viewer [`SightAdmission`] values retain only the
/// derived contribution that client's map accepted; this is the sole owner of
/// that client's latch and cached geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SightSource {
    pub(crate) latched: bool,
    pub(crate) owner: InternedId,
    pub(crate) origin: (u16, u16, i32),
    pub(crate) radius: i32,
}

impl From<&SightAdmission> for SightSource {
    fn from(admission: &SightAdmission) -> Self {
        Self {
            latched: true,
            owner: admission.owner,
            origin: admission.origin,
            radius: admission.radius,
        }
    }
}

fn sight_latched_by_default() -> bool {
    true
}

impl Hash for SightAdmission {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        let Self {
            latched,
            owner,
            origin,
            radius,
            cells,
        } = self;
        latched.hash(hasher);
        owner.hash(hasher);
        origin.hash(hasher);
        radius.hash(hasher);
        // Retain the ordered derived real-cell footprint.
        hash_records(cells, hasher, |&(rx, ry)| {
            let [x0, x1] = rx.to_le_bytes();
            let [y0, y1] = ry.to_le_bytes();
            [x0, x1, y0, y1]
        });
    }
}

/// One retained native Type Sight integer, initialized by the existing
/// production type reader. Exact zero and effective/clamped range are queries
/// of this authority, never independently stored facts. Re-entry and
/// snapshots retain the raw value (`InfantryUnlimbo51E0EF`,70AF87).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SightFacts {
    range: i32,
}

impl SightFacts {
    /// Synthetic constructor prior; production stamps the actual type value.
    pub(crate) const fn new(range: u16) -> Self {
        Self {
            range: range as i32,
        }
    }

    /// The raw signed type Sight value in cells.
    pub(crate) const fn range(self) -> i32 {
        self.range
    }

    /// The type's `Sight=` is exactly 0.
    pub(crate) const fn is_zero(self) -> bool {
        self.range == 0
    }

    /// Construction's existing native-reader Type Sight value.
    pub(crate) fn stamp_type_sight(&mut self, sight: i32) {
        self.range = sight;
    }

    /// Install a fixture's range.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_vision_range_for_test(&mut self, range: u16) {
        self.range = i32::from(range);
    }
}

/// Native Foot65C/664 countdown and Techno264 sight height, one logical
/// native client per viewer. Admission release does not reset either history.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SightRefreshTimers {
    initial: crate::sim::timer::CdTimer,
    by_viewer: std::collections::BTreeMap<InternedId, SightRefreshClient>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct SightRefreshClient {
    timer: crate::sim::timer::CdTimer,
    last_height: i32,
}
impl SightRefreshTimers {
    pub(crate) fn at_construction(frame: u32) -> Self {
        Self {
            initial: crate::sim::timer::CdTimer::started(frame as i32, 0),
            by_viewer: Default::default(),
        }
    }
    pub(crate) fn timer(&self, viewer: InternedId) -> crate::sim::timer::CdTimer {
        self.by_viewer
            .get(&viewer)
            .map_or(self.initial, |state| state.timer)
    }
    pub(crate) fn last_height(&self, viewer: InternedId) -> i32 {
        // Techno constructor6F2D9C initializes264 independently of sight250.
        self.by_viewer
            .get(&viewer)
            .map_or(0, |state| state.last_height)
    }
    fn client_mut(&mut self, viewer: InternedId) -> &mut SightRefreshClient {
        self.by_viewer.entry(viewer).or_insert(SightRefreshClient {
            timer: self.initial,
            last_height: 0,
        })
    }
    pub(crate) fn set_last_height(&mut self, viewer: InternedId, height: i32) {
        self.client_mut(viewer).last_height = height;
    }
    pub(crate) fn reload(&mut self, viewer: InternedId, frame: u32) {
        self.client_mut(viewer).timer = crate::sim::timer::CdTimer::started(frame as i32, 15);
    }

    /// The viewer has its own countdown.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn has_timer_for_test(&self, viewer: InternedId) -> bool {
        self.by_viewer.contains_key(&viewer)
    }
}
