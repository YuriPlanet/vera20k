//! Signed frame-anchored countdown timer shared by simulation systems.

use serde::{Deserialize, Serialize};

/// Raw sentinel stored while a countdown is paused.
pub const PAUSED_START_FRAME: i32 = -1;

/// A signed countdown anchored to the global simulation frame.
///
/// While running, `duration` is the original countdown length. Pausing
/// replaces it with the remaining length and switches `start_frame` to the
/// paused sentinel; resuming only captures a new frame anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CdTimer {
    start_frame: i32,
    duration: i32,
}

impl Default for CdTimer {
    fn default() -> Self {
        Self {
            start_frame: PAUSED_START_FRAME,
            duration: 0,
        }
    }
}

impl CdTimer {
    /// Preserve the two authoritative signed dwords verbatim.
    #[inline]
    pub const fn from_raw(start_frame: i32, duration: i32) -> Self {
        Self {
            start_frame,
            duration,
        }
    }

    /// Construct and start a countdown at `current_frame`.
    #[inline]
    pub const fn started(current_frame: i32, duration: i32) -> Self {
        Self {
            start_frame: current_frame,
            duration,
        }
    }

    /// Return the raw signed frame anchor.
    #[inline]
    pub const fn start_frame(self) -> i32 {
        self.start_frame
    }

    /// Return the raw signed countdown duration.
    #[inline]
    pub const fn duration(self) -> i32 {
        self.duration
    }

    /// Re-anchor this timer and replace its countdown duration.
    #[inline]
    pub fn start(&mut self, current_frame: i32, duration: i32) {
        self.start_frame = current_frame;
        self.duration = duration;
    }

    /// Return the signed remaining time using wrapping frame subtraction.
    #[inline]
    pub fn remaining(self, current_frame: i32) -> i32 {
        if self.start_frame == PAUSED_START_FRAME {
            self.duration
        } else {
            let elapsed = current_frame.wrapping_sub(self.start_frame);
            if elapsed < self.duration {
                self.duration.wrapping_sub(elapsed)
            } else {
                0
            }
        }
    }

    /// Expiry is the exact zero boundary.
    #[inline]
    pub fn expired(self, current_frame: i32) -> bool {
        self.remaining(current_frame) == 0
    }

    /// Freeze the current remainder in `duration`.
    #[inline]
    pub fn pause(&mut self, current_frame: i32) {
        if self.start_frame != PAUSED_START_FRAME {
            self.duration = self.remaining(current_frame);
            self.start_frame = PAUSED_START_FRAME;
        }
    }

    /// Resume a paused countdown from `current_frame`.
    #[inline]
    pub fn resume(&mut self, current_frame: i32) {
        if self.start_frame == PAUSED_START_FRAME {
            self.start_frame = current_frame;
        }
    }

    #[cfg(test)]
    #[inline]
    pub const fn is_paused(self) -> bool {
        self.start_frame == PAUSED_START_FRAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_expires_at_exact_boundary() {
        let timer = CdTimer::started(100, 5);

        assert_eq!(timer.remaining(100), 5);
        assert_eq!(timer.remaining(104), 1);
        assert_eq!(timer.remaining(105), 0);
        assert!(timer.expired(105));
        assert_eq!(timer.remaining(106), 0);
    }

    #[test]
    fn signed_frame_subtraction_wraps() {
        let timer = CdTimer::started(i32::MAX - 1, 4);

        assert_eq!(timer.remaining(i32::MIN + 1), 1);
        assert_eq!(timer.remaining(i32::MIN + 2), 0);
    }

    #[test]
    fn signed_negative_elapsed_returns_the_wrapping_remainder() {
        let timer = CdTimer::started(0, 6);

        assert_eq!(timer.remaining(i32::MIN), i32::MIN.wrapping_add(6));
        assert!(!timer.expired(i32::MIN));
    }

    #[test]
    fn pause_freezes_and_resume_reanchors_remaining_time() {
        let mut timer = CdTimer::started(100, 10);

        timer.pause(103);
        assert!(timer.is_paused());
        assert_eq!(timer.duration(), 7);
        assert_eq!(timer.remaining(500), 7);

        timer.resume(500);
        assert!(!timer.is_paused());
        assert_eq!(timer.start_frame(), 500);
        assert_eq!(timer.remaining(506), 1);
        assert!(timer.expired(507));
    }

    #[test]
    fn pausing_an_expired_timer_stores_zero() {
        let mut timer = CdTimer::started(10, 3);

        timer.pause(13);

        assert_eq!(timer, CdTimer::from_raw(PAUSED_START_FRAME, 0));
        assert!(timer.expired(999));
    }

    #[test]
    fn raw_paused_duration_is_not_implicitly_clamped() {
        let timer = CdTimer::from_raw(PAUSED_START_FRAME, -7);

        assert_eq!(timer.remaining(123), -7);
        assert!(!timer.expired(123));
    }
}
