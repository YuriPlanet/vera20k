//! The phase 2 headless client: the production `ClientSession` decides every
//! UPLINK, fallback PULL and IO heartbeat (`poll_uplink`,
//! `poll_repair_pull`, `heartbeat_uplink`, `next_uplink_deadline`; spec §6.7).
//! This file supplies only what spec §6.7 gives the sequencing layer and the
//! transport: content (batches, checksums, pace), the record stream reader,
//! the gap report, READY_REQUEST before the start (§6.0), PULL while
//! restoring (§7.7), the hop keepalive and the renewal handshake.

use std::collections::HashMap;

use super::client::{ClientLog, HeadlessClient, Outgoing};
use super::reader::{Record, StreamReader};
use crate::{
    CHECKSUM_INTERVAL, ClientLinkStats, ClientSession, ORDERING_BATCH_CAPACITY, OrderEvent,
    OrderPace, OrderingGap, RelayError, ServerEvent,
};

/// Spec §6.0: READY_REQUEST every 125 ms until `started`.
const CONFIGURE_INTERVAL_MS: u64 = 125;
/// Spec §6.3 pace segment: one measurement a second.
const PACE_INTERVAL_MS: u64 = 1_000;
/// The PULL spacing while restoring (spec §7.7).
const RESTORE_PULL_MS: u64 = 100;
/// Hop keepalive and handshake retries, src/net/udp.rs line 1370-1374 (in
/// 16 ms buckets), and its 10 s readiness timeout (line 1361-1365). Spec
/// appendix A.4 keeps the 2 s PING.
const PING_INTERVAL_MS: u64 = 125 * 16;
const HELLO_INTERVAL_MS: u64 = 62 * 16;
const FORGET_READINESS_MS: u64 = 625 * 16;
/// LEAVE retries, src/net/udp.rs line 1339-1341.
const LEAVE_RETRY_MS: u64 = 16;
/// The pace report's tick cost, spec appendix A.4 packet 30.
const TICK_MICROS: u32 = 2_100;

fn due(last: Option<u64>, now: u64, interval: u64) -> bool {
    last.is_none_or(|last| now.saturating_sub(last) >= interval)
}

pub struct AdaptiveClient {
    session: ClientSession,
    candidate: Option<ClientSession>,
    hop: usize,
    source: u8,
    certificate: Vec<u8>,
    payload_key: [u8; 32],
    checksum_fault: Option<u32>,
    command_bytes: usize,
    fill: u8,
    last_probe: Option<u64>,
    last_candidate_probe: Option<u64>,
    last_transport_response: Option<u64>,
    reader: StreamReader,
    inbox: Vec<OrderEvent>,
    pace_fps: u16,
    last_configure: Option<u64>,
    started_at: Option<u64>,
    reported_next: u64,
    reported_gap: Option<OrderingGap>,
    restoring: bool,
    last_restore_pull: Option<u64>,
    queued: Vec<u64>,
    batch_issues: HashMap<u64, Vec<u64>>,
    frame_issues: HashMap<u32, Vec<u64>>,
    executed_frame: u32,
    dispatched: u32,
    last_pace: Option<u64>,
    samples_due: Vec<u32>,
    leaving: bool,
    last_leave: Option<u64>,
    left: bool,
    out: Vec<Outgoing>,
    services: u64,
    in_service: bool,
    pub log: ClientLog,
}

impl AdaptiveClient {
    pub fn new(
        session: ClientSession,
        certificate: Vec<u8>,
        payload_key: [u8; 32],
        fps: u16,
        command_bytes: usize,
        checksum_fault: Option<u32>,
    ) -> Self {
        Self {
            source: session.source(),
            session,
            candidate: None,
            hop: 0,
            certificate,
            payload_key,
            checksum_fault,
            command_bytes,
            fill: 0,
            last_probe: None,
            last_candidate_probe: None,
            last_transport_response: None,
            reader: StreamReader::default(),
            inbox: Vec::new(),
            pace_fps: fps,
            last_configure: None,
            started_at: None,
            reported_next: 0,
            reported_gap: None,
            restoring: false,
            last_restore_pull: None,
            queued: Vec::new(),
            batch_issues: HashMap::new(),
            frame_issues: HashMap::new(),
            executed_frame: 0,
            dispatched: 0,
            last_pace: None,
            samples_due: Vec::new(),
            leaving: false,
            last_leave: None,
            left: false,
            out: Vec::new(),
            services: 0,
            in_service: false,
            log: ClientLog::default(),
        }
    }

    fn push(&mut self, bytes: Vec<u8>, hop: usize) {
        self.out.push(Outgoing {
            bytes,
            hop,
            service: self.in_service.then_some(self.services),
            heartbeat: false,
        });
    }

    fn sent(&mut self, result: Result<(), RelayError>, bytes: Vec<u8>) {
        match result {
            Ok(()) => self.push(bytes, self.hop),
            Err(error) => {
                *self
                    .log
                    .errors
                    .entry(format!("send {error:?}"))
                    .or_default() += 1
            }
        }
    }

    fn checksum(&self, frame: u32) -> u32 {
        let value = frame.wrapping_mul(0x9E37_79B1);
        if self.checksum_fault == Some(frame) {
            value ^ 1
        } else {
            value
        }
    }

    fn turn_ms(&self) -> u64 {
        u64::from(crate::turn_ms(self.pace_fps))
    }

    fn handle(&mut self, event: OrderEvent) {
        match event {
            OrderEvent::Status(status) => self.reader.status(&status),
            OrderEvent::Entry(entry) => self.reader.insert(&entry),
            OrderEvent::Error { code, .. } => {
                *self.log.errors.entry(format!("{code:?}")).or_default() += 1;
            }
            OrderEvent::Left => self.left = true,
            _ => {}
        }
    }

    /// Reads the contiguous records; a frame runs when its frame end is read.
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
                } => {
                    self.pace_fps = frames_per_second;
                    self.session
                        .set_ordering_turn_ms(crate::turn_ms(frames_per_second));
                }
                _ => {}
            }
        }
        if advanced {
            self.log.advances.push(at_us);
        }
    }

    /// The sequencing layer's content (spec §6.7): one batch of every queued
    /// command, checksum samples as frames run, pace once a second.
    fn content(&mut self, now: u64, at_us: u64) {
        if self.executed_frame == 0 || self.restoring {
            return;
        }
        if !self.queued.is_empty() {
            let fits = ORDERING_BATCH_CAPACITY / self.command_bytes;
            let take = self.queued.len().min(fits);
            // A batch is its command records alone, no version byte or
            // FRAME_INFO (bandwidth-compaction spec §4).
            let len = take * self.command_bytes;
            let mut bytes = Vec::with_capacity(len);
            while bytes.len() < len {
                self.fill = self.fill.wrapping_add(1);
                bytes.push(self.fill);
            }
            match self.session.seal_ordering_batch(&bytes) {
                Ok(id) => {
                    self.log.seals.push((at_us, id));
                    self.log.plaintexts.push((id, bytes));
                    let issued = self.queued.drain(..take).collect();
                    self.batch_issues.insert(id, issued);
                }
                Err(RelayError::Full) => {}
                Err(error) => {
                    *self
                        .log
                        .errors
                        .entry(format!("seal {error:?}"))
                        .or_default() += 1
                }
            }
        }
        for frame in std::mem::take(&mut self.samples_due) {
            let mut samples = Vec::new();
            if frame > CHECKSUM_INTERVAL {
                let previous = frame - CHECKSUM_INTERVAL;
                samples.push((previous, self.checksum(previous)));
            }
            samples.push((frame, self.checksum(frame)));
            if let Err(error) = self.session.queue_ordering_checksums(&samples, now) {
                *self
                    .log
                    .errors
                    .entry(format!("checksum {error:?}"))
                    .or_default() += 1;
            }
        }
        if due(self.last_pace, now, PACE_INTERVAL_MS) {
            let ran = self.last_pace.map_or(self.pace_fps, |last| {
                (u64::from(self.dispatched) * 1000 / now.saturating_sub(last).max(1))
                    .min(u64::from(u16::MAX)) as u16
            });
            self.last_pace = Some(now);
            self.dispatched = 0;
            self.session
                .queue_ordering_pace(OrderPace::KeepingUp, ran, Some(TICK_MICROS));
        }
    }

    fn serve(&mut self, now: u64, at_us: u64) {
        if self.left {
            return;
        }
        // The transport: keepalive, readiness timeout, renewal handshake
        // retries (src/net/udp.rs line 1361-1405).
        if self
            .last_transport_response
            .is_some_and(|last| now.saturating_sub(last) > FORGET_READINESS_MS)
        {
            self.session.forget_readiness();
        }
        let interval = if self.session.is_ready() {
            PING_INTERVAL_MS
        } else {
            HELLO_INTERVAL_MS
        };
        if due(self.last_probe, now, interval) {
            self.last_probe = Some(now);
            let mut out = Vec::new();
            let result = self.session.probe_at(now, &mut out);
            self.sent(result, out);
        }
        if self.candidate.is_some() && due(self.last_candidate_probe, now, HELLO_INTERVAL_MS) {
            self.last_candidate_probe = Some(now);
            let mut out = Vec::new();
            if self
                .candidate
                .as_mut()
                .unwrap()
                .probe_at(now, &mut out)
                .is_ok()
            {
                self.push(out, self.hop + 1);
            }
        }
        if self.leaving {
            // No UPLINK follows a Leave (spec §11 row 8).
            if due(self.last_leave, now, LEAVE_RETRY_MS) {
                self.last_leave = Some(now);
                let mut out = Vec::new();
                let result = self.session.leave_ordering_at(now, &mut out);
                self.sent(result, out);
            }
            return;
        }
        for event in std::mem::take(&mut self.inbox) {
            self.handle(event);
        }
        self.assemble(at_us);
        if !self.reader.started() {
            if self.session.is_ready() && due(self.last_configure, now, CONFIGURE_INTERVAL_MS) {
                self.last_configure = Some(now);
                let mut out = Vec::new();
                let result =
                    self.session
                        .configure_ordering_at(&self.certificate, [7; 32], now, &mut out);
                self.sent(result, out);
            }
            return;
        }
        self.started_at.get_or_insert(now);
        if self.last_pace.is_none() {
            // The first measurement is a second after the start.
            self.last_pace = Some(now);
        }
        // The stream reader's reports (spec §2 `recv_next`, §6.3 row 2).
        if self.reader.next() != self.reported_next {
            self.reported_next = self.reader.next();
            self.session.set_ordering_received_next(self.reported_next);
        }
        let gap = self.reader.gap_kind();
        if gap != self.reported_gap {
            self.reported_gap = gap;
            self.session.set_ordering_gap(gap, now);
        }
        let turn_ms = self.turn_ms();
        let rttvar = u64::from(self.session.link_stats().rttvar_ms.unwrap_or(50));
        self.log
            .gap_triggers
            .observe(gap, now, turn_ms, rttvar, at_us);
        // Spec §6.7 item 7: while restoring, PULL every 100 ms only while
        // a gap remains.
        if self.restoring && gap.is_some() && due(self.last_restore_pull, now, RESTORE_PULL_MS) {
            self.last_restore_pull = Some(now);
            let mut out = Vec::new();
            let result = self
                .session
                .pull_ordering_at(self.reader.next(), now, &mut out);
            self.sent(result, out);
        }
        self.content(now, at_us);
        // One poll per service: at most one UPLINK (spec §6.3).
        let mut out = Vec::new();
        if self.session.poll_uplink(now, &mut out) {
            self.push(out, self.hop);
        }
        let mut out = Vec::new();
        if self.session.poll_repair_pull(now, &mut out) {
            self.push(out, self.hop);
            self.log.repair_pulls.push(at_us);
        }
        let mut out = Vec::new();
        if self.session.heartbeat_uplink(now, &mut out) {
            self.push(out, self.hop);
            self.out.last_mut().unwrap().heartbeat = true;
        }
    }

    fn receive_datagram(&mut self, bytes: &[u8], now: u64) {
        // A renewal candidate reads first, as src/net/udp.rs line 1578-1614.
        if let Some(candidate) = self.candidate.as_mut()
            && let Ok(event) = candidate.receive_at(bytes, now)
        {
            match event {
                ServerEvent::Challenge(cookie) => {
                    let cookie = cookie.to_vec();
                    let mut out = Vec::new();
                    if candidate.confirm_at(&cookie, now, &mut out).is_ok() {
                        self.push(out, self.hop + 1);
                    }
                }
                ServerEvent::Ready => {
                    let mut next = self.candidate.take().expect("candidate");
                    match next.adopt_ordering_from(&mut self.session) {
                        Ok(()) => {
                            self.session = next;
                            self.hop += 1;
                            self.last_transport_response = Some(now);
                        }
                        Err(error) => {
                            *self
                                .log
                                .errors
                                .entry(format!("adopt {error:?}"))
                                .or_default() += 1
                        }
                    }
                }
                _ => {
                    *self
                        .log
                        .refused
                        .entry("candidate event".into())
                        .or_default() += 1
                }
            }
            return;
        }
        match self.session.receive_at(bytes, now) {
            Ok(ServerEvent::Challenge(cookie)) => {
                let cookie = cookie.to_vec();
                let mut out = Vec::new();
                let result = self.session.confirm_at(&cookie, now, &mut out);
                self.sent(result, out);
            }
            Ok(ServerEvent::Ready) => self.last_transport_response = Some(now),
            Ok(ServerEvent::Ordering(event)) => {
                self.last_transport_response = Some(now);
                self.inbox.push(event);
            }
            Ok(ServerEvent::OrderingPack(events)) => {
                self.last_transport_response = Some(now);
                self.inbox.extend(events);
            }
            Ok(ServerEvent::Payload { .. }) => self.last_transport_response = Some(now),
            Err(error) => *self.log.refused.entry(format!("{error:?}")).or_default() += 1,
        }
    }
}

impl HeadlessClient for AdaptiveClient {
    fn deliver(&mut self, bytes: &[u8], now_ms: u64, _at_us: u64) {
        self.receive_datagram(bytes, now_ms);
    }
    fn tick(&mut self, now_ms: u64, at_us: u64) {
        self.services += 1;
        self.in_service = true;
        self.serve(now_ms, at_us);
        self.in_service = false;
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
    fn log(&self) -> &ClientLog {
        &self.log
    }
    fn wakeups(&self) -> Vec<u64> {
        let mut wakeups = Vec::new();
        if self.left || self.leaving {
            return wakeups;
        }
        if !self.reader.started() {
            if let Some(last) = self.last_configure {
                wakeups.push(last + CONFIGURE_INTERVAL_MS);
            }
            return wakeups;
        }
        wakeups.extend(self.session.next_uplink_deadline());
        wakeups.extend(self.last_pace.map(|last| last + PACE_INTERVAL_MS));
        wakeups
    }
    fn leave(&mut self) {
        self.leaving = true;
    }
    fn renew(&mut self, ticket: &[u8], packet_key: [u8; 32], now_ms: u64) {
        match self.session.renewal(ticket, packet_key, self.payload_key) {
            Ok(mut candidate) => {
                let mut out = Vec::new();
                if candidate.probe_at(now_ms, &mut out).is_ok() {
                    self.push(out, self.hop + 1);
                }
                self.last_candidate_probe = Some(now_ms);
                self.candidate = Some(candidate);
            }
            Err(error) => {
                *self
                    .log
                    .errors
                    .entry(format!("renew {error:?}"))
                    .or_default() += 1
            }
        }
    }
    fn set_restoring(&mut self, restoring: bool) {
        self.restoring = restoring;
        self.session.set_ordering_restore(restoring);
    }
    fn mac_failures(&self) -> u64 {
        self.session.mac_failures()
            + self
                .candidate
                .as_ref()
                .map_or(0, |candidate| candidate.mac_failures())
    }
    fn link_stats(&self) -> Option<ClientLinkStats> {
        Some(self.session.link_stats())
    }
}
