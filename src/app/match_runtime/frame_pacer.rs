//! App-local wall-clock services for a running scenario.
//!
//! The deterministic simulation never reads wall time. The frame pacer decides
//! whether one outer event-loop iteration may admit a gameplay frame, while the
//! scenario elapsed clock supplies the score screen's match duration.

use std::time::Instant;

const FRAME_BUCKET_SHIFT: u32 = 4;
const FRAME_BUCKET_MS: u64 = 1 << FRAME_BUCKET_SHIFT;
/// Wake interval of the hidden-window match service loop: half a pacer
/// bucket, so an admitted frame is never late by more than half a bucket.
pub(crate) const HIDDEN_SERVICE_WAKE: std::time::Duration =
    std::time::Duration::from_millis(FRAME_BUCKET_MS / 2);
const MIN_TIMED_GAME_SPEED: u8 = 1;
const MAX_TIMED_GAME_SPEED: u8 = 6;
const ELAPSED_CLOCK_STOPPED: u32 = u32::MAX;
const SCORE_BUCKETS_PER_SECOND: i32 = 60;

#[cfg(windows)]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeGetTime() -> u32;
}

/// Return the app-local pacing clock.
///
/// On Windows this is the same wrapping millisecond authority used by the
/// retail executable. Other targets retain a monotonic development fallback.
pub(crate) fn wall_clock_ms(fallback_epoch: Instant, now: Instant) -> u64 {
    #[cfg(windows)]
    {
        let _ = (fallback_epoch, now);
        // SAFETY: timeGetTime takes no arguments and returns the process-wide
        // Windows uptime counter as an unsigned 32-bit millisecond word.
        u64::from(unsafe { timeGetTime() })
    }
    #[cfg(not(windows))]
    {
        now.duration_since(fallback_epoch).as_millis() as u64
    }
}

#[derive(Debug, Default)]
pub(crate) struct LocalFramePacer {
    last_frame_start_bucket: Option<u32>,
}

/// App-owned elapsed-time clock used by the end-of-match score dialog.
///
/// This is deliberately separate from deterministic simulation time: changing
/// game speed changes the number of gameplay frames, not the wall duration the
/// retail score screen reports.
#[derive(Debug)]
pub(crate) struct ScenarioElapsedClock {
    start_bucket: u32,
    accumulated_buckets: u32,
}

impl Default for ScenarioElapsedClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ScenarioElapsedClock {
    pub(crate) const fn new() -> Self {
        Self {
            start_bucket: ELAPSED_CLOCK_STOPPED,
            accumulated_buckets: 0,
        }
    }

    /// Arm a fresh scenario at zero elapsed time.
    ///
    /// gamemd provenance: Scenario elapsed clock; verified
    /// ScenarioClass__Start_Scenario @ 0x00683AB0 writes the current
    /// WallClock__Get16msBucket @ 0x006C8C40 value to ScenarioClass+0x614.
    pub(crate) fn start(&mut self, now_ms: u64) {
        self.accumulated_buckets = 0;
        self.start_bucket = frame_bucket(now_ms);
    }

    /// Accumulate the current armed span and install retail's `-1` sentinel.
    ///
    /// gamemd provenance: Scenario elapsed clock; verified
    /// StateMachine__EnterPause @ 0x00683EB0 accumulates ScenarioClass+0x61C
    /// and sentinelizes ScenarioClass+0x614 in offline modes 0 and 5.
    pub(crate) fn pause(&mut self, now_ms: u64) {
        if self.start_bucket == ELAPSED_CLOCK_STOPPED {
            return;
        }
        self.accumulated_buckets = self
            .accumulated_buckets
            .wrapping_add(frame_bucket(now_ms).wrapping_sub(self.start_bucket));
        self.start_bucket = ELAPSED_CLOCK_STOPPED;
    }

    /// Re-arm a stopped clock without charging the stopped span.
    ///
    /// gamemd provenance: Scenario elapsed clock; verified
    /// StateMachine__ExitPause @ 0x00683FB0 writes a fresh 16-ms bucket to
    /// ScenarioClass+0x614 when leaving an offline modal pause.
    pub(crate) fn resume(&mut self, now_ms: u64) {
        if self.start_bucket == ELAPSED_CLOCK_STOPPED {
            self.start_bucket = frame_bucket(now_ms);
        }
    }

    /// Stop the clock and return the signed, truncating score-dialog seconds.
    pub(crate) fn stop(&mut self, now_ms: u64) -> i32 {
        self.pause(now_ms);
        self.elapsed_seconds(now_ms)
    }

    /// Forget any prior match and leave the clock disarmed.
    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Sample the retail raw accumulator without changing clock state.
    fn elapsed_buckets(&self, now_ms: u64) -> u32 {
        self.accumulated_buckets.wrapping_add(
            (self.start_bucket != ELAPSED_CLOCK_STOPPED)
                .then(|| frame_bucket(now_ms).wrapping_sub(self.start_bucket))
                .unwrap_or(0),
        )
    }

    /// gamemd provenance: Score elapsed-time formatting; verified
    /// ScoreDialog__WndProc @ 0x005C9B10 reads ScenarioClass+0x614/+0x61C,
    /// treats the wrapped accumulator as signed, and divides it by 60.
    pub(crate) fn elapsed_seconds(&self, now_ms: u64) -> i32 {
        (self.elapsed_buckets(now_ms) as i32) / SCORE_BUCKETS_PER_SECOND
    }
}

impl LocalFramePacer {
    pub(crate) const fn new() -> Self {
        Self {
            last_frame_start_bucket: None,
        }
    }

    pub(crate) fn should_admit(&self, now_ms: u64, game_speed: u8, paused: bool) -> bool {
        if paused {
            return false;
        }
        if game_speed == 0 {
            return true;
        }
        let Some(last_bucket) = self.last_frame_start_bucket else {
            return true;
        };
        let required_buckets =
            u32::from(game_speed.clamp(MIN_TIMED_GAME_SPEED, MAX_TIMED_GAME_SPEED));
        let elapsed = frame_bucket(now_ms).wrapping_sub(last_bucket) as i32;
        elapsed >= required_buckets as i32
    }

    pub(crate) fn record_admitted_frame(&mut self, frame_start_ms: u64) {
        self.last_frame_start_bucket = Some(frame_bucket(frame_start_ms));
    }

    /// Forget the prior pacing window so the next unpaused frame runs now.
    pub(crate) fn reset_for_immediate_frame(&mut self) {
        self.last_frame_start_bucket = None;
    }

    /// Suppress another automatic frame until one full pacing window passes.
    ///
    /// This is for explicit single-step/capture work. Normal match entry and
    /// modal resume use `reset_for_immediate_frame`.
    pub(crate) fn reanchor(&mut self, now_ms: u64) {
        self.last_frame_start_bucket = Some(frame_bucket(now_ms));
    }

    #[cfg(test)]
    fn next_deadline_ms(&self, game_speed: u8) -> Option<u64> {
        if game_speed == 0 {
            return None;
        }
        let last_bucket = self.last_frame_start_bucket?;
        let required_buckets =
            u32::from(game_speed.clamp(MIN_TIMED_GAME_SPEED, MAX_TIMED_GAME_SPEED));
        Some(
            u64::from(last_bucket)
                .saturating_add(u64::from(required_buckets))
                .saturating_mul(FRAME_BUCKET_MS),
        )
    }
}

const fn frame_bucket(now_ms: u64) -> u32 {
    (now_ms as u32) >> FRAME_BUCKET_SHIFT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_unpaused_iteration_is_immediately_eligible() {
        assert!(LocalFramePacer::new().should_admit(123, 1, false));
    }

    #[test]
    fn timed_speeds_require_the_exact_bucket_distance() {
        for speed in MIN_TIMED_GAME_SPEED..=MAX_TIMED_GAME_SPEED {
            let mut pacer = LocalFramePacer::new();
            pacer.record_admitted_frame(32);
            let deadline = 32 + u64::from(speed) * FRAME_BUCKET_MS;
            assert!(!pacer.should_admit(deadline - 1, speed, false));
            assert!(pacer.should_admit(deadline, speed, false));
            assert_eq!(pacer.next_deadline_ms(speed), Some(deadline));
        }
    }

    #[test]
    fn speed_zero_is_uncapped() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(160);
        assert!(pacer.should_admit(160, 0, false));
        assert_eq!(pacer.next_deadline_ms(0), None);
    }

    #[test]
    fn pause_does_not_consume_an_eligible_frame() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(160);
        assert!(!pacer.should_admit(176, 1, true));
        assert!(pacer.should_admit(176, 1, false));
    }

    #[test]
    fn a_long_stall_still_admits_only_one_frame() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(0);
        assert!(pacer.should_admit(10_000, 1, false));
        pacer.record_admitted_frame(10_000);
        assert!(!pacer.should_admit(10_000, 1, false));
    }

    #[test]
    fn reanchor_discards_elapsed_wall_time() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(0);
        assert!(pacer.should_admit(10_000, 1, false));
        pacer.reanchor(10_000);
        assert!(!pacer.should_admit(10_000, 1, false));
        assert!(pacer.should_admit(10_016, 1, false));
    }

    #[test]
    fn out_of_range_speed_clamps_to_six_buckets() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(0);
        assert!(!pacer.should_admit(95, u8::MAX, false));
        assert!(pacer.should_admit(96, u8::MAX, false));
    }

    #[test]
    fn native_signed_bucket_subtraction_stalls_at_uptime_rollover() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(u64::from(u32::MAX - 7));

        assert!(!pacer.should_admit(u64::from(u32::MAX) + 1, 1, false));
    }

    #[test]
    fn scenario_elapsed_clock_uses_retail_sixty_bucket_seconds() {
        for (buckets, seconds) in [(59_u64, 0), (60, 1), (119, 1), (120, 2)] {
            let mut clock = ScenarioElapsedClock::new();
            clock.start(0);
            assert_eq!(clock.elapsed_seconds(buckets << 4), seconds);
        }
    }

    #[test]
    fn scenario_elapsed_clock_excludes_explicit_modal_time() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(10 << 4);
        clock.pause(70 << 4);
        assert_eq!(clock.elapsed_seconds(10_000 << 4), 1);

        clock.resume(1_070 << 4);
        assert_eq!(clock.stop(1_130 << 4), 2);
    }

    #[test]
    fn scenario_elapsed_clock_uninterrupted_wall_span_ignores_frame_activity() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(200 << 4);

        // No clock hook runs for focus loss or for any number of sim frames.
        assert_eq!(clock.elapsed_seconds(320 << 4), 2);
    }

    #[test]
    fn scenario_elapsed_clock_preserves_native_rollover_arithmetic() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(u64::from(u32::MAX - 15));

        // timeGetTime wraps before the >>4. Native's uncorrected unsigned
        // subtraction becomes negative only when the score path casts to i32.
        assert_eq!(clock.elapsed_seconds(u64::from(u32::MAX) + 1), -4_473_924);
    }
}
