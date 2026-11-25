//! Spawn-only test switch `VERA20K_SPAWN_FRAME_COST=<frame>`
//! (tools/spawn/README.md): from that simulation frame on, a `-SPAWN` launch
//! records what its match costs the main thread, and its
//! `spawn_result.json` reports it as `frame_cost`. `tools.scale_bench` times
//! the client with it. Read only for `-SPAWN` launches; ordinary play never
//! consults it, and nothing here reaches the simulation.
//!
//! What it counts is the main thread's own CPU time (`thread_load`), so a
//! wait for the display, the frame hand-off or a tick on the simulation
//! thread costs nothing:
//!
//! - **a picture**: from the draw's start to its hand-off, the part of
//!   `render_frame` after the match service;
//! - **a simulation frame's service**: every `service_match_runtime` pass,
//!   which runs a finished tick's tail and prepares and lends the next (the
//!   frame the draw reads and the world copy, docs/sim-thread.md), less the
//!   ticks it ran inline. Passes that draw nothing count too.
//!
//! The two add up to what one picture of one simulation frame costs, whatever
//! the picture rate and the tick rate happened to be.

use crate::app::AppState;
use crate::app::diagnostics::thread_load::calling_thread_cpu;
use crate::util::time::Duration;

/// A launch's record. Empty until the match reaches its first frame to count.
#[derive(Debug, Default)]
pub(crate) struct FrameCost {
    /// The first simulation frame to count from.
    from_frame: u32,
    /// The simulation frame the last service pass saw, once counting.
    frames: Option<(u32, u32)>,
    /// Each drawn picture's main-thread time.
    pictures: Vec<Duration>,
    /// The service passes' main-thread time, inline ticks included.
    service: Duration,
    /// The inline ticks' main-thread time inside `service`.
    inline_ticks: Duration,
    /// Each simulation frame's own time, on whichever thread ran it.
    ticks: Vec<Duration>,
    /// How many of `ticks` ran on the simulation thread.
    lent: u32,
}

/// The calling thread's CPU time at the start of a counted span; `None`
/// while nothing counts, so an ordinary launch reads no clock.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Span(Option<Duration>);

impl Span {
    fn elapsed(self) -> Option<Duration> {
        Some(calling_thread_cpu()?.saturating_sub(self.0?))
    }
}

impl FrameCost {
    /// The record a launch keeps, from `VERA20K_SPAWN_FRAME_COST`.
    pub(crate) fn requested() -> Option<Self> {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            Self::from_value(std::env::var("VERA20K_SPAWN_FRAME_COST").ok().as_deref())
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            None
        }
    }

    /// An unset or unparsable value keeps no record.
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        expect(dead_code, reason = "the browser build reads no test switch")
    )]
    fn from_value(value: Option<&str>) -> Option<Self> {
        let from_frame = value?.trim().parse().ok()?;
        log::info!("Spawn test switch: counting frame cost from frame {from_frame}");
        Some(Self {
            from_frame,
            ..Self::default()
        })
    }

    /// The match is at simulation frame `frame`.
    pub(crate) fn observe_frame(&mut self, frame: u32) {
        if frame >= self.from_frame {
            let first = self.frames.map_or(frame, |(first, _)| first);
            self.frames = Some((first, frame));
        }
    }

    fn counting(&self) -> bool {
        self.frames.is_some()
    }

    /// What `spawn_result.json` carries; `None` before the first counted
    /// picture and simulation frame.
    pub(crate) fn report(&self) -> Option<FrameCostReport> {
        let (first_frame, last_frame) = self.frames?;
        if self.pictures.is_empty() || self.ticks.is_empty() {
            return None;
        }
        let ms = |time: Duration| time.as_secs_f64() * 1000.0;
        let mean = |times: &[Duration]| ms(times.iter().sum()) / times.len() as f64;
        let mut pictures = self.pictures.clone();
        pictures.sort_unstable();
        let mut ticks = self.ticks.clone();
        ticks.sort_unstable();
        let at = |sorted: &[Duration], fraction: f64| {
            ms(sorted[((sorted.len() - 1) as f64 * fraction).round() as usize])
        };
        let draw_ms = mean(&pictures);
        // A simulation frame's service: the passes' time, less the ticks
        // they ran themselves, over the frames they served.
        let service_ms =
            ms(self.service.saturating_sub(self.inline_ticks)) / self.ticks.len() as f64;
        Some(FrameCostReport {
            first_frame,
            last_frame,
            frame_ms: draw_ms + service_ms,
            draw_ms,
            draw_p50_ms: at(&pictures, 0.5),
            draw_p99_ms: at(&pictures, 0.99),
            service_ms,
            pictures: pictures.len(),
            tick_ms: mean(&ticks),
            tick_p99_ms: at(&ticks, 0.99),
            ticks: ticks.len(),
            lent_ticks: self.lent,
        })
    }
}

/// `spawn_result.json`'s `frame_cost` (tools/spawn/README.md): milliseconds
/// of the main thread's CPU time, but for the ticks' own wall time.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct FrameCostReport {
    /// The simulation frames the record spans.
    pub(crate) first_frame: u32,
    pub(crate) last_frame: u32,
    /// One picture and one simulation frame's service: `draw_ms + service_ms`.
    pub(crate) frame_ms: f64,
    /// The mean picture, its median and its 99th percentile.
    pub(crate) draw_ms: f64,
    pub(crate) draw_p50_ms: f64,
    pub(crate) draw_p99_ms: f64,
    /// The mean simulation frame's service, without the frame's own tick.
    pub(crate) service_ms: f64,
    pub(crate) pictures: usize,
    /// The mean simulation frame's own wall time and its 99th percentile.
    pub(crate) tick_ms: f64,
    pub(crate) tick_p99_ms: f64,
    pub(crate) ticks: usize,
    /// The simulation frames that ran on the simulation thread.
    pub(crate) lent_ticks: u32,
}

fn counting(state: &mut AppState) -> Option<&mut FrameCost> {
    state
        .frontend
        .external_progress
        .frame_cost
        .as_mut()
        .filter(|cost| cost.counting())
}

/// Start a span on the calling thread.
pub(crate) fn begin(state: &AppState) -> Span {
    let counting = state
        .frontend
        .external_progress
        .frame_cost
        .as_ref()
        .is_some_and(FrameCost::counting);
    Span(counting.then(calling_thread_cpu).flatten())
}

/// A `service_match_runtime` pass ended.
pub(crate) fn end_service(state: &mut AppState, span: Span) {
    if let Some((cost, spent)) = counting(state).zip(span.elapsed()) {
        cost.service += spent;
    }
}

/// A tick run inline by the service ended.
pub(crate) fn end_inline_tick(state: &mut AppState, span: Span) {
    if let Some((cost, spent)) = counting(state).zip(span.elapsed()) {
        cost.inline_ticks += spent;
    }
}

/// A picture was handed off.
pub(crate) fn end_picture(state: &mut AppState, span: Span) {
    if let Some((cost, spent)) = counting(state).zip(span.elapsed()) {
        cost.pictures.push(spent);
    }
}

/// A simulation frame took `tick` on its thread: the simulation thread when
/// `lent`.
pub(crate) fn tick(state: &mut AppState, tick: Duration, lent: bool) {
    if let Some(cost) = counting(state) {
        cost.ticks.push(tick);
        cost.lent += u32::from(lent);
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn the_switch_reads_a_frame_and_counts_from_it() {
        assert!(FrameCost::from_value(None).is_none());
        assert!(FrameCost::from_value(Some("soon")).is_none());
        let mut cost = FrameCost::from_value(Some(" 300 ")).expect("a frame");
        cost.observe_frame(299);
        assert!(!cost.counting());
        cost.observe_frame(300);
        cost.observe_frame(450);
        assert_eq!(cost.frames, Some((300, 450)));
    }

    #[test]
    fn a_frame_costs_its_picture_and_its_service_without_inline_ticks() {
        let ms = Duration::from_millis;
        let mut cost = FrameCost {
            frames: Some((300, 302)),
            pictures: vec![ms(4), ms(6), ms(8), ms(6)],
            // Two simulation frames' passes, one of which ran its 20 ms tick.
            service: ms(32),
            inline_ticks: ms(20),
            ticks: vec![ms(20), ms(30)],
            lent: 1,
            ..FrameCost::default()
        };
        let report = cost.report().expect("pictures and ticks were counted");
        assert_eq!((report.draw_ms, report.service_ms), (6.0, 6.0));
        assert_eq!(report.frame_ms, 12.0);
        assert_eq!((report.draw_p50_ms, report.draw_p99_ms), (6.0, 8.0));
        assert_eq!((report.tick_ms, report.tick_p99_ms), (25.0, 30.0));
        assert_eq!(
            (report.pictures, report.ticks, report.lent_ticks),
            (4, 2, 1)
        );
        // Nothing to report before a picture is counted.
        cost.pictures.clear();
        assert_eq!(cost.report(), None);
    }
}
