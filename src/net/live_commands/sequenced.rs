//! VERA online ordering adaptation. The authenticated relay seals an immutable
//! timeline; no client infers a missing participant's input or rewrites a tick.
//! Native Event/OutList/DoList and the ordinary simulation transaction retain
//! gameplay authority. Fresh packet caches make each encrypted batch standalone.

use std::collections::{BTreeMap, VecDeque};

use vera20k_relay_protocol::{
    ORDERING_BATCH_CAPACITY, OrderEntry, OrderError, OrderEvent, OrderingCertificate,
};

use super::{LiveCommandError, StartupAgreement, StartupError};
use crate::net::event_packet::{EventPacketCodec, decode_frame_info};
use crate::net::udp::LanUdpTransport;
use crate::sim::command::{CommandRecord, ExitRecord, REMOVE_PLAYER_OPCODE};
use crate::sim::command_queue::FrameInfo;
use crate::sim::world::{NetworkCommandQueues, NetworkQueueContext, NetworkQueuePass};
use crate::util::time::Instant;

const BUFFERED_FRAMES: usize = 64;
const REORDER_WINDOW: u64 = 256;
const MAX_FRAME_RECORDS: usize = 4096;
const SAMPLE_HISTORY: usize = 4096;
const BATCH_HEADER: usize = 9;
const PUSH_PROBE_INTERVAL_MS: i32 = 500;
const PUSH_REPROBE_INTERVAL_MS: i32 = 5_000;
const PUSH_WATERMARK_INTERVAL_MS: i32 = 100;
const REPLAY_RETRY_INTERVAL_MS: i32 = 100;
const PUSH_SILENCE_REPAIR_MS: u32 = 1_000;
const PAUSE_PROBE_INTERVAL_MS: i32 = 500;
const PAUSE_COMPATIBILITY_PROBE_INTERVAL_MS: i32 = 5_000;
const COMMAND_TRACE_HISTORY: usize = 256;
// A relay record this build cannot read ends the match once it has held the
// cursor, or before the start every Status, this long while the relay keeps
// repeating it. Silence alone never ends a match.
const UNREADABLE_LIMIT_MS: u32 = 5_000;
const UNREADABLE_REPEAT_MS: u32 = 1_000;

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

#[derive(Debug)]
enum PushDelivery {
    Probing { attempts: u8 },
    Active,
    PullOnly,
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
    #[error("online simulation differs at frame {frame}")]
    Desync { frame: u32 },
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
    last_sent: Option<u32>,
}

#[derive(Debug, Default)]
struct CommittedFrame {
    frame: u32,
    records: Vec<CommandRecord>,
    reports: Vec<(u32, u32)>,
    // Remote Houses whose connection the relay ended in this frame.
    lost: Vec<i8>,
}

#[derive(Debug)]
pub(super) struct SequencedSession {
    houses: [Option<i8>; 30],
    local_source: u8,
    agreement: [u8; 32],
    pub(super) fps: u16,
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
    push: PushDelivery,
    last_push_control: Option<u32>,
    push_watermark: u64,
    push_confirmations: u64,
    remote_next_sequence: u64,
    repair_gap_since: Option<u32>,
    pause: CanonicalPause,
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
    // frame order. Late reports outside this cache are not compared to aliases.
    samples: VecDeque<(u32, u32)>,
    current: Option<(NetworkQueueContext, bool)>,
    sent_commands: u64,
    command_traces: VecDeque<CommandDeliveryTrace>,
    // Released when their frame dispatches; the app posts each line.
    lost: Vec<i8>,
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
            started: false,
            head_frame: 0,
            error: None,
            last_response: None,
            last_progress: None,
            first_service: None,
            statuses: 0,
            unreadable: None,
            last_control: None,
            push: PushDelivery::Probing { attempts: 0 },
            last_push_control: None,
            push_watermark: 1,
            push_confirmations: 0,
            remote_next_sequence: 1,
            repair_gap_since: None,
            pause: CanonicalPause::default(),
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
            && self.head_frame.saturating_sub(frame) <= u32::from(self.fps) / 2
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
    pub(super) fn sent_commands(&self) -> u64 {
        self.sent_commands
    }

    pub(super) fn command_traces(&self) -> &VecDeque<CommandDeliveryTrace> {
        &self.command_traces
    }

    fn trace_assignment(&mut self, batch_id: u64, frame: u32, received_at: Instant) {
        if let Some(trace) = trace_mut(&mut self.command_traces, batch_id) {
            trace.assigned.get_or_insert((frame, received_at));
        }
    }

    pub(super) fn timing(&self) -> Option<SequencedTiming> {
        matches!(self.push, PushDelivery::Active)
            .then_some(self.latest_frame_receipt)
            .flatten()
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
                "Rejoin: frame {:?}, frontier {}, head {}, next sequence {} of {}, buffered {}, started {}, statuses {}, push {:?}",
                self.current.map(|(context, done)| (context.frame, done)),
                self.frontier(),
                self.head_frame,
                self.next_sequence,
                self.remote_next_sequence,
                self.frames.len(),
                self.started,
                self.statuses,
                self.push,
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
        for pending in &mut self.submissions {
            // An unready route or failed local send cannot start the remote
            // receipt timer. The next service retries the same retained bytes.
            if pending.id > self.accepted_batch && service_due(pending.last_sent, now, 100) {
                // A fresh transmission goes out twice. The relay accepts batch
                // IDs in order, so one lost datagram would otherwise hold this
                // and every later batch until the retry; the copy is an
                // idempotent retry of identical ciphertext.
                let copies = if pending.last_sent.is_none() { 2 } else { 1 };
                let mut sent = false;
                for _ in 0..copies {
                    sent |= transport
                        .submit_ordering(pending.id, &pending.bytes)
                        .is_ok();
                }
                if sent {
                    pending.last_sent = Some(now);
                    if let Some(trace) = trace_mut(&mut self.command_traces, pending.id) {
                        trace.sent_at.get_or_insert_with(Instant::now);
                    }
                }
            }
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

    fn service_delivery(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if self.started {
            match &mut self.push {
                PushDelivery::Probing { attempts } => {
                    // Old relays reject unknown controls, but three lost
                    // replies look the same. Pull meanwhile and keep probing
                    // at a slower rate; only a relay's explicit refusal ends
                    // the handshake.
                    let interval = if *attempts < 3 {
                        PUSH_PROBE_INTERVAL_MS
                    } else {
                        PUSH_REPROBE_INTERVAL_MS
                    };
                    if service_due(self.last_push_control, now, interval) {
                        let confirmations = transport.relay_confirmations();
                        if transport
                            .set_ordering_push(true, self.next_sequence)
                            .is_ok()
                        {
                            *attempts = attempts.saturating_add(1);
                            self.last_push_control = Some(now);
                            self.push_watermark = self.next_sequence;
                            self.push_confirmations = confirmations;
                        }
                    }
                }
                PushDelivery::Active => {
                    // RelayServer::confirm starts a renewed lease or a new path
                    // without the old subscription. Re-subscribe on any READY
                    // rather than up to a watermark interval later; after a
                    // keepalive READY that is only an early watermark.
                    let confirmations = transport.relay_confirmations();
                    if (confirmations != self.push_confirmations
                        || self.next_sequence.saturating_sub(self.push_watermark) >= 32
                        || service_due(self.last_push_control, now, PUSH_WATERMARK_INTERVAL_MS))
                        && transport
                            .set_ordering_push(true, self.next_sequence)
                            .is_ok()
                    {
                        self.last_push_control = Some(now);
                        self.push_watermark = self.next_sequence;
                        self.push_confirmations = confirmations;
                    }
                }
                PushDelivery::PullOnly => {}
            }
        }
        let active_push = self.started && matches!(self.push, PushDelivery::Active);
        let gap = self.cursor_gap();
        // A status can overtake its immediately following pushed datagrams.
        // Allow one nominal frame for reordering before requesting repair;
        // healthy delivery must not turn every watermark into another Pull.
        let gap_ready = if gap {
            let since = *self.repair_gap_since.get_or_insert(now);
            now.wrapping_sub(since) >= 1000_u32.div_ceil(u32::from(self.fps))
        } else {
            self.repair_gap_since = None;
            false
        };
        let silent = self
            .last_response
            .is_some_and(|last| now.wrapping_sub(last) > PUSH_SILENCE_REPAIR_MS);
        let pull_needed = !active_push || gap_ready || silent;
        let interval = if active_push {
            REPLAY_RETRY_INTERVAL_MS
        } else if self.started {
            16
        } else {
            125
        };
        if pull_needed && service_due(self.last_control, now, interval) {
            if !self.started {
                let _ = transport.configure_ordering(self.agreement);
                // Before the first request that can bring back an entry.
                transport.set_ordering_restore(self.restoring);
            }
            if transport.pull_ordering(self.next_sequence).is_ok() {
                self.last_control = Some(now);
            }
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
            OrderEvent::PushState {
                enabled,
                next_sequence,
            } => {
                if next_sequence == 0 || next_sequence > self.next_sequence {
                    return Err(SequencedError::InvalidTimeline);
                }
                self.push = if enabled {
                    PushDelivery::Active
                } else {
                    PushDelivery::PullOnly
                };
            }
            OrderEvent::Ack { batch_id, frame } => {
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
                    | OrderEntry::Pause { sequence, .. } => *sequence,
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
                    self.push = PushDelivery::Probing { attempts: 0 };
                    self.last_push_control = None;
                    self.repair_gap_since = None;
                }
                OrderError::NotStarted | OrderError::Backpressure => {}
                OrderError::BatchGap => {
                    let accepted = expected
                        .checked_sub(1)
                        .ok_or(SequencedError::InvalidTimeline)?;
                    // Reordered submissions and retries of older accepted
                    // IDs both return the relay's next expected ID. Its
                    // authenticated prefix receipt never moves backwards.
                    self.accept_through(accepted)?;
                    if expected > self.accepted_batch {
                        for pending in &mut self.submissions {
                            if pending.id >= expected {
                                pending.last_sent = None;
                            }
                        }
                    }
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
        let frames = self.head_frame.saturating_sub(current_frame);
        (frames > u32::from(self.fps) / 2)
            .then_some(super::NetworkRecoveryStatus::CatchingUp { frames })
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
                    let decoded = bytes
                        .as_ref()
                        .and_then(|bytes| decode_batch(bytes.as_bytes(), house, frame).ok());
                    let Some((records, report)) = decoded else {
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
                    self.assembling.reports.push(report);
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
                        {
                            trace.frame_received_at.get_or_insert(received_at);
                        }
                    }
                    self.latest_frame_receipt = Some(SequencedTiming {
                        frame,
                        received_at,
                        frames_per_second: self.fps,
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
                        || !self.assembling.reports.is_empty()
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
                            frames_per_second: self.fps,
                            clock_epoch: epoch,
                        });
                    }
                    self.finish_pause_request();
                }
            }
        }
        Ok(())
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
        // Unacknowledged records remain the authoritative OutList prefix.
        // Encode only its not-yet-sealed suffix; do not mirror or requeue it.
        let sample_due = context.frame % 15 == 0 && self.sample_sent != Some(context.frame);
        if self.submissions.len()
            < vera20k_relay_protocol::ClientSession::ORDERING_SUBMISSION_WINDOW
        {
            let records = queues
                .local_records_after(self.submissions.iter().flat_map(|batch| &batch.records))
                .map_err(super::CommandExchangeError::from)?;
            if !records.is_empty() || sample_due {
                if records
                    .iter()
                    .any(|record| !gameplay_opcode(record.opcode()))
                {
                    return Err(SequencedError::InvalidTimeline.into());
                }
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
                        ORDERING_BATCH_CAPACITY - BATCH_HEADER,
                    )
                    .map_err(super::CommandExchangeError::from)?;
                if !records.is_empty() && encoded.mirrors.is_empty() {
                    return Err(SequencedError::InvalidTimeline.into());
                }
                let mut bytes = Vec::with_capacity(ORDERING_BATCH_CAPACITY);
                bytes.push(1);
                bytes.extend(context.frame.to_le_bytes());
                bytes.extend(context.checksum.to_le_bytes());
                bytes.extend(encoded.bytes);
                self.sample_sent = Some(context.frame);
                let next_batch = self
                    .next_batch
                    .checked_add(1)
                    .ok_or(SequencedError::InvalidTimeline)?;
                if !encoded.mirrors.is_empty() {
                    if self.command_traces.len() == COMMAND_TRACE_HISTORY {
                        self.command_traces.pop_front();
                    }
                    self.command_traces.push_back(CommandDeliveryTrace {
                        batch_id: self.next_batch,
                        sent_at: None,
                        assigned: None,
                        frame_received_at: None,
                    });
                }
                self.submissions.push_back(Submission {
                    id: self.next_batch,
                    records: encoded.mirrors,
                    bytes,
                    last_sent: None,
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
        for (sample_frame, checksum) in frame.reports {
            if let Some((_, local)) = self.samples.iter().find(|(f, _)| *f == sample_frame)
                && checksum != *local
            {
                return Err(SequencedError::Desync {
                    frame: sample_frame,
                }
                .into());
            }
        }
        for record in frame.records {
            if !queues.admit_received(record) {
                return Err(SequencedError::QueueFull.into());
            }
        }
        self.lost.extend(frame.lost);
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

fn decode_batch(
    bytes: &[u8],
    house: i8,
    frame: u32,
) -> Result<(Vec<CommandRecord>, (u32, u32)), SequencedError> {
    if bytes.len() < BATCH_HEADER + 14 || bytes.len() > ORDERING_BATCH_CAPACITY || bytes[0] != 1 {
        return Err(SequencedError::InvalidTimeline);
    }
    let sample_frame = u32::from_le_bytes(bytes[1..5].try_into().unwrap());
    let checksum = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
    let packet = &bytes[BATCH_HEADER..];
    if decode_frame_info(packet).is_none_or(|info| info.house_id != house || info.event_frame != 0)
    {
        return Err(SequencedError::InvalidTimeline);
    }
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
    Ok((records, (sample_frame, checksum)))
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
        };
        session
            .receive(OrderEvent::Status(status), Instant::now())
            .unwrap();
        assert_eq!(session.recovery_status(now, 0), None);
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

    /// A batch the payload key cannot open departs its sender in its frame
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
                bytes: None,
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
                .filter_map(|reply| match client.receive(&reply.bytes).unwrap() {
                    ServerEvent::Ordering(event) => Some(Ok(event)),
                    ServerEvent::Challenge(cookie) => Some(Err(cookie.to_vec())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let certificate = OrderingCertificate {
            room: [9; 16],
            sources_mask: 3,
            tick_hz: 30,
            expires: 2000,
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
            client.probe(&mut bytes).unwrap();
            let [Err(cookie)] = &events(client, &bytes, 0)[..] else {
                panic!("challenge");
            };
            client.confirm(&cookie.clone(), &mut bytes).unwrap();
            events(client, &bytes, 0);
            client
                .configure_ordering(&certificate, [7; 32], &mut bytes)
                .unwrap();
            events(client, &bytes, 0);
        }
        for batch_id in 1..=2 {
            clients[1]
                .submit_ordering(batch_id, b"not a batch", &mut bytes)
                .unwrap();
            events(&mut clients[1], &bytes, 0);
        }
        clients[1].leave_ordering(&mut bytes).unwrap();
        events(&mut clients[1], &bytes, 100);
        clients[0].pull_ordering(1, &mut bytes).unwrap();
        let mut houses = [None; 30];
        houses[..2].copy_from_slice(&[Some(0), Some(1)]);
        let mut session = SequencedSession::with_roster(houses, 0, [7; 32], 30);
        for event in events(&mut clients[0], &bytes, 300) {
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
