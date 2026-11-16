//! The performance panel's figures and the rows they read as. The figures
//! close once a second, so the rows hold still long enough to read;
//! `PerformanceOverlay` lays the rows out and draws them.

use crate::app::diagnostics::core_load::{self, CoreLoadSampler};
use crate::app::diagnostics::state::RenderTimingMeter;
use crate::app::diagnostics::thread_load::{ThreadLoadSampler, ThreadLoads};
use crate::app::match_runtime::frame_pacer::LoopCounts;
use crate::app::presentation::render::RenderPhases;
use crate::i18n::{tr, tr_fill};
use crate::sim::frame_profile::PHASE_COUNT;
use crate::util::time::{Duration, Instant};

/// The panel's green, and the yellow and red it marks hot spots with.
pub(super) const GREEN: [u8; 3] = [60, 210, 45];
pub(super) const WARM: [u8; 3] = [249, 255, 0];
pub(super) const HOT: [u8; 3] = [255, 0, 0];
/// A core bar's empty part.
pub(super) const TRACK: [u8; 3] = [30, 84, 24];

const WINDOW: Duration = Duration::from_secs(1);
/// The gap between a row's entries.
pub(super) const DOT: &str = "  ·  ";
/// Areas under this share of the window stay off the Most CPU row.
const MIN_AREA_PERCENT: f32 = 1.0;
/// Room for the object pass's parts beside the costliest drawing.
const MOST_AREAS: usize = 8;

/// The labels of the simulation's frame-profile phases, by
/// `frame_profile::phase` slot.
const SIM_AREAS: [&str; PHASE_COUNT] = [
    "VERA:CpuCommandsIn",
    "VERA:CpuCommands",
    "VERA:CpuTriggers",
    "VERA:CpuShroud",
    "VERA:CpuLighting",
    "VERA:CpuOreGrowth",
    "VERA:CpuBombs",
    "VERA:CpuSuperweapons",
    "VERA:CpuRadiation",
    "VERA:CpuTeamAI",
    "VERA:CpuUnitAI",
    "VERA:CpuPathfinding",
    "VERA:CpuTargeting",
    "VERA:CpuMovement",
    "VERA:CpuFiring",
    "VERA:CpuBuildings",
    "VERA:CpuBullets",
    "VERA:CpuEffects",
    "VERA:CpuWakes",
    "VERA:CpuVision",
    "VERA:CpuCombat",
    "VERA:CpuIonBlasts",
    "VERA:CpuCrates",
    "VERA:CpuProduction",
    "VERA:CpuLateAI",
    "VERA:CpuOverlays",
    "VERA:CpuDebugChecks",
    "VERA:CpuAnims",
    "VERA:CpuScore",
    "VERA:CpuStateHash",
];

/// The labels of the render's phases, in `RenderPhases::spans` order.
const RENDER_AREAS: [&str; RenderPhases::COUNT] = [
    "VERA:CpuDrawWorld",
    "VERA:CpuDrawDebug",
    "VERA:CpuDrawShroud",
    "VERA:CpuDrawUI",
    "VERA:CpuDrawSidebar",
    "VERA:CpuGpuUpload",
    "VERA:CpuDrawLights",
    "VERA:CpuDrawEffects",
    "VERA:CpuDrawCalls",
    "VERA:CpuDriverHandoff",
    "VERA:CpuDrawOther",
];

/// A running match's figures, read when a window closes.
#[derive(Clone, Copy)]
pub(super) struct MatchSample {
    pub(super) units: usize,
    /// `FrameProfile::totals_ns`: frames and nanoseconds per phase.
    pub(super) profile: (u64, [u128; PHASE_COUNT]),
    /// The frame pacer's running counts.
    pub(super) loop_counts: LoopCounts,
}

/// A match's rates over the last window, per second: the simulation frames
/// ticked on the main thread and on the simulation thread, and the event
/// loop's turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LoopRates {
    pub(super) main_ticks: f32,
    pub(super) simulation_ticks: f32,
    pub(super) turns: f32,
}

/// The last closed window's figures.
#[derive(Default)]
pub(super) struct Shown {
    pub(super) frame_ms: f32,
    pub(super) tps: Option<f32>,
    pub(super) units: Option<usize>,
    /// The costliest areas first: their labels and percent of the window's
    /// wall time.
    pub(super) areas: Vec<(&'static str, f32)>,
    pub(super) core_loads: Option<Vec<f32>>,
    /// The cores the process may use and the system's
    /// (`core_load::allowed_cores`).
    pub(super) allowed_cores: Option<(u32, u32)>,
    pub(super) threads: Option<ThreadLoads>,
    pub(super) loop_rates: Option<LoopRates>,
    /// The GPU's mean time on each of VERA's frames, in milliseconds.
    pub(super) gpu_ms: Option<f32>,
}

/// The open window. Reset while the panel is hidden, so showing it measures
/// afresh instead of averaging over the hidden time.
#[derive(Default)]
pub(super) struct PanelStats {
    window_start: Option<Instant>,
    render: RenderTimingMeter,
    sim_baseline: Option<(u64, [u128; PHASE_COUNT])>,
    loop_baseline: Option<LoopCounts>,
    cores: CoreLoadSampler,
    threads: ThreadLoadSampler,
    pub(super) shown: Shown,
}

impl PanelStats {
    pub(super) fn record_render(&mut self, phases: RenderPhases) {
        self.render.record(phases);
    }

    /// Whether the window has run its second; the first draw after showing
    /// closes one at once, so the first row has figures straight away.
    pub(super) fn due(&self, now: Instant) -> bool {
        self.window_start
            .is_none_or(|start| now.duration_since(start) >= WINDOW)
    }

    pub(super) fn close_window(
        &mut self,
        now: Instant,
        frame_ms: f32,
        tps: Option<f32>,
        sample: Option<MatchSample>,
        gpu_ms: Option<f32>,
    ) {
        let wall = self
            .window_start
            .replace(now)
            .map(|start| now.duration_since(start));
        let render = std::mem::take(&mut self.render).totals();
        let sim_spans = sample
            .zip(self.sim_baseline)
            .and_then(|(sample, before)| profile_delta(before, sample.profile));
        self.sim_baseline = sample.map(|sample| sample.profile);
        let loop_counts = sample
            .zip(self.loop_baseline)
            .and_then(|(sample, before)| sample.loop_counts.since(before));
        self.loop_baseline = sample.map(|sample| sample.loop_counts);
        self.shown = Shown {
            frame_ms,
            tps,
            units: sample.map(|sample| sample.units),
            areas: wall.map_or_else(Vec::new, |wall| most_cpu(wall, sim_spans.as_ref(), &render)),
            core_loads: self.cores.sample(),
            allowed_cores: core_load::allowed_cores(),
            threads: self.threads.sample(now),
            loop_rates: wall
                .zip(loop_counts)
                .and_then(|(wall, counts)| loop_rates(wall, counts)),
            gpu_ms,
        };
    }
}

/// Nanoseconds per phase between two profile readings, or `None` when the
/// profile started over in between (another match, or measurement restarted).
fn profile_delta(
    (before_frames, before): (u64, [u128; PHASE_COUNT]),
    (frames, now): (u64, [u128; PHASE_COUNT]),
) -> Option<[u128; PHASE_COUNT]> {
    if frames < before_frames {
        return None;
    }
    let mut spans = [0; PHASE_COUNT];
    for ((span, now), before) in spans.iter_mut().zip(now).zip(before) {
        *span = now.checked_sub(before)?;
    }
    Some(spans)
}

/// `counts` over `wall`, per second.
fn loop_rates(wall: Duration, counts: LoopCounts) -> Option<LoopRates> {
    let seconds = wall.as_secs_f64();
    let rate = |count: u64| (count as f64 / seconds) as f32;
    (seconds > 0.0).then(|| LoopRates {
        main_ticks: rate(counts.inline_frames),
        simulation_ticks: rate(counts.lent_frames),
        turns: rate(counts.turns),
    })
}

/// The simulation and render areas that took the most of `wall`.
fn most_cpu(
    wall: Duration,
    sim: Option<&[u128; PHASE_COUNT]>,
    render: &[Duration; RenderPhases::COUNT],
) -> Vec<(&'static str, f32)> {
    let wall_ns = wall.as_nanos().max(1) as f64;
    let sim = sim
        .into_iter()
        .flat_map(|spans| SIM_AREAS.into_iter().zip(spans.iter().copied()));
    let render = RENDER_AREAS
        .into_iter()
        .zip(render.iter().map(Duration::as_nanos));
    let mut areas: Vec<_> = sim
        .chain(render)
        .map(|(name, ns)| (name, (ns as f64 * 100.0 / wall_ns) as f32))
        .filter(|&(_, percent)| percent >= MIN_AREA_PERCENT)
        .collect();
    areas.sort_by(|a, b| b.1.total_cmp(&a.1));
    areas.truncate(MOST_AREAS);
    areas
}

/// One stretch of a row: text in a color, or one bar per core.
#[derive(Debug, PartialEq)]
pub(super) enum Part {
    Text(String, [u8; 3]),
    Bars(Vec<f32>),
}

/// The panel's rows in the language in use ([`crate::i18n`]): rates and
/// units, Most CPU, Cores and the GPU, each row only once it has something
/// to say, and with `thread_rows` (the Thread Stats key) Threads and a
/// match's Ticks.
pub(super) fn rows(shown: &Shown, thread_rows: bool) -> Vec<Vec<Part>> {
    let mut rows = vec![vec![Part::Text(first_row(shown), GREEN)]];
    if let Some((first, rest)) = shown.areas.split_first() {
        let mut row = vec![
            Part::Text(format!("{}  ", tr("VERA:FpsPanelMostCpu")), GREEN),
            Part::Text(area(first), WARM),
        ];
        if !rest.is_empty() {
            let rest = rest.iter().map(|other| format!("{DOT}{}", area(other)));
            row.push(Part::Text(rest.collect(), GREEN));
        }
        rows.push(row);
    }
    if let Some(loads) = shown.core_loads.as_ref().filter(|loads| !loads.is_empty()) {
        let average = format!("{:.0}", loads.iter().sum::<f32>() / loads.len() as f32);
        let average = tr_fill("VERA:FpsPanelAverage", &[("percent", &average)]);
        let mut row = vec![
            Part::Text(format!("{}  ", tr("VERA:FpsPanelCores")), GREEN),
            Part::Bars(loads.clone()),
            Part::Text(format!("   {average}"), GREEN),
        ];
        // Only when the system holds the game to fewer cores than it has.
        if let Some((allowed, total)) = shown
            .allowed_cores
            .filter(|(allowed, total)| allowed < total)
        {
            let allowed = tr_fill(
                "VERA:FpsPanelCoresAllowed",
                &[
                    ("allowed", &allowed.to_string()),
                    ("total", &total.to_string()),
                ],
            );
            row.push(Part::Text(format!("{DOT}{allowed}"), WARM));
        }
        rows.push(row);
    }
    // Which of the game's threads the cores' load is: a share of one core
    // each, so the rest of the threads together can pass 100.
    if let Some(threads) = shown.threads.filter(|_| thread_rows) {
        let load = |key, percent: f32| tr_fill(key, &[("percent", &format!("{percent:.0}"))]);
        rows.push(vec![
            Part::Text(format!("{}  ", tr("VERA:FpsPanelThreads")), GREEN),
            Part::Text(
                format!(
                    "{}{DOT}{}{DOT}{}",
                    load("VERA:FpsPanelMainThread", threads.main),
                    load("VERA:FpsPanelSimThread", threads.simulation),
                    load("VERA:FpsPanelOtherThreads", threads.others),
                ),
                GREEN,
            ),
        ]);
    }
    // Where a match's ticks ran, and how often the event loop turned.
    if let Some(rates) = shown.loop_rates.filter(|_| thread_rows) {
        let count = |key, rate: f32| tr_fill(key, &[("count", &thousands(rate.round() as usize))]);
        rows.push(vec![
            Part::Text(format!("{}  ", tr("VERA:FpsPanelTicks")), GREEN),
            Part::Text(
                format!(
                    "{}{DOT}{}{DOT}{}",
                    count("VERA:FpsPanelTicksMain", rates.main_ticks),
                    count("VERA:FpsPanelTicksSim", rates.simulation_ticks),
                    count("VERA:FpsPanelLoopTurns", rates.turns),
                ),
                GREEN,
            ),
        ]);
    }
    // The GPU's busy share of the time between frames, under one bar like a
    // core's, then its time per frame.
    let frame_ms = Some(shown.frame_ms).filter(|ms| ms.is_finite() && *ms > 0.0);
    if let Some((gpu_ms, frame_ms)) = shown.gpu_ms.zip(frame_ms) {
        let load = (gpu_ms * 100.0 / frame_ms).min(100.0);
        let percent = tr_fill(
            "VERA:FpsPanelGpuLoad",
            &[("percent", &format!("{load:.0}"))],
        );
        let time = tr_fill("VERA:FpsPanelFrameTime", &[("ms", &milliseconds(gpu_ms))]);
        rows.push(vec![
            Part::Text(format!("{}  ", tr("VERA:FpsPanelGpu")), GREEN),
            Part::Bars(vec![load]),
            Part::Text(format!("   {percent}{DOT}{time}"), GREEN),
        ]);
    }
    rows
}

/// `ms` with one decimal under 10 and none from there.
fn milliseconds(ms: f32) -> String {
    if ms < 10.0 {
        format!("{ms:.1}")
    } else {
        format!("{ms:.0}")
    }
}

fn first_row(shown: &Shown) -> String {
    let frame_ms = shown.frame_ms;
    let mut row = if frame_ms.is_finite() && frame_ms > 0.0 {
        let fps = format!("{:.0}", 1000.0 / frame_ms);
        format!(
            "{}{DOT}{}",
            tr_fill("VERA:FpsPanelFps", &[("fps", &fps)]),
            tr_fill("VERA:FpsPanelFrameTime", &[("ms", &milliseconds(frame_ms))])
        )
    } else {
        tr_fill("VERA:FpsPanelFps", &[("fps", &"--")])
    };
    if let Some(tps) = shown.tps {
        let tps = format!("{tps:.0}");
        let sim = tr_fill("VERA:FpsPanelSim", &[("tps", &tps)]);
        row.push_str(&format!("{DOT}{sim}"));
    }
    if let Some(units) = shown.units {
        let units = tr_fill("VERA:FpsPanelUnits", &[("count", &thousands(units))]);
        row.push_str(&format!("{DOT}{units}"));
    }
    row
}

fn area(&(key, percent): &(&str, f32)) -> String {
    let percent = format!("{percent:.0}");
    tr_fill(
        "VERA:FpsPanelArea",
        &[("area", &tr(key)), ("percent", &percent)],
    )
}

fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    fn text(part: &Part) -> &str {
        match part {
            Part::Text(text, _) => text,
            Part::Bars(_) => "",
        }
    }

    #[test]
    fn panel_rows_read_rates_costliest_areas_and_cores() {
        let shown = Shown {
            frame_ms: 1000.0 / 60.0,
            tps: Some(30.0),
            units: Some(12_345),
            areas: vec![
                ("VERA:CpuMovement", 38.4),
                ("VERA:CpuDrawWorld", 27.0),
                ("VERA:CpuVision", 11.0),
            ],
            core_loads: Some(vec![90.0, 10.0]),
            ..Shown::default()
        };
        let rows = rows(&shown, false);
        assert_eq!(
            text(&rows[0][0]),
            "FPS 60  ·  17 ms  ·  Sim 30 TPS  ·  Units 12,345"
        );
        assert_eq!(rows[1][1], Part::Text("Movement 38%".to_owned(), WARM));
        assert_eq!(text(&rows[1][2]), "  ·  Draw world 27%  ·  Vision 11%");
        assert_eq!(rows[2][1], Part::Bars(vec![90.0, 10.0]));
        assert_eq!(text(&rows[2][2]), "   avg 50%");
        assert_eq!(rows.len(), 3);
        // Shell screens: no match figures, no measured areas or cores yet.
        let shell = rows_of(Shown {
            frame_ms: 4.0,
            ..Shown::default()
        });
        assert_eq!(shell, ["FPS 250  ·  4.0 ms"]);
    }

    #[test]
    fn the_gpu_row_reads_the_gpus_share_of_the_time_between_frames() {
        let gpu = |gpu_ms, frame_ms| {
            rows(
                &Shown {
                    frame_ms,
                    gpu_ms: Some(gpu_ms),
                    ..Shown::default()
                },
                false,
            )
            .pop()
            .expect("a first row")
        };
        let row = gpu(5.0, 20.0);
        assert_eq!(text(&row[0]), "GPU  ");
        assert_eq!(row[1], Part::Bars(vec![25.0]));
        assert_eq!(text(&row[2]), "   25%  ·  5.0 ms");
        // A GPU-bound frame fills the bar, and no frame rate yet means no row.
        assert_eq!(gpu(30.0, 20.0)[1], Part::Bars(vec![100.0]));
        assert_eq!(gpu(5.0, f32::NAN).len(), 1);
    }

    /// The rows' text with the thread rows on.
    fn rows_of(shown: Shown) -> Vec<String> {
        rows(&shown, true)
            .iter()
            .map(|row| row.iter().map(text).collect())
            .collect()
    }

    #[test]
    fn every_panel_label_has_english_text() {
        let rows = rows_of(Shown {
            frame_ms: f32::NAN,
            tps: Some(30.0),
            areas: SIM_AREAS
                .iter()
                .chain(&RENDER_AREAS)
                .map(|&key| (key, 1.0))
                .collect(),
            core_loads: Some(vec![40.0]),
            allowed_cores: Some((1, 2)),
            threads: Some(ThreadLoads {
                main: 1.0,
                simulation: 1.0,
                others: 1.0,
            }),
            loop_rates: Some(LoopRates {
                main_ticks: 1.0,
                simulation_ticks: 1.0,
                turns: 1.0,
            }),
            ..Shown::default()
        });
        assert_eq!(rows[0], "FPS --  ·  Sim 30 TPS");
        assert!(rows[1].starts_with("Most CPU:  Commands in 1%  ·  Commands 1%"));
        assert_eq!(rows[2], "Cores     avg 40%  ·  1 of 2 allowed");
        assert_eq!(rows.len(), 5);
        for text in &rows {
            assert!(!text.contains("MISSING:"), "{text}");
        }
    }

    /// The rows that say which thread a full core is: each thread's share
    /// of one core, where a match's ticks ran, and the cores the system
    /// allows when it holds some back. The thread rows show only while
    /// their own key has them on.
    #[test]
    fn thread_and_tick_rows_say_which_thread_does_the_work() {
        let counts = |inline_frames, lent_frames, turns| LoopCounts {
            inline_frames,
            lent_frames,
            turns,
        };
        // 1.5 seconds: 21 ticks here, none lent, 1,860 loop turns.
        let rates = counts(121, 40, 2_860)
            .since(counts(100, 40, 1_000))
            .and_then(|counts| loop_rates(Duration::from_millis(1_500), counts));
        let shown = Shown {
            frame_ms: 50.0,
            core_loads: Some(vec![100.0, 10.0]),
            allowed_cores: Some((1, 8)),
            threads: Some(ThreadLoads {
                main: 96.6,
                simulation: 0.0,
                others: 112.0,
            }),
            loop_rates: rates,
            ..Shown::default()
        };
        assert_eq!(super::rows(&shown, false).len(), 2);
        let rows = rows_of(shown);
        assert_eq!(rows[1], "Cores     avg 55%  ·  1 of 8 allowed");
        assert_eq!(
            rows[2],
            "Threads  Main 97%  ·  Simulation 0%  ·  Others 112%"
        );
        assert_eq!(
            rows[3],
            "Ticks  14 on main  ·  0 on simulation  ·  1,240 loop turns"
        );
        // Every core allowed: nothing to say. A pacer that started over: no row.
        let rows = rows_of(Shown {
            frame_ms: 50.0,
            core_loads: Some(vec![100.0, 10.0]),
            allowed_cores: Some((8, 8)),
            loop_rates: counts(0, 0, 0)
                .since(counts(100, 40, 1_000))
                .and_then(|counts| loop_rates(Duration::from_secs(1), counts)),
            ..Shown::default()
        });
        assert_eq!(rows, ["FPS 20  ·  50 ms", "Cores     avg 55%"]);
    }

    #[test]
    fn most_cpu_ranks_sim_and_render_areas_and_skips_a_restarted_profile() {
        let mut sim = [0u128; PHASE_COUNT];
        sim[crate::sim::frame_profile::phase::MOVEMENT] = 300_000_000;
        sim[crate::sim::frame_profile::phase::VISION] = 5_000_000;
        let mut render = [Duration::ZERO; RenderPhases::COUNT];
        render[0] = Duration::from_millis(200);
        render[9] = Duration::from_millis(400);
        render[10] = Duration::from_millis(5);
        assert_eq!(
            most_cpu(Duration::from_secs(1), Some(&sim), &render),
            [
                ("VERA:CpuDriverHandoff", 40.0),
                ("VERA:CpuMovement", 30.0),
                ("VERA:CpuDrawWorld", 20.0)
            ]
        );
        let later = sim.map(|ns| ns + 1);
        assert_eq!(profile_delta((10, sim), (5, later)), None);
        assert_eq!(profile_delta((10, later), (12, sim)), None);
        assert_eq!(
            profile_delta((10, sim), (12, later)),
            Some([1; PHASE_COUNT])
        );
    }
}
