//! App-local wall-clock services for a running scenario.
//!
//! The deterministic simulation never reads wall time. The frame pacer decides
//! whether one outer event-loop iteration may admit a gameplay frame, while the
//! scenario elapsed clock supplies the score screen's match duration.

use crate::net::live_commands::SequencedTiming;
use crate::util::time::{Duration, Instant};

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

/// Return system uptime for the app's wall-clock consumers.
///
/// Original6C8C40/50 reads timeGetTime, then selects the low32 millisecond
/// word before shifting. Keep a continuing system clock across process
/// restarts: Mouse5BE092 retains raw Power Flash timestamps. Consumers select
/// their own units/word; the deterministic simulation never reads this clock.
/// Windows: learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timegettime
/// Darwin RAW includes sleep (Apple Libc/gen/clock_gettime.c); Linux BOOTTIME
/// includes suspend (docs.kernel.org/core-api/timekeeping.html). Exact historic
/// Windows suspend behavior is not established by the same-boot restart proof.
pub(crate) fn wall_clock_ms() -> u64 {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        static START: std::sync::OnceLock<crate::util::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(crate::util::time::Instant::now)
            .elapsed()
            .as_millis() as u64
    }
    #[cfg(windows)]
    {
        // SAFETY: timeGetTime takes no arguments and returns the process-wide
        // Windows uptime counter as an unsigned 32-bit millisecond word.
        u64::from(unsafe { timeGetTime() })
    }
    #[cfg(unix)]
    {
        #[cfg(target_os = "macos")]
        let clock = libc::CLOCK_MONOTONIC_RAW;
        #[cfg(target_os = "linux")]
        let clock = libc::CLOCK_BOOTTIME;
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let clock = libc::CLOCK_MONOTONIC;
        let mut sample = std::mem::MaybeUninit::<libc::timespec>::uninit();
        // SAFETY: sample points to writable, aligned timespec storage. The
        // valid system clock writes it on success, checked before reading.
        let result = unsafe { libc::clock_gettime(clock, sample.as_mut_ptr()) };
        assert_eq!(
            result,
            0,
            "presentation uptime: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: the successful clock_gettime call initialized both fields.
        let sample = unsafe { sample.assume_init() };
        (sample.tv_sec as u64) * 1_000 + (sample.tv_nsec as u64) / 1_000_000
    }
}

#[derive(Debug, Default)]
pub(crate) struct LocalFramePacer {
    last_frame_start_ms: Option<u32>,
    last_presented: Option<crate::util::time::Instant>,
    frames_since_present: u32,
    chunks_since_present: u8,
    render_deferred: bool,
    total_render_deferrals: u64,
    online: Option<OnlinePacer>,
    /// DEBUG ONLY, not a gamemd feature: frames run per paced pass in an
    /// offline Skirmish, 1 to `MAX_DEBUG_SPEED`, set by the Ctrl+Shift dev
    /// chord. Every frame keeps its own transaction, so the simulation and its
    /// replay log are unchanged.
    debug_speed: u8,
}

#[cfg(feature = "dev-overlays")]
const MAX_DEBUG_SPEED: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnlineFrameDecision {
    WaitingForCanonical,
    HoldUntil(Instant),
    Run,
    CatchUp,
}
impl OnlineFrameDecision {
    pub(crate) fn admits(self) -> bool {
        matches!(self, Self::Run | Self::CatchUp)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OnlinePacingStatus {
    pub(crate) reserve_micros: u32,
    pub(crate) underruns: u64,
    pub(crate) catching_up: bool,
}

/// A local playout schedule, never another simulation or relay timeline.
/// Receipt frames are immutable network facts; `next_frame` always comes
/// from Simulation. Late jitter changes only the bounded reserve; an early
/// receipt moves the origin, so no hold exceeds that reserve.
#[derive(Debug, Clone)]
struct OnlinePacer {
    origin_frame: u32,
    origin_at: Instant,
    receipt: SequencedTiming,
    reserve: Duration,
    decay_at: Instant,
    stable: Duration,
    underrun: Option<(u32, Instant)>,
    underruns: u64,
    catching_up: bool,
}
impl OnlinePacer {
    fn new(receipt: SequencedTiming, now: Instant) -> Self {
        Self {
            origin_frame: receipt.frame,
            origin_at: receipt.received_at,
            receipt,
            reserve: Duration::ZERO,
            decay_at: now,
            stable: Duration::ZERO,
            underrun: None,
            underruns: 0,
            catching_up: false,
        }
    }

    fn same_clock(&self, receipt: SequencedTiming) -> bool {
        self.receipt.frames_per_second == receipt.frames_per_second
            && self.receipt.clock_epoch == receipt.clock_epoch
    }

    /// A confirmed relay resume starts a new clock at its socket receipt.
    /// Retain only the bounded jitter estimate, never paused-time debt or an
    /// underrun from the intentionally closed frontier.
    fn reset_clock(&mut self, receipt: SequencedTiming, now: Instant) {
        let reserve = (self.receipt.frames_per_second == receipt.frames_per_second)
            .then_some(self.reserve)
            .unwrap_or_default();
        let underruns = self.underruns;
        *self = Self::new(receipt, now);
        self.reserve = reserve.min(Duration::from_millis(20)).min(self.span(1));
        self.underruns = underruns;
    }

    fn span(&self, frames: u32) -> Duration {
        // One division of the whole frame distance avoids rounded per-tick
        // drift (45Hz is not an integral number of milliseconds/nanoseconds).
        Duration::from_nanos(
            u64::from(frames) * 1_000_000_000 / u64::from(self.receipt.frames_per_second),
        )
    }

    /// The schedule's receipt time for `frame`, before the reserve.
    fn scheduled(&self, frame: u32) -> Instant {
        if frame >= self.origin_frame {
            self.origin_at
                .checked_add(self.span(frame - self.origin_frame))
                .unwrap_or(self.origin_at)
        } else {
            self.origin_at
                .checked_sub(self.span(self.origin_frame - frame))
                .unwrap_or(self.origin_at)
        }
    }

    fn deadline(&self, frame: u32) -> Instant {
        let base = self.scheduled(frame);
        base.checked_add(self.reserve).unwrap_or(base)
    }

    fn learn(&mut self, required: Duration, now: Instant) {
        let required = required.min(Duration::from_millis(20)).min(self.span(1));
        if required > self.reserve {
            self.reserve = required;
            self.decay_at = now;
            self.stable = Duration::ZERO;
        }
    }

    fn observe(&mut self, receipt: SequencedTiming, now: Instant, recovering: bool) {
        if receipt.frame > self.receipt.frame {
            if !recovering && !self.catching_up {
                let interval = receipt
                    .received_at
                    .saturating_duration_since(self.receipt.received_at);
                self.learn(
                    interval.saturating_sub(self.span(receipt.frame - self.receipt.frame)),
                    now,
                );
            }
            // No frame beats the path's fastest delivery, so a receipt before
            // its scheduled time proves the origin late: a delayed first
            // receipt, or a re-anchor on a repair burst's arrival. Holding to
            // that origin would add a delay the reserve never reports.
            if receipt.received_at < self.scheduled(receipt.frame) {
                self.origin_frame = receipt.frame;
                self.origin_at = receipt.received_at;
            }
            self.receipt = receipt;
        }
        if !recovering && !self.catching_up && self.underrun.is_none() {
            // Stable delivery returns 1ms per second. Accrue it between
            // observations: a slow renderer catches up after most passes,
            // and each burst only pauses the decay instead of restarting it.
            self.stable += now.saturating_duration_since(self.decay_at);
            let seconds = self.stable.as_secs();
            self.reserve = self.reserve.saturating_sub(Duration::from_millis(seconds));
            self.stable -= Duration::from_secs(seconds);
        }
        self.decay_at = now;
    }

    fn decide(
        &mut self,
        now: Instant,
        next_frame: u32,
        frontier: u32,
        receipt: SequencedTiming,
        recovering: bool,
    ) -> OnlineFrameDecision {
        self.observe(receipt, now, recovering);
        let available = if next_frame <= frontier {
            frontier - next_frame + 1
        } else {
            0
        };
        let deadline = self.deadline(next_frame);
        if recovering {
            self.catching_up = true;
        }
        if available == 0 {
            if !recovering
                && !self.catching_up
                && now >= deadline
                && self.underrun.is_none_or(|(frame, _)| frame != next_frame)
            {
                self.underrun = Some((next_frame, deadline));
                self.underruns = self.underruns.saturating_add(1);
                self.stable = Duration::ZERO;
            }
            return OnlineFrameDecision::WaitingForCanonical;
        }
        if let Some((frame, deadline)) = self.underrun.take()
            && frame == next_frame
            && !recovering
        {
            self.learn(receipt.received_at.saturating_duration_since(deadline), now);
        }
        if !self.catching_up
            && (available >= 3
                || (available >= 2
                    && now.saturating_duration_since(self.deadline(next_frame)) > self.span(2)))
        {
            self.catching_up = true;
        }
        if self.catching_up {
            if recovering || available > 1 {
                return OnlineFrameDecision::CatchUp;
            }
            // A real recovery/new playout phase may re-anchor. Ordinary late
            // receipts never move the phase origin later.
            self.origin_frame = self.receipt.frame;
            self.origin_at = self.receipt.received_at;
            self.catching_up = false;
        }
        let deadline = self.deadline(next_frame);
        if now >= deadline {
            OnlineFrameDecision::Run
        } else {
            OnlineFrameDecision::HoldUntil(deadline)
        }
    }
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
    // A relay hold preserves the native clock's running intent. It is local
    // presentation state, never saved with Scenario+614/+61C.
    multiplayer_resume: Option<bool>,
}

/// Temporary source-clock continuation across the synchronous save write.
pub(crate) struct ScenarioClockSave {
    accumulated_buckets: u32,
    resume_source: bool,
}

impl ScenarioClockSave {
    pub(crate) fn accumulated_buckets(&self) -> u32 {
        self.accumulated_buckets
    }
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
            multiplayer_resume: None,
        }
    }

    /// ScenarioSave689310 stops +614 and accumulates +61C before689378's
    /// stream write. The source resumes at689454 only if it was running.
    /// Original wrapping/bucket controls: storage_oracle/saved_game_layout.
    pub(crate) fn begin_save(&mut self, now_ms: u64) -> ScenarioClockSave {
        let resume_source = self
            .multiplayer_resume
            .unwrap_or(self.start_bucket != ELAPSED_CLOCK_STOPPED);
        self.pause(now_ms);
        ScenarioClockSave {
            accumulated_buckets: self.accumulated_buckets,
            resume_source,
        }
    }

    pub(crate) fn finish_save(&mut self, saved: ScenarioClockSave, now_ms: u64) {
        if saved.resume_source {
            self.resume(now_ms);
        }
    }

    /// ScenarioLoad689630..641 rearms the saved stopped +614 without
    /// resetting accumulated score time. PostRead683560 resets another timer.
    pub(crate) fn from_saved_accumulator(accumulated_buckets: u32, now_ms: u64) -> Self {
        Self {
            start_bucket: frame_bucket(now_ms),
            accumulated_buckets,
            multiplayer_resume: None,
        }
    }

    /// Arm a fresh scenario at zero elapsed time.
    ///
    /// gamemd provenance: Scenario elapsed clock; verified
    /// ScenarioClass__Start_Scenario @ 0x00683AB0 writes the current
    /// WallClock__Get16msBucket @ 0x006C8C40 value to ScenarioClass+0x614.
    pub(crate) fn start(&mut self, now_ms: u64) {
        self.accumulated_buckets = 0;
        self.multiplayer_resume = None;
        self.start_bucket = frame_bucket(now_ms);
    }

    /// Accumulate the current armed span and install retail's `-1` sentinel.
    ///
    /// gamemd provenance: Scenario elapsed clock; verified
    /// StateMachine__EnterPause @ 0x00683EB0 accumulates ScenarioClass+0x61C
    /// and sentinelizes ScenarioClass+0x614 in offline modes 0 and 5.
    pub(crate) fn pause(&mut self, now_ms: u64) {
        if let Some(resume) = &mut self.multiplayer_resume {
            *resume = false;
        }
        self.accumulate_and_stop(now_ms);
    }

    fn accumulate_and_stop(&mut self, now_ms: u64) {
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
        if let Some(resume) = &mut self.multiplayer_resume {
            *resume = true;
        } else if self.start_bucket == ELAPSED_CLOCK_STOPPED {
            self.start_bucket = frame_bucket(now_ms);
        }
    }

    /// Compose the relay's completed-frame hold with native clock ownership.
    /// Native pause/resume calls still update their intent while this hold
    /// prevents time from accumulating; releasing it never undoes their stop.
    pub(crate) fn set_multiplayer_paused(&mut self, paused: bool, now_ms: u64) {
        match (paused, self.multiplayer_resume) {
            (true, None) => {
                self.multiplayer_resume = Some(self.start_bucket != ELAPSED_CLOCK_STOPPED);
                self.accumulate_and_stop(now_ms);
            }
            (false, Some(resume)) => {
                self.multiplayer_resume = None;
                if resume {
                    self.resume(now_ms);
                }
            }
            _ => {}
        }
    }

    /// Stop the clock and return the signed, truncating score-dialog seconds.
    pub(crate) fn stop(&mut self, now_ms: u64) -> i32 {
        self.stop_buckets(now_ms) / SCORE_BUCKETS_PER_SECOND
    }

    /// Stop the clock and return the raw signed Scenario+0x61C accumulator.
    /// The campaign score writer compares each raw counter after signed
    /// division by 60 (`0x006C9231..33`), so callers that choose a result must
    /// retain this value through that comparison.
    pub(crate) fn stop_buckets(&mut self, now_ms: u64) -> i32 {
        self.pause(now_ms);
        self.accumulated_buckets as i32
    }

    /// Forget any prior match and leave the clock disarmed.
    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Sample the retail raw accumulator without changing clock state.
    #[cfg(any(
        test,
        all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
    ))]
    fn elapsed_buckets(&self, now_ms: u64) -> u32 {
        self.accumulated_buckets
            .wrapping_add(if self.start_bucket != ELAPSED_CLOCK_STOPPED {
                frame_bucket(now_ms).wrapping_sub(self.start_bucket)
            } else {
                0
            })
    }

    /// gamemd provenance: Score elapsed-time formatting; verified
    /// ScoreDialog__WndProc @ 0x005C9B10 reads ScenarioClass+0x614/+0x61C,
    /// treats the wrapped accumulator as signed, and divides it by 60.
    /// The browser match report reads it after abort/outcome stopped the clock.
    #[cfg(any(
        test,
        all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(crate) fn elapsed_seconds(&self, now_ms: u64) -> i32 {
        (self.elapsed_buckets(now_ms) as i32) / SCORE_BUCKETS_PER_SECOND
    }
}

impl LocalFramePacer {
    /// The sequencer owns the agreed network cadence. Buffered complete
    /// frames, rather than a second local timer, admit recovery ticks.
    /// Frame zero is the startup handshake and must keep servicing its owner.
    pub(crate) fn admits_canonical_frame(frame: u32, committed_frontier: u32) -> bool {
        frame == 0 || frame <= committed_frontier
    }

    pub(crate) fn permits_catch_up(attempts: usize, elapsed: std::time::Duration) -> bool {
        attempts < 8 && elapsed < std::time::Duration::from_millis(8)
    }

    pub(crate) const fn new() -> Self {
        Self {
            last_frame_start_ms: None,
            last_presented: None,
            frames_since_present: 0,
            chunks_since_present: 0,
            render_deferred: false,
            total_render_deferrals: 0,
            online: None,
            debug_speed: 1,
        }
    }

    pub(crate) fn debug_speed(&self) -> u8 {
        self.debug_speed
    }

    /// Step the debug speed by one within 1x..=10x.
    #[cfg(feature = "dev-overlays")]
    pub(crate) fn change_debug_speed(&mut self, faster: bool) -> u8 {
        let speed = if faster {
            self.debug_speed + 1
        } else {
            self.debug_speed - 1
        };
        self.debug_speed = speed.clamp(1, MAX_DEBUG_SPEED);
        self.debug_speed
    }

    pub(crate) fn online_frame_decision(
        &mut self,
        now: Instant,
        next_frame: u32,
        frontier: u32,
        receipt: SequencedTiming,
        resuming: bool,
        recovering: bool,
    ) -> OnlineFrameDecision {
        // A retained transaction already owns its pacing admission. Missing
        // canonical bytes still block it; a local reserve never does.
        if next_frame == 0 || resuming {
            return if Self::admits_canonical_frame(next_frame, frontier) {
                OnlineFrameDecision::Run
            } else {
                OnlineFrameDecision::WaitingForCanonical
            };
        }
        if receipt.frames_per_second == 0 {
            return OnlineFrameDecision::WaitingForCanonical;
        }
        match self.online.as_mut() {
            Some(online) if !online.same_clock(receipt) => online.reset_clock(receipt, now),
            None => self.online = Some(OnlinePacer::new(receipt, now)),
            _ => {}
        }
        self.online
            .as_mut()
            .unwrap()
            .decide(now, next_frame, frontier, receipt, recovering)
    }

    /// Read-only continuation uses the same policy on a small derived copy;
    /// it cannot learn a second time or consume any simulation/network state.
    pub(crate) fn online_catch_up_ready(
        &self,
        now: Instant,
        next_frame: u32,
        frontier: u32,
        receipt: SequencedTiming,
        recovering: bool,
    ) -> bool {
        if next_frame == 0 || receipt.frames_per_second == 0 {
            return false;
        }
        let mut online = self
            .online
            .as_ref()
            .cloned()
            .unwrap_or_else(|| OnlinePacer::new(receipt, now));
        if !online.same_clock(receipt) {
            online.reset_clock(receipt, now);
        }
        matches!(
            online.decide(now, next_frame, frontier, receipt, recovering),
            OnlineFrameDecision::CatchUp
        )
    }

    pub(crate) fn online_pacing_status(&self) -> Option<OnlinePacingStatus> {
        self.online.as_ref().map(|online| OnlinePacingStatus {
            reserve_micros: online.reserve.as_micros() as u32,
            underruns: online.underruns,
            catching_up: online.catching_up,
        })
    }

    pub(crate) fn reset_for_new_world(&mut self) {
        self.reset_for_immediate_frame();
        self.online = None;
        self.debug_speed = 1;
    }

    /// Clear a queued continuation on entry or loss of render eligibility.
    /// Neither operation pretends its surface was presented.
    pub(crate) fn clear_render_continuation(&mut self) {
        self.render_deferred = false;
    }

    /// Yield to winit between ordinary online catch-up chunks. Only completed
    /// transactions count; neither a blocked frame nor a modal may defer.
    /// Four existing chunks contain at most 32 ticks, below checksum history.
    pub(crate) fn defer_online_render(
        &mut self,
        now: crate::util::time::Instant,
        completed_frames: u32,
        last_frame_completed: bool,
        next_canonical: Option<(u32, u32)>,
    ) -> bool {
        self.frames_since_present = self.frames_since_present.saturating_add(completed_frames);
        self.chunks_since_present = self
            .chunks_since_present
            .saturating_add(u8::from(completed_frames > 0));
        self.render_deferred = completed_frames > 0
            && last_frame_completed
            && next_canonical.is_some_and(|(frame, frontier)| frame > 0 && frame <= frontier)
            && self.last_presented.is_some_and(|last| {
                now.saturating_duration_since(last) < std::time::Duration::from_millis(50)
            })
            && self.frames_since_present < 32
            && self.chunks_since_present < 4;
        if self.render_deferred {
            self.total_render_deferrals = self.total_render_deferrals.saturating_add(1);
        }
        self.render_deferred
    }

    pub(crate) fn render_deferred(&self) -> bool {
        self.render_deferred
    }

    pub(crate) fn total_render_deferrals(&self) -> u64 {
        self.total_render_deferrals
    }

    /// Reset only after a successful presentation. A shell/loading surface
    /// cannot make the first battlefield presentation eligible for deferral.
    pub(crate) fn record_presented(&mut self, now: crate::util::time::Instant, battlefield: bool) {
        self.last_presented = battlefield.then_some(now);
        self.frames_since_present = 0;
        self.chunks_since_present = 0;
        self.render_deferred = false;
    }

    /// Read the same bucket interval used by `should_admit`, for diagnostics.
    pub(crate) fn budget_ms(&self, game_speed: u8, network_fps: Option<(u16, u32)>) -> Option<f32> {
        if let Some((fps, delay)) = network_fps {
            return Some(network_interval_ms(fps).saturating_add(delay) as f32);
        }
        (game_speed != 0).then(|| {
            f32::from(game_speed.clamp(MIN_TIMED_GAME_SPEED, MAX_TIMED_GAME_SPEED))
                * FRAME_BUCKET_MS as f32
        })
    }

    /// Read-only presentation phase of the existing bucket-paced frame.
    /// Use the pacer's bucket origin, not the end of a potentially expensive
    /// simulation/render preparation, to avoid restarting visual time late.
    pub(crate) fn interpolation_elapsed_ms(&self, now_ms: u64) -> Option<u32> {
        self.last_frame_start_ms.map(|ms| {
            (now_ms as u32).wrapping_sub((ms >> FRAME_BUCKET_SHIFT) << FRAME_BUCKET_SHIFT)
        })
    }

    pub(crate) fn should_admit(
        &self,
        now_ms: u64,
        game_speed: u8,
        network_fps: Option<(u16, u32)>,
        paused: bool,
    ) -> bool {
        if paused {
            return false;
        }
        // MainTick55D4FB chooses 1000/FPS for network games, and
        // MainThrottle55E1BC consumes that millisecond timer. Offline pacing
        // retains its distinct 16ms bucket clock (55E2B4).
        if let Some((fps, delay)) = network_fps {
            return self.last_frame_start_ms.is_none_or(|last| {
                (now_ms as u32).wrapping_sub(last) as i32
                    >= network_interval_ms(fps).saturating_add(delay) as i32
            });
        }
        if game_speed == 0 {
            return true;
        }
        let Some(last_bucket) = self.last_frame_start_ms.map(|ms| ms >> FRAME_BUCKET_SHIFT) else {
            return true;
        };
        let required_buckets =
            u32::from(game_speed.clamp(MIN_TIMED_GAME_SPEED, MAX_TIMED_GAME_SPEED));
        let elapsed = frame_bucket(now_ms).wrapping_sub(last_bucket) as i32;
        elapsed >= required_buckets as i32
    }

    pub(crate) fn record_admitted_frame(&mut self, frame_start_ms: u64) {
        self.last_frame_start_ms = Some(frame_start_ms as u32);
    }

    /// Forget the prior pacing window so the next unpaused frame runs now.
    pub(crate) fn reset_for_immediate_frame(&mut self) {
        self.last_frame_start_ms = None;
    }

    /// Suppress another automatic frame until one full pacing window passes.
    ///
    /// This is for explicit single-step/capture work. Normal match entry and
    /// modal resume use `reset_for_immediate_frame`.
    pub(crate) fn reanchor(&mut self, now_ms: u64) {
        self.last_frame_start_ms = Some(now_ms as u32);
    }

    #[cfg(test)]
    fn next_deadline_ms(&self, game_speed: u8) -> Option<u64> {
        if game_speed == 0 {
            return None;
        }
        let last_bucket = self.last_frame_start_ms? >> FRAME_BUCKET_SHIFT;
        let required_buckets =
            u32::from(game_speed.clamp(MIN_TIMED_GAME_SPEED, MAX_TIMED_GAME_SPEED));
        Some(
            u64::from(last_bucket)
                .saturating_add(u64::from(required_buckets))
                .saturating_mul(FRAME_BUCKET_MS),
        )
    }
}

fn network_interval_ms(frames_per_second: u16) -> u32 {
    // MainTick55D499 has the explicit FPS0 fallback of33 milliseconds.
    if frames_per_second == 0 {
        33
    } else {
        1000 / u32::from(frames_per_second)
    }
}

pub(crate) const fn frame_bucket(now_ms: u64) -> u32 {
    (now_ms as u32) >> FRAME_BUCKET_SHIFT
}

/// `WallClock__Get16msBucket @ 0x006C8C40` now.
pub(crate) fn wall_clock_bucket() -> u32 {
    frame_bucket(wall_clock_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_receipt(at: Instant, frame: u32) -> SequencedTiming {
        SequencedTiming {
            frame,
            received_at: at,
            frames_per_second: 45,
            clock_epoch: 0,
        }
    }

    #[cfg(feature = "dev-overlays")]
    #[test]
    fn debug_speed_steps_within_1x_to_10x_and_resets_for_a_new_world() {
        let mut pacer = LocalFramePacer::new();
        assert_eq!(pacer.change_debug_speed(false), 1);
        for _ in 0..12 {
            pacer.change_debug_speed(true);
        }
        assert_eq!(pacer.debug_speed(), 10);
        pacer.reset_for_new_world();
        assert_eq!(pacer.debug_speed(), 1);
    }

    #[test]
    fn confirmed_resume_reanchors_once_without_pause_debt_or_losing_jitter_reserve() {
        let start = Instant::now();
        let tick = Duration::from_nanos(1_000_000_000 / 45);
        let mut pacer = LocalFramePacer::new();
        pacer.online_frame_decision(start, 1, 1, push_receipt(start, 1), false, false);
        let late = push_receipt(start + tick + Duration::from_millis(15), 2);
        pacer.online_frame_decision(late.received_at, 2, 2, late, false, false);
        let resumed_at = start + Duration::from_secs(30);
        let resumed = SequencedTiming {
            clock_epoch: 1,
            ..push_receipt(resumed_at, 2)
        };
        let due = resumed_at + tick + Duration::from_millis(15);
        assert!(!pacer.online_catch_up_ready(resumed_at, 3, 3, resumed, false));
        for _ in 0..2 {
            assert_eq!(
                pacer.online_frame_decision(resumed_at, 3, 3, resumed, false, false),
                OnlineFrameDecision::HoldUntil(due)
            );
        }
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 15_000);
        assert_eq!(
            pacer.online_frame_decision(due, 3, 3, resumed, false, false),
            OnlineFrameDecision::Run
        );
        assert_eq!(pacer.online.as_ref().unwrap().origin_at, resumed_at);
    }

    #[test]
    fn early_receipt_moves_a_late_origin_and_repeated_queries_are_idempotent() {
        let start = Instant::now();
        let mut pacer = LocalFramePacer::new();
        let delayed = push_receipt(start + Duration::from_millis(15), 1);
        assert_eq!(
            pacer.online_frame_decision(delayed.received_at, 1, 1, delayed, false, false),
            OnlineFrameDecision::Run
        );
        // Frame 2 arrives 15ms before the delayed anchor's schedule. Holding it
        // to that schedule would delay every later frame without any reserve.
        let on_time = push_receipt(start + Duration::from_nanos(1_000_000_000 / 45), 2);
        for _ in 0..2 {
            assert_eq!(
                pacer.online_frame_decision(on_time.received_at, 2, 2, on_time, false, false),
                OnlineFrameDecision::Run
            );
        }
        assert_eq!(
            pacer.online.as_ref().unwrap().origin_at,
            on_time.received_at
        );
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 0);
        assert_eq!(pacer.online_pacing_status().unwrap().underruns, 0);
        assert_eq!(
            pacer.online.as_ref().unwrap().deadline(1_001),
            on_time.received_at + Duration::from_nanos(999 * 1_000_000_000 / 45)
        );
        let duplicate = push_receipt(on_time.received_at + Duration::from_millis(15), 2);
        pacer.online_frame_decision(duplicate.received_at, 2, 2, duplicate, false, false);
        assert_eq!(
            pacer.online.as_ref().unwrap().origin_at,
            on_time.received_at
        );
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 0);
    }

    #[test]
    fn pushed_jitter_and_underruns_learn_once_with_bounded_reserve_and_slow_decay() {
        let start = Instant::now();
        let tick = Duration::from_nanos(1_000_000_000 / 45);
        let mut pacer = LocalFramePacer::new();
        pacer.online_frame_decision(start, 1, 1, push_receipt(start, 1), false, false);
        let late = push_receipt(start + tick + Duration::from_millis(15), 2);
        pacer.online_frame_decision(late.received_at, 2, 2, late, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 15_000);
        assert_eq!(
            pacer.online.as_ref().unwrap().origin_at,
            start,
            "ordinary jitter must not move the origin"
        );
        pacer.online_frame_decision(late.received_at, 2, 2, late, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 15_000);
        let burst = push_receipt(start + tick * 2 + Duration::from_millis(100), 3);
        pacer.online_frame_decision(burst.received_at, 3, 3, burst, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 20_000);

        pacer.reset_for_new_world();
        pacer.online_frame_decision(start, 1, 1, push_receipt(start, 1), false, false);
        pacer.online_frame_decision(late.received_at, 2, 2, late, false, false);
        let steady = push_receipt(start + Duration::from_nanos(93 * 1_000_000_000 / 45), 94);
        pacer.online_frame_decision(steady.received_at, 94, 94, steady, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 13_000);
        let due = pacer.online.as_ref().unwrap().deadline(95);
        assert_eq!(
            pacer.online_frame_decision(due, 95, 94, steady, false, false),
            OnlineFrameDecision::WaitingForCanonical
        );
        pacer.online_frame_decision(due + Duration::from_millis(2), 95, 94, steady, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().underruns, 1);
        let arrived = push_receipt(due + Duration::from_millis(30), 95);
        pacer.online_frame_decision(arrived.received_at, 95, 95, arrived, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 20_000);
    }

    #[test]
    fn pushed_reserve_decays_across_render_paced_catch_up_bursts() {
        let start = Instant::now();
        let tick = Duration::from_nanos(1_000_000_000 / 45);
        let mut pacer = LocalFramePacer::new();
        pacer.online_frame_decision(start, 1, 1, push_receipt(start, 1), false, false);
        let late = push_receipt(start + tick + Duration::from_millis(15), 2);
        pacer.online_frame_decision(late.received_at, 2, 2, late, false, false);
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 15_000);
        // A 15Hz renderer services three on-time frames per pass, so every
        // pass starts a short catch-up. Six seconds must still decay.
        let mut next = 3;
        for pass in 1..=90 {
            let frontier = 2 + pass * 3;
            let receipt = push_receipt(start + tick * (frontier - 1), frontier);
            let now = receipt.received_at + Duration::from_millis(1);
            while next <= frontier
                && pacer
                    .online_frame_decision(now, next, frontier, receipt, false, false)
                    .admits()
            {
                next += 1;
            }
        }
        assert!(next > 260, "the passes kept up");
        assert!(pacer.online_pacing_status().unwrap().reserve_micros <= 10_000);
    }

    #[test]
    fn pushed_recovery_preserves_canonical_gaps_and_transaction_resume_then_resets() {
        let start = Instant::now();
        let mut pacer = LocalFramePacer::new();
        let first = push_receipt(start, 1);
        pacer.online_frame_decision(start, 1, 1, first, false, false);
        let bulk = push_receipt(start + Duration::from_nanos(19 * 1_000_000_000 / 45), 20);
        for next in 2..20 {
            assert_eq!(
                pacer.online_frame_decision(bulk.received_at, next, 20, bulk, false, true),
                OnlineFrameDecision::CatchUp
            );
        }
        assert!(pacer.online_catch_up_ready(bulk.received_at, 19, 20, bulk, true));
        let delayed_service = bulk.received_at + Duration::from_millis(80);
        assert_eq!(
            pacer.online_frame_decision(delayed_service, 20, 20, bulk, false, false),
            OnlineFrameDecision::Run
        );
        assert!(!pacer.online_pacing_status().unwrap().catching_up);
        assert_eq!(pacer.online.as_ref().unwrap().origin_at, bulk.received_at);
        assert_eq!(
            pacer.online_frame_decision(delayed_service, 20, 20, bulk, false, false),
            OnlineFrameDecision::Run
        );
        assert_eq!(pacer.online.as_ref().unwrap().origin_at, bulk.received_at);
        assert_eq!(
            pacer.online_frame_decision(bulk.received_at, 21, 20, bulk, true, false),
            OnlineFrameDecision::WaitingForCanonical
        );
        assert_eq!(
            pacer.online_frame_decision(bulk.received_at, 21, 21, bulk, true, false),
            OnlineFrameDecision::Run,
            "a retained transaction must ignore a future playout deadline"
        );
        assert_eq!(
            pacer.online_frame_decision(bulk.received_at, 0, 0, bulk, false, false),
            OnlineFrameDecision::Run
        );
        pacer.reset_for_new_world();
        assert!(pacer.online_pacing_status().is_none());
        let restart = push_receipt(bulk.received_at + Duration::from_secs(10), 1);
        assert_eq!(
            pacer.online_frame_decision(restart.received_at, 1, 1, restart, false, false),
            OnlineFrameDecision::Run
        );
        assert_eq!(pacer.online_pacing_status().unwrap().reserve_micros, 0);
    }

    #[test]
    fn online_render_continuation_is_bounded_and_resets_only_on_present() {
        let now = crate::util::time::Instant::now();
        let mut pacer = LocalFramePacer::new();
        assert!(
            !pacer.defer_online_render(now, 8, true, Some((10, 100))),
            "first surface must present"
        );
        pacer.record_presented(now, true);
        assert!(pacer.defer_online_render(now, 8, true, Some((18, 100))));
        assert!(pacer.render_deferred());
        pacer.clear_render_continuation();
        assert!(
            !pacer.render_deferred(),
            "hidden/ineligible windows cancel the queued continuation"
        );
        assert_eq!(pacer.total_render_deferrals(), 1);
        assert!(pacer.defer_online_render(now, 8, true, Some((26, 100))));
        assert!(pacer.defer_online_render(now, 8, true, Some((34, 100))));
        assert!(
            !pacer.defer_online_render(now, 8, true, Some((42, 100))),
            "32 ticks force a present"
        );
        pacer.clear_render_continuation();
        assert!(
            !pacer.defer_online_render(now, 1, true, Some((43, 100))),
            "a failed acquisition cannot reset the budget"
        );
        pacer.record_presented(now, true);
        assert!(pacer.defer_online_render(now, 1, true, Some((44, 100))));
        assert!(!pacer.defer_online_render(
            now + std::time::Duration::from_millis(50),
            1,
            true,
            Some((45, 100))
        ));
        pacer.record_presented(now, false);
        assert!(
            !pacer.defer_online_render(now, 1, true, Some((45, 100))),
            "a loading surface does not replace the first battlefield present"
        );
        pacer.record_presented(now, true);
        for _ in 0..3 {
            assert!(pacer.defer_online_render(now, 1, true, Some((45, 100))));
        }
        assert!(
            !pacer.defer_online_render(now, 1, true, Some((45, 100))),
            "four short chunks also force a present"
        );
    }

    #[test]
    fn online_render_continuation_requires_completed_work_and_next_canonical_frame() {
        let now = crate::util::time::Instant::now();
        for (completed, last_completed, next) in [
            (0, true, Some((10, 20))),
            (1, false, Some((10, 20))),
            (1, true, None), // offline, focus/modal/terminal, or exact-step exclusion
            (1, true, Some((0, 20))),
            (1, true, Some((21, 20))),
        ] {
            let mut pacer = LocalFramePacer::new();
            pacer.record_presented(now, true);
            assert!(!pacer.defer_online_render(now, completed, last_completed, next));
        }
    }

    #[test]
    fn canonical_frontier_admits_buffered_ticks_without_a_local_timer_and_bounds_work() {
        assert!(
            LocalFramePacer::admits_canonical_frame(0, 0),
            "startup keeps polling"
        );
        assert!(LocalFramePacer::admits_canonical_frame(17, 19));
        assert!(LocalFramePacer::admits_canonical_frame(19, 19));
        assert!(
            !LocalFramePacer::admits_canonical_frame(20, 19),
            "no missing frame is skipped"
        );
        assert!(LocalFramePacer::permits_catch_up(
            7,
            std::time::Duration::from_millis(7)
        ));
        assert!(!LocalFramePacer::permits_catch_up(
            8,
            std::time::Duration::ZERO
        ));
        assert!(!LocalFramePacer::permits_catch_up(
            1,
            std::time::Duration::from_millis(8)
        ));
    }

    #[test]
    fn network_fps_uses_millisecond_timer_and_resets_for_rematch() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(17);
        assert!(!pacer.should_admit(38, 0, Some((45, 0)), false));
        assert!(pacer.should_admit(39, 6, Some((45, 0)), false));
        assert_eq!(pacer.budget_ms(0, Some((45, 0))), Some(22.0));
        assert!(!pacer.should_admit(68, 0, Some((45, 30)), false));
        assert!(pacer.should_admit(69, 0, Some((45, 30)), false));
        assert!(!pacer.should_admit(49, 0, Some((0, 0)), false));
        assert!(pacer.should_admit(50, 0, Some((0, 0)), false));
        pacer.record_admitted_frame(u64::from(u32::MAX - 7));
        assert!(pacer.should_admit(8, 6, Some((60, 0)), false));
        pacer.reset_for_immediate_frame();
        assert!(pacer.should_admit(8, 6, Some((30, 0)), false));
    }

    #[test]
    fn render_interpolation_phase_uses_pacing_bucket_and_wraps_with_uptime() {
        let mut pacer = LocalFramePacer::new();
        assert_eq!(pacer.interpolation_elapsed_ms(0), None);
        pacer.record_admitted_frame(31);
        assert_eq!(pacer.interpolation_elapsed_ms(31), Some(15));
        assert_eq!(pacer.interpolation_elapsed_ms(47), Some(31));
        pacer.record_admitted_frame(u64::from(u32::MAX - 7));
        assert_eq!(pacer.interpolation_elapsed_ms(8), Some(24));
    }
    #[test]
    fn wall_clock_process_probe() {
        if std::env::var_os("VERA20K_PACING_CLOCK_PROBE").is_some() {
            println!("VERA20K_PACING_CLOCK_MS={}", wall_clock_ms());
        }
    }

    #[test]
    fn wall_clock_continues_between_processes() {
        // Raw Mouse-saved Flash timestamps use the same clock after a cold
        // process restart. A new process must not install its own epoch.
        let before = wall_clock_ms() as u32;
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::match_runtime::frame_pacer::tests::wall_clock_process_probe",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("VERA20K_PACING_CLOCK_PROBE", "1")
            .output()
            .unwrap();
        let after = wall_clock_ms() as u32;
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let child: u64 = stdout
            .split_once("VERA20K_PACING_CLOCK_MS=")
            .expect("child clock sample")
            .1
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let child = child as u32;
        assert!(
            child.wrapping_sub(before) <= after.wrapping_sub(before),
            "new process clock {child} outside parent samples {before}..{after}"
        );
    }

    #[test]
    fn timed_speeds_require_the_exact_bucket_distance() {
        for speed in MIN_TIMED_GAME_SPEED..=MAX_TIMED_GAME_SPEED {
            let mut pacer = LocalFramePacer::new();
            pacer.record_admitted_frame(32);
            let deadline = 32 + u64::from(speed) * FRAME_BUCKET_MS;
            assert!(!pacer.should_admit(deadline - 1, speed, None, false));
            assert!(pacer.should_admit(deadline, speed, None, false));
            assert_eq!(pacer.next_deadline_ms(speed), Some(deadline));
        }
    }

    #[test]
    fn native_signed_bucket_subtraction_stalls_at_uptime_rollover() {
        let mut pacer = LocalFramePacer::new();
        pacer.record_admitted_frame(u64::from(u32::MAX - 7));

        assert!(!pacer.should_admit(u64::from(u32::MAX) + 1, 1, None, false));
    }

    #[test]
    fn scenario_elapsed_clock_excludes_explicit_modal_time() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(10 << 4);
        clock.pause(70 << 4);
        assert_eq!(clock.elapsed_seconds(10_000 << 4), 1);

        clock.resume(1_070 << 4);
        assert_eq!(clock.stop(1_130 << 4), 2);
        // A stopped clock (abort/outcome) still reports its accumulated time.
        assert_eq!(clock.elapsed_seconds(5_000 << 4), 2);
    }

    #[test]
    fn multiplayer_clock_preserves_native_pause_and_resume_intent() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(0);
        clock.set_multiplayer_paused(true, 10 << 4);
        clock.pause(20 << 4); // A native hold enters while the relay is paused.
        clock.set_multiplayer_paused(false, 30 << 4);
        assert_eq!(clock.elapsed_buckets(40 << 4), 10);
        clock.set_multiplayer_paused(true, 40 << 4);
        // A native hold ends during the shared pause. Audio remains paused,
        // while this production owner must remember the elapsed resume.
        crate::app::match_runtime::scenario_pause::resume_with_owners(
            &mut clock,
            None,
            false,
            true,
            50 << 4,
        );
        assert_eq!(clock.elapsed_buckets(60 << 4), 10);
        clock.set_multiplayer_paused(false, 70 << 4);
        assert_eq!(clock.elapsed_buckets(80 << 4), 20);
    }

    #[test]
    fn multiplayer_clock_save_restores_intent_without_releasing_the_hold() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(0);
        clock.set_multiplayer_paused(true, 10 << 4);
        let saved = clock.begin_save(20 << 4);
        assert_eq!(saved.accumulated_buckets(), 10);
        clock.finish_save(saved, 30 << 4);
        assert_eq!(clock.elapsed_buckets(40 << 4), 10);
        clock.set_multiplayer_paused(false, 50 << 4);
        assert_eq!(clock.elapsed_buckets(60 << 4), 20);
    }

    #[test]
    fn multiplayer_clock_terminal_stop_reset_and_load_drop_resume_intent() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(0);
        clock.set_multiplayer_paused(true, 10 << 4);
        assert_eq!(clock.stop_buckets(20 << 4), 10);
        clock.set_multiplayer_paused(false, 30 << 4);
        assert_eq!(clock.elapsed_buckets(40 << 4), 10);
        clock.set_multiplayer_paused(true, 50 << 4);
        clock.reset();
        clock.set_multiplayer_paused(false, 60 << 4);
        assert_eq!(clock.elapsed_buckets(70 << 4), 0);
        clock.set_multiplayer_paused(true, 80 << 4);
        clock.start(90 << 4);
        assert_eq!(clock.elapsed_buckets(100 << 4), 10);
        let loaded = ScenarioElapsedClock::from_saved_accumulator(77, 110 << 4);
        assert_eq!(loaded.elapsed_buckets(120 << 4), 87);
    }

    #[test]
    fn scenario_elapsed_clock_preserves_native_rollover_arithmetic() {
        let mut clock = ScenarioElapsedClock::new();
        clock.start(u64::from(u32::MAX - 15));

        // timeGetTime wraps before the >>4. Native's uncorrected unsigned
        // subtraction becomes negative only when the score path casts to i32.
        assert_eq!(clock.elapsed_seconds(u64::from(u32::MAX) + 1), -4_473_924);
    }

    #[test]
    fn saved_elapsed_clock_matches_original_executable() {
        #[derive(serde::Deserialize)]
        struct Input {
            name: String,
            elapsed_before: [u32; 3],
            milliseconds: Vec<u64>,
        }
        #[derive(serde::Deserialize)]
        struct Boundary {
            after: [u32; 3],
        }
        #[derive(serde::Deserialize)]
        struct Case {
            input: Input,
            saved_elapsed: [u32; 3],
            loaded_elapsed: [u32; 3],
            boundaries: Vec<Boundary>,
        }
        #[derive(serde::Deserialize)]
        struct Corpus {
            scenario_elapsed_clock: Vec<Case>,
        }
        let corpus: Corpus = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/saved_game_layout.json",
        ))
        .unwrap();
        assert_eq!(corpus.scenario_elapsed_clock.len(), 6);
        for case in corpus.scenario_elapsed_clock {
            let input = case.input;
            let mut clock = ScenarioElapsedClock {
                start_bucket: input.elapsed_before[0],
                accumulated_buckets: input.elapsed_before[2],
                ..ScenarioElapsedClock::new()
            };
            let saved = clock.begin_save(input.milliseconds[0]);
            assert_eq!(clock.start_bucket, case.saved_elapsed[0], "{}", input.name);
            assert_eq!(
                saved.accumulated_buckets(),
                case.saved_elapsed[2],
                "{}",
                input.name
            );
            let saved_accumulator = saved.accumulated_buckets();
            clock.finish_save(saved, input.milliseconds[1]);
            let source_after = case.boundaries[0].after;
            assert_eq!(clock.start_bucket, source_after[0], "{}", input.name);
            assert_eq!(clock.accumulated_buckets, source_after[2], "{}", input.name);

            // The final native clock call rearms the loaded elapsed timer.
            // Its opaque middle word and the separate post-read auxiliary
            // timer have no corresponding app clock state.
            let loaded = ScenarioElapsedClock::from_saved_accumulator(
                saved_accumulator,
                *input.milliseconds.last().unwrap(),
            );
            assert_eq!(
                loaded.start_bucket, case.loaded_elapsed[0],
                "{}",
                input.name
            );
            assert_eq!(
                loaded.accumulated_buckets, case.loaded_elapsed[2],
                "{}",
                input.name
            );
        }
    }
}
