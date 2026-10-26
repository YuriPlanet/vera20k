//! App-local wall-clock services for a running scenario.
//!
//! The deterministic simulation never reads wall time. The frame pacer decides
//! whether one outer event-loop iteration may admit a gameplay frame, while the
//! scenario elapsed clock supplies the score screen's match duration.

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
        START.get_or_init(crate::util::time::Instant::now).elapsed().as_millis() as u64
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
        }
    }

    /// ScenarioSave689310 stops +614 and accumulates +61C before689378's
    /// stream write. The source resumes at689454 only if it was running.
    /// Original wrapping/bucket controls: storage_oracle/saved_game_layout.
    pub(crate) fn begin_save(&mut self, now_ms: u64) -> ScenarioClockSave {
        let resume_source = self.start_bucket != ELAPSED_CLOCK_STOPPED;
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
    #[cfg(test)]
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
    #[cfg(test)]
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
        }
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
