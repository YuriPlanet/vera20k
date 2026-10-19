//! Process diagnostics owner (F12 `DiagnosticsState`): debug overlay toggles,
//! the frame stepper, the frame timer and the parity digest sink.
//!
//! Match-lifetime diagnostic replay lives in `app::match_diagnostics`; this
//! owner is process-scoped tooling state.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const FRAME_TIMER_WINDOW: usize = 60;

/// Rolling frame-time tracker. Sampled once per `render_frame`.
pub(crate) struct FrameTimer {
    samples: VecDeque<Duration>,
    last_tick: Option<Instant>,
}

impl FrameTimer {
    pub(crate) fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(FRAME_TIMER_WINDOW),
            last_tick: None,
        }
    }

    /// Record one frame boundary. Call from the top of `render_frame`.
    pub(crate) fn sample(&mut self, now: Instant) {
        if let Some(prev) = self.last_tick {
            let dt = now - prev;
            if self.samples.len() == FRAME_TIMER_WINDOW {
                self.samples.pop_front();
            }
            self.samples.push_back(dt);
        }
        self.last_tick = Some(now);
    }

    /// Mean frame time in milliseconds over the current window, or 0
    /// if no samples have been recorded yet.
    pub(crate) fn frame_ms_mean(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let total_ns: u128 = self.samples.iter().map(|d| d.as_nanos()).sum();
        let mean_ns: u128 = total_ns / self.samples.len() as u128;
        (mean_ns as f64 / 1_000_000.0) as f32
    }
}

impl Default for FrameTimer {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct DiagnosticsState {
    presentation_clock: PresentationClock,
    /// One-shot: advance a single sim tick while paused (Period).
    pub(crate) debug_frame_step_requested: bool,
    /// PathGrid walkability overlay toggle (P / F9).
    pub(crate) debug_show_pathgrid: bool,
    /// Per-overlay SpeedType override for the terrain-cost overlay; `None`
    /// derives it from the selected unit.
    pub(crate) debug_terrain_cost_speed_type: Option<crate::rules::locomotor_type::SpeedType>,
    /// Cell grid overlay toggle.
    pub(crate) debug_show_cell_grid: bool,
    /// Heightmap overlay toggle.
    pub(crate) debug_show_heightmap: bool,
    /// Foot-path step overlay toggle (Ctrl+Shift+O): draws the selected units'
    /// remaining route as a numbered line, so the shape and the step order both
    /// read at a glance. Independent of the PathGrid terrain-cost overlay.
    pub(crate) debug_show_foot_path: bool,
    /// Optional per-tick parity digest capture (diagnostics; never perturbs
    /// the run being measured).
    pub(crate) parity_digest_sink: Option<crate::sim::parity_digest::ParityDigestSink>,
    /// Rolling frame-time statistics for map observation metadata.
    pub(crate) frame_timer: FrameTimer,
}

/// A diagnostic input policy, never a second simulation clock. The map route
/// is one-shot and exits with its capture session; normal sessions retain their
/// existing, distinct tooltip and radar wall-clock epochs.
enum PresentationClock {
    Wall,
    MapExactStep,
}

pub(crate) const MAP_PRESENTATION_CLOCK_POLICY: &str = "map-exact-step-presentation-v1";
pub(crate) const MAP_PRESENTATION_INTERVAL_MS: u64 = 22;

impl DiagnosticsState {
    pub(crate) fn new(
        parity_digest_sink: Option<crate::sim::parity_digest::ParityDigestSink>,
    ) -> Self {
        Self {
            presentation_clock: PresentationClock::Wall,
            debug_frame_step_requested: false,
            debug_show_pathgrid: false,
            debug_terrain_cost_speed_type: None,
            debug_show_cell_grid: false,
            debug_show_heightmap: false,
            debug_show_foot_path: false,
            parity_digest_sink,
            frame_timer: FrameTimer::new(),
        }
    }

    pub(crate) fn use_map_presentation_clock(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(self.presentation_clock, PresentationClock::Wall),
            "map presentation clock already installed"
        );
        anyhow::ensure!(
            u64::from(crate::app::types::SIM_TICK_MS) == MAP_PRESENTATION_INTERVAL_MS,
            "compiled step differs from map presentation clock policy"
        );
        self.presentation_clock = PresentationClock::MapExactStep;
        Ok(())
    }

    fn presentation_ms(&self, committed_tick: u64) -> Option<u64> {
        match self.presentation_clock {
            PresentationClock::Wall => None,
            PresentationClock::MapExactStep => Some(
                committed_tick
                    .checked_mul(MAP_PRESENTATION_INTERVAL_MS)
                    .expect("bounded map observation tick overflows presentation time"),
            ),
        }
    }
}

impl crate::app::AppState {
    /// Read committed simulation state on demand. Trigger announcements are
    /// posted inside exact advancement, before its receipt returns to capture;
    /// caching the previous draw's time would timestamp them one tick early.
    pub(crate) fn diagnostic_presentation_ms(&self) -> Option<u64> {
        let tick = self
            .match_state
            .sim_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.simulation.session.tick);
        self.diag.presentation_ms(tick)
    }

    pub(crate) fn radar_presentation_ms(&self) -> u64 {
        self.diagnostic_presentation_ms()
            .unwrap_or_else(crate::app::match_runtime::frame_pacer::wall_clock_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_map_policy_overrides_existing_clock_sources() {
        let mut diagnostics = DiagnosticsState::new(None);
        assert_eq!(diagnostics.presentation_ms(1), None);
        diagnostics.use_map_presentation_clock().unwrap();
        for (tick, expected) in [(0, 0), (1, 22), (3, 66), (100_000, 2_200_000)] {
            assert_eq!(diagnostics.presentation_ms(tick), Some(expected));
        }
        assert!(diagnostics.use_map_presentation_clock().is_err());
        assert_eq!(DiagnosticsState::new(None).presentation_ms(1), None);
    }

    #[test]
    fn committed_step_message_keeps_its_full_lifetime() {
        let mut diagnostics = DiagnosticsState::new(None);
        diagnostics.use_map_presentation_clock().unwrap();
        let mut messages = crate::ui::messages::MessageList::new(0, 0, 6, 600);
        let pause_clock = crate::ui::messages::PauseAwareClock::default();
        let now = |tick| pause_clock.now(diagnostics.presentation_ms(tick).unwrap());
        messages.add_message(
            &crate::ui::messages::MessagePost {
                prefix: None,
                text: "Mission Accomplished",
                rgb: [1.0; 3],
                timeout_ms: Some(4_000),
                silent: true,
            },
            now(1),
            &|text| text.len() as i32,
        );
        assert_eq!(messages.messages()[0].deadline_ms, Some(4_022));
        // A cached pre-step timestamp would incorrectly expire at tick182.
        assert!(!messages.manage(now(182)));
        assert!(messages.manage(now(183)));
        assert!(messages.messages().is_empty());
    }

    #[test]
    fn frame_timer_empty_returns_zero() {
        let t = FrameTimer::new();
        assert_eq!(t.frame_ms_mean(), 0.0);
    }

    #[test]
    fn frame_timer_single_sample_is_still_zero() {
        // First sample establishes the baseline; no delta yet.
        let mut t = FrameTimer::new();
        t.sample(Instant::now());
        assert_eq!(t.frame_ms_mean(), 0.0);
    }

    #[test]
    fn frame_timer_two_samples_record_one_delta() {
        let mut t = FrameTimer::new();
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(16);
        t.sample(t0);
        t.sample(t1);
        let mean = t.frame_ms_mean();
        assert!((mean - 16.0).abs() < 0.5, "expected ~16ms, got {mean}");
    }

    #[test]
    fn frame_timer_window_caps_at_60() {
        let mut t = FrameTimer::new();
        let t0 = Instant::now();
        for i in 0..200 {
            t.sample(t0 + Duration::from_millis(16 * i));
        }
        assert_eq!(t.samples.len(), FRAME_TIMER_WINDOW);
    }
}
