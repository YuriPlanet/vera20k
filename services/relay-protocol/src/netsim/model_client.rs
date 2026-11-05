//! Phase 1's only model client (e2e-plan §1.2): the current production
//! client's sending policy, rebuilt here because that policy lives in the
//! game (`src/net/live_commands/sequenced.rs`, `SequencedSession`) and its
//! transport (`src/net/udp.rs`). Constants and order are copied from those
//! files; each cites its line. Phase 2 replaces it with the production
//! `ClientSession` scheduler and keeps this one only for the baseline.
//!
//! It does no IO: the simulation hands it received datagrams and service
//! times, and takes the datagrams it would send.
//!
//! Left out: pause requests, rate-marker playout, hop renewal and rejoin.
//! It never ends the match: fatal errors (HistoryGap, a DESYNC record) are
//! counted and play goes on, so a run always measures its whole window.

use std::collections::{BTreeMap, HashMap, VecDeque};

use super::client::{HeadlessClient, Outgoing};
use super::reader::{GapTriggers, Record, StreamReader};
use crate::{
    CHECKSUM_INTERVAL, ClientSession, ORDERING_BATCH_CAPACITY, OrderError, OrderEvent, OrderPace,
    ServerEvent,
};

// src/net/live_commands/sequenced.rs
/// Line 28: push opt-in retry while no PushState arrived.
const PUSH_PROBE_INTERVAL_MS: u64 = 500;
/// Line 29: after three unanswered opt-ins.
const PUSH_REPROBE_INTERVAL_MS: u64 = 5_000;
/// Line 30: push watermark refresh.
const PUSH_WATERMARK_INTERVAL_MS: u64 = 100;
/// Line 798: a watermark this far behind goes at once.
const PUSH_WATERMARK_LAG: u64 = 32;
/// Line 31: repair Pull spacing while push is active.
const REPLAY_RETRY_INTERVAL_MS: u64 = 100;
/// Line 836: Pull spacing after the start until push is active.
const STARTED_PULL_INTERVAL_MS: u64 = 16;
/// Line 838: READY_REQUEST + Pull spacing before the start.
const CONFIGURE_INTERVAL_MS: u64 = 125;
/// Line 32: no ordering reply for this long starts repair Pulls.
const PUSH_SILENCE_REPAIR_MS: u64 = 1_000;
/// Line 33 and 34: pause capability probe, slower once a relay never answered.
const PAUSE_PROBE_INTERVAL_MS: u64 = 500;
const PAUSE_COMPATIBILITY_PROBE_INTERVAL_MS: u64 = 5_000;
/// Line 41: pace report spacing.
const PACE_REPORT_INTERVAL_MS: u64 = 1_000;
/// Line 678: an unacknowledged batch is sent again this often.
const SUBMIT_RESEND_MS: u64 = 100;
/// Line 683: a batch's first transmission goes out twice.
const FIRST_COPIES: usize = 2;
/// Line 27 and 1464: a batch's plaintext starts with its version byte.
const BATCH_VERSION: u8 = 2;
/// A constant simulation tick cost for the VO2C pace report, in µs.
const TICK_MICROS: u32 = 2_000;

// src/net/udp.rs, in 16 ms wall buckets.
/// Line 1370-1374: PING every 125 buckets once ready, HELLO every 62 before.
const PING_INTERVAL_MS: u64 = 125 * 16;
const HELLO_INTERVAL_MS: u64 = 62 * 16;
/// Line 1361-1365: no relay datagram for 625 buckets forgets readiness.
const FORGET_READINESS_MS: u64 = 625 * 16;

/// src/net/live_commands/sequenced.rs line 245, `service_due`.
fn due(last: Option<u64>, now: u64, interval: u64) -> bool {
    last.is_none_or(|last| now.saturating_sub(last) >= interval)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Push {
    Probing { attempts: u8 },
    Active,
    PullOnly,
}

#[derive(Debug)]
struct Submission {
    id: u64,
    bytes: Vec<u8>,
    last_sent: Option<u64>,
}

/// What the model did, timestamped in virtual µs for the measurement window.
#[derive(Debug, Default)]
pub struct ModelLog {
    /// A gap outlived its one-frame grace while push was active
    /// (`delivery.gaps`, line 826).
    pub gaps: Vec<u64>,
    /// Pulls sent while push was active (`delivery.repair_pulls`, line 849).
    pub repair_pulls: Vec<u64>,
    /// Transmissions of a batch after its first (line 683).
    pub resends: Vec<u64>,
    /// (issue time, execution time) of each command, both in µs.
    pub commands: Vec<(u64, u64)>,
    /// Times frame ends advanced the executed frame.
    pub advances: Vec<u64>,
    pub errors: BTreeMap<String, u64>,
    /// Datagrams `ClientSession::receive` refused (duplicates, replays).
    pub refused: BTreeMap<String, u64>,
    /// Spec §6.3 row 2 triggers this stream would make.
    pub gap_triggers: GapTriggers,
    /// (time µs, batch id) of each batch sealed (adaptive client).
    pub seals: Vec<(u64, u64)>,
}

pub struct ModelClient {
    session: ClientSession,
    source: u8,
    certificate: Vec<u8>,
    pace_fps: u16,
    checksum_fault: Option<u32>,
    // src/net/udp.rs relay route.
    last_probe: Option<u64>,
    last_transport_response: Option<u64>,
    confirmations: u64,
    // src/net/live_commands/sequenced.rs SequencedSession.
    reader: StreamReader,
    inbox: Vec<OrderEvent>,
    last_response: Option<u64>,
    push: Push,
    last_push_control: Option<u64>,
    push_watermark: u64,
    push_confirmations: u64,
    repair_gap_since: Option<u64>,
    gap_counted: bool,
    last_control: Option<u64>,
    submissions: VecDeque<Submission>,
    next_batch: u64,
    accepted_batch: u64,
    pause_relay_supported: bool,
    pause_supported_sources: u32,
    pause_active_sources: u32,
    last_pause_control: Option<u64>,
    pause_probe_attempts: u8,
    pace_last_report: Option<u64>,
    dispatched: u32,
    executed_frame: u32,
    samples_due: Vec<u32>,
    // Content and observation.
    queued: Vec<u64>,
    batch_issues: HashMap<u64, Vec<u64>>,
    frame_issues: HashMap<u32, Vec<u64>>,
    command_bytes: usize,
    fill: u8,
    out: Vec<Outgoing>,
    services: u64,
    in_service: bool,
    pub log: ModelLog,
}

impl ModelClient {
    pub fn new(
        session: ClientSession,
        certificate: Vec<u8>,
        players: u8,
        fps: u16,
        command_bytes: usize,
        checksum_fault: Option<u32>,
    ) -> Self {
        Self {
            source: session.source(),
            session,
            certificate,
            pace_fps: fps,
            checksum_fault,
            last_probe: None,
            last_transport_response: None,
            confirmations: 0,
            reader: StreamReader::default(),
            inbox: Vec::new(),
            last_response: None,
            push: Push::Probing { attempts: 0 },
            last_push_control: None,
            push_watermark: 1,
            push_confirmations: 0,
            repair_gap_since: None,
            gap_counted: false,
            last_control: None,
            submissions: VecDeque::new(),
            next_batch: 1,
            accepted_batch: 0,
            pause_relay_supported: false,
            pause_supported_sources: 0,
            pause_active_sources: (1u32 << players) - 1,
            last_pause_control: None,
            pause_probe_attempts: 0,
            pace_last_report: None,
            dispatched: 0,
            executed_frame: 0,
            samples_due: Vec::new(),
            queued: Vec::new(),
            batch_issues: HashMap::new(),
            frame_issues: HashMap::new(),
            command_bytes,
            fill: 0,
            out: Vec::new(),
            services: 0,
            in_service: false,
            log: ModelLog::default(),
        }
    }

    fn send(&mut self, result: Result<(), crate::RelayError>, bytes: Vec<u8>) -> bool {
        if result.is_ok() {
            self.out.push(Outgoing {
                bytes,
                hop: 0,
                service: self.in_service.then_some(self.services),
                heartbeat: false,
            });
        }
        result.is_ok()
    }

    /// The transport's receive path (src/net/udp.rs line 1618-1665): a
    /// challenge is confirmed at once; ordering events wait for the game's
    /// next service.
    fn receive_datagram(&mut self, bytes: &[u8], now_ms: u64) {
        let mut out = Vec::new();
        match self.session.receive(bytes) {
            Ok(ServerEvent::Challenge(cookie)) => {
                let cookie = cookie.to_vec();
                let result = self.session.confirm(&cookie, &mut out);
                self.send(result, out);
            }
            Ok(ServerEvent::Ready) => {
                self.last_transport_response = Some(now_ms);
                self.confirmations += 1;
            }
            Ok(ServerEvent::Ordering(event)) => {
                self.last_transport_response = Some(now_ms);
                self.inbox.push(event);
            }
            Ok(ServerEvent::OrderingPack(events)) => {
                self.last_transport_response = Some(now_ms);
                self.inbox.extend(events);
            }
            Ok(ServerEvent::Payload { .. }) => self.last_transport_response = Some(now_ms),
            Err(error) => *self.log.refused.entry(format!("{error:?}")).or_default() += 1,
        }
    }

    /// One game-loop pass: the transport keepalive, `service` and
    /// `service_pending_commands` (src/net/live_commands.rs line 465-494).
    /// `now_ms` is the 16 ms wall bucket in ms, as production passes it
    /// (`now_wall_bucket.wrapping_shl(4)`, line 472).
    fn serve(&mut self, now_ms: u64, at_us: u64) {
        self.services += 1;
        self.in_service = true;
        self.service_transport(now_ms);
        self.service(now_ms, at_us);
        self.flush(now_ms, at_us);
        self.in_service = false;
    }

    /// src/net/udp.rs line 1361-1405.
    fn service_transport(&mut self, now_ms: u64) {
        if self
            .last_transport_response
            .is_some_and(|last| now_ms.saturating_sub(last) > FORGET_READINESS_MS)
        {
            self.session.forget_readiness();
        }
        let interval = if self.session.is_ready() {
            PING_INTERVAL_MS
        } else {
            HELLO_INTERVAL_MS
        };
        if due(self.last_probe, now_ms, interval) {
            self.last_probe = Some(now_ms);
            let mut out = Vec::new();
            let result = self.session.probe(&mut out);
            self.send(result, out);
        }
    }

    /// `SequencedSession::service`, line 623-702.
    fn service(&mut self, now: u64, at_us: u64) {
        for event in std::mem::take(&mut self.inbox) {
            self.last_response = Some(now);
            self.receive(event);
        }
        self.assemble(at_us);
        let turn_ms = u64::from(crate::turn_ms(self.pace_fps));
        self.log
            .gap_triggers
            .observe(self.reader.gap_kind(), now, turn_ms, at_us);
        self.service_delivery(now);
        self.service_pause(now);
        self.service_pace(now);
        for index in 0..self.submissions.len() {
            let pending = &self.submissions[index];
            if pending.id > self.accepted_batch && due(pending.last_sent, now, SUBMIT_RESEND_MS) {
                let copies = if pending.last_sent.is_none() {
                    FIRST_COPIES
                } else {
                    self.log.resends.push(at_us);
                    1
                };
                let (id, bytes) = (pending.id, pending.bytes.clone());
                let mut sent = false;
                for _ in 0..copies {
                    let mut out = Vec::new();
                    let result = self.session.submit_ordering(id, &bytes, &mut out);
                    sent |= self.send(result, out);
                }
                if sent {
                    self.submissions[index].last_sent = Some(now);
                }
            }
        }
    }

    /// `SequencedSession::receive`, line 911-1046.
    fn receive(&mut self, event: OrderEvent) {
        match event {
            OrderEvent::Status(status) => self.reader.status(&status),
            OrderEvent::PauseStatus {
                supported_sources,
                active_sources,
                ..
            } => {
                self.pause_relay_supported = true;
                self.pause_supported_sources |= supported_sources;
                self.pause_active_sources &= active_sources;
            }
            OrderEvent::PushState { enabled, .. } => {
                self.push = if enabled {
                    Push::Active
                } else {
                    Push::PullOnly
                };
            }
            OrderEvent::Ack { batch_id, .. } => {
                self.accepted_batch = self.accepted_batch.max(batch_id);
            }
            OrderEvent::Entry(entry) => {
                if let crate::OrderEntry::Batch {
                    source, batch_id, ..
                } = &entry
                    && *source == self.source
                {
                    self.accepted_batch = self.accepted_batch.max(*batch_id);
                }
                self.reader.insert(&entry);
            }
            OrderEvent::Error { code, expected } => {
                *self.log.errors.entry(format!("{code:?}")).or_default() += 1;
                match code {
                    OrderError::NotConfigured => {
                        self.reader.forget_start();
                        self.last_control = None;
                        self.push = Push::Probing { attempts: 0 };
                        self.last_push_control = None;
                        self.repair_gap_since = None;
                    }
                    OrderError::BatchGap => {
                        let accepted = expected.saturating_sub(1);
                        self.accepted_batch = self.accepted_batch.max(accepted);
                        if expected > self.accepted_batch {
                            for pending in &mut self.submissions {
                                if pending.id >= expected {
                                    pending.last_sent = None;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            OrderEvent::Ready { .. } | OrderEvent::Left | OrderEvent::Unrecognized => {}
        }
    }

    /// Reads the contiguous records. A frame counts as executed when its
    /// frame end is read: this client has no playout buffer.
    fn assemble(&mut self, at_us: u64) {
        let mut advanced = false;
        while let Some(record) = self.reader.pop() {
            match record {
                Record::Batch {
                    frame,
                    source,
                    batch_id,
                } if source == self.source => {
                    if let Some(issues) = self.batch_issues.remove(&batch_id) {
                        self.frame_issues.entry(frame).or_default().extend(issues);
                    }
                }
                Record::FrameEnd { frame } => {
                    self.executed_frame = frame;
                    self.dispatched += 1;
                    advanced = true;
                    if frame % CHECKSUM_INTERVAL == 0 {
                        self.samples_due.push(frame);
                    }
                    for issued in self.frame_issues.remove(&frame).unwrap_or_default() {
                        self.log.commands.push((issued, at_us));
                    }
                }
                Record::Rate {
                    frames_per_second, ..
                } => self.pace_fps = frames_per_second,
                _ => {}
            }
        }
        if advanced {
            self.log.advances.push(at_us);
        }
    }

    /// `SequencedSession::service_delivery`, line 765-853.
    fn service_delivery(&mut self, now: u64) {
        let started = self.reader.started();
        let next = self.reader.next();
        if started {
            match self.push {
                Push::Probing { attempts } => {
                    let interval = if attempts < 3 {
                        PUSH_PROBE_INTERVAL_MS
                    } else {
                        PUSH_REPROBE_INTERVAL_MS
                    };
                    if due(self.last_push_control, now, interval) {
                        let mut out = Vec::new();
                        let result = self.session.set_ordering_push(true, next, &mut out);
                        if self.send(result, out) {
                            self.push = Push::Probing {
                                attempts: attempts.saturating_add(1),
                            };
                            self.last_push_control = Some(now);
                            self.push_watermark = next;
                            self.push_confirmations = self.confirmations;
                        }
                    }
                }
                Push::Active => {
                    if self.confirmations != self.push_confirmations
                        || next.saturating_sub(self.push_watermark) >= PUSH_WATERMARK_LAG
                        || due(self.last_push_control, now, PUSH_WATERMARK_INTERVAL_MS)
                    {
                        let mut out = Vec::new();
                        let result = self.session.set_ordering_push(true, next, &mut out);
                        if self.send(result, out) {
                            self.last_push_control = Some(now);
                            self.push_watermark = next;
                            self.push_confirmations = self.confirmations;
                        }
                    }
                }
                Push::PullOnly => {}
            }
        }
        let active_push = started && self.push == Push::Active;
        // Line 813-820: one nominal frame of grace for reordering.
        let gap_ready = if self.reader.gap() {
            let since = *self.repair_gap_since.get_or_insert(now);
            now - since >= 1000u64.div_ceil(u64::from(self.pace_fps))
        } else {
            self.repair_gap_since = None;
            self.gap_counted = false;
            false
        };
        if active_push && gap_ready && !self.gap_counted {
            self.gap_counted = true;
            self.log.gaps.push(now * 1000);
        }
        let silent = self
            .last_response
            .is_some_and(|last| now.saturating_sub(last) > PUSH_SILENCE_REPAIR_MS);
        let pull_needed = !active_push || gap_ready || silent;
        let interval = if active_push {
            REPLAY_RETRY_INTERVAL_MS
        } else if started {
            STARTED_PULL_INTERVAL_MS
        } else {
            CONFIGURE_INTERVAL_MS
        };
        if pull_needed && due(self.last_control, now, interval) {
            if !started {
                let mut out = Vec::new();
                let result = self
                    .session
                    .configure_ordering(&self.certificate, [7; 32], &mut out);
                self.send(result, out);
                self.session.set_ordering_restore(false);
            }
            let mut out = Vec::new();
            let result = self.session.pull_ordering(next, &mut out);
            if self.send(result, out) {
                self.last_control = Some(now);
                if active_push {
                    self.log.repair_pulls.push(now * 1000);
                }
            }
        }
    }

    /// `SequencedSession::service_pause`, line 704-729, without requests.
    fn service_pause(&mut self, now: u64) {
        if !self.reader.started() {
            return;
        }
        // `pause_status().available`, line 579-590.
        let available = self.pause_relay_supported
            && self.pause_active_sources != 0
            && self.pause_supported_sources & self.pause_active_sources
                == self.pause_active_sources;
        // Line 745-753.
        let interval = if !self.pause_relay_supported && self.pause_probe_attempts >= 3 {
            PAUSE_COMPATIBILITY_PROBE_INTERVAL_MS
        } else {
            PAUSE_PROBE_INTERVAL_MS
        };
        if !available && due(self.last_pause_control, now, interval) {
            let mut out = Vec::new();
            let result = self.session.control_ordering_pause(0, 0, false, &mut out);
            if self.send(result, out) {
                self.last_pause_control = Some(now);
                self.pause_probe_attempts = self.pause_probe_attempts.saturating_add(1);
            }
        }
    }

    /// `SequencedSession::service_pace` and `PaceMeter::report`, line
    /// 153-190 and 731-743. Frames run as they arrive, so it keeps up.
    fn service_pace(&mut self, now: u64) {
        if !self.reader.started() || !due(self.pace_last_report, now, PACE_REPORT_INTERVAL_MS) {
            return;
        }
        let ran = self.pace_last_report.map_or(self.pace_fps, |last| {
            (u64::from(self.dispatched) * 1000 / now.saturating_sub(last).max(1))
                .min(u64::from(u16::MAX)) as u16
        });
        self.pace_last_report = Some(now);
        self.dispatched = 0;
        let mut out = Vec::new();
        let result = self.session.report_ordering_pace(
            OrderPace::KeepingUp,
            ran,
            Some(TICK_MICROS),
            &mut out,
        );
        self.send(result, out);
    }

    fn checksum(&self, frame: u32) -> u32 {
        let value = frame.wrapping_mul(0x9E37_79B1);
        if self.checksum_fault == Some(frame) {
            value ^ 1
        } else {
            value
        }
    }

    /// `SequencedSession::flush_local_commands`, line 1378-1491: checksum
    /// samples, then one batch of every queued command, sent at once.
    fn flush(&mut self, now: u64, at_us: u64) {
        if !self.reader.started() || self.executed_frame == 0 {
            return;
        }
        while self
            .submissions
            .front()
            .is_some_and(|pending| pending.id <= self.accepted_batch)
        {
            self.submissions.pop_front();
        }
        // Line 1416-1432: the newest sample and the one before it.
        for frame in std::mem::take(&mut self.samples_due) {
            let mut reports = vec![(frame, self.checksum(frame))];
            if frame > CHECKSUM_INTERVAL {
                let previous = frame - CHECKSUM_INTERVAL;
                reports.push((previous, self.checksum(previous)));
            }
            let mut out = Vec::new();
            let result = self.session.report_ordering_checksums(&reports, &mut out);
            self.send(result, out);
        }
        if self.submissions.len() < ClientSession::ORDERING_SUBMISSION_WINDOW
            && !self.queued.is_empty()
        {
            let fits = ORDERING_BATCH_CAPACITY / self.command_bytes;
            let taken: Vec<u64> = self.queued.drain(..self.queued.len().min(fits)).collect();
            // `command_bytes` per command, the version byte included.
            let mut bytes = vec![BATCH_VERSION];
            for _ in 1..taken.len() * self.command_bytes {
                self.fill = self.fill.wrapping_add(1);
                bytes.push(self.fill);
            }
            let id = self.next_batch;
            self.next_batch += 1;
            self.batch_issues.insert(id, taken);
            self.submissions.push_back(Submission {
                id,
                bytes,
                last_sent: None,
            });
            self.service(now, at_us);
        }
    }
}

impl HeadlessClient for ModelClient {
    fn deliver(&mut self, bytes: &[u8], now_ms: u64, _at_us: u64) {
        self.receive_datagram(bytes, now_ms);
    }
    fn tick(&mut self, now_ms: u64, at_us: u64) {
        self.serve(now_ms, at_us);
    }
    fn take_output(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.out)
    }
    fn started(&self) -> bool {
        self.reader.started()
    }
    fn reader(&self) -> &StreamReader {
        &self.reader
    }
    fn queue_command(&mut self, at_us: u64) {
        self.queued.push(at_us);
    }
    fn log(&self) -> &ModelLog {
        &self.log
    }
}
