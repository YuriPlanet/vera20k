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
use crate::sim::command::{CommandRecord, ExitRecord};
use crate::sim::command_queue::FrameInfo;
use crate::sim::world::{NetworkCommandQueues, NetworkQueueContext, NetworkQueuePass};
use crate::util::time::Instant;

const BUFFERED_FRAMES: usize = 64;
const REORDER_WINDOW: u64 = 256;
const MAX_FRAME_RECORDS: usize = 4096;
const SAMPLE_HISTORY: usize = 4096;
const BATCH_HEADER: usize = 9;
const PUSH_PROBE_INTERVAL_MS: i32 = 500;
const PUSH_WATERMARK_INTERVAL_MS: i32 = 100;
const REPLAY_RETRY_INTERVAL_MS: i32 = 100;
const PUSH_SILENCE_REPAIR_MS: u32 = 1_000;

/// The socket's latest contiguous-frame receipt, independent of when the app
/// drains it. Presentation can pace these frames; only the sequencer admits
/// canonical commands. This clock never enters deterministic simulation.
#[derive(Debug, Clone, Copy)]
pub struct SequencedTiming {
    pub frame: u32,
    pub received_at: Instant,
    pub frames_per_second: u16,
}

#[derive(Debug)]
enum PushDelivery {
    Probing { attempts: u8 },
    Active,
    PullOnly,
}

#[derive(Debug)]
struct ReceivedEntry {
    entry: OrderEntry,
    received_at: Instant,
}

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
    first_service: Option<u32>,
    last_control: Option<u32>,
    push: PushDelivery,
    last_push_control: Option<u32>,
    push_watermark: u64,
    remote_next_sequence: u64,
    repair_gap_since: Option<u32>,
    next_sequence: u64,
    entries: BTreeMap<u64, ReceivedEntry>,
    contiguous_received_at: Option<Instant>,
    latest_frame_receipt: Option<SequencedTiming>,
    assembling: CommittedFrame,
    frames: VecDeque<CommittedFrame>,
    last_batch: [u64; 30],
    next_batch: u64,
    accepted_batch: u64,
    submissions: VecDeque<Submission>,
    sample_sent: Option<u32>,
    // Derived checksum samples from immutable post-Logic contexts, evicted in
    // frame order. Late reports outside this cache are not compared to aliases.
    samples: VecDeque<(u32, u32)>,
    current: Option<(NetworkQueueContext, bool)>,
    sent_commands: u64,
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
        let mask = houses
            .iter()
            .enumerate()
            .filter(|(_, h)| h.is_some())
            .fold(0, |m, (s, _)| m | (1 << s));
        let (identity, fps, count) = agreement.sequencing_identity()?;
        if mask != cert.sources_mask || fps != cert.tick_hz || count != mask.count_ones() as usize {
            return Err(StartupError::InvalidAdmission.into());
        }
        Ok(Self {
            houses,
            local_source,
            agreement: identity,
            fps: cert.tick_hz,
            started: false,
            head_frame: 0,
            error: None,
            last_response: None,
            first_service: None,
            last_control: None,
            push: PushDelivery::Probing { attempts: 0 },
            last_push_control: None,
            push_watermark: 1,
            remote_next_sequence: 1,
            repair_gap_since: None,
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
            next_batch: 1,
            accepted_batch: 0,
            submissions: VecDeque::new(),
            sample_sent: None,
            samples: VecDeque::new(),
            current: None,
            sent_commands: 0,
        })
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

    pub(super) fn timing(&self) -> Option<SequencedTiming> {
        matches!(self.push, PushDelivery::Active)
            .then_some(self.latest_frame_receipt)
            .flatten()
    }

    pub(super) fn service(&mut self, transport: &mut LanUdpTransport, now: u32) {
        self.first_service.get_or_insert(now);
        while let Some(received) = transport.pop_ordering_received_with_time() {
            self.last_response = Some(now);
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
        self.service_delivery(transport, now);
        for pending in &mut self.submissions {
            // An unready route or failed local send cannot start the remote
            // receipt timer. The next service retries the same retained bytes.
            if pending.id > self.accepted_batch
                && service_due(pending.last_sent, now, 100)
                && transport
                    .submit_ordering(pending.id, &pending.bytes)
                    .is_ok()
            {
                pending.last_sent = Some(now);
            }
        }
        // Network silence is recoverable. Keep the same stream cursor,
        // unacknowledged bytes and world until the route/lease returns. Neither
        // a timeout nor peer progress may manufacture an EXIT or fail a frame.
    }

    fn service_delivery(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if self.started {
            match &mut self.push {
                PushDelivery::Probing { attempts } => {
                    if service_due(self.last_push_control, now, PUSH_PROBE_INTERVAL_MS) {
                        if *attempts == 3 {
                            // Old relays reject unknown controls. Keep their
                            // established pull path after a bounded handshake.
                            self.push = PushDelivery::PullOnly;
                        } else if transport
                            .set_ordering_push(true, self.next_sequence)
                            .is_ok()
                        {
                            *attempts += 1;
                            self.last_push_control = Some(now);
                            self.push_watermark = self.next_sequence;
                        }
                    }
                }
                PushDelivery::Active => {
                    if (self.next_sequence.saturating_sub(self.push_watermark) >= 32
                        || service_due(self.last_push_control, now, PUSH_WATERMARK_INTERVAL_MS))
                        && transport
                            .set_ordering_push(true, self.next_sequence)
                            .is_ok()
                    {
                        self.last_push_control = Some(now);
                        self.push_watermark = self.next_sequence;
                    }
                }
                PushDelivery::PullOnly => {}
            }
        }
        let active_push = self.started && matches!(self.push, PushDelivery::Active);
        let gap = self.frames.len() < BUFFERED_FRAMES
            && (self.remote_next_sequence > self.next_sequence
                || self
                    .entries
                    .first_key_value()
                    .is_some_and(|(seq, _)| *seq > self.next_sequence));
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
            }
            if transport.pull_ordering(self.next_sequence).is_ok() {
                self.last_control = Some(now);
            }
        }
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
            OrderEvent::Status(status) => {
                if status.tick_hz != self.fps {
                    return Err(SequencedError::InvalidTimeline);
                }
                if status.oldest_seq > self.next_sequence {
                    return Err(SequencedError::HistoryExpired);
                }
                self.started |= status.started;
                self.head_frame = self.head_frame.max(status.head_frame);
                self.remote_next_sequence = self.remote_next_sequence.max(status.next_seq);
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
            OrderEvent::Ack { batch_id, .. } => {
                if batch_id == 0 {
                    return Err(SequencedError::InvalidTimeline);
                }
                self.accept_through(batch_id)?;
            }
            OrderEvent::Entry(entry) => {
                let sequence = match &entry {
                    OrderEntry::Batch {
                        sequence,
                        source,
                        batch_id,
                        ..
                    } => {
                        if *source == self.local_source {
                            self.accept_through(*batch_id)?;
                        }
                        *sequence
                    }
                    OrderEntry::FrameEnd { sequence, .. }
                    | OrderEntry::Departure { sequence, .. } => *sequence,
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
                    let (records, report) = decode_batch(bytes.as_bytes(), house, frame)?;
                    if self.assembling.records.len() + records.len() > MAX_FRAME_RECORDS {
                        return Err(SequencedError::QueueFull);
                    }
                    self.assembling.records.extend(records);
                    self.assembling.reports.push(report);
                }
                OrderEntry::Departure { frame, source, .. } => {
                    if frame != self.assembling.frame {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    let house =
                        self.houses[usize::from(source)].ok_or(SequencedError::InvalidTimeline)?;
                    self.assembling.records.push(
                        ExitRecord {
                            house_id: house,
                            frame,
                        }
                        .encode()
                        .map_err(|_| SequencedError::InvalidTimeline)?,
                    );
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
                    self.latest_frame_receipt = Some(SequencedTiming {
                        frame,
                        received_at: self.contiguous_received_at.expect("consumed frame end"),
                        frames_per_second: self.fps,
                    });
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
