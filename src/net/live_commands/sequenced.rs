//! VERA online ordering adaptation. The authenticated relay seals an immutable
//! timeline; no client infers a missing participant's input or rewrites a tick.
//! Native Event/OutList/DoList and the ordinary simulation transaction retain
//! gameplay authority. Fresh packet caches make each batch standalone.

use std::collections::{BTreeMap, VecDeque};

use vera20k_relay_protocol::{
    CHECKSUM_INTERVAL, ClientLinkStats, ORDERING_BATCH_CAPACITY, OrderEntry, OrderError,
    OrderEvent, OrderPace, OrderingCertificate, paced_rates,
};

use super::{LiveCommandError, StartupAgreement, StartupError};
use crate::net::event_packet::{EVENT_PACKET_HEADER_LEN, EventPacketCodec};
use crate::net::udp::LanUdpTransport;
use crate::sim::command::{CommandRecord, ExitRecord, REMOVE_PLAYER_OPCODE};
use crate::sim::command_queue::FrameInfo;
use crate::sim::world::{NetworkCommandQueues, NetworkQueueContext, NetworkQueuePass};
use crate::util::time::Instant;

const BUFFERED_FRAMES: usize = 64;
const REORDER_WINDOW: u64 = 256;
const MAX_FRAME_RECORDS: usize = 4096;
const SAMPLE_HISTORY: usize = 4096;
// READY_REQUEST resends until the start (spec §6.0).
const CONFIGURE_INTERVAL_MS: i32 = 125;
// A restoring process reads history by PULL every 100 ms (spec §7.7).
const REPLAY_RETRY_INTERVAL_MS: i32 = 100;
const PAUSE_PROBE_INTERVAL_MS: i32 = 500;
const PAUSE_COMPATIBILITY_PROBE_INTERVAL_MS: i32 = 5_000;
const COMMAND_TRACE_HISTORY: usize = 256;
// A relay record this build cannot read ends the match once it has held the
// cursor, or before the start every Status, this long while the relay keeps
// repeating it. Silence alone never ends a match.
const UNREADABLE_LIMIT_MS: u32 = 5_000;
const UNREADABLE_REPEAT_MS: u32 = 1_000;
const PACE_REPORT_INTERVAL_MS: i32 = 1_000;
// Frames must wait here this long before this PC counts as lagging.
const PACE_BACKLOG_MS: u32 = 2_000;
// Ticks kept for one pace report's tick-cost percentile.
const MAX_TICK_COSTS: usize = 256;

/// App diagnostics for one match's ordered delivery, counted by the session
/// that owns it. Wall-clock observations; never simulation or delivery input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryCounters {
    /// The relay session's link measurements as last read (spec §10.1),
    /// including the stream gaps it was told of and its fallback Pulls.
    pub link: Option<ClientLinkStats>,
    /// Local command batches whose sealed frame came back, with the
    /// first-transmission to frame-receipt time of each.
    pub round_trips: u64,
    pub round_trip_total_ms: u64,
    pub round_trip_max_ms: u64,
    /// The sampled frame whose checksums differed, from the relay's DESYNC.
    pub desync_frame: Option<u32>,
}

/// App diagnostics for one local command batch: its first transmission, the
/// relay's frame assignment and the contiguous receipt of that sealed frame.
/// Socket receipt times; this never enters simulation or delivery decisions.
#[derive(Debug, Clone, Copy)]
pub struct CommandDeliveryTrace {
    pub batch_id: u64,
    pub sent_at: Option<Instant>,
    pub assigned: Option<(u32, Instant)>,
    pub frame_received_at: Option<Instant>,
}

/// The socket's latest contiguous-frame receipt, independent of when the app
/// drains it. Presentation can pace these frames; only the sequencer admits
/// canonical commands. This clock never enters deterministic simulation.
#[derive(Debug, Clone, Copy)]
pub struct SequencedTiming {
    pub frame: u32,
    pub received_at: Instant,
    pub frames_per_second: u16,
    /// Changes at a synchronized pause/resume boundary. Presentation must
    /// not turn time spent paused into simulation catch-up or receipt jitter.
    pub clock_epoch: u64,
}

/// Authenticated relay capability and the latest contiguous pause marker.
/// A client still finishes `frame` before it is stopped; this state never
/// pauses a local simulation ahead of another participant's canonical input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MultiplayerPauseStatus {
    pub available: bool,
    pub paused: bool,
    pub frame: u32,
    pub epoch: u64,
    pub pending: bool,
}

/// A relay rate change, released when its first frame dispatches. The relay
/// slows a match whose player cannot keep up (relay README, Pace).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSpeedChange {
    /// This House's PC could not keep up, so the match runs slower.
    SlowedBy(i8),
    /// The match runs at its own speed again.
    Normal,
}

/// This PC's progress against the relay's clock, reported about once a
/// second. Lagging means frames waited here for `PACE_BACKLOG_MS`; behind
/// also means two report windows in a row with frames waiting ran fewer
/// than the clock sealed, but some: a world that ran none was held by
/// something other than speed.
#[derive(Debug, Default)]
struct PaceMeter {
    backlog_since: Option<u32>,
    last_report: Option<u32>,
    dispatched: u32,
    slow_windows: u8,
    // Simulation tick costs, in microseconds, since the last report.
    tick_costs: Vec<u32>,
    // A minimised or covered window may be throttled by its system.
    hidden: bool,
}

impl PaceMeter {
    /// One simulation tick's cost, for the next report's 90th percentile.
    fn record_tick_cost(&mut self, micros: u32) {
        // A report window holds about a second of ticks; a stalled report
        // keeps the latest.
        if self.tick_costs.len() == MAX_TICK_COSTS {
            self.tick_costs.remove(0);
        }
        self.tick_costs.push(micros);
    }

    fn tick_cost_p90(&mut self) -> Option<u32> {
        if self.tick_costs.is_empty() {
            return None;
        }
        let rank = (self.tick_costs.len() * 9).div_ceil(10) - 1;
        let (_, p90, _) = self.tick_costs.select_nth_unstable(rank);
        let p90 = *p90;
        self.tick_costs.clear();
        Some(p90.max(1))
    }

    /// Samples the frames waiting here and returns the report due now, with
    /// the frames run per second since the last report.
    fn report(
        &mut self,
        now: u32,
        buffered: usize,
        pace_fps: u16,
        measured: bool,
    ) -> Option<(OrderPace, u16, Option<u32>)> {
        if measured && !self.hidden && buffered >= usize::from(pace_fps / 8).max(4) {
            self.backlog_since.get_or_insert(now);
        } else {
            self.backlog_since = None;
        }
        if !service_due(self.last_report, now, PACE_REPORT_INTERVAL_MS) {
            return None;
        }
        let ran = self.last_report.map_or(pace_fps, |last| {
            (u64::from(self.dispatched) * 1000 / u64::from(now.wrapping_sub(last).max(1)))
                .min(u64::from(u16::MAX)) as u16
        });
        self.slow_windows = if ran > 0 && ran < pace_fps && self.backlog_since.is_some() {
            self.slow_windows.saturating_add(1)
        } else {
            0
        };
        self.last_report = Some(now);
        self.dispatched = 0;
        let lagging = self
            .backlog_since
            .is_some_and(|since| now.wrapping_sub(since) >= PACE_BACKLOG_MS);
        Some((
            match (lagging, self.slow_windows >= 2) {
                (false, _) => OrderPace::KeepingUp,
                (true, true) => OrderPace::Behind,
                (true, false) => OrderPace::CatchingUp,
            },
            ran,
            self.tick_cost_p90(),
        ))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CanonicalPause {
    paused: bool,
    frame: u32,
    epoch: u64,
}

#[derive(Debug)]
struct PauseRequest {
    id: u64,
    expected_epoch: u64,
    paused: bool,
    last_sent: Option<u32>,
    acknowledged_epoch: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct Unreadable {
    since: u32,
    last: u32,
    next_sequence: u64,
    statuses: u64,
}

#[derive(Debug)]
struct ReceivedEntry {
    entry: OrderEntry,
    received_at: Instant,
}

fn trace_mut(
    traces: &mut VecDeque<CommandDeliveryTrace>,
    batch_id: u64,
) -> Option<&mut CommandDeliveryTrace> {
    traces
        .iter_mut()
        .rev()
        .find(|trace| trace.batch_id == batch_id)
}

/// How long a running match's ordered stream may stand still before the
/// client shows Reconnecting (and Quit leaves without the stream).
const STALL_MS: u32 = 5_000;

fn service_due(last: Option<u32>, now: u32, interval: i32) -> bool {
    last.is_none_or(|last| super::probe_due(last, now, interval))
}

#[derive(Debug, thiserror::Error)]
pub enum SequencedError {
    #[error("online command history expired; reconnect to a new match")]
    HistoryExpired,
    #[error("online command timeline is invalid")]
    InvalidTimeline,
    /// Worded like the LAN path's `ChecksumMismatch`, so either reads the
    /// same in a log: the House whose report differs and both checksums.
    #[error(
        "online simulation differs for house {house} at frame {frame}: \
         local {local:#010x}, remote {remote:#010x}"
    )]
    Desync {
        frame: u32,
        house: i8,
        local: u32,
        remote: u32,
    },
    #[error("online ordering service cannot recover this match")]
    Unavailable,
    #[error("online command queue is full")]
    QueueFull,
    #[error("this online match needs a newer VERA20k")]
    NewerClientRequired,
}

#[derive(Debug)]
struct Submission {
    id: u64,
    records: Vec<CommandRecord>,
    bytes: Vec<u8>,
    // Handed to the relay session, which schedules its UPLINKs (spec §6).
    sealed: bool,
}

/// The relay's DESYNC record: the sampled frame and the outlier's and the
/// other side's source and checksum.
#[derive(Debug, Clone, Copy)]
struct RelayDesync {
    sample_frame: u32,
    outlier: (u8, u32),
    other: (u8, u32),
}

#[derive(Debug, Default)]
struct CommittedFrame {
    frame: u32,
    records: Vec<CommandRecord>,
    // Remote Houses whose connection the relay ended in this frame.
    lost: Vec<i8>,
    // A rate marker after the previous frame.
    speed: Option<MatchSpeedChange>,
    // A DESYNC record after the previous frame ends the match here.
    desync: Option<RelayDesync>,
}

#[derive(Debug)]
pub(super) struct SequencedSession {
    houses: [Option<i8>; 30],
    local_source: u8,
    agreement: [u8; 32],
    /// The certificate's rate, which the simulation keeps.
    fps: u16,
    /// The relay's current sealing rate, which paces playout.
    pub(super) pace_fps: u16,
    pace_meter: PaceMeter,
    started: bool,
    head_frame: u32,
    error: Option<LiveCommandError>,
    last_response: Option<u32>,
    /// When the ordered stream last moved (its head, next sequence or our
    /// cursor). A relay can keep answering while its stream stands still:
    /// federated relays whose link failed.
    last_progress: Option<(u32, (u32, u64, u64))>,
    first_service: Option<u32>,
    statuses: u64,
    unreadable: Option<Unreadable>,
    last_control: Option<u32>,
    remote_next_sequence: u64,
    delivery: DeliveryCounters,
    pause: CanonicalPause,
    /// VERA: the current pause found this game catching up, and it has not
    /// reached the pause yet; see `take_caught_up_in_pause`.
    pause_found_behind: bool,
    pause_relay_supported: bool,
    pause_supported_sources: u32,
    pause_active_sources: u32,
    pause_probe_attempts: u8,
    last_pause_control: Option<u32>,
    pause_request: Option<PauseRequest>,
    next_pause_request: u64,
    keep_running_for_exit: bool,
    next_sequence: u64,
    entries: BTreeMap<u64, ReceivedEntry>,
    contiguous_received_at: Option<Instant>,
    latest_frame_receipt: Option<SequencedTiming>,
    assembling: CommittedFrame,
    frames: VecDeque<CommittedFrame>,
    last_batch: [u64; 30],
    // Sources whose batch could not be read; each departed at that batch's
    // frame and the rest of its stream is ignored.
    refused: u32,
    next_batch: u64,
    // This process replaces one that left a match under way. Its batch counter
    // lies in the history it is reading, so it submits nothing until caught up.
    restoring: bool,
    last_restore_log: Option<u32>,
    accepted_batch: u64,
    submissions: VecDeque<Submission>,
    sample_sent: Option<u32>,
    // Derived checksum samples from immutable post-Logic contexts, evicted in
    // frame order: the reports to the relay, and this PC's side of a desync.
    samples: VecDeque<(u32, u32)>,
    current: Option<(NetworkQueueContext, bool)>,
    sent_commands: u64,
    command_traces: VecDeque<CommandDeliveryTrace>,
    // Released when their frame dispatches; the app posts each line.
    lost: Vec<i8>,
    speed_changes: Vec<MatchSpeedChange>,
}

impl SequencedSession {
    pub(super) fn new(
        agreement: &StartupAgreement,
        transport: &LanUdpTransport,
        local_house: i8,
    ) -> Result<Self, LiveCommandError> {
        let cert = OrderingCertificate::parse(
            &transport
                .ordering_certificate()
                .ok_or(StartupError::InvalidAdmission)?,
        )
        .map_err(|_| StartupError::InvalidAdmission)?;
        let local_source = transport
            .relay_source()
            .ok_or(StartupError::InvalidAdmission)?;
        let mut houses = [None; 30];
        houses[usize::from(local_source)] = Some(local_house);
        for (house, endpoint) in transport.connections().peers() {
            let source = vera20k_relay_protocol::endpoint_peer(endpoint)
                .ok_or(StartupError::InvalidAdmission)?;
            houses[usize::from(source)] =
                Some(i8::try_from(house).map_err(|_| StartupError::InvalidAdmission)?);
        }
        let mask = roster_mask(&houses);
        let (identity, fps, count) = agreement.sequencing_identity()?;
        if mask != cert.sources_mask || fps != cert.tick_hz || count != mask.count_ones() as usize {
            return Err(StartupError::InvalidAdmission.into());
        }
        Ok(Self::with_roster(
            houses,
            local_source,
            identity,
            cert.tick_hz,
        ))
    }

    fn with_roster(
        houses: [Option<i8>; 30],
        local_source: u8,
        agreement: [u8; 32],
        fps: u16,
    ) -> Self {
        let mask = roster_mask(&houses);
        Self {
            houses,
            local_source,
            agreement,
            fps,
            pace_fps: fps,
            pace_meter: PaceMeter::default(),
            started: false,
            head_frame: 0,
            error: None,
            last_response: None,
            last_progress: None,
            first_service: None,
            statuses: 0,
            unreadable: None,
            last_control: None,
            remote_next_sequence: 1,
            delivery: DeliveryCounters::default(),
            pause: CanonicalPause::default(),
            pause_found_behind: false,
            pause_relay_supported: false,
            pause_supported_sources: 0,
            pause_active_sources: mask,
            pause_probe_attempts: 0,
            last_pause_control: None,
            pause_request: None,
            next_pause_request: 1,
            keep_running_for_exit: false,
            next_sequence: 1,
            entries: BTreeMap::new(),
            contiguous_received_at: None,
            latest_frame_receipt: None,
            assembling: CommittedFrame {
                frame: 1,
                ..Default::default()
            },
            frames: VecDeque::new(),
            last_batch: [0; 30],
            refused: 0,
            next_batch: 1,
            restoring: false,
            last_restore_log: None,
            accepted_batch: 0,
            submissions: VecDeque::new(),
            sample_sent: None,
            samples: VecDeque::new(),
            current: None,
            sent_commands: 0,
            command_traces: VecDeque::new(),
            lost: Vec::new(),
            speed_changes: Vec::new(),
        }
    }

    pub(super) fn begin_restore(&mut self) {
        self.restoring = true;
    }

    pub(super) fn restoring(&self) -> bool {
        self.restoring
    }

    /// Ends a restore once the world at `frame` has reached the shared one.
    /// A status can only postdate the last batch the departed process sent,
    /// so history read past that status holds this source's final batch ID.
    fn settle_restore(&mut self, frame: u32) -> Result<(), SequencedError> {
        if self.statuses > 0
            && self.next_sequence >= self.remote_next_sequence
            && self.head_frame.saturating_sub(frame) <= u32::from(self.pace_fps) / 2
        {
            let last = self.last_batch[usize::from(self.local_source)];
            self.next_batch = last.checked_add(1).ok_or(SequencedError::InvalidTimeline)?;
            self.accepted_batch = last;
            self.restoring = false;
            log::info!(
                "Rejoined the match at frame {frame}; next batch {}",
                self.next_batch
            );
        }
        Ok(())
    }

    /// Remote Houses whose connection the relay ended in dispatched frames.
    pub(super) fn take_lost(&mut self) -> Vec<i8> {
        std::mem::take(&mut self.lost)
    }

    /// The relay's rate changes in dispatched frames.
    pub(super) fn take_speed_changes(&mut self) -> Vec<MatchSpeedChange> {
        std::mem::take(&mut self.speed_changes)
    }

    pub(super) fn set_presentation_hidden(&mut self, hidden: bool) {
        self.pace_meter.hidden = hidden;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn mark_started_for_test(&mut self) {
        self.started = true;
    }

    pub(super) fn frontier(&self) -> u32 {
        self.frames.back().map_or_else(
            || {
                self.current
                    .filter(|(_, done)| *done)
                    .map_or(0, |(c, _)| c.frame)
            },
            |f| f.frame,
        )
    }

    pub(super) fn started(&self) -> bool {
        self.started
    }
    pub(super) fn failed(&self) -> bool {
        self.error.is_some()
    }
    pub(super) fn record_tick_cost(&mut self, micros: u32) {
        self.pace_meter.record_tick_cost(micros);
    }

    pub(super) fn sent_commands(&self) -> u64 {
        self.sent_commands
    }

    pub(super) fn command_traces(&self) -> &VecDeque<CommandDeliveryTrace> {
        &self.command_traces
    }

    pub(super) fn delivery_counters(&self) -> DeliveryCounters {
        self.delivery
    }

    /// The relay's sealed head as this PC last learned it.
    pub(super) fn head_frame(&self) -> u32 {
        self.head_frame
    }

    pub(super) fn slowed(&self) -> bool {
        self.pace_fps < self.fps
    }

    fn trace_assignment(&mut self, batch_id: u64, frame: u32, received_at: Instant) {
        if let Some(trace) = trace_mut(&mut self.command_traces, batch_id) {
            trace.assigned.get_or_insert((frame, received_at));
        }
    }

    pub(super) fn timing(&self) -> Option<SequencedTiming> {
        self.latest_frame_receipt
    }

    pub(super) fn pause_status(&self) -> MultiplayerPauseStatus {
        MultiplayerPauseStatus {
            available: self.started
                && !self.failed()
                && self.pause_relay_supported
                && self.pause_active_sources != 0
                && self.pause_supported_sources & self.pause_active_sources
                    == self.pause_active_sources,
            pending: self.pause_request.is_some(),
            paused: self.pause.paused,
            frame: self.pause.frame,
            epoch: self.pause.epoch,
        }
    }

    pub(super) fn request_pause(&mut self, paused: bool) -> bool {
        if !self.pause_status().available
            || self.pause_request.is_some()
            || self.pause.paused == paused
        {
            return false;
        }
        let Some(next) = self.next_pause_request.checked_add(1) else {
            return false;
        };
        self.pause_request = Some(PauseRequest {
            id: self.next_pause_request,
            expected_epoch: self.pause.epoch,
            paused,
            last_sent: None,
            acknowledged_epoch: None,
        });
        self.next_pause_request = next;
        true
    }

    pub(super) fn keep_running_for_exit(&mut self) {
        self.keep_running_for_exit = true;
    }

    pub(super) fn exit_requested(&self) -> bool {
        self.keep_running_for_exit
    }

    pub(super) fn service(&mut self, transport: &mut LanUdpTransport, now: u32) {
        self.first_service.get_or_insert(now);
        if self.restoring && service_due(self.last_restore_log, now, 5000) {
            self.last_restore_log = Some(now);
            log::info!(
                "Rejoin: frame {:?}, frontier {}, head {}, next sequence {} of {}, buffered {}, started {}, statuses {}",
                self.current.map(|(context, done)| (context.frame, done)),
                self.frontier(),
                self.head_frame,
                self.next_sequence,
                self.remote_next_sequence,
                self.frames.len(),
                self.started,
                self.statuses,
            );
        }
        while let Some(received) = transport.pop_ordering_received_with_time() {
            self.last_response = Some(now);
            if matches!(received.event, OrderEvent::Unrecognized) {
                self.note_unreadable(now);
                continue;
            }
            match self.receive(received.event, received.received_at) {
                Ok(()) => {}
                Err(error) => {
                    self.error.get_or_insert(error.into());
                }
            }
        }
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.assemble() {
            self.error = Some(error.into());
            return;
        }
        if let Err(error) = self.check_unreadable(now) {
            self.error = Some(error.into());
            return;
        }
        let position = (
            self.head_frame,
            self.remote_next_sequence,
            self.next_sequence,
        );
        if self.last_progress.is_none_or(|(_, last)| last != position) {
            self.last_progress = Some((now, position));
        }
        self.service_delivery(transport, now);
        self.service_pause(transport, now);
        self.service_pace(transport, now);
        if let Err(error) = self.seal_submissions(transport) {
            self.error = Some(error.into());
            return;
        }
        // The relay session decides every UPLINK (spec §6.3); this layer only
        // hands over its stream position.
        let gap = self.ordering_gap();
        transport.report_ordering_position(
            self.next_sequence,
            gap,
            vera20k_relay_protocol::turn_ms(self.pace_fps),
        );
        if self.started {
            let _ = transport.poll_uplink();
        }
        // Network silence is recoverable until the relay departs this source
        // after ten minutes. Keep the same stream cursor,
        // unacknowledged bytes and world until the route/lease returns. Neither
        // a timeout nor peer progress may manufacture an EXIT or fail a frame.
    }

    fn service_pause(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if !self.started {
            return;
        }
        // A confirmed UI exit remains in the native command queue. Keep its
        // timeline running until teardown, including when a pending pause or
        // another participant's pause wins before that EXIT is dispatched.
        if self.keep_running_for_exit && self.pause.paused && self.pause_request.is_none() {
            self.request_pause(false);
        }
        if let Some(request) = &mut self.pause_request {
            if service_due(request.last_sent, now, 100)
                && transport
                    .control_ordering_pause(request.id, request.expected_epoch, request.paused)
                    .is_ok()
            {
                request.last_sent = Some(now);
            }
        } else if !self.pause_status().available
            && service_due(self.last_pause_control, now, self.pause_probe_interval())
            && transport.control_ordering_pause(0, 0, false).is_ok()
        {
            self.last_pause_control = Some(now);
            self.pause_probe_attempts = self.pause_probe_attempts.saturating_add(1);
        }
    }

    fn service_pace(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if !self.started {
            return;
        }
        let measured = !self.restoring && !self.pause.paused && !self.keep_running_for_exit;
        if let Some((pace, ran, tick_micros)) =
            self.pace_meter
                .report(now, self.frames.len(), self.pace_fps, measured)
        {
            // It rides the next UPLINK once; a lost one only goes stale.
            transport.queue_ordering_pace(pace, ran, tick_micros);
        }
    }

    fn pause_probe_interval(&self) -> i32 {
        if !self.pause_relay_supported && self.pause_probe_attempts >= 3 {
            // Older relays ignore this extension. Keep overhead low without
            // mistaking a lost capability reply for permanent incompatibility.
            PAUSE_COMPATIBILITY_PROBE_INTERVAL_MS
        } else {
            PAUSE_PROBE_INTERVAL_MS
        }
    }

    fn finish_pause_request(&mut self) {
        if self.pause_request.as_ref().is_some_and(|request| {
            request
                .acknowledged_epoch
                .is_some_and(|epoch| epoch <= self.pause.epoch)
        }) {
            self.pause_request = None;
        }
    }

    /// Seals each local batch with the relay session in order; it retries
    /// them until acknowledged (spec §6.1-§6.3). An unready route seals on a
    /// later service.
    fn seal_submissions(&mut self, transport: &mut LanUdpTransport) -> Result<(), SequencedError> {
        for pending in self
            .submissions
            .iter_mut()
            .filter(|pending| !pending.sealed)
        {
            let Ok(id) = transport.seal_ordering_batch(&pending.bytes) else {
                break;
            };
            if id != pending.id {
                return Err(SequencedError::InvalidTimeline);
            }
            pending.sealed = true;
            if let Some(trace) = trace_mut(&mut self.command_traces, pending.id) {
                trace.sent_at.get_or_insert_with(Instant::now);
            }
        }
        Ok(())
    }

    /// The gap the uplink scheduler acts on (spec §6.3 row 2), while the
    /// frame buffer has room for what is missing.
    fn ordering_gap(&self) -> Option<vera20k_relay_protocol::OrderingGap> {
        use vera20k_relay_protocol::OrderingGap;
        if self.frames.len() >= BUFFERED_FRAMES {
            return None;
        }
        if self
            .entries
            .first_key_value()
            .is_some_and(|(seq, _)| *seq > self.next_sequence)
        {
            Some(OrderingGap::Reordered)
        } else if self.remote_next_sequence > self.next_sequence {
            Some(OrderingGap::BehindStatus)
        } else {
            None
        }
    }

    fn service_delivery(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if !self.started {
            // Only READY_REQUEST until the start: its STATUS replies carry
            // the start, and there are no records to pull (spec §6.0).
            if service_due(self.last_control, now, CONFIGURE_INTERVAL_MS)
                && transport.configure_ordering(self.agreement).is_ok()
            {
                // Before the first request that can bring back an entry.
                transport.set_ordering_restore(self.restoring);
                self.last_control = Some(now);
            }
            return;
        }
        if self.restoring {
            // RESTORING: the relay pushes no new records; read them by PULL.
            if self.cursor_gap()
                && service_due(self.last_control, now, REPLAY_RETRY_INTERVAL_MS)
                && transport.pull_ordering(self.next_sequence).is_ok()
            {
                self.last_control = Some(now);
            }
            return;
        }
        // Gaps go to the relay first by UPLINK; the session pulls only
        // after its grace (spec §7.7).
        let _ = transport.poll_repair_pull();
        if let Some(stats) = transport.relay_link_stats() {
            self.delivery.link = Some(stats);
        }
    }

    fn note_unreadable(&mut self, now: u32) {
        match &mut self.unreadable {
            // Only a relay that keeps repeating the record holds the match;
            // after a pause the five seconds start again.
            Some(unreadable) if now.wrapping_sub(unreadable.last) <= UNREADABLE_REPEAT_MS => {
                unreadable.last = now
            }
            _ => {
                self.unreadable = Some(Unreadable {
                    since: now,
                    last: now,
                    next_sequence: self.next_sequence,
                    statuses: self.statuses,
                })
            }
        }
    }

    /// Released clients cannot be changed: a newer relay's record or Status
    /// this build cannot read would otherwise stall it forever. Unreadable
    /// replies that hold nothing back are ignored.
    fn check_unreadable(&mut self, now: u32) -> Result<(), SequencedError> {
        let Some(unreadable) = self.unreadable else {
            return Ok(());
        };
        let cursor_held = self.next_sequence == unreadable.next_sequence && self.cursor_gap();
        let status_held = !self.started && self.statuses == unreadable.statuses;
        if !cursor_held && !status_held {
            self.unreadable = None;
        } else if now.wrapping_sub(unreadable.since) >= UNREADABLE_LIMIT_MS
            && now.wrapping_sub(unreadable.last) <= UNREADABLE_REPEAT_MS
        {
            return Err(SequencedError::NewerClientRequired);
        }
        Ok(())
    }

    /// The relay holds entries past the contiguous cursor, and the frame
    /// buffer has room for them.
    fn cursor_gap(&self) -> bool {
        self.frames.len() < BUFFERED_FRAMES
            && (self.remote_next_sequence > self.next_sequence
                || self
                    .entries
                    .first_key_value()
                    .is_some_and(|(seq, _)| *seq > self.next_sequence))
    }

    fn accept_through(&mut self, id: u64) -> Result<(), SequencedError> {
        if id >= self.next_batch {
            return Err(SequencedError::InvalidTimeline);
        }
        self.accepted_batch = self.accepted_batch.max(id);
        Ok(())
    }

    fn receive(&mut self, event: OrderEvent, received_at: Instant) -> Result<(), SequencedError> {
        match event {
            OrderEvent::Left => return Err(SequencedError::InvalidTimeline),
            OrderEvent::Ready { .. } => {}
            // service times these while they hold the cursor or start.
            OrderEvent::Unrecognized => {}
            OrderEvent::Status(status) => {
                self.statuses += 1;
                if status.tick_hz != self.fps {
                    return Err(SequencedError::InvalidTimeline);
                }
                if status.oldest_seq > self.next_sequence {
                    return Err(SequencedError::HistoryExpired);
                }
                if status.started && !self.started {
                    // Frame 1 is sealed a frame period after the start, so
                    // the stall clock starts now, not when loading did.
                    self.last_progress = None;
                }
                self.started |= status.started;
                self.head_frame = self.head_frame.max(status.head_frame);
                self.remote_next_sequence = self.remote_next_sequence.max(status.next_seq);
                self.pause_active_sources &= status.active_sources;
            }
            OrderEvent::PauseStatus {
                supported_sources,
                active_sources,
                request_id,
                epoch,
                ..
            } => {
                let sources = roster_mask(&self.houses);
                if (supported_sources | active_sources) & !sources != 0
                    || request_id >= self.next_pause_request
                {
                    return Err(SequencedError::InvalidTimeline);
                }
                self.pause_relay_supported = true;
                // Capabilities only accumulate and sources only depart. Old
                // replies cannot undo a newer availability observation.
                self.pause_supported_sources |= supported_sources;
                self.pause_active_sources &= active_sources;
                if let Some(request) = &mut self.pause_request
                    && request_id >= request.id
                {
                    request.acknowledged_epoch = Some(
                        request
                            .acknowledged_epoch
                            .map_or(epoch, |old| old.max(epoch)),
                    );
                }
                self.finish_pause_request();
            }
            OrderEvent::Ack {
                batch_id, frame, ..
            } => {
                if batch_id == 0 {
                    return Err(SequencedError::InvalidTimeline);
                }
                self.accept_through(batch_id)?;
                self.trace_assignment(batch_id, frame, received_at);
            }
            OrderEvent::Entry(entry) => {
                let sequence = match &entry {
                    OrderEntry::Batch {
                        sequence,
                        source,
                        batch_id,
                        frame,
                        ..
                    } => {
                        // A returning process's own source in the stream is
                        // the process it replaces.
                        if *source == self.local_source && !self.restoring {
                            self.accept_through(*batch_id)?;
                            self.trace_assignment(*batch_id, *frame, received_at);
                        }
                        *sequence
                    }
                    OrderEntry::FrameEnd { sequence, .. }
                    | OrderEntry::Departure { sequence, .. }
                    | OrderEntry::Pause { sequence, .. }
                    | OrderEntry::Rate { sequence, .. }
                    | OrderEntry::Desync { sequence, .. } => *sequence,
                };
                if sequence >= self.next_sequence && sequence - self.next_sequence < REORDER_WINDOW
                {
                    self.entries
                        .entry(sequence)
                        .or_insert(ReceivedEntry { entry, received_at });
                }
            }
            OrderEvent::Error { code, expected } => match code {
                OrderError::NotConfigured => {
                    self.started = false;
                    self.last_control = None;
                }
                OrderError::NotStarted | OrderError::Backpressure => {}
                OrderError::BatchGap => {
                    let accepted = expected
                        .checked_sub(1)
                        .ok_or(SequencedError::InvalidTimeline)?;
                    // The relay's authenticated prefix receipt never moves
                    // backwards. It resets no batch: the session's RTO
                    // resends them (spec §6.4).
                    self.accept_through(accepted)?;
                }
                OrderError::HistoryGap => return Err(SequencedError::HistoryExpired),
                OrderError::Inactive | OrderError::StorageUnavailable => {
                    return Err(SequencedError::Unavailable);
                }
                _ => return Err(SequencedError::InvalidTimeline),
            },
        }
        Ok(())
    }

    pub(super) fn recovery_status(
        &self,
        now: u32,
        current_frame: u32,
    ) -> Option<super::NetworkRecoveryStatus> {
        let last = self.last_response.or(self.first_service)?;
        if now.wrapping_sub(last) > 3_000 {
            return Some(super::NetworkRecoveryStatus::Reconnecting);
        }
        // Every running frame ends with a FrameEnd entry, so a started,
        // unpaused match whose stream has stood still while this client
        // waits for it is stalled even though the relay answers.
        if self.started
            && !self.pause.paused
            && current_frame >= self.head_frame
            && self
                .last_progress
                .is_some_and(|(at, _)| now.wrapping_sub(at) > STALL_MS)
        {
            return Some(super::NetworkRecoveryStatus::Reconnecting);
        }
        self.catching_up_frames(current_frame)
            .map(|frames| super::NetworkRecoveryStatus::CatchingUp { frames })
    }

    /// How far this game is behind the relay's head, when that is more than
    /// half a second of frames: the Catching up status line.
    fn catching_up_frames(&self, current_frame: u32) -> Option<u32> {
        let frames = self.head_frame.saturating_sub(current_frame);
        (frames > u32::from(self.pace_fps) / 2).then_some(frames)
    }

    /// VERA: true once, when this game reaches a pause that found it
    /// catching up, so it can tell the others it has caught up. `reached` is
    /// whether the simulation has reached the pause; a resume first sends
    /// nothing, nor does a pause the relay has already ended, which a
    /// rejoining game replays while still behind. gamemd's lockstep makes
    /// everyone wait for a slow player, so it has no such line.
    pub(super) fn take_caught_up_in_pause(&mut self, reached: bool, current_frame: u32) -> bool {
        if !self.pause.paused {
            self.pause_found_behind = false;
        } else if reached {
            return std::mem::take(&mut self.pause_found_behind)
                && self.catching_up_frames(current_frame).is_none();
        } else if self.catching_up_frames(current_frame).is_some() {
            self.pause_found_behind = true;
        }
        false
    }

    fn assemble(&mut self) -> Result<(), SequencedError> {
        while self.frames.len() < BUFFERED_FRAMES {
            let Some(received) = self.entries.remove(&self.next_sequence) else {
                break;
            };
            self.contiguous_received_at = Some(
                self.contiguous_received_at
                    .map_or(received.received_at, |old| old.max(received.received_at)),
            );
            self.next_sequence = self
                .next_sequence
                .checked_add(1)
                .ok_or(SequencedError::InvalidTimeline)?;
            match received.entry {
                OrderEntry::Batch {
                    frame,
                    source,
                    batch_id,
                    bytes,
                    ..
                } => {
                    if frame != self.assembling.frame
                        || batch_id != self.last_batch[usize::from(source)] + 1
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.last_batch[usize::from(source)] = batch_id;
                    let house =
                        self.houses[usize::from(source)].ok_or(SequencedError::InvalidTimeline)?;
                    if self.refused & 1 << source != 0 {
                        continue;
                    }
                    let decoded = decode_batch(&bytes, house, frame).ok();
                    let Some(records) = decoded else {
                        // VERA relay adaptation: every client reads the same
                        // ordered bytes, so each refuses the same batch. Its
                        // sender leaves at this frame, as by Leave, instead
                        // of one bad batch ending the match for everyone.
                        log::warn!(
                            "Relay source {source} sent an unreadable batch; it leaves at frame {frame}."
                        );
                        self.refused |= 1 << source;
                        self.assembling.records.push(
                            ExitRecord {
                                house_id: house,
                                frame,
                            }
                            .encode()
                            .map_err(|_| SequencedError::InvalidTimeline)?,
                        );
                        continue;
                    };
                    if self.assembling.records.len() + records.len() > MAX_FRAME_RECORDS {
                        return Err(SequencedError::QueueFull);
                    }
                    self.assembling.records.extend(records);
                }
                OrderEntry::Departure {
                    frame,
                    source,
                    lost,
                    ..
                } => {
                    if frame != self.assembling.frame {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    let house =
                        self.houses[usize::from(source)].ok_or(SequencedError::InvalidTimeline)?;
                    if self.refused & 1 << source != 0 {
                        // It already left at its refused batch.
                        continue;
                    }
                    let record = if lost {
                        // Silence ended this place: native connection loss.
                        // DestroyConnection(house, 1) (0x005DA750) sends
                        // EVENT_REMOVEPLAYER (0x005DA898..0x005DA8F0), which
                        // kills the House on every peer; each peer retires its
                        // own connection and posts the line.
                        if source != self.local_source {
                            self.assembling.lost.push(house);
                        }
                        CommandRecord::encode(
                            REMOVE_PLAYER_OPCODE,
                            i32::from(house),
                            frame as i32,
                            &i32::from(house).to_le_bytes(),
                        )
                    } else {
                        ExitRecord {
                            house_id: house,
                            frame,
                        }
                        .encode()
                    };
                    self.assembling
                        .records
                        .push(record.map_err(|_| SequencedError::InvalidTimeline)?);
                }
                OrderEntry::FrameEnd { frame, .. } => {
                    if frame != self.assembling.frame {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    let next = frame
                        .checked_add(1)
                        .ok_or(SequencedError::InvalidTimeline)?;
                    self.frames.push_back(std::mem::replace(
                        &mut self.assembling,
                        CommittedFrame {
                            frame: next,
                            ..Default::default()
                        },
                    ));
                    let received_at = self.contiguous_received_at.expect("consumed frame end");
                    for trace in &mut self.command_traces {
                        if trace
                            .assigned
                            .is_some_and(|(assigned, _)| assigned == frame)
                            && trace.frame_received_at.is_none()
                        {
                            trace.frame_received_at = Some(received_at);
                            if let Some(sent_at) = trace.sent_at {
                                let ms = received_at.saturating_duration_since(sent_at).as_millis()
                                    as u64;
                                self.delivery.round_trips += 1;
                                self.delivery.round_trip_total_ms += ms;
                                self.delivery.round_trip_max_ms =
                                    self.delivery.round_trip_max_ms.max(ms);
                            }
                        }
                    }
                    self.latest_frame_receipt = Some(SequencedTiming {
                        frame,
                        received_at,
                        frames_per_second: self.pace_fps,
                        clock_epoch: self.pause.epoch,
                    });
                }
                OrderEntry::Pause {
                    frame,
                    epoch,
                    paused,
                    source,
                    ..
                } => {
                    if frame.checked_add(1) != Some(self.assembling.frame)
                        || !self.assembling.records.is_empty()
                        || epoch.checked_sub(1) != Some(self.pause.epoch)
                        || paused == self.pause.paused
                        || self.houses[usize::from(source)].is_none()
                        || (!paused && frame != self.pause.frame)
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.pause.paused = paused;
                    self.pause.frame = frame;
                    self.pause.epoch = epoch;
                    // The authenticated sequencer only emits these markers
                    // after every active source advertises support. That is
                    // sufficient proof even if all of our status replies were
                    // lost; departed sources can never rejoin this roster.
                    self.pause_relay_supported = true;
                    self.pause_supported_sources |= self.pause_active_sources;
                    if !paused {
                        // Anchor resumed playout to this real socket receipt.
                        // The stopped wall time is absent from the timeline.
                        self.latest_frame_receipt = Some(SequencedTiming {
                            frame,
                            received_at: self.contiguous_received_at.expect("consumed resume"),
                            frames_per_second: self.pace_fps,
                            clock_epoch: epoch,
                        });
                    }
                    self.finish_pause_request();
                }
                OrderEntry::Rate {
                    frame,
                    frames_per_second,
                    source,
                    ..
                } => {
                    // Slowdowns name a player; raises name nobody.
                    let slowed_by = source.map(|source| self.houses[usize::from(source)]);
                    if frame.checked_add(1) != Some(self.assembling.frame)
                        || !self.assembling.records.is_empty()
                        || !paced_rates(self.fps).contains(&frames_per_second)
                        || frames_per_second == self.pace_fps
                        || slowed_by.is_some() != (frames_per_second < self.pace_fps)
                        || slowed_by.is_some_and(|house| house.is_none())
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.pace_fps = frames_per_second;
                    self.assembling.speed = match slowed_by.flatten() {
                        Some(house) => Some(MatchSpeedChange::SlowedBy(house)),
                        None => (frames_per_second == self.fps).then_some(MatchSpeedChange::Normal),
                    };
                    // Later frames arrive at the new rate from this receipt.
                    self.latest_frame_receipt = Some(SequencedTiming {
                        frame,
                        received_at: self.contiguous_received_at.expect("consumed rate marker"),
                        frames_per_second,
                        clock_epoch: self.pause.epoch,
                    });
                }
                OrderEntry::Desync {
                    frame,
                    sample_frame,
                    outlier,
                    outlier_checksum,
                    other,
                    other_checksum,
                    ..
                } => {
                    if frame.checked_add(1) != Some(self.assembling.frame)
                        || !self.assembling.records.is_empty()
                        || self.houses[usize::from(outlier)].is_none()
                        || self.houses[usize::from(other)].is_none()
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.assembling.desync = Some(RelayDesync {
                        sample_frame,
                        outlier: (outlier, outlier_checksum),
                        other: (other, other_checksum),
                    });
                }
            }
        }
        Ok(())
    }

    /// The relay's desync from this PC's side: the House it differs from
    /// (the outlier, or the other side when this PC is the outlier), this
    /// PC's own sample when it holds one, and that House's checksum.
    fn desync_error(&self, desync: RelayDesync) -> SequencedError {
        let ((source, remote), (_, reported)) = if desync.outlier.0 == self.local_source {
            (desync.other, desync.outlier)
        } else {
            (desync.outlier, desync.other)
        };
        let local = self
            .samples
            .iter()
            .rev()
            .find(|(frame, _)| *frame == desync.sample_frame)
            .map_or(reported, |&(_, checksum)| checksum);
        SequencedError::Desync {
            frame: desync.sample_frame,
            house: self.houses[usize::from(source)].expect("validated desync source"),
            local,
            remote,
        }
    }

    /// Service fresh input without another Logic visit. The current context
    /// is an already sampled post-Logic/pre-dispatch CRC, even when its frame
    /// has completed. It is historical metadata; only the relay assigns the
    /// command's execution frame. This path never consumes canonical frames.
    pub(super) fn service_pending_commands(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        now: u32,
    ) {
        if !self.started || self.error.is_some() {
            return;
        }
        let Some((context, _)) = self.current.filter(|(context, _)| context.frame != 0) else {
            return;
        };
        if let Err(error) = self.flush_local_commands(transport, queues, context, now) {
            // The regular frame boundary owns terminal error delivery. Its
            // admission gate also observes this retained error while stalled.
            self.error = Some(error);
        }
    }

    fn flush_local_commands(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        now: u32,
    ) -> Result<(), LiveCommandError> {
        if self.restoring {
            self.settle_restore(context.frame)?;
            if !self.restoring {
                transport.set_ordering_restore(false);
            }
        }
        if self.restoring {
            // Orders given to a world still replaying the past are dropped
            // here, the one boundary every local command crosses.
            let records: Vec<_> = queues
                .local_records_after(std::iter::empty())
                .map_err(super::CommandExchangeError::from)?
                .into_iter()
                .cloned()
                .collect();
            queues
                .commit_local_submission(&records)
                .map_err(super::CommandExchangeError::from)?;
            return Ok(());
        }
        while self
            .submissions
            .front()
            .is_some_and(|pending| pending.id <= self.accepted_batch)
        {
            let pending = self.submissions.pop_front().unwrap();
            queues
                .commit_local_submission(&pending.records)
                .map_err(super::CommandExchangeError::from)?;
            self.sent_commands += pending.records.len() as u64;
        }
        // The relay compares every PC's post-Logic checksum of each sampled
        // frame (relay README, Checksums). The previous sample goes again,
        // so one lost report loses nothing.
        if context.frame % CHECKSUM_INTERVAL == 0 && self.sample_sent != Some(context.frame) {
            let mut reports = vec![(context.frame, context.checksum)];
            reports.extend(
                self.samples
                    .iter()
                    .rev()
                    .find(|(frame, _)| *frame + CHECKSUM_INTERVAL == context.frame)
                    .copied(),
            );
            reports.reverse();
            if transport.queue_ordering_checksums(&reports).is_ok() {
                self.sample_sent = Some(context.frame);
            }
        }
        // Unacknowledged records remain the authoritative OutList prefix.
        // Encode only its not-yet-sealed suffix; do not mirror or requeue it.
        if self.submissions.len()
            < vera20k_relay_protocol::ClientSession::ORDERING_SUBMISSION_WINDOW
        {
            let records = queues
                .local_records_after(self.submissions.iter().flat_map(|batch| &batch.records))
                .map_err(super::CommandExchangeError::from)?;
            if !records.is_empty() {
                if records
                    .iter()
                    .any(|record| !gameplay_opcode(record.opcode()))
                {
                    return Err(SequencedError::InvalidTimeline.into());
                }
                // A batch is the packet's command records alone: no version
                // byte and no FRAME_INFO, which every reader rebuilds
                // (bandwidth-compaction spec §4).
                let encoded = EventPacketCodec::default()
                    .encode_packet(
                        &records,
                        FrameInfo {
                            house_id: self.houses[usize::from(self.local_source)].unwrap(),
                            event_frame: 0,
                            checksum: 0,
                            timing_word: 0,
                            delay: 0,
                        },
                        0,
                        EVENT_PACKET_HEADER_LEN + ORDERING_BATCH_CAPACITY,
                    )
                    .map_err(super::CommandExchangeError::from)?;
                if encoded.mirrors.is_empty() {
                    return Err(SequencedError::InvalidTimeline.into());
                }
                let bytes = encoded.bytes[EVENT_PACKET_HEADER_LEN..].to_vec();
                let next_batch = self
                    .next_batch
                    .checked_add(1)
                    .ok_or(SequencedError::InvalidTimeline)?;
                if self.command_traces.len() == COMMAND_TRACE_HISTORY {
                    self.command_traces.pop_front();
                }
                self.command_traces.push_back(CommandDeliveryTrace {
                    batch_id: self.next_batch,
                    sent_at: None,
                    assigned: None,
                    frame_received_at: None,
                });
                self.submissions.push_back(Submission {
                    id: self.next_batch,
                    records: encoded.mirrors,
                    bytes,
                    sealed: false,
                });
                self.next_batch = next_batch;
                self.service(transport, now);
            }
        }
        Ok(())
    }

    pub(super) fn poll(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        now: u32,
    ) -> Result<NetworkQueuePass, LiveCommandError> {
        self.service(transport, now);
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        if let Some((old, done)) = self.current {
            if context.frame == old.frame {
                if context.checksum != old.checksum {
                    return Err(LiveCommandError::ChangedFrame(context.frame));
                }
                if done {
                    return Ok(if context.frame == 0 {
                        NetworkQueuePass::SkipDispatch
                    } else {
                        NetworkQueuePass::Dispatch
                    });
                }
            } else if !done || context.frame != old.frame + 1 {
                return Err(LiveCommandError::FrameSequence {
                    expected: old.frame + 1,
                    actual: context.frame,
                });
            }
        } else if context.frame != 0 {
            return Err(LiveCommandError::FrameSequence {
                expected: 0,
                actual: context.frame,
            });
        }
        self.current = Some((context, false));
        if !self.started {
            return Ok(NetworkQueuePass::Waiting);
        }
        if context.frame == 0 {
            self.current = Some((context, true));
            return Ok(NetworkQueuePass::SkipDispatch);
        }
        if self
            .samples
            .back()
            .is_none_or(|(frame, _)| *frame != context.frame)
        {
            if self.samples.len() == SAMPLE_HISTORY {
                self.samples.pop_front();
            }
            self.samples.push_back((context.frame, context.checksum));
        }
        self.flush_local_commands(transport, queues, context, now)?;
        if self
            .frames
            .front()
            .is_none_or(|frame| frame.frame != context.frame)
        {
            return Ok(NetworkQueuePass::Waiting);
        }
        let frame = self.frames.pop_front().unwrap();
        if let Some(desync) = frame.desync {
            self.delivery
                .desync_frame
                .get_or_insert(desync.sample_frame);
            return Err(self.desync_error(desync).into());
        }
        for record in frame.records {
            if !queues.admit_received(record) {
                return Err(SequencedError::QueueFull.into());
            }
        }
        self.lost.extend(frame.lost);
        self.pace_meter.dispatched = self.pace_meter.dispatched.saturating_add(1);
        // A rejoining world replays the past without its lines.
        if !self.restoring {
            self.speed_changes.extend(frame.speed);
        }
        self.current = Some((context, true));
        Ok(NetworkQueuePass::Dispatch)
    }
}

fn gameplay_opcode(opcode: u8) -> bool {
    !matches!(
        opcode,
        0 | 0x0d | 0x19 | 0x1a | 0x1b | 0x1c | 0x1f | 0x20 | 0x21 | 0x23 | 0x24
    )
}

fn roster_mask(houses: &[Option<i8>; 30]) -> u32 {
    houses
        .iter()
        .enumerate()
        .filter(|(_, h)| h.is_some())
        .fold(0, |m, (s, _)| m | (1 << s))
}

/// A batch's command records (bandwidth-compaction spec §4): the packet
/// rebuilt with the FRAME_INFO of its source's `house`, its record's
/// `frame` and zeros, then the unchanged `EventPacketCodec` decode.
fn decode_batch(bytes: &[u8], house: i8, frame: u32) -> Result<Vec<CommandRecord>, SequencedError> {
    if bytes.is_empty() || bytes.len() > ORDERING_BATCH_CAPACITY {
        return Err(SequencedError::InvalidTimeline);
    }
    let header = FrameInfo {
        house_id: house,
        event_frame: i32::try_from(frame).map_err(|_| SequencedError::InvalidTimeline)?,
        checksum: 0,
        timing_word: 0,
        delay: 0,
    }
    .encode();
    let mut packet = Vec::with_capacity(EVENT_PACKET_HEADER_LEN + bytes.len());
    packet.extend_from_slice(&header.as_bytes()[..EVENT_PACKET_HEADER_LEN]);
    packet.extend_from_slice(bytes);
    let packet = packet.as_slice();
    let mut records = Vec::new();
    let mut invalid = false;
    let mut header = true;
    let decoded = EventPacketCodec::default()
        .decode_packet(packet, |mut record| {
            if header {
                header = false;
                return true;
            }
            if record.house_id() != house || !gameplay_opcode(record.opcode()) {
                invalid = true;
                return false;
            }
            record.set_frame_stamp(frame as i32);
            records.push(record);
            true
        })
        .map_err(|_| SequencedError::InvalidTimeline)?;
    if invalid
        || decoded.consumed_bytes != packet.len()
        || records.len() > crate::sim::command_queue::OUT_LIST_CAPACITY
    {
        return Err(SequencedError::InvalidTimeline);
    }
    Ok(records)
}

#[cfg(test)]
mod service_clock_tests {
    use super::service_due;

    #[test]
    fn retry_deadline_survives_bucket_rounding_and_uptime_wrap() {
        assert!(service_due(None, 1008, 100));
        assert!(!service_due(Some(1018), 1008, 100));
        assert!(!service_due(Some(1018), 1117, 100));
        assert!(service_due(Some(1018), 1118, 100));
        assert!(!service_due(Some(u32::MAX - 50), 48, 100));
        assert!(service_due(Some(u32::MAX - 50), 49, 100));
    }
}

#[cfg(test)]
mod desync_tests {
    use super::*;

    /// The relay's DESYNC ends the match in the frame after it. The log
    /// names the outlier's House against this PC's own sample, or the other
    /// side's House when this PC is the outlier.
    #[test]
    fn a_relay_desync_names_the_house_this_pc_differs_from() {
        let mut houses = [None; 30];
        houses[..3].copy_from_slice(&[Some(0), Some(4), Some(2)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        session.samples.push_back((15, 0x1111));
        for entry in [
            OrderEntry::FrameEnd {
                sequence: 1,
                frame: 1,
            },
            OrderEntry::Desync {
                sequence: 2,
                frame: 1,
                sample_frame: 15,
                outlier: 1,
                outlier_checksum: 0x2a2a,
                other: 2,
                other_checksum: 0x1110,
            },
            OrderEntry::FrameEnd {
                sequence: 3,
                frame: 2,
            },
        ] {
            session
                .receive(OrderEvent::Entry(entry), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        let desyncs: Vec<_> = session
            .frames
            .iter()
            .map(|frame| frame.desync.is_some())
            .collect();
        assert_eq!(desyncs, [false, true]);
        let desync = session.frames[1].desync.unwrap();
        assert_eq!(
            session.desync_error(desync).to_string(),
            "online simulation differs for house 4 at frame 15: \
             local 0x00001111, remote 0x00002a2a"
        );
        // Without its own sample, the outlier's local value is its report.
        session.samples.clear();
        let outlier = RelayDesync {
            outlier: (0, 0x2a2a),
            ..desync
        };
        assert_eq!(
            session.desync_error(outlier).to_string(),
            "online simulation differs for house 2 at frame 15: \
             local 0x00002a2a, remote 0x00001110"
        );
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;
    use vera20k_relay_protocol::OrderStatus;

    fn status(next_seq: u64, head_frame: u32) -> OrderEvent {
        OrderEvent::Status(OrderStatus {
            head_frame,
            oldest_seq: 1,
            next_seq,
            active_sources: 3,
            tick_hz: 30,
            start_delay_ms: 0,
            started: true,
            feedback: Default::default(),
        })
    }

    /// The counter the departed process reached comes from the history: its
    /// own first ID would make the relay end this client's match (`BatchGap`).
    #[test]
    fn resumes_the_batch_counter_only_once_the_world_caught_up() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [7; 32], 30);
        session.begin_restore();
        session.last_batch[0] = 41;
        session.next_sequence = 500;

        // Nothing is known of the relay yet.
        session.settle_restore(0).unwrap();
        assert!(session.restoring());
        session.receive(status(900, 1000), Instant::now()).unwrap();
        // History still to read, then a world still far behind it.
        session.settle_restore(1000).unwrap();
        assert!(session.restoring());
        session.next_sequence = 900;
        session.settle_restore(900).unwrap();
        assert!(session.restoring());

        session.settle_restore(990).unwrap();
        assert!(!session.restoring());
        assert_eq!((session.next_batch, session.accepted_batch), (42, 41));
    }
}

#[cfg(test)]
mod stall_tests {
    use super::*;

    /// Federated relays whose link failed still answer, but the ordered
    /// stream stops: a running, unpaused match shows Reconnecting, so Quit
    /// leaves without waiting on that stream.
    #[test]
    fn a_stream_that_stands_still_while_its_relay_answers_is_reconnecting() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        session.started = true;
        session.head_frame = 40;
        session.last_progress = Some((1_000, (40, 90, 90)));
        session.last_response = Some(1_000 + STALL_MS);
        assert_eq!(session.recovery_status(1_000 + STALL_MS, 40), None);
        assert_eq!(
            session.recovery_status(1_000 + STALL_MS + 1, 40),
            Some(super::super::NetworkRecoveryStatus::Reconnecting)
        );
        session.pause.paused = true;
        assert_eq!(session.recovery_status(1_000 + STALL_MS + 1, 40), None);
    }

    fn paused_at_100() -> SequencedSession {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        session.started = true;
        session.head_frame = 100;
        session.pause.paused = true;
        session.pause.frame = 100;
        session
    }

    /// A game the pause finds more than half a second behind says once that
    /// it caught up, when it reaches the pause.
    #[test]
    fn a_game_a_pause_finds_catching_up_says_once_that_it_caught_up() {
        let mut session = paused_at_100();
        assert!(
            !session.take_caught_up_in_pause(false, 84),
            "16 frames behind"
        );
        assert!(session.take_caught_up_in_pause(true, 101));
        assert!(!session.take_caught_up_in_pause(true, 101), "once a pause");
    }

    /// A game keeping up says nothing, nor does one a resume reaches first,
    /// nor a rejoining one replaying a pause the relay has already ended.
    #[test]
    fn a_game_keeping_up_or_resumed_before_the_pause_says_nothing() {
        let mut session = paused_at_100();
        assert!(
            !session.take_caught_up_in_pause(false, 85),
            "15 frames behind"
        );
        assert!(!session.take_caught_up_in_pause(true, 101));
        let mut session = paused_at_100();
        assert!(!session.take_caught_up_in_pause(false, 60));
        session.head_frame = 300;
        assert!(
            !session.take_caught_up_in_pause(true, 101),
            "the relay has resumed and run on"
        );
        let mut session = paused_at_100();
        assert!(!session.take_caught_up_in_pause(false, 60));
        session.pause.paused = false;
        assert!(!session.take_caught_up_in_pause(false, 70));
        session.pause.paused = true;
        session.head_frame = 200;
        session.pause.frame = 200;
        assert!(!session.take_caught_up_in_pause(true, 201), "a later pause");
    }

    /// Loading leaves the position still for many seconds; the start itself
    /// must not read as a stall before frame 1 arrives.
    #[test]
    fn the_start_of_a_match_is_not_a_stall() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        session.last_progress = Some((1_000, (0, 1, 1)));
        let now = 1_000 + 40_000;
        session.last_response = Some(now);
        let status = vera20k_relay_protocol::OrderStatus {
            head_frame: 0,
            oldest_seq: 1,
            next_seq: 1,
            active_sources: 3,
            tick_hz: 30,
            start_delay_ms: 0,
            started: true,
            feedback: Default::default(),
        };
        session
            .receive(OrderEvent::Status(status), Instant::now())
            .unwrap();
        assert_eq!(session.recovery_status(now, 0), None);
    }
}

#[cfg(test)]
mod delivery_counter_tests {
    use super::*;

    /// A local batch's round trip runs from its first transmission to the
    /// receipt of the frame the relay sealed it into, counted once.
    #[test]
    fn a_sealed_frame_counts_each_local_batch_round_trip_once() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        let sent = Instant::now();
        session.command_traces.push_back(CommandDeliveryTrace {
            batch_id: 1,
            sent_at: Some(sent),
            assigned: Some((1, sent)),
            frame_received_at: None,
        });
        let received = sent + std::time::Duration::from_millis(40);
        let frame_end =
            |sequence, frame| OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame });
        session.receive(frame_end(1, 1), received).unwrap();
        session.assemble().unwrap();
        session.receive(frame_end(2, 2), received).unwrap();
        session.assemble().unwrap();
        let counters = session.delivery_counters();
        assert_eq!(
            (
                counters.round_trips,
                counters.round_trip_total_ms,
                counters.round_trip_max_ms
            ),
            (1, 40, 40)
        );
    }
}

#[cfg(test)]
mod departure_tests {
    use super::*;
    use crate::sim::command::EXIT_OPCODE;

    /// Silence ends a place as native connection loss: EVENT_REMOVEPLAYER
    /// kills the House on every client, and only a remote House is reported
    /// for its line. A Leave stays EXIT.
    #[test]
    fn lost_departures_remove_the_house_and_leave_stays_exit() {
        let mut houses = [None; 30];
        houses[..3].copy_from_slice(&[Some(0), Some(1), Some(2)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        let departure = |sequence, frame, source, lost| {
            OrderEvent::Entry(OrderEntry::Departure {
                sequence,
                frame,
                source,
                lost,
            })
        };
        for event in [
            departure(1, 1, 1, true),
            departure(2, 1, 0, true),
            OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: 3,
                frame: 1,
            }),
            departure(4, 2, 2, false),
            OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: 5,
                frame: 2,
            }),
        ] {
            session.receive(event, Instant::now()).unwrap();
        }
        session.assemble().unwrap();
        let frames: Vec<_> = session
            .frames
            .iter()
            .map(|frame| {
                let records: Vec<_> = frame
                    .records
                    .iter()
                    .map(|record| (record.opcode(), record.house_id(), record.payload()[0]))
                    .collect();
                (records, frame.lost.clone())
            })
            .collect();
        assert_eq!(
            frames,
            [
                (
                    vec![(REMOVE_PLAYER_OPCODE, 1, 1), (REMOVE_PLAYER_OPCODE, 0, 0)],
                    vec![1]
                ),
                (vec![(EXIT_OPCODE, 2, 0)], vec![]),
            ]
        );
    }

    /// A record this build cannot read ends the match once it has held the
    /// cursor for five seconds while the relay repeats it; one that holds
    /// nothing back is ignored.
    #[test]
    fn an_unreadable_record_ends_the_match_only_while_it_holds_the_cursor() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        session.mark_started_for_test();
        session.note_unreadable(0);
        session.check_unreadable(6_000).unwrap();
        // A newer departure at 1 holds the frame end that follows it.
        session.note_unreadable(6_000);
        let frame_end = OrderEntry::FrameEnd {
            sequence: 2,
            frame: 1,
        };
        session
            .receive(OrderEvent::Entry(frame_end), Instant::now())
            .unwrap();
        session.assemble().unwrap();
        for now in (6_500..=10_500).step_by(500) {
            session.note_unreadable(now);
        }
        session.check_unreadable(10_999).unwrap();
        session.note_unreadable(11_000);
        assert!(matches!(
            session.check_unreadable(11_000),
            Err(SequencedError::NewerClientRequired)
        ));
    }

    /// Before the start an unreadable Status ends the match the same way; a
    /// readable Status in between keeps it waiting.
    #[test]
    fn an_unreadable_status_ends_the_match_before_the_start() {
        use vera20k_relay_protocol::OrderStatus;
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        session.note_unreadable(0);
        let status = OrderStatus {
            head_frame: 0,
            oldest_seq: 1,
            next_seq: 1,
            active_sources: 3,
            tick_hz: 30,
            start_delay_ms: 0,
            started: false,
            feedback: Default::default(),
        };
        session
            .receive(OrderEvent::Status(status), Instant::now())
            .unwrap();
        session.check_unreadable(5_000).unwrap();
        for now in (5_000..=10_000).step_by(1_000) {
            session.note_unreadable(now);
        }
        assert!(matches!(
            session.check_unreadable(10_000),
            Err(SequencedError::NewerClientRequired)
        ));
    }

    /// A batch that does not decode departs its sender in its frame
    /// instead of leaving a sequence gap that every pull repeats.
    #[test]
    fn an_unreadable_batch_departs_its_sender_in_its_frame() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        for event in [
            OrderEntry::FrameEnd {
                sequence: 1,
                frame: 1,
            },
            OrderEntry::Batch {
                sequence: 2,
                frame: 2,
                source: 1,
                batch_id: 1,
                bytes: b"not a batch".to_vec(),
            },
            OrderEntry::FrameEnd {
                sequence: 3,
                frame: 2,
            },
        ] {
            session
                .receive(OrderEvent::Entry(event), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        let frames: Vec<_> = session
            .frames
            .iter()
            .map(|frame| {
                let records: Vec<_> = frame
                    .records
                    .iter()
                    .map(|record| (record.opcode(), record.house_id()))
                    .collect();
                (frame.frame, records)
            })
            .collect();
        assert_eq!(frames, [(1, vec![]), (2, vec![(EXIT_OPCODE, 1)])]);
    }

    /// A batch whose contents do not decode ends only its sender's place: it
    /// leaves at that frame and its later batches and departure are ignored.
    #[test]
    fn a_malformed_batch_departs_its_sender_instead_of_ending_the_match() {
        use std::net::{Ipv4Addr, SocketAddrV4};
        use vera20k_relay_protocol::{
            ClientSession, RelayServer, SecretKey, ServerEvent, Ticket, derive_packet_key,
        };
        let key = SecretKey::new([5; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut events = |client: &mut ClientSession, bytes: &[u8], now_ms| {
            let from = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31000 + u16::from(client.source()));
            let replies = server.process_at(from, bytes, 1000 + now_ms / 1000, now_ms);
            replies
                .unwrap()
                .into_iter()
                .flat_map(
                    |reply| match client.receive_at(&reply.bytes, now_ms).unwrap() {
                        ServerEvent::Ordering(event) => vec![Ok(event)],
                        ServerEvent::OrderingPack(events) => events.into_iter().map(Ok).collect(),
                        ServerEvent::Challenge(cookie) => vec![Err(cookie.to_vec())],
                        _ => Vec::new(),
                    },
                )
                .collect::<Vec<_>>()
        };
        let certificate = OrderingCertificate {
            room: [9; 16],
            sources_mask: 3,
            tick_hz: 30,
            expires: 2000,
            pacing: Default::default(),
        }
        .sign(&key)
        .unwrap();
        let mut clients: Vec<_> = (0..2u8)
            .map(|source| {
                let ticket = Ticket {
                    room: [9; 16],
                    player: [source + 1; 16],
                    source,
                    generation: 1,
                    expires: 1200,
                    nonce: [source + 1; 16],
                }
                .sign(&key)
                .unwrap();
                let packet_key = *derive_packet_key(&key, &ticket).expose_for_https();
                ClientSession::new(&ticket, packet_key, [7; 32]).unwrap()
            })
            .collect();
        let mut bytes = Vec::new();
        for client in &mut clients {
            client.probe_at(0, &mut bytes).unwrap();
            let [Err(cookie)] = &events(client, &bytes, 0)[..] else {
                panic!("challenge");
            };
            client.confirm_at(&cookie.clone(), 0, &mut bytes).unwrap();
            events(client, &bytes, 0);
            client
                .configure_ordering_at(&certificate, [7; 32], 0, &mut bytes)
                .unwrap();
            events(client, &bytes, 0);
        }
        // The start is due 200 ms after both were ready; the next
        // READY_REQUEST's STATUS reports it.
        for client in &mut clients {
            client
                .configure_ordering_at(&certificate, [7; 32], 300, &mut bytes)
                .unwrap();
            events(client, &bytes, 300);
        }
        for batch_id in 1..=2 {
            assert_eq!(
                clients[1].seal_ordering_batch(b"not a batch").unwrap(),
                batch_id
            );
            assert!(clients[1].poll_uplink(300 + batch_id, &mut bytes));
            events(&mut clients[1], &bytes, 300 + batch_id);
        }
        clients[1].leave_ordering_at(400, &mut bytes).unwrap();
        events(&mut clients[1], &bytes, 400);
        clients[0].pull_ordering_at(1, 700, &mut bytes).unwrap();
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [7; 32], 30);
        for event in events(&mut clients[0], &bytes, 700) {
            if let Ok(event @ OrderEvent::Entry(_)) = event {
                session.receive(event, Instant::now()).unwrap();
            }
        }
        session.assemble().unwrap();
        let records: Vec<_> = session
            .frames
            .iter()
            .flat_map(|frame| &frame.records)
            .map(|record| (record.opcode(), record.house_id()))
            .collect();
        assert!(session.frames.len() > 2);
        assert_eq!(records, [(EXIT_OPCODE, 1)]);
    }
}

#[cfg(test)]
mod pace_tests {
    use super::*;

    /// Lagging needs frames waiting two seconds; behind also needs two report
    /// windows in a row that ran some frames but fewer than the clock sealed.
    /// A world that ran none, or one running faster, only catches up; a
    /// hidden window starts over.
    #[test]
    fn a_pc_is_behind_only_while_frames_wait_and_it_runs_some_but_too_few() {
        use OrderPace::*;
        let mut meter = PaceMeter::default();
        let window = |meter: &mut PaceMeter, now: u32, ran: u32| {
            meter.dispatched = ran;
            meter.report(now, 10, 30, true)
        };
        assert_eq!(meter.report(0, 10, 30, true), Some((KeepingUp, 30, None)));
        assert_eq!(meter.report(500, 3, 30, true), None);
        assert_eq!(window(&mut meter, 1000, 20), Some((KeepingUp, 20, None)));
        assert_eq!(window(&mut meter, 2000, 20), Some((KeepingUp, 20, None)));
        assert_eq!(window(&mut meter, 3000, 20), Some((Behind, 20, None)));
        assert_eq!(window(&mut meter, 4000, 0), Some((CatchingUp, 0, None)));
        assert_eq!(window(&mut meter, 5000, 20), Some((CatchingUp, 20, None)));
        assert_eq!(window(&mut meter, 6000, 60), Some((CatchingUp, 60, None)));
        meter.hidden = true;
        assert_eq!(window(&mut meter, 7000, 20), Some((KeepingUp, 20, None)));
        // Each report carries the window's 90th-percentile tick cost.
        for micros in (1..=10).map(|step| step * 1000) {
            meter.record_tick_cost(micros);
        }
        assert_eq!(
            window(&mut meter, 8000, 20),
            Some((KeepingUp, 20, Some(9000)))
        );
        assert_eq!(window(&mut meter, 9000, 20), Some((KeepingUp, 20, None)));
    }

    /// A rate marker paces the frames after it, keeps the simulation's
    /// rate, and names the slow player in the first frame it applies to.
    #[test]
    fn a_rate_marker_paces_later_frames_and_names_the_slow_player() {
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(4)]);
        let mut session = SequencedSession::with_roster(houses, 0, [0; 32], 30);
        let rate = |sequence, frame, frames_per_second, source| OrderEntry::Rate {
            sequence,
            frame,
            frames_per_second,
            source,
        };
        let frame_end = |sequence, frame| OrderEntry::FrameEnd { sequence, frame };
        for entry in [
            frame_end(1, 1),
            rate(2, 1, 15, Some(1)),
            frame_end(3, 2),
            rate(4, 2, 30, None),
            frame_end(5, 3),
        ] {
            session
                .receive(OrderEvent::Entry(entry), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        let speeds: Vec<_> = session.frames.iter().map(|frame| frame.speed).collect();
        assert_eq!(
            speeds,
            [
                None,
                Some(MatchSpeedChange::SlowedBy(4)),
                Some(MatchSpeedChange::Normal)
            ]
        );
        assert_eq!((session.fps, session.pace_fps), (30, 30));
        // Ten frames a second is three rows below this match's thirty.
        session
            .receive(OrderEvent::Entry(rate(6, 3, 10, Some(1))), Instant::now())
            .unwrap();
        assert!(matches!(
            session.assemble(),
            Err(SequencedError::InvalidTimeline)
        ));
    }
}
