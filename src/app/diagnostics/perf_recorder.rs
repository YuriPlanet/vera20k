//! The perf build's per-frame CSV recorder.
//!
//! Compiled only with the `perf` feature. It writes one row per rendered
//! frame to `perf/perf-<unix_nanos>-<pid>.csv` under the process data folder
//! (or the working directory), with every SIM phase, render phase, path batch
//! counter, process counter and the GPU mean. All of these are derived timing
//! owned outside the simulation; nothing here is hashed or affects gameplay.
//!
//! This is a tool, not a game feature: the `perf` build records for the whole
//! run with no runtime toggle. The two timing ledgers it reads are forced on
//! by the `perf` cfg (`sim_tick.rs`, `render/mod.rs`).

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::PathBuf;

use vera20k_core::sim::frame_profile::{FramePhases, PHASE_KEYS};
use vera20k_core::sim::movement::PathBatchStats;

use crate::app::presentation::render::RENDER_PHASE_KEYS;
use crate::app::presentation::render::RenderPhases;
use crate::app::process_stats::ProcessStatsSampler;
use crate::util::time::Duration;

/// Flush the buffered CSV every this many frames, besides `Drop`.
const FLUSH_EVERY: u64 = 120;

/// The SIM-side part of one perf row, captured at the tick's tail where the
/// live runtime is guaranteed home. Stored on [`DiagnosticsState`](crate::app::diagnostics::state::DiagnosticsState)
/// so the present — which may run beside a lent tick, when `home_runtime()`
/// is `None` — can read the most recently committed tick instead of seeing
/// an empty snapshot. This is what keeps the CSV from being ~99% blank under
/// load.
#[derive(Clone, Copy)]
pub(crate) struct PerfTickSnapshot {
    pub tick: u64,
    pub binary_frame: u32,
    pub objects: usize,
    pub houses: usize,
    pub sim: Option<(FramePhases, Duration)>,
    pub path: Option<PathBatchStats>,
}

/// One frame's worth of recorded values. The app gathers these (reading only
/// snapshots and derived ledgers) and hands them to [`PerfRecorder::record`].
pub(crate) struct PerfRow {
    pub tick: u64,
    pub binary_frame: u32,
    pub wall_ms: f64,
    pub objects: usize,
    pub houses: usize,
    /// The most recent completed SIM frame and its whole span.
    pub sim: Option<(FramePhases, Duration)>,
    pub render: Option<RenderPhases>,
    pub path: Option<PathBatchStats>,
    /// GPU mean from the destructive readback (windowed, lags a few frames).
    pub gpu_ms: Option<f32>,
}

pub(crate) struct PerfRecorder {
    writer: Option<BufWriter<File>>,
    frames: u64,
    /// Own sampler so recording never competes with the dashboard's sampler.
    process: ProcessStatsSampler,
}

impl Default for PerfRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfRecorder {
    pub(crate) fn new() -> Self {
        let writer = Self::open().map(BufWriter::new);
        Self {
            writer,
            frames: 0,
            process: ProcessStatsSampler::default(),
        }
    }

    fn open() -> Option<File> {
        let mut dir = crate::util::platform_dir::get()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        dir.push("perf");
        if let Err(error) = std::fs::create_dir_all(&dir) {
            log::warn!("perf: cannot create {}: {error}", dir.display());
            return None;
        }
        let name = format!(
            "perf-{}-{}.csv",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
            std::process::id()
        );
        let path = dir.join(name);
        match File::create(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(Self::header().as_bytes()) {
                    log::warn!("perf: cannot write header to {}: {error}", path.display());
                    return None;
                }
                log::info!("perf: recording to {}", path.display());
                Some(file)
            }
            Err(error) => {
                log::warn!("perf: cannot create {}: {error}", path.display());
                None
            }
        }
    }

    /// The CSV header: base columns, then 30 SIM phases + total, 14 render
    /// phases, 11 path-batch counters, 5 process counters, and the GPU mean.
    fn header() -> String {
        let mut columns = vec![
            "tick".to_owned(),
            "binary_frame".to_owned(),
            "wall_ms".to_owned(),
            "objects".to_owned(),
            "houses".to_owned(),
        ];
        columns.extend(PHASE_KEYS.iter().map(|key| format!("sim_{key}")));
        columns.push("sim_total_ms".to_owned());
        columns.extend(RENDER_PHASE_KEYS.iter().map(|key| format!("render_{key}")));
        columns.extend([
            "path_waiting".to_owned(),
            "path_searched".to_owned(),
            "path_answered".to_owned(),
            "path_left_out".to_owned(),
            "path_steps".to_owned(),
            "path_urgency2_steps".to_owned(),
            "path_urgency1_steps".to_owned(),
            "path_last_ms".to_owned(),
            "path_mean_ms".to_owned(),
            "path_peak_ms".to_owned(),
            "path_samples".to_owned(),
        ]);
        columns.extend([
            "cpu_percent".to_owned(),
            "cpu_cores".to_owned(),
            "threads".to_owned(),
            "working_set_mb".to_owned(),
            "peak_working_set_mb".to_owned(),
            "gpu_ms".to_owned(),
        ]);
        format!("{}\n", columns.join(","))
    }

    /// Format one row. `process` is sampled here so the recorder owns it.
    fn row(&mut self, sample: PerfRow) -> String {
        let ms = |duration: Duration| format!("{:.3}", duration.as_secs_f64() * 1000.0);
        let mut values = vec![
            sample.tick.to_string(),
            sample.binary_frame.to_string(),
            format!("{:.3}", sample.wall_ms),
            sample.objects.to_string(),
            sample.houses.to_string(),
        ];
        match sample.sim {
            Some((phases, total)) => {
                for slot in 0..PHASE_KEYS.len() {
                    values.push(ms(phases.get(slot)));
                }
                values.push(ms(total));
            }
            None => values.extend(std::iter::repeat_n(String::new(), PHASE_KEYS.len() + 1)),
        }
        match sample.render {
            Some(phases) => values.extend(
                phases
                    .spans()
                    .into_iter()
                    .map(|span| format!("{:.3}", span.as_secs_f64() * 1000.0)),
            ),
            None => values.extend(std::iter::repeat_n(String::new(), RENDER_PHASE_KEYS.len())),
        }
        match sample.path {
            Some(path) => values.extend([
                path.waiting.to_string(),
                path.searched.to_string(),
                path.answered.to_string(),
                path.left_out.to_string(),
                path.steps.to_string(),
                path.urgency2_steps.to_string(),
                path.urgency1_steps.to_string(),
                format!("{:.3}", path.last_ms),
                format!("{:.3}", path.mean_ms),
                format!("{:.3}", path.peak_ms),
                path.samples.to_string(),
            ]),
            None => values.extend(std::iter::repeat_n(String::new(), 11)),
        }
        let process = self.process.sample();
        values.extend([
            format!("{:.2}", process.cpu_percent),
            process.cpu_cores.to_string(),
            process.threads.to_string(),
            format!("{:.1}", process.working_set_mb),
            format!("{:.1}", process.peak_working_set_mb),
        ]);
        values.push(
            sample
                .gpu_ms
                .map_or_else(String::new, |ms| format!("{ms:.3}")),
        );
        format!("{}\n", values.join(","))
    }

    pub(crate) fn record(&mut self, sample: PerfRow) {
        // Build the row first: it mutably borrows self (process sampler). Only
        // then take the writer, so the two mutable borrows do not overlap.
        let row = self.row(sample);
        let failed = match self.writer.as_mut() {
            Some(writer) => writer.write_all(row.as_bytes()).is_err(),
            None => return,
        };
        if failed {
            log::warn!("perf: write failed; disabling recorder");
            self.writer = None;
            return;
        }
        self.frames += 1;
        if self.frames % FLUSH_EVERY == 0 {
            self.flush();
        }
    }

    pub(crate) fn flush(&mut self) {
        if let Some(writer) = self.writer.as_mut()
            && let Err(error) = writer.flush()
        {
            log::warn!("perf: flush failed: {error}");
        }
    }
}

impl Drop for PerfRecorder {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn phase_key_tables_are_consistent() {
        // The recorder zips these tables with the ledgers' span arrays; a
        // mismatch would silently misalign every CSV column.
        assert_eq!(
            PHASE_KEYS.len(),
            vera20k_core::sim::frame_profile::PHASE_COUNT
        );
        assert_eq!(RENDER_PHASE_KEYS.len(), RenderPhases::COUNT);
    }

    #[test]
    fn header_has_all_columns_in_order() {
        let header = PerfRecorder::header();
        let columns: Vec<&str> = header.trim_end().split(',').collect();
        assert_eq!(
            columns.len(),
            5 + PHASE_KEYS.len() + 1 + RENDER_PHASE_KEYS.len() + 11 + 5 + 1
        );
        assert_eq!(
            &columns[..5],
            &["tick", "binary_frame", "wall_ms", "objects", "houses"]
        );
        assert_eq!(columns[5], "sim_commands_ingress");
        assert_eq!(columns[5 + PHASE_KEYS.len()], "sim_total_ms");
        assert_eq!(columns[6 + PHASE_KEYS.len()], "render_snapshot");
        assert_eq!(columns.last().copied(), Some("gpu_ms"));
    }

    fn perf_row(with_sim: bool) -> PerfRow {
        let sim = if with_sim {
            let mut phases = FramePhases::default();
            phases.set(0, Duration::from_millis(2));
            Some((phases, Duration::from_millis(40)))
        } else {
            None
        };
        PerfRow {
            tick: 7,
            binary_frame: 7,
            wall_ms: 16.5,
            objects: 100,
            houses: 30,
            sim,
            render: None,
            path: None,
            gpu_ms: Some(3.25),
        }
    }

    #[test]
    fn a_snapshot_row_has_populated_sim_cells_and_one_without_does_not() {
        let mut recorder = PerfRecorder {
            writer: None,
            frames: 0,
            process: ProcessStatsSampler::default(),
        };
        let with = recorder.row(perf_row(true));
        let cells: Vec<&str> = with.trim_end().split(',').collect();
        // Base 5, then 30 sim phases + total all present when snapshot exists.
        assert_eq!(cells[5], "2.000", "first SIM phase from the snapshot");
        assert_eq!(cells[5 + PHASE_KEYS.len()], "40.000", "SIM total from the snapshot");
        let without = recorder.row(perf_row(false));
        let cells: Vec<&str> = without.trim_end().split(',').collect();
        assert_eq!(cells[5], "", "no snapshot leaves SIM cells blank");
        assert_eq!(cells[5 + PHASE_KEYS.len()], "", "and no SIM total");
        // Both rows keep the base columns and the GPU value.
        assert_eq!(cells[0], "7");
        assert_eq!(cells[3], "100", "objects");
        assert_eq!(cells[4], "30", "houses");
        assert_eq!(cells.last().copied(), Some("3.250"));
    }
}
