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
            simulation.set_frame_profiling(active);
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
    fn paused_disconnect_and_reconnect_start_a_fresh_observation_window() {
        let mut simulation = crate::sim::world::Simulation::with_seed(41);
        let mut observer = ObserverSession::default();
        assert!(observer.synchronize(Some(&mut simulation), true));
        simulation.advance_tick(&[], None, None, None, 67);
        assert_eq!(simulation.frame_profile().frames(), 1);
        let paused_tick = simulation.session.tick;
        let saved = crate::util::save_codec::serialize(&simulation).unwrap();
        assert!(observer.synchronize(Some(&mut simulation), false));
        assert_eq!(simulation.frame_profile().frames(), 0);
        assert!(observer.synchronize(Some(&mut simulation), true));
        assert_eq!(simulation.frame_profile().frames(), 0);
        assert_eq!(simulation.session.tick, paused_tick);
        assert_eq!(
            crate::util::save_codec::serialize(&simulation).unwrap(),
            saved
        );
        simulation.advance_tick(&[], None, None, None, 67);
        assert!(!observer.synchronize(Some(&mut simulation), true));
        assert_eq!(simulation.frame_profile().frames(), 1);
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
}
