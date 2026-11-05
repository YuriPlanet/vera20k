//! Per-client counters and the JSON report (e2e-plan §1.3).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{ClientLinkStats, ReaderLinkStats};

/// IPv4 and UDP headers, counted into every datagram's bytes.
pub const IP_UDP_BYTES: usize = 28;

/// Datagrams one way, by the inspector's kind names.
#[derive(Debug, Default, Clone)]
pub struct Traffic {
    pub packets: u64,
    pub bytes: u64,
    pub kinds: BTreeMap<&'static str, (u64, u64)>,
}

impl Traffic {
    pub fn add(&mut self, kind: &'static str, wire_bytes: usize) {
        let bytes = (wire_bytes + IP_UDP_BYTES) as u64;
        self.packets += 1;
        self.bytes += bytes;
        let entry = self.kinds.entry(kind).or_default();
        entry.0 += 1;
        entry.1 += bytes;
    }
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct Direction {
    pub packets_per_s: f64,
    pub bytes_per_s: f64,
    /// Datagrams per second by kind.
    pub kinds: BTreeMap<String, f64>,
    /// Datagrams the network lost (rules, outages, random, queue), per second.
    pub net_lost_per_s: f64,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct Distribution {
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl Distribution {
    pub fn of(mut samples: Vec<f64>) -> Self {
        samples.sort_by(f64::total_cmp);
        let rank = |p: f64| {
            if samples.is_empty() {
                0.0
            } else {
                // Nearest rank.
                let index = ((p * samples.len() as f64).ceil() as usize).clamp(1, samples.len());
                samples[index - 1]
            }
        };
        Self {
            count: samples.len(),
            p50: rank(0.50),
            p95: rank(0.95),
            p99: rank(0.99),
            max: samples.last().copied().unwrap_or(0.0),
        }
    }
}

/// One client, or the mean client of a group.
#[derive(Debug, Serialize, Clone, Default)]
pub struct ClientReport {
    pub label: String,
    pub clients: usize,
    pub up: Direction,
    pub down: Direction,
    /// Command issued to its frame executed at the issuing client, in ms.
    pub command_latency_ms: Distribution,
    /// Time between frame-end advances, in ms.
    pub turn_interval_ms: Distribution,
    /// Advances more than two turns apart.
    pub hitches: f64,
    /// Gaps reported to the session (§10.1 `gaps`, whole run).
    pub gaps: f64,
    /// PULL requests sent in the window.
    pub pulls: f64,
    /// Of those, fallback PULLs from `poll_repair_pull` (spec §7.7).
    pub repair_pulls: f64,
    /// RTO expiries that resent batches (§10.1 `rto_timeouts`, whole run).
    pub resends: f64,
    /// UPLINKs per batch first sent in the window.
    pub submits_per_batch: f64,
    pub batches: f64,
    pub executed_frames: f64,
    /// Ordering errors received, by code.
    pub errors: BTreeMap<String, f64>,
    /// Datagrams the client session refused (duplicates, replays).
    pub refused: BTreeMap<String, f64>,
    /// Gap UPLINKs spec §6.3 row 2 would send on this stream: one per gap,
    /// at once when reordered, after `turn_ms` behind a STATUS. Counted the
    /// same way for both clients, so the baseline shows how often the
    /// adaptive transport would fire it.
    pub gap_uplink_triggers: f64,
    /// Second gap UPLINKs for gaps still open `max(16, 4 rttvar)` ms
    /// after the first (§6.7 item 1).
    pub gap_uplink_resends: f64,
    /// The adaptive client's §10.1 statistics at the end of the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkSummary>,
    /// The relay's §10.2 view of this reader at the end of the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader: Option<ReaderSummary>,
}

/// `ClientLinkStats` (spec §10.1) as the report prints it.
#[derive(Debug, Serialize, Clone, Default)]
pub struct LinkSummary {
    pub uplink: String,
    pub up_loss_permille: u16,
    /// Mean over folds (spec §10.1).
    pub up_loss_permille_mean: f64,
    pub up_loss_permille_max: u16,
    pub uplink_state_ms: [u64; 4],
    pub uplink_state_entries: [u64; 4],
    pub copies: u64,
    pub rto_timeouts: u64,
    pub rto_backoff_max: u32,
    pub datagrams_received: u64,
    pub gaps: u64,
    pub gaps_repaired_by_push: u64,
    pub repair_pulls: u64,
    pub srtt_p50_ms: Option<u32>,
    pub srtt_p99_ms: Option<u32>,
    pub base_ms: Option<u32>,
    pub delayed_ms: u64,
}

impl LinkSummary {
    pub fn of(stats: ClientLinkStats) -> Self {
        Self {
            uplink: format!("{:?}", stats.uplink),
            up_loss_permille: stats.up_loss_permille,
            up_loss_permille_mean: if stats.up_loss_folds == 0 {
                0.0
            } else {
                stats.up_loss_permille_sum as f64 / stats.up_loss_folds as f64
            },
            up_loss_permille_max: stats.up_loss_permille_max,
            uplink_state_ms: stats.uplink_state_ms,
            uplink_state_entries: stats.uplink_state_entries,
            copies: stats.copies,
            rto_timeouts: stats.rto_timeouts,
            rto_backoff_max: stats.rto_backoff_max,
            datagrams_received: stats.datagrams_received,
            gaps: stats.gaps,
            gaps_repaired_by_push: stats.gaps_repaired_by_push,
            repair_pulls: stats.repair_pulls,
            srtt_p50_ms: stats.srtt_p50_ms,
            srtt_p99_ms: stats.srtt_p99_ms,
            base_ms: stats.base_ms,
            delayed_ms: stats.delayed_ms,
        }
    }
}

/// `ReaderLinkStats` (spec §10.2) as the report prints it.
#[derive(Debug, Serialize, Clone, Default)]
pub struct ReaderSummary {
    pub downlink: String,
    pub down_loss_permille: u16,
    pub piggyback_depth: u8,
    /// Datagrams by send-log kind: non-PACK, new, repeat, retransmission,
    /// tail probe (spec §4.2).
    pub packets_by_kind: [u64; 5],
    pub bytes_by_kind: [u64; 5],
}

impl ReaderSummary {
    pub fn of(stats: ReaderLinkStats) -> Self {
        Self {
            downlink: format!("{:?}", stats.downlink),
            down_loss_permille: stats.down_loss_permille,
            piggyback_depth: stats.piggyback_depth,
            packets_by_kind: stats.packets_by_kind,
            bytes_by_kind: stats.bytes_by_kind,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Report {
    pub scenario: String,
    pub seed: u64,
    pub players: u8,
    pub tick_hz: u16,
    pub turn_ms: f64,
    pub measured_ms: u64,
    pub apm: f64,
    /// Every client read the same record stream (up to the shortest).
    pub stream_consistent: bool,
    /// DESYNC records in the stream (a `desync` fault makes one).
    pub desync_records: usize,
    /// Clients whose frames stopped advancing for the last two seconds.
    pub stalled_clients: usize,
    /// Datagrams the inspector could not read.
    pub malformed: u64,
    /// Datagrams the relay rejected, by error and kind.
    pub relay_rejects: BTreeMap<String, u64>,
    /// The whole room's relay traffic, counted at the network layer.
    pub relay: RelayTraffic,
    pub groups: Vec<ClientReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clients: Vec<ClientReport>,
}

/// The relay's datagrams and bytes a second (IP/UDP headers included):
/// `sent` as it sent them, before any loss; `received` as they reached it.
#[derive(Debug, Serialize, Clone, Default)]
pub struct RelayTraffic {
    pub sent_packets_per_s: f64,
    pub sent_bytes_per_s: f64,
    pub received_packets_per_s: f64,
    pub received_bytes_per_s: f64,
}

impl RelayTraffic {
    pub fn of(sent: &Traffic, received: &Traffic, seconds: f64) -> Self {
        Self {
            sent_packets_per_s: sent.packets as f64 / seconds,
            sent_bytes_per_s: sent.bytes as f64 / seconds,
            received_packets_per_s: received.packets as f64 / seconds,
            received_bytes_per_s: received.bytes as f64 / seconds,
        }
    }
}

/// The mean client of `members`, with latency and turn samples pooled.
pub fn mean(label: &str, members: &[(&ClientReport, Vec<f64>, Vec<f64>)]) -> ClientReport {
    let n = members.len().max(1) as f64;
    let avg = |get: &dyn Fn(&ClientReport) -> f64| {
        members.iter().map(|(report, ..)| get(report)).sum::<f64>() / n
    };
    let direction = |get: &dyn Fn(&ClientReport) -> &Direction| {
        let mut kinds = BTreeMap::new();
        for (report, ..) in members {
            for (kind, rate) in &get(report).kinds {
                *kinds.entry(kind.clone()).or_insert(0.0) += rate / n;
            }
        }
        Direction {
            packets_per_s: avg(&|report| get(report).packets_per_s),
            bytes_per_s: avg(&|report| get(report).bytes_per_s),
            kinds,
            net_lost_per_s: avg(&|report| get(report).net_lost_per_s),
        }
    };
    let sum_maps = |get: &dyn Fn(&ClientReport) -> &BTreeMap<String, f64>| {
        let mut map = BTreeMap::new();
        for (report, ..) in members {
            for (key, value) in get(report) {
                *map.entry(key.clone()).or_insert(0.0) += value / n;
            }
        }
        map
    };
    ClientReport {
        label: label.to_string(),
        clients: members.len(),
        up: direction(&|report| &report.up),
        down: direction(&|report| &report.down),
        command_latency_ms: Distribution::of(
            members.iter().flat_map(|(_, l, _)| l.clone()).collect(),
        ),
        turn_interval_ms: Distribution::of(
            members.iter().flat_map(|(_, _, t)| t.clone()).collect(),
        ),
        hitches: avg(&|report| report.hitches),
        gaps: avg(&|report| report.gaps),
        pulls: avg(&|report| report.pulls),
        repair_pulls: avg(&|report| report.repair_pulls),
        resends: avg(&|report| report.resends),
        submits_per_batch: avg(&|report| report.submits_per_batch),
        batches: avg(&|report| report.batches),
        executed_frames: avg(&|report| report.executed_frames),
        errors: sum_maps(&|report| &report.errors),
        refused: sum_maps(&|report| &report.refused),
        gap_uplink_triggers: avg(&|report| report.gap_uplink_triggers),
        gap_uplink_resends: avg(&|report| report.gap_uplink_resends),
        link: None,
        reader: None,
    }
}
