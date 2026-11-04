//! App-owned network quality statistics for one online (relay-sequenced)
//! match: catch-up, ordered delivery, local cost and relay pacing.
//!
//! Wall-clock observations only; the deterministic simulation never reads
//! them. A world install resets them, the match's transport return closes
//! them (logged once as `Network quality:` and handed to the lobby's
//! `finish`), and the closed report stays for the external launch result
//! until the next install.

use crate::net::live_commands::{DeliveryCounters, NetworkRecoveryStatus};
use crate::util::time::{Duration, Instant};

use super::super::AppState;

/// Fixed-width millisecond histogram: 0.25 ms buckets up to 256 ms, then one
/// overflow bucket. A whole match fits in a few KiB.
#[derive(Debug, Default)]
struct MsHistogram {
    counts: Vec<u32>,
    samples: u64,
}

const BUCKETS_PER_MS: f32 = 4.0;
const HISTOGRAM_BUCKETS: usize = 1024;

impl MsHistogram {
    fn record(&mut self, ms: f32) {
        if self.counts.is_empty() {
            self.counts = vec![0; HISTOGRAM_BUCKETS + 1];
        }
        let bucket = ((ms.max(0.0) * BUCKETS_PER_MS) as usize).min(HISTOGRAM_BUCKETS);
        self.counts[bucket] = self.counts[bucket].saturating_add(1);
        self.samples += 1;
    }

    /// The upper edge of the bucket holding the `fraction` quantile, in ms.
    fn quantile(&self, fraction: f64) -> Option<f32> {
        if self.samples == 0 {
            return None;
        }
        let rank = ((self.samples as f64) * fraction).ceil().max(1.0) as u64;
        let mut seen = 0u64;
        for (bucket, &count) in self.counts.iter().enumerate() {
            seen += u64::from(count);
            if seen >= rank {
                return Some((bucket + 1) as f32 / BUCKETS_PER_MS);
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct NetworkQualityReport {
    /// Time the record watched the match in play; its catch-up, unfocused and
    /// slowed times are parts of it.
    pub(crate) played_seconds: f64,
    pub(crate) catch_up: CatchUpReport,
    pub(crate) network: DeliveryReport,
    pub(crate) performance: PerformanceReport,
    pub(crate) pacing: PacingReport,
    pub(crate) outcome: OutcomeReport,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct CatchUpReport {
    pub(crate) episodes: u32,
    pub(crate) seconds: f64,
    /// Frames the relay had sealed past the local simulation, at most.
    pub(crate) max_backlog_frames: u32,
    pub(crate) skipped_draws: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct DeliveryReport {
    pub(crate) gaps: u64,
    pub(crate) repair_pulls: u64,
    /// First transmission of a local batch to receipt of its sealed frame.
    pub(crate) command_round_trip_ms: Option<RoundTripReport>,
    /// The playout reserve the pacer learned from late receipts, at most.
    pub(crate) max_jitter_reserve_ms: f64,
    pub(crate) underruns: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct RoundTripReport {
    pub(crate) mean: u64,
    pub(crate) max: u64,
    pub(crate) samples: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct PerformanceReport {
    pub(crate) tick_ms_p50: Option<f32>,
    pub(crate) tick_ms_p99: Option<f32>,
    pub(crate) fps_p50: Option<f32>,
    /// The slowest twentieth of presented frames.
    pub(crate) fps_p5: Option<f32>,
    pub(crate) unfocused_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct PacingReport {
    pub(crate) slowdowns: u32,
    /// The House named by each slowdown marker, in order.
    pub(crate) slowed_by: Vec<i8>,
    pub(crate) slowed_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct OutcomeReport {
    pub(crate) desync_frame: Option<u32>,
    pub(crate) reconnects: u32,
}

/// Live values for the telemetry dashboard.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(feature = "telemetry-dashboard"), allow(dead_code))]
pub(crate) struct NetworkQualityLive {
    pub(crate) backlog_frames: u32,
    pub(crate) catching_up: bool,
    pub(crate) skipped_draws: u64,
    pub(crate) relay_slowed: bool,
}

// A slowdown list longer than this is still counted, not listed.
const MAX_SLOWED_BY: usize = 64;

#[derive(Debug, Default)]
pub(crate) struct NetworkQualityStats {
    active: bool,
    last_pass: Option<Instant>,
    deferrals_at_start: u64,
    live: NetworkQualityLive,
    catch_up_episodes: u32,
    catch_up: Duration,
    max_backlog_frames: u32,
    max_reserve_micros: u32,
    underruns: u64,
    played: Duration,
    unfocused: Duration,
    slowed: Duration,
    reconnecting: bool,
    reconnects: u32,
    slowdowns: u32,
    slowed_by: Vec<i8>,
    delivery: DeliveryCounters,
    tick_ms: MsHistogram,
    frame_ms: MsHistogram,
    last_present: Option<Instant>,
    closed: Option<NetworkQualityReport>,
}

impl NetworkQualityStats {
    /// A new world starts a new record; render deferrals are a process total.
    pub(crate) fn reset(&mut self, render_deferrals: u64) {
        *self = Self {
            deferrals_at_start: render_deferrals,
            ..Self::default()
        };
    }

    pub(crate) fn record_tick(&mut self, tick_ms: f32) {
        if self.active {
            self.tick_ms.record(tick_ms);
        }
    }

    /// One presented battlefield picture.
    pub(crate) fn record_present(&mut self, now: Instant) {
        if !self.active {
            return;
        }
        if let Some(last) = self.last_present.replace(now) {
            self.frame_ms
                .record(now.saturating_duration_since(last).as_secs_f32() * 1000.0);
        }
    }

    pub(crate) fn record_slowdown(&mut self, house: i8) {
        if !self.active {
            return;
        }
        self.slowdowns += 1;
        if self.slowed_by.len() < MAX_SLOWED_BY {
            self.slowed_by.push(house);
        }
    }

    #[cfg(feature = "telemetry-dashboard")]
    pub(crate) fn live(&self) -> Option<NetworkQualityLive> {
        self.active.then_some(self.live)
    }

    /// The closed report, or the running one while the match is still on.
    pub(crate) fn report(&self) -> Option<NetworkQualityReport> {
        self.closed
            .clone()
            .or_else(|| self.active.then(|| self.summarize()))
    }

    fn summarize(&self) -> NetworkQualityReport {
        let delivery = self.delivery;
        let fps = |ms: Option<f32>| ms.map(|ms| (1000.0 / ms * 10.0).round() / 10.0);
        let seconds = |span: Duration| (span.as_millis() as f64) / 1000.0;
        NetworkQualityReport {
            played_seconds: seconds(self.played),
            catch_up: CatchUpReport {
                episodes: self.catch_up_episodes,
                seconds: seconds(self.catch_up),
                max_backlog_frames: self.max_backlog_frames,
                skipped_draws: self.live.skipped_draws,
            },
            network: DeliveryReport {
                gaps: delivery.gaps,
                repair_pulls: delivery.repair_pulls,
                command_round_trip_ms: (delivery.round_trips > 0).then(|| RoundTripReport {
                    mean: delivery.round_trip_total_ms / delivery.round_trips,
                    max: delivery.round_trip_max_ms,
                    samples: delivery.round_trips,
                }),
                max_jitter_reserve_ms: f64::from(self.max_reserve_micros) / 1000.0,
                underruns: self.underruns,
            },
            performance: PerformanceReport {
                tick_ms_p50: self.tick_ms.quantile(0.5),
                tick_ms_p99: self.tick_ms.quantile(0.99),
                fps_p50: fps(self.frame_ms.quantile(0.5)),
                fps_p5: fps(self.frame_ms.quantile(0.95)),
                unfocused_seconds: seconds(self.unfocused),
            },
            pacing: PacingReport {
                slowdowns: self.slowdowns,
                slowed_by: self.slowed_by.clone(),
                slowed_seconds: seconds(self.slowed),
            },
            outcome: OutcomeReport {
                desync_frame: delivery.desync_frame,
                reconnects: self.reconnects,
            },
        }
    }
}

/// Sample one match service pass of an online match in progress.
pub(crate) fn observe_pass(state: &mut AppState) {
    let now = Instant::now();
    let in_game = state.frontend.screen == crate::app::GameScreen::InGame;
    let sample = state
        .match_state
        .live_network
        .as_ref()
        .filter(|live| in_game && live.uses_sequenced_lockstep())
        .zip(state.match_state.sim_runtime.as_ref())
        .map(|(live, runtime)| {
            let frame = runtime.simulation.session().binary_frame();
            let reconnecting = live.recovery_status(
                crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
                frame,
            ) == Some(NetworkRecoveryStatus::Reconnecting);
            (
                live.relay_head_frame().unwrap_or(0).saturating_sub(frame),
                live.relay_slowed(),
                live.delivery_counters().unwrap_or_default(),
                reconnecting,
            )
        });
    let pacing = state.platform.frame_pacer.online_pacing_status();
    let deferrals = state.platform.frame_pacer.total_render_deferrals();
    let focused = state.platform.window_active;
    let stats = &mut state.match_state.network_quality;
    let Some((backlog, slowed, delivery, reconnecting)) = sample else {
        stats.last_pass = None;
        return;
    };
    stats.active = true;
    let elapsed = stats
        .last_pass
        .replace(now)
        .map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
    stats.played += elapsed;
    let catching_up = pacing.is_some_and(|pacing| pacing.catching_up);
    if catching_up {
        stats.catch_up += elapsed;
        if !stats.live.catching_up {
            stats.catch_up_episodes += 1;
        }
    }
    if !focused {
        stats.unfocused += elapsed;
    }
    if slowed {
        stats.slowed += elapsed;
    }
    if reconnecting && !stats.reconnecting {
        stats.reconnects += 1;
    }
    stats.reconnecting = reconnecting;
    if let Some(pacing) = pacing {
        stats.max_reserve_micros = stats.max_reserve_micros.max(pacing.reserve_micros);
        stats.underruns = pacing.underruns;
    }
    stats.max_backlog_frames = stats.max_backlog_frames.max(backlog);
    stats.delivery = delivery;
    stats.live = NetworkQualityLive {
        backlog_frames: backlog,
        catching_up,
        skipped_draws: deferrals.saturating_sub(stats.deferrals_at_start),
        relay_slowed: slowed,
    };
}

/// Close the record as the online match returns its transport: the report it
/// closed, once, for the lobby's `finish`.
pub(crate) fn close(state: &mut AppState) -> Option<NetworkQualityReport> {
    let stats = &mut state.match_state.network_quality;
    if !stats.active || stats.closed.is_some() {
        return None;
    }
    let report = stats.summarize();
    stats.active = false;
    match serde_json::to_string(&report) {
        Ok(json) => log::info!("Network quality: {json}"),
        Err(error) => log::warn!("Network quality report: {error}"),
    }
    stats.closed = Some(report.clone());
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quantiles read the bucket edge at or above the rank: one slow frame
    /// in twenty already sets the 5th-percentile frame rate.
    #[test]
    fn the_slowest_twentieth_sets_the_fifth_percentile() {
        let mut frames = MsHistogram::default();
        for _ in 0..19 {
            frames.record(16.0);
        }
        frames.record(100.0);
        assert_eq!(frames.quantile(0.5), Some(16.25));
        assert_eq!(frames.quantile(0.95), Some(16.25));
        frames.record(100.0);
        assert_eq!(frames.quantile(0.95), Some(100.25));
        assert_eq!(MsHistogram::default().quantile(0.5), None);
    }

    /// The lobby reads `finish`'s report by these names
    /// (docs/lobby-protocol.md) and drops one it cannot read.
    #[test]
    fn the_report_keeps_the_names_the_lobby_reads() {
        let report = serde_json::to_value(NetworkQualityStats::default().summarize()).unwrap();
        assert_eq!(
            report,
            serde_json::json!({
                "played_seconds": 0.0,
                "catch_up": {"episodes": 0, "seconds": 0.0, "max_backlog_frames": 0, "skipped_draws": 0},
                "network": {"gaps": 0, "repair_pulls": 0, "command_round_trip_ms": null,
                    "max_jitter_reserve_ms": 0.0, "underruns": 0},
                "performance": {"tick_ms_p50": null, "tick_ms_p99": null, "fps_p50": null,
                    "fps_p5": null, "unfocused_seconds": 0.0},
                "pacing": {"slowdowns": 0, "slowed_by": [], "slowed_seconds": 0.0},
                "outcome": {"desync_frame": null, "reconnects": 0},
            })
        );
    }
}
