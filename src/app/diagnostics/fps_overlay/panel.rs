//! The FPS panel's figures and the rows they read as. The figures close once a
//! second, so the rows hold still long enough to read; `FpsOverlay` lays the
//! rows out and draws them.

use crate::app::diagnostics::core_load::CoreLoadSampler;
use crate::app::diagnostics::state::RenderTimingMeter;
use crate::app::presentation::interpolation::SimulationReadout;
use crate::app::presentation::render::RenderPhases;
use crate::sim::frame_profile::PHASE_COUNT;
use crate::util::logging::LoggedProblem;
use crate::util::time::{Duration, Instant};

/// The panel's green, and the yellow and red it marks hot spots with.
pub(super) const GREEN: [u8; 3] = [60, 210, 45];
pub(super) const WARM: [u8; 3] = [249, 255, 0];
pub(super) const HOT: [u8; 3] = [255, 0, 0];
/// A core bar's empty part.
pub(super) const TRACK: [u8; 3] = [30, 84, 24];

const WINDOW: Duration = Duration::from_secs(1);
const DOT: &str = "  ·  ";
/// Areas under this share of the window stay off the Most CPU row.
const MIN_AREA_PERCENT: f32 = 1.0;
const MOST_AREAS: usize = 5;

/// The simulation's frame-profile phases, by `frame_profile::phase` slot.
const SIM_AREAS: [&str; PHASE_COUNT] = [
    "Commands in",
    "Commands",
    "Triggers",
    "Shroud",
    "Lighting",
    "Ore growth",
    "Bombs",
    "Superweapons",
    "Radiation",
    "Team AI",
    "Objects",
    "Wall crush",
    "Wakes",
    "Vision",
    "Combat",
    "Passengers",
    "Ion blasts",
    "Crates",
    "Production",
    "Docks",
    "Spawn fog",
    "Late AI",
    "Overlays",
    "Debug checks",
    "Anims",
    "Score",
    "State hash",
];

/// The render's phases, in `RenderPhases::spans` order.
const RENDER_AREAS: [&str; 7] = [
    "Draw world",
    "Draw debug",
    "Draw shroud",
    "Draw UI",
    "Draw sidebar",
    "GPU upload",
    "Draw other",
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
    /// The costliest areas first, in percent of the window's wall time.
    pub(super) areas: Vec<(&'static str, f32)>,
    pub(super) core_loads: Option<Vec<f32>>,
    /// The newest warning or error, and how long before the window closed.
    pub(super) problem: Option<(log::Level, String, Duration)>,
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
        problem: Option<LoggedProblem>,
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
            problem: problem.map(|problem| {
                (
                    problem.level,
                    problem.message,
                    now.saturating_duration_since(problem.at),
                )
            }),
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
    render: &[Duration; 7],
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

/// The panel in GAME.FNT: rates and units, Most CPU, Cores and the newest
/// warning, each row only once it has something to say.
pub(super) fn rows(shown: &Shown) -> Vec<Vec<Part>> {
    let mut rows = vec![vec![Part::Text(first_row(shown), GREEN)]];
    if let Some((first, rest)) = shown.areas.split_first() {
        let mut row = vec![
            Part::Text("Most CPU:  ".to_owned(), GREEN),
            Part::Text(area(first), WARM),
        ];
        if !rest.is_empty() {
            let rest = rest.iter().map(|other| format!("{DOT}{}", area(other)));
            row.push(Part::Text(rest.collect(), GREEN));
        }
        rows.push(row);
    }
    if let Some(loads) = shown.core_loads.as_ref().filter(|loads| !loads.is_empty()) {
        let average = loads.iter().sum::<f32>() / loads.len() as f32;
        rows.push(vec![
            Part::Text("Cores  ".to_owned(), GREEN),
            Part::Bars(loads.clone()),
            Part::Text(format!("   avg {average:.0}%"), GREEN),
        ]);
    }
    if let Some((level, message, age)) = &shown.problem {
        let (label, color) = match level {
            log::Level::Error => ("Error", HOT),
            _ => ("Warning", WARM),
        };
        let message = message.lines().next().unwrap_or("").replace('\t', " ");
        rows.push(vec![Part::Text(
            format!("{label}{DOT}{}{DOT}{}", ago(*age), message.trim()),
            color,
        )]);
    }
    rows
}

/// Before GAME.FNT loads: the first row in the built-in font's characters.
pub(super) fn fallback_rows(shown: &Shown) -> Vec<Vec<Part>> {
    vec![vec![Part::Text(
        simulation_readout_text(shown.frame_ms, shown.simulation),
        GREEN,
    )]]
}

fn first_row(shown: &Shown) -> String {
    let frame_ms = shown.frame_ms;
    let mut row = if frame_ms.is_finite() && frame_ms > 0.0 {
        let ms = if frame_ms < 10.0 {
            format!("{frame_ms:.1}")
        } else {
            format!("{frame_ms:.0}")
        };
        format!("FPS {:.0}{DOT}{ms} ms", 1000.0 / frame_ms)
    } else {
        "FPS --".to_owned()
    };
    if let Some(simulation) = shown.simulation {
        let interpolation = match (simulation.interpolation, simulation.sampling) {
            (true, true) => "on",
            (true, false) => "bypass",
            (false, _) => "off",
        };
        row.push_str(&format!(
            "{DOT}Sim {:.0} TPS{DOT}Interp {interpolation}",
            simulation.tps
        ));
    }
    if let Some(units) = shown.units {
        row.push_str(&format!("{DOT}Units {}", thousands(units)));
    }
    row
}

fn area(&(name, percent): &(&str, f32)) -> String {
    format!("{name} {percent:.0}%")
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

fn ago(age: Duration) -> String {
    match age.as_secs() {
        seconds @ 0..60 => format!("{seconds} s ago"),
        seconds @ 60..3_600 => format!("{} min ago", seconds / 60),
        seconds => format!("{} h ago", seconds / 3_600),
    }
}

fn readout_text(frame_ms: f32) -> String {
    if !frame_ms.is_finite() || frame_ms <= 0.0 {
        "FPS --".to_owned()
    } else {
        // The built-in font is uppercase-only and has no decimal point.
        format!("FPS {:.0}  {:.0} MS", 1000.0 / frame_ms, frame_ms)
    }
}

fn simulation_readout_text(frame_ms: f32, simulation: Option<SimulationReadout>) -> String {
    let mut text = readout_text(frame_ms);
    if let Some(simulation) = simulation {
        text.push_str(&format!(
            "  SIM {:.0} TPS  INTERP {}",
            simulation.tps,
            if simulation.interpolation && simulation.sampling {
                "ON"
            } else if simulation.interpolation {
                "BYPASS"
            } else {
                "OFF"
            },
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const READOUT: SimulationReadout = SimulationReadout {
        tps: 30.0,
        interpolation: true,
        sampling: true,
    };

    fn text(part: &Part) -> &str {
        match part {
            Part::Text(text, _) => text,
            Part::Bars(_) => "",
        }
    }

    #[test]
    fn panel_rows_read_rates_costliest_areas_cores_and_the_newest_warning() {
        let shown = Shown {
            frame_ms: 1000.0 / 60.0,
            simulation: Some(READOUT),
            units: Some(12_345),
            areas: vec![("Objects", 38.4), ("Draw world", 27.0), ("Vision", 11.0)],
            core_loads: Some(vec![90.0, 10.0]),
            problem: Some((
                log::Level::Warn,
                "missing art\tfor HTNK\nsecond line".to_owned(),
                Duration::from_secs(75),
            )),
        };
        let rows = rows(&shown);
        assert_eq!(
            text(&rows[0][0]),
            "FPS 60  ·  17 ms  ·  Sim 30 TPS  ·  Interp on  ·  Units 12,345"
        );
        assert_eq!(rows[1][1], Part::Text("Objects 38%".to_owned(), WARM));
        assert_eq!(text(&rows[1][2]), "  ·  Draw world 27%  ·  Vision 11%");
        assert_eq!(rows[2][1], Part::Bars(vec![90.0, 10.0]));
        assert_eq!(text(&rows[2][2]), "   avg 50%");
        assert_eq!(
            rows[3][0],
            Part::Text(
                "Warning  ·  1 min ago  ·  missing art for HTNK".to_owned(),
                WARM
            )
        );
        // Shell screens: no match figures, no measured areas or cores yet.
        let shell = rows_of(Shown {
            frame_ms: 4.0,
            ..Shown::default()
        });
        assert_eq!(shell, ["FPS 250  ·  4.0 ms"]);
    }

    fn rows_of(shown: Shown) -> Vec<String> {
        rows(&shown)
            .iter()
            .map(|row| row.iter().map(text).collect())
            .collect()
    }

    #[test]
    fn most_cpu_ranks_sim_and_render_areas_and_skips_a_restarted_profile() {
        let mut sim = [0u128; PHASE_COUNT];
        sim[crate::sim::frame_profile::phase::OBJECTS] = 300_000_000;
        sim[crate::sim::frame_profile::phase::VISION] = 5_000_000;
        let mut render = [Duration::ZERO; 7];
        render[0] = Duration::from_millis(200);
        render[6] = Duration::from_millis(400);
        assert_eq!(
            most_cpu(Duration::from_secs(1), Some(&sim), &render),
            [
                ("Draw other", 40.0),
                ("Objects", 30.0),
                ("Draw world", 20.0)
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

    #[test]
    fn fallback_readout_stays_in_the_built_in_font() {
        let supported = crate::render::bit_font::fallback_5x7_glyphs();
        for frame_ms in [0.0, f32::NAN, 0.5, 1000.0 / 60.0, 1234.0] {
            for (interpolation, sampling) in [(true, true), (false, false), (true, false)] {
                let simulation = SimulationReadout {
                    interpolation,
                    sampling,
                    ..READOUT
                };
                for ch in simulation_readout_text(frame_ms, Some(simulation)).chars() {
                    assert!(
                        supported.iter().any(|(glyph, _)| *glyph == ch),
                        "missing {ch}"
                    );
                }
            }
        }
        assert_eq!(readout_text(f32::NAN), "FPS --");
        assert_eq!(
            simulation_readout_text(1000.0 / 60.0, Some(READOUT)),
            "FPS 60  17 MS  SIM 30 TPS  INTERP ON"
        );
    }
}
