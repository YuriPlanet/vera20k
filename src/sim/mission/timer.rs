//! Frame-anchored timer primitives used by Mission-related systems.
//!
//! Both read the shared signed timer ([`CdTimer`]) against the simulation's
//! unsigned frame counter. `MissionTimer` keeps the public raw-bit fields used
//! by docking, gates, and miners; `MissionDispatchTimer` is the Mission
//! dispatch timer.
use serde::{Deserialize, Serialize};

use crate::sim::timer::CdTimer;

/// [`MissionTimer`]'s unarmed anchor. `u32::MAX`: the live counter starts at 0
/// and would take ~3.3 years at 15fps to reach it, so it is never a live value.
///
/// "Unarmed" reads as "due" only because the default timer pairs it with a zero
/// duration — the anchor alone does not force readiness, exactly as on
/// [`MissionDispatchTimer`].
pub const SENTINEL: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissionTimer {
    start_frame: u32,
    duration: u32,
}

impl MissionTimer {
    /// See the field.
    pub(crate) const fn start_frame(&self) -> u32 {
        self.start_frame
    }

    /// A fixture's value.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_start_frame_for_test(&mut self, start_frame: u32) {
        self.start_frame = start_frame;
    }

    /// See the field.
    pub(crate) const fn duration(&self) -> u32 {
        self.duration
    }
}

/// Native Mission-dispatch timer state: a [`CdTimer`] whose frame is the
/// simulation's unsigned counter reinterpreted as the same 32 raw bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MissionDispatchTimer(CdTimer);

impl MissionDispatchTimer {
    /// Construct a timer anchored at the supplied simulation frame and due
    /// immediately because its delay is zero.
    #[inline]
    pub const fn at_frame(frame: u32) -> Self {
        Self(CdTimer::started(frame as i32, 0))
    }

    /// Preserve raw native timer dwords without normalization.
    #[inline]
    pub const fn from_raw(start_frame: i32, delay: i32) -> Self {
        Self(CdTimer::from_raw(start_frame, delay))
    }

    /// Return the raw signed start-frame dword.
    #[inline]
    pub const fn start_frame(self) -> i32 {
        self.0.start_frame()
    }

    /// Return the raw signed delay dword.
    #[inline]
    pub const fn delay(self) -> i32 {
        self.0.duration()
    }

    /// Native Mission-dispatch readiness: the timer has expired.
    /// `MissionClass::Mission_Dispatch` 0x005B3060 tests the start at
    /// 0x005B308C and, on the unanchored `-1`, skips the elapsed computation
    /// at 0x005B3091-0x005B309D, so the shared `TEST EAX,EAX / JNZ` at
    /// 0x005B309F sees the raw delay: an unanchored timer is due only when
    /// its delay is also zero.
    #[inline]
    pub fn due(self, now: u32) -> bool {
        self.0.expired(now as i32)
    }

    /// The signed wrapping remainder while dispatch is pending.
    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    pub fn remaining_if_pending(self, now: u32) -> Option<i32> {
        let remaining = self.0.remaining(now as i32);
        (remaining != 0).then_some(remaining)
    }
}

impl Default for MissionTimer {
    fn default() -> Self {
        Self {
            start_frame: SENTINEL,
            duration: 0,
        }
    }
}

impl MissionTimer {
    #[inline]
    fn countdown(self) -> CdTimer {
        CdTimer::from_raw(self.start_frame as i32, self.duration as i32)
    }

    /// Construct armed at `start_frame` for `duration` frames.
    #[inline]
    pub fn armed(start_frame: u32, duration: u32) -> Self {
        Self {
            start_frame,
            duration,
        }
    }

    /// `true` once `duration` frames have elapsed since `start_frame` (inclusive),
    /// or always when unarmed.
    #[inline]
    pub fn due(self, now: u32) -> bool {
        self.countdown().expired(now as i32)
    }

    /// Re-anchor at `now` for `n` frames.
    #[inline]
    pub fn defer(&mut self, now: u32, n: u32) {
        self.start_frame = now;
        self.duration = n;
    }

    /// Alias of [`MissionTimer::defer`].
    #[inline]
    pub fn arm(&mut self, now: u32, n: u32) {
        self.defer(now, n);
    }

    /// Disarm → always due.
    #[cfg(test)]
    #[inline]
    pub fn clear(&mut self) {
        self.start_frame = SENTINEL;
        self.duration = 0;
    }

    /// `true` while a live frame anchor is set.
    #[cfg(test)]
    #[inline]
    pub fn is_armed(self) -> bool {
        self.start_frame != SENTINEL
    }

    /// Frames since the anchor (0 when unarmed).
    #[cfg(test)]
    #[inline]
    pub fn elapsed(self, now: u32) -> u32 {
        if self.start_frame == SENTINEL {
            0
        } else {
            now.wrapping_sub(self.start_frame)
        }
    }

    /// Frames left before due (0 when unarmed or already due; saturating).
    #[inline]
    pub fn remaining(self, now: u32) -> u32 {
        self.countdown().remaining(now as i32).max(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_dispatch_raw_fields_and_frame_bits_round_trip() {
        let raw = MissionDispatchTimer::from_raw(i32::MIN, i32::MAX);
        assert_eq!(raw.start_frame(), i32::MIN);
        assert_eq!(raw.delay(), i32::MAX);

        let high_bit_frame = MissionDispatchTimer::at_frame(0x8000_0000);
        assert_eq!(high_bit_frame.start_frame(), i32::MIN);
        assert_eq!(high_bit_frame.delay(), 0);
        assert!(high_bit_frame.due(0x8000_0000));
    }

    #[test]
    fn signed_dispatch_unanchored_start_is_due_only_on_a_zero_delay() {
        // 0x005B308C jumps past the elapsed computation, so the raw delay is
        // what the shared `TEST EAX,EAX` at 0x005B309F sees.
        let live = MissionDispatchTimer::from_raw(-1, i32::MAX);
        assert!(!live.due(0));
        assert!(!live.due(u32::MAX));
        assert_eq!(live.remaining_if_pending(0), Some(i32::MAX));
        assert_eq!(live.remaining_if_pending(u32::MAX), Some(i32::MAX));

        let expired = MissionDispatchTimer::from_raw(-1, 0);
        assert!(expired.due(0));
        assert!(expired.due(u32::MAX));
        assert_eq!(expired.remaining_if_pending(0), None);
    }

    #[test]
    fn signed_dispatch_negative_delay_is_due_at_anchor() {
        let timer = MissionDispatchTimer::from_raw(10, -1);

        assert!(timer.due(10));
        assert_eq!(timer.remaining_if_pending(10), None);
    }

    #[test]
    fn signed_dispatch_exact_boundary_is_inclusive() {
        let timer = MissionDispatchTimer::from_raw(10, 5);

        assert!(!timer.due(14));
        assert_eq!(timer.remaining_if_pending(14), Some(1));
        assert!(timer.due(15));
        assert_eq!(timer.remaining_if_pending(15), None);
    }

    #[test]
    fn signed_dispatch_wraparound_uses_signed_elapsed_comparison() {
        let start = i32::MAX - 1;
        let now = (i32::MIN + 1) as u32;

        let pending = MissionDispatchTimer::from_raw(start, 4);
        assert!(!pending.due(now));
        assert_eq!(pending.remaining_if_pending(now), Some(1));

        let due = MissionDispatchTimer::from_raw(start, 3);
        assert!(due.due(now));
        assert_eq!(due.remaining_if_pending(now), None);
    }

    #[test]
    fn signed_dispatch_remaining_uses_wrapping_subtraction() {
        let timer = MissionDispatchTimer::from_raw(10, i32::MAX);
        let now = (10i32.wrapping_add(i32::MIN)) as u32;

        assert!(!timer.due(now));
        assert_eq!(timer.remaining_if_pending(now), Some(-1));
    }

    #[test]
    fn unarmed_is_always_due() {
        let t = MissionTimer::default();
        assert!(!t.is_armed());
        assert!(t.due(0));
        assert!(t.due(12_345));
    }

    #[test]
    fn inclusive_due_boundary() {
        let t = MissionTimer::armed(10, 5);
        assert!(t.is_armed());
        assert!(!t.due(14)); // 14 - 10 = 4 < 5
        assert!(t.due(15)); // 15 - 10 = 5 >= 5 (inclusive)
        assert!(t.due(16));
    }

    #[test]
    fn defer_zero_is_due_next_check() {
        let mut t = MissionTimer::armed(100, 50);
        t.defer(200, 0);
        assert_eq!((t.start_frame, t.duration), (200, 0));
        assert!(t.due(200)); // 0 >= 0
    }

    #[test]
    fn arm_aliases_defer() {
        let mut a = MissionTimer::default();
        a.arm(10, 7);
        assert_eq!((a.start_frame, a.duration), (10, 7));
    }

    #[test]
    fn clear_makes_due_again() {
        let mut t = MissionTimer::armed(10, 100);
        assert!(!t.due(20));
        t.clear();
        assert!(!t.is_armed());
        assert!(t.due(20));
    }

    #[test]
    fn elapsed_and_remaining() {
        let t = MissionTimer::armed(10, 5);
        assert_eq!(t.elapsed(13), 3);
        assert_eq!(t.remaining(13), 2);
        assert_eq!(t.remaining(15), 0);
        assert_eq!(t.remaining(99), 0); // saturating, never underflows
        let s = MissionTimer::default();
        assert_eq!(s.elapsed(50), 0);
        assert_eq!(s.remaining(50), 0);
    }

    #[test]
    fn reversal_arithmetic_matches_gate_reverse() {
        // Mirrors the building gate's direction-reversal math: a transition armed
        // at frame 100 for 39 ticks, reversed at frame 110 against a nominal total
        // of 39, yields duration 10 with the start-frame baseline preserved.
        let mut t = MissionTimer::armed(100, 39);
        let total = 39u32;
        let elapsed = t.elapsed(110);
        let live_remaining = t.duration.saturating_sub(elapsed);
        t.duration = total.saturating_sub(live_remaining);
        assert_eq!(t.duration, 10);
        assert_eq!(t.start_frame, 100);
    }

    #[test]
    fn wraparound_delta_is_correct() {
        // Anchor near the top of u32; `now` has wrapped past 0.
        let t = MissionTimer::armed(u32::MAX - 2, 5);
        assert!(t.due(2)); // 2 - (MAX-2) wraps to 5 -> due (5 >= 5)
        assert!(!t.due(1)); // wraps to 4 -> not due
    }
}
