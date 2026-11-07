//! The performance panel's figures and the rows they read as. The figures
//! close once a second, so the rows hold still long enough to read;
//! `PerformanceOverlay` lays the rows out and draws them.

use crate::app::diagnostics::core_load::CoreLoadSampler;
use crate::app::diagnostics::state::RenderTimingMeter;
use crate::app::presentation::interpolation::SimulationReadout;
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
const MOST_AREAS: usize = 5;

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
    "VERA:CpuObjects",
    "VERA:CpuWallCrush",
    "VERA:CpuWakes",
    "VERA:CpuVision",
    "VERA:CpuCombat",
    "VERA:CpuPassengers",
    "VERA:CpuIonBlasts",
    "VERA:CpuCrates",
    "VERA:CpuProduction",
    "VERA:CpuDocks",
    "VERA:CpuSpawnFog",
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
}

/// The last closed window's figures.
#[derive(Default)]
pub(super) struct Shown {
    pub(super) frame_ms: f32,
    pub(super) simulation: Option<SimulationReadout>,
    pub(super) units: Option<usize>,
    /// The costliest areas first: their labels and percent of the window's
    /// wall time.
    pub(super) areas: Vec<(&'static str, f32)>,
    pub(super) core_loads: Option<Vec<f32>>,
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
    cores: CoreLoadSampler,
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
        simulation: Option<SimulationReadout>,
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
        self.shown = Shown {
            frame_ms,
            simulation,
            units: sample.map(|sample| sample.units),
            areas: wall.map_or_else(Vec::new, |wall| most_cpu(wall, sim_spans.as_ref(), &render)),
            core_loads: self.cores.sample(),
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
/// units, Most CPU and Cores, each row only once it has something to say.
pub(super) fn rows(shown: &Shown) -> Vec<Vec<Part>> {
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
        rows.push(vec![
            Part::Text(format!("{}  ", tr("VERA:FpsPanelCores")), GREEN),
            Part::Bars(loads.clone()),
            Part::Text(format!("   {average}"), GREEN),
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
    if let Some(simulation) = shown.simulation {
        let tps = format!("{:.0}", simulation.tps);
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

#[cfg(test)]
mod tests {
    use super::*;

    const READOUT: SimulationReadout = SimulationReadout {
        tps: 30.0,
        sampling: true,
    };

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
            simulation: Some(READOUT),
            units: Some(12_345),
            areas: vec![
                ("VERA:CpuObjects", 38.4),
                ("VERA:CpuDrawWorld", 27.0),
                ("VERA:CpuVision", 11.0),
            ],
            core_loads: Some(vec![90.0, 10.0]),
            gpu_ms: None,
        };
        let rows = rows(&shown);
        assert_eq!(
            text(&rows[0][0]),
            "FPS 60  ·  17 ms  ·  Sim 30 TPS  ·  Units 12,345"
        );
        assert_eq!(rows[1][1], Part::Text("Objects 38%".to_owned(), WARM));
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
            rows(&Shown {
                frame_ms,
                gpu_ms: Some(gpu_ms),
                ..Shown::default()
            })
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

    fn rows_of(shown: Shown) -> Vec<String> {
        rows(&shown)
            .iter()
            .map(|row| row.iter().map(text).collect())
            .collect()
    }

    #[test]
    fn every_panel_label_has_english_text() {
        let rows = rows_of(Shown {
            frame_ms: f32::NAN,
            simulation: Some(READOUT),
            areas: SIM_AREAS
                .iter()
                .chain(&RENDER_AREAS)
                .map(|&key| (key, 1.0))
                .collect(),
            core_loads: Some(vec![40.0]),
            ..Shown::default()
        });
        assert_eq!(rows[0], "FPS --  ·  Sim 30 TPS");
        assert!(rows[1].starts_with("Most CPU:  Commands in 1%  ·  Commands 1%"));
        assert_eq!(rows[2], "Cores     avg 40%");
        for text in &rows {
            assert!(!text.contains("MISSING:"), "{text}");
        }
    }

    #[test]
    fn most_cpu_ranks_sim_and_render_areas_and_skips_a_restarted_profile() {
        let mut sim = [0u128; PHASE_COUNT];
        sim[crate::sim::frame_profile::phase::OBJECTS] = 300_000_000;
        sim[crate::sim::frame_profile::phase::VISION] = 5_000_000;
        let mut render = [Duration::ZERO; RenderPhases::COUNT];
        render[0] = Duration::from_millis(200);
        render[9] = Duration::from_millis(400);
        render[10] = Duration::from_millis(5);
        assert_eq!(
            most_cpu(Duration::from_secs(1), Some(&sim), &render),
            [
                ("VERA:CpuDriverHandoff", 40.0),
                ("VERA:CpuObjects", 30.0),
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
