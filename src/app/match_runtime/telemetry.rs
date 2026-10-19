//! App-owned tick timing ring for the dashboard telemetry snapshot.
//!
//! The ring samples the wall-clock cost of each simulation frame advance
//! (the `SimRuntime::advance_frame` transaction in `sim_tick`) and summarizes
//! it into `sim::telemetry::TimingSample`. It lives beside the runtime in
//! `MatchState`, never inside `Simulation` — the same app-owned diagnostic
//! discipline as `match_diagnostics`: the deterministic simulation never
//! reads it, and `loading::transitions` clears it beside the per-match clock
//! so a window never mixes two timelines.
//!
//! Frame rate is deliberately not sampled here: presentation cadence is a
//! render-loop measurement, not a simulation fact, and mixing the two would
//! misattribute render-bound stalls to the tick. The snapshot's `fps` comes
//! from `DiagnosticsState::frame_timer` (the render loop) and its `tick_hz`
//! from [`TickRateMeter`] (committed ticks), so each number keeps its owner.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::sim::telemetry::TimingSample;

/// Samples retained: 4 seconds at the retail 60 ticks/second. Long enough
/// for stable percentiles, short enough that a stall scrolls out.
const RING_CAPACITY: usize = 240;

/// Fixed-capacity ring of recent tick costs, in milliseconds.
pub struct TickTimingRing {
    samples: VecDeque<f32>,
}

impl TickTimingRing {
    /// Record one frame's sim-advance cost, evicting the oldest sample once
    /// the window is full.
    pub fn push(&mut self, tick_ms: f32) {
        if self.samples.len() == RING_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(tick_ms);
    }

    /// Drop every retained sample — called when a new match installs its
    /// simulation.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Summarize the window. `budget_ms` is the frame's tick budget in
    /// milliseconds; `budget_utilization` above 1.0 means the tick cost
    /// exceeded the budget and the pacer can no longer hold the tick rate.
    /// `fps` and `tick_hz` are filled by the caller (they come from other
    /// owners: the render-frame timer and the committed-tick meter).
    pub fn summarize(&self, budget_ms: Option<f32>) -> TimingSample {
        let budget_ms = budget_ms.filter(|value| value.is_finite() && *value > 0.0);
        let mut sorted: Vec<f32> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let tick_ms = self.samples.back().copied().unwrap_or(0.0);
        TimingSample {
            tick_ms,
            p50_ms: percentile(&sorted, 0.5),
            p99_ms: percentile(&sorted, 0.99),
            budget_ms,
            budget_utilization: budget_ms.map(|budget| tick_ms / budget),
            fps: 0.0,
            tick_hz: 0.0,
        }
    }
}

impl Default for TickTimingRing {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(RING_CAPACITY),
        }
    }
}

/// Ticks per history sample: one per second at the retail 15 Hz logic rate.
const HISTORY_TICKS: u32 = 15;
/// Ticks between report points: five seconds at the retail 15 Hz logic rate.
pub const REPORT_POINT_TICKS: u64 = 75;
/// Report points kept: two hours at that rate.
const REPORT_POINT_CAPACITY: usize = 1440;

/// Report-grade samples kept for the whole match.
///
/// Each one is exactly what the dashboard's match report reads, so a past
/// point renders the same report as the live view. It is recorded whether or
/// not a dashboard is attached; a client that connects mid-match receives the
/// ring once. App-owned like the other observation rings, and never read by
/// the simulation.
#[derive(Default)]
pub struct ReportHistory {
    points: VecDeque<crate::sim::telemetry::ReportPoint>,
}

impl ReportHistory {
    /// Whether `tick` closes a report point.
    pub fn due(tick: u64) -> bool {
        tick != 0 && tick % REPORT_POINT_TICKS == 0
    }

    pub fn push(&mut self, point: crate::sim::telemetry::ReportPoint) {
        if self.points.len() == REPORT_POINT_CAPACITY {
            self.points.pop_front();
        }
        self.points.push_back(point);
    }

    pub fn points(&self) -> Vec<crate::sim::telemetry::ReportPoint> {
        self.points.iter().cloned().collect()
    }

    /// The newest point. It rides every snapshot, so a long session keeps
    /// gaining reportable points after the initial backfill.
    pub fn latest(&self) -> Option<crate::sim::telemetry::ReportPoint> {
        self.points.back().cloned()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
/// Two hours of samples at that rate; older ones scroll out.
const HISTORY_CAPACITY: usize = 7200;

/// Whole-match timeline: one 1 Hz summary of the simulation's own cost.
///
/// Unlike the live timing ring this records whether or not a dashboard is
/// attached, so a client that connects mid-match receives the whole match
/// instead of a hole. It lives beside `TickTimingRing` in `MatchState`:
/// app-owned, never read by the simulation, cleared with the per-match clock.
/// Render and phase-ledger readings stay out: those are measured only while a
/// client is attached, so a hole marked as such is more honest than a zero.
#[derive(Default)]
pub struct MatchHistory {
    samples: VecDeque<crate::sim::telemetry::MatchHistorySample>,
    ticks: u32,
    sum_ms: f32,
    peak_ms: f32,
    entities: u32,
    houses: u32,
    path_batch: Option<crate::sim::movement::PathBatchStats>,
}

impl MatchHistory {
    /// Record one committed tick; the sample closes every [`HISTORY_TICKS`]
    /// ticks. `tick_ms` is that advance's wall time; the other readings are
    /// the sample's last state.
    pub fn record_tick(
        &mut self,
        tick: u64,
        tick_ms: f32,
        entities: usize,
        houses: usize,
        path_batch: Option<crate::sim::movement::PathBatchStats>,
    ) {
        let cast = |count: usize| u32::try_from(count).unwrap_or(u32::MAX);
        self.ticks += 1;
        self.sum_ms += tick_ms;
        self.peak_ms = self.peak_ms.max(tick_ms);
        self.entities = cast(entities);
        self.houses = cast(houses);
        self.path_batch = path_batch;
        if self.ticks < HISTORY_TICKS {
            return;
        }
        if self.samples.len() == HISTORY_CAPACITY {
            self.samples.pop_front();
        }
        self.samples
            .push_back(crate::sim::telemetry::MatchHistorySample {
                tick,
                tick_ms: self.sum_ms / self.ticks as f32,
                peak_ms: self.peak_ms,
                entities: self.entities,
                houses: self.houses,
                path_batch: self.path_batch,
            });
        self.ticks = 0;
        self.sum_ms = 0.0;
        self.peak_ms = 0.0;
    }

    /// The whole timeline, oldest first, for one wire payload.
    pub fn samples(&self) -> Vec<crate::sim::telemetry::MatchHistorySample> {
        self.samples.iter().cloned().collect()
    }

    /// Drop every sample — a new match starts a new timeline.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// App-owned subscription epoch. Synchronization runs outside tick admission,
/// so leaving/rejoining while paused starts a fresh observation window too.
#[derive(Default)]
pub(crate) struct ObserverSession {
    active: bool,
}

impl ObserverSession {
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    pub(crate) fn synchronize(
        &mut self,
        simulation: Option<&mut crate::sim::world::Simulation>,
        active: bool,
    ) -> bool {
        if let Some(simulation) = simulation {
            // The ledger feeds report points whether or not a dashboard is
            // attached; only the live observation window resets on this
            // transition (see `telemetry_capture::reset_live_window`).
            simulation.set_frame_profiling(true);
        }
        let changed = self.active != active;
        self.active = active;
        changed
    }
}

/// Nearest-rank percentile of an ascending slice; 0.0 for an empty window.
fn percentile(sorted: &[f32], fraction: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (fraction * sorted.len() as f32).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Committed-tick rate over a rolling wall-clock window.
///
/// `record` is called once per committed tick (never per frame): the rate is
/// ticks divided by the window's wall time, so a game-speed change or a
/// stalled pacer shows up as the real tick rate rather than the pacer's
/// nominal one. A silent meter (paused runtime) decays to zero after
/// [`SILENCE_TIMEOUT`].
pub struct TickRateMeter {
    window_start: Option<Instant>,
    last_event: Option<Instant>,
    events: u32,
    rate: f32,
}

/// Rate window: long enough to smooth a 45 Hz stream, short enough to react.
const RATE_WINDOW: Duration = Duration::from_millis(500);
/// No committed tick for this long means the rate is truly zero.
const SILENCE_TIMEOUT: Duration = Duration::from_millis(1500);

impl TickRateMeter {
    /// Record one committed tick.
    pub fn record(&mut self, now: Instant) {
        let start = *self.window_start.get_or_insert(now);
        self.events += 1;
        self.last_event = Some(now);
        let elapsed = now.saturating_duration_since(start);
        if elapsed >= RATE_WINDOW {
            // Intervals, not ticks: N ticks span N-1 intervals, which is what
            // "ticks per second" means.
            let intervals = self.events.saturating_sub(1) as f32;
            self.rate = intervals / elapsed.as_secs_f32();
            // This event also anchors the next window's first interval.
            self.events = 1;
            self.window_start = Some(now);
        }
    }

    /// Committed ticks per second, or 0 when the meter has gone quiet.
    pub fn rate(&self, now: Instant) -> f32 {
        match self.last_event {
            Some(last) if now.saturating_duration_since(last) <= SILENCE_TIMEOUT => self.rate,
            _ => 0.0,
        }
    }

    /// Drop the window — a new match starts a new timeline.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

impl Default for TickRateMeter {
    fn default() -> Self {
        Self {
            window_start: None,
            last_event: None,
            events: 0,
            rate: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiling_continues_across_attach_and_detach() {
        let mut simulation = crate::sim::world::Simulation::with_seed(41);
        let mut observer = ObserverSession::default();
        assert!(observer.synchronize(Some(&mut simulation), true));
        simulation.advance_tick(&[], None, None, None, 67);
        assert_eq!(simulation.frame_profile().frames(), 1);
        let paused_tick = simulation.session.tick();
        let saved = crate::util::save_codec::serialize(&simulation).unwrap();
        // A dashboard leaving must not stop or reset the ledger: report points
        // are captured without a client.
        assert!(observer.synchronize(Some(&mut simulation), false));
        assert_eq!(
            simulation.frame_profile().frames(),
            1,
            "measurement continues after detach"
        );
        assert!(observer.synchronize(Some(&mut simulation), true));
        assert_eq!(
            simulation.frame_profile().frames(),
            1,
            "re-attaching does not reset the ledger"
        );
        assert_eq!(simulation.session.tick(), paused_tick);
        assert_eq!(
            crate::util::save_codec::serialize(&simulation).unwrap(),
            saved
        );
        simulation.advance_tick(&[], None, None, None, 67);
        assert_eq!(simulation.frame_profile().frames(), 2);
        assert!(!observer.synchronize(Some(&mut simulation), true));
    }

    #[test]
    fn uncapped_speed_has_no_budget_percentage() {
        let mut ring = TickTimingRing::default();
        ring.push(30.0);
        let unbounded = ring.summarize(None);
        assert_eq!(unbounded.budget_ms, None);
        assert_eq!(unbounded.budget_utilization, None);
        assert_eq!(ring.summarize(Some(48.0)).budget_utilization, Some(0.625));
    }

    #[test]
    fn empty_window_summarizes_to_zero() {
        let sample = TickTimingRing::default().summarize(Some(16.0));
        assert_eq!(sample.tick_ms, 0.0);
        assert_eq!(sample.p50_ms, 0.0);
        assert_eq!(sample.p99_ms, 0.0);
        assert_eq!(sample.budget_ms, Some(16.0));
        assert_eq!(sample.budget_utilization, Some(0.0));
    }

    #[test]
    fn summarize_reports_latest_and_percentiles() {
        let mut ring = TickTimingRing::default();
        for ms in [1.0, 2.0, 3.0, 4.0] {
            ring.push(ms);
        }
        let sample = ring.summarize(Some(16.0));
        assert_eq!(sample.tick_ms, 4.0, "tick_ms is the most recent sample");
        assert_eq!(sample.p50_ms, 2.0, "nearest-rank median of four samples");
        assert_eq!(sample.p99_ms, 4.0);
        assert_eq!(sample.budget_ms, Some(16.0));
        assert_eq!(sample.budget_utilization, Some(4.0 / 16.0));
    }

    #[test]
    fn window_evicts_oldest_samples() {
        let mut ring = TickTimingRing::default();
        for ms in 0..RING_CAPACITY {
            ring.push(ms as f32);
        }
        ring.push(999.0);
        let sample = ring.summarize(Some(16.0));
        assert_eq!(sample.tick_ms, 999.0);
        assert_eq!(
            sample.p50_ms, 120.0,
            "the window kept the newest {} samples",
            RING_CAPACITY
        );
    }

    #[test]
    fn tick_rate_meter_reports_committed_ticks_per_second() {
        let mut meter = TickRateMeter::default();
        let start = Instant::now();
        // One tick every 50 ms: 20 Hz. The window closes at 500 ms.
        for step in 1..=20u32 {
            meter.record(start + Duration::from_millis(u64::from(step) * 50));
        }
        let rate = meter.rate(start + Duration::from_millis(1000));
        assert!((rate - 20.0).abs() < 0.01, "expected 20 Hz, got {rate}");
    }

    #[test]
    fn tick_rate_keeps_the_boundary_tick_in_the_next_window() {
        let mut meter = TickRateMeter::default();
        let start = Instant::now();
        for step in 0..=30u32 {
            let now = start + Duration::from_millis(u64::from(step) * 50);
            meter.record(now);
            if step > 0 && step % 10 == 0 {
                assert_eq!(meter.rate(now), 20.0);
            }
        }
    }

    #[test]
    fn tick_rate_meter_decays_to_zero_when_silent() {
        let mut meter = TickRateMeter::default();
        let start = Instant::now();
        for step in 1..=20u32 {
            meter.record(start + Duration::from_millis(u64::from(step) * 50));
        }
        assert_eq!(
            meter.rate(start + Duration::from_millis(4000)),
            0.0,
            "a paused runtime has a true zero rate, not a stale one"
        );
    }

    #[test]
    fn clear_drops_the_window() {
        let mut ring = TickTimingRing::default();
        ring.push(5.0);
        ring.clear();
        assert_eq!(ring.summarize(Some(16.0)).tick_ms, 0.0);
    }

    #[test]
    fn history_closes_one_sample_per_second_of_ticks() {
        let mut history = MatchHistory::default();
        for tick in 1..=HISTORY_TICKS as u64 {
            history.record_tick(tick, tick as f32, 100 + tick as usize, 3, None);
        }
        let samples = history.samples();
        assert_eq!(samples.len(), 1, "15 ticks make one sample");
        assert_eq!(samples[0].tick, HISTORY_TICKS as u64);
        assert!(
            (samples[0].tick_ms - 8.0).abs() < 1e-4,
            "the mean of 1..=15 ms: {:?}",
            samples[0].tick_ms
        );
        assert_eq!(samples[0].peak_ms, 15.0);
        assert_eq!(samples[0].entities, 115);
        assert_eq!(samples[0].houses, 3);
        assert!(samples[0].path_batch.is_none());
        history.record_tick(16, 1.0, 1, 1, None);
        assert_eq!(
            history.samples().len(),
            1,
            "a partial sample is not published"
        );
    }

    #[test]
    fn history_keeps_the_newest_window_and_clears_with_the_match() {
        let mut history = MatchHistory::default();
        let full = (HISTORY_CAPACITY + 1) * HISTORY_TICKS as usize;
        for tick in 0..full {
            history.record_tick(tick as u64, 10.0, 5, 2, None);
        }
        let samples = history.samples();
        assert_eq!(samples.len(), HISTORY_CAPACITY, "the window is bounded");
        assert_eq!(
            samples[0].tick,
            HISTORY_TICKS as u64 + HISTORY_TICKS as u64 - 1,
            "the oldest sample scrolled out"
        );
        assert_eq!(samples[HISTORY_CAPACITY - 1].tick, (full - 1) as u64);
        history.clear();
        assert!(history.samples().is_empty());
    }

    fn report_point(tick: u64) -> crate::sim::telemetry::ReportPoint {
        crate::sim::telemetry::ReportPoint {
            tick,
            entities_total: 1,
            map_width: 64,
            map_height: 64,
            houses: Vec::new(),
            timing: None,
            profile: Some(crate::sim::frame_profile::FrameProfileReport::default()),
            render: None,
            path_batch: None,
        }
    }

    #[test]
    fn report_points_close_every_five_seconds_and_keep_the_newest() {
        assert!(
            !ReportHistory::due(0),
            "tick zero is the launch, not a sample"
        );
        assert!(!ReportHistory::due(REPORT_POINT_TICKS - 1));
        assert!(ReportHistory::due(REPORT_POINT_TICKS));
        let mut history = ReportHistory::default();
        for index in 0..REPORT_POINT_CAPACITY + 2 {
            history.push(report_point(index as u64 * REPORT_POINT_TICKS));
        }
        let points = history.points();
        assert_eq!(points.len(), REPORT_POINT_CAPACITY, "the window is bounded");
        assert_eq!(
            points[0].tick,
            2 * REPORT_POINT_TICKS,
            "the oldest points scrolled out"
        );
        history.clear();
        assert!(history.points().is_empty());
    }
}
