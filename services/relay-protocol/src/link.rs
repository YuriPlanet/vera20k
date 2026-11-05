//! Adaptive hop transport between a client and its relay: per-hop link
//! measurement, the uplink scheduler and its observation interfaces. The
//! contract is `services/relay/adaptive-transport/spec.md` ("the spec").
//!
//! Every client entry point is a pure state machine step. Time is always the
//! caller's `now_ms: u64`, one monotonic millisecond clock for the whole
//! session (the spec's service clock of §6.5 and the socket clock of §4.4 are
//! this same clock); nothing here reads a system clock. A datagram's receive
//! time is the moment its socket read returned, not when a game thread
//! handled it.

mod codec;
mod measure;
mod uplink;

#[cfg(test)]
pub(crate) use codec::Uplink;
pub(crate) use codec::{
    FEEDBACK_LEN, FLAG_PROBE, FLAG_PUSH, FLAG_RESTORING, LINK_DELAYED, LINK_JITTERY, PaceSegment,
    Reader, UPLINK_REQUEST, append_feedback, decode_uplink, read_feedback,
};
pub(crate) use measure::{
    JitterMeter, LossCursor, LossStats, RttEstimator, StateClock, link_state, pto, reader_rtt,
};

use crate::{
    ClientSession, OrderError, OrderEvent, OrderPace, OrderStatus, RelayError, RelayServer,
    ServerEvent,
};

/// A direction's link state (spec §5.2), first match wins: offline, then
/// congested, then lossy, then good. The discriminant is the wire value of
/// `UPLINK.link` bits 0..1 (spec §3.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LinkState {
    /// None of the other states holds.
    #[default]
    Good = 0,
    /// `loss‰` is inside the lossy hysteresis band: entered at 5‰ or more,
    /// left at 2‰ or less, only after at least one fold (spec §4.3, §5.4).
    Lossy = 1,
    /// DELAYED holds, or (uplink only) a Backpressure error arrived in the
    /// last 2000 ms (spec §5.2, §5.3).
    Congested = 2,
    /// Client: no authenticated datagram of this hop for 1000 ms. Relay: no
    /// datagram of this reader's hop for 1500 ms (spec §5.1).
    Offline = 3,
}

impl LinkState {
    /// Index of this state in the per-state arrays of [`ClientLinkStats`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The receive window of one hop (spec §4.1): the highest authenticated hop
/// sequence received, and a bitmap where bit `i` set means `high - i` was
/// received. `high == 0` (with `bits == 0`) means nothing counted yet.
///
/// Not counted: the relay's CHALLENGE (server kind 0) and LEAVE_ACK (server
/// kind 5), whose envelope sequence echoes the client's, and the client's
/// HELLO. The relay's `up` window starts at the hop's CONFIRM; the client's
/// `down` window starts at the first READY.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecvWindow {
    pub high: u64,
    pub bits: u64,
}

/// The hop feedback the relay appends to ACK and STATUS (spec §3.3, §3.4),
/// read from its `up` window at the moment the reply was encoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HopFeedback {
    /// `up_high` / `up_bits`: the client hop sequences the relay received.
    pub up: RecvWindow,
    /// Milliseconds from the relay receiving datagram `up.high` to encoding
    /// this reply, saturated at `u16::MAX`. A saturated value is never used
    /// as an RTT sample (spec §11 row 17).
    pub echo_delay_ms: u16,
}

/// What a lost or missing stretch of the record stream looks like to the
/// sequencing layer, for uplink condition 2 (spec §6.3) and the fallback
/// PULL (spec §7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingGap {
    /// The reorder buffer holds a record with `seq > recv_next`: true
    /// reordering or loss. Condition 2 applies at once.
    Reordered,
    /// Nothing past `recv_next` is buffered, but a STATUS reported
    /// `next_seq > recv_next`. Condition 2 applies only once this has lasted
    /// `turn_ms` (a status heartbeat can overtake the same round's PACK).
    BehindStatus,
}

/// A snapshot of one client hop's uplink measurement and scheduler, and of
/// its downlink receipt (spec §4, §5, §10.1). Durations are milliseconds of
/// the caller's `now_ms` clock. State values are as of the last
/// `receive_at`, `poll_uplink` or `heartbeat_uplink` call; reading the
/// snapshot evaluates nothing.
///
/// Measurement state (`up_loss_permille`, `uplink`, `severe`, RTT fields and
/// `delayed`) carries across a hop renewal (spec §4.7); `down` and all the
/// per-hop sequence state start again on the new hop. The §10.1 counters
/// cover the whole match: they also carry across renewals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientLinkStats {
    /// The uplink's state (spec §5.2), as sent in `UPLINK.link` bits 0..1.
    pub uplink: LinkState,
    /// Smoothed uplink loss in per mille, updated at each fold of at least
    /// 32 judged packets (spec §4.3): `loss = (3 * loss + sample) / 4`.
    pub up_loss_permille: u16,
    /// Severe loss, a sub-band of lossy: entered at 150‰ or more, left at
    /// 100‰ or less (spec §5.2).
    pub severe: bool,
    /// RFC 6298 smoothed RTT in ms (spec §4.4); `None` before the first sample.
    pub srtt_ms: Option<u32>,
    /// RFC 6298 RTT variance in ms; `None` before the first sample.
    pub rttvar_ms: Option<u32>,
    /// Base RTT in ms, the minimum over the current and previous 10 s
    /// windows (spec §4.4); `None` before the first sample.
    pub base_ms: Option<u32>,
    /// RTT samples taken on this match (spec §4.4); DELAYED needs 8.
    pub rtt_samples: u64,
    /// Queueing delay judged (spec §5.3), sent as `UPLINK.link` bit 2.
    pub delayed: bool,
    /// The current RTO backoff exponent `k` (spec §4.5).
    pub rto_backoff: u32,

    // --- §10.1 uplink ---
    /// Folds of `up_loss_permille` so far; with `up_loss_permille_sum` gives
    /// the per-fold mean.
    pub up_loss_folds: u64,
    /// Sum of `up_loss_permille` after each fold.
    pub up_loss_permille_sum: u64,
    /// Highest `up_loss_permille` after any fold.
    pub up_loss_permille_max: u16,
    /// Milliseconds spent in each uplink state, indexed by [`LinkState::index`].
    pub uplink_state_ms: [u64; 4],
    /// Times the uplink entered each state, indexed by [`LinkState::index`].
    /// The initial Good state is not an entry.
    pub uplink_state_entries: [u64; 4],
    /// Lossy-state copies sent (spec §6.3 row 6).
    pub copies: u64,
    /// RTO expiries that resent unacknowledged batches (spec §6.3 row 4).
    pub rto_timeouts: u64,
    /// Highest RTO backoff exponent reached.
    pub rto_backoff_max: u32,

    // --- §10.1 downlink ---
    /// Authenticated datagrams counted into the `down` windows.
    pub datagrams_received: u64,
    /// Gaps reported through `set_ordering_gap` (a `None` to `Some` change).
    pub gaps: u64,
    /// Gaps that closed after an UPLINK was sent for them and before any
    /// fallback PULL: repaired by piggyback or relay retransmission.
    pub gaps_repaired_by_push: u64,
    /// Fallback PULLs sent by `poll_repair_pull` (spec §7.7).
    pub repair_pulls: u64,

    // --- §10.1 RTT ---
    /// Median of `srtt_ms` as it stood after each RTT sample; `None` before any.
    pub srtt_p50_ms: Option<u32>,
    /// 99th percentile of `srtt_ms` as it stood after each RTT sample.
    pub srtt_p99_ms: Option<u32>,
    /// Milliseconds during which `delayed` was true.
    pub delayed_ms: u64,

    // --- §10.1 jitter (spec §4.8) ---
    /// Smoothed arrival jitter J of new-turn packs, in µs.
    pub jitter_us: u64,
    /// Highest J so far, in µs.
    pub jitter_max_us: u64,
    /// JITTERY (`UPLINK.link` bit 3): J reached `max(10, turn_ms / 4)` ms
    /// after at least 16 samples, until it falls to half that.
    pub jittery: bool,
    /// Milliseconds during which JITTERY held, and times it was entered.
    pub jittery_ms: u64,
    pub jittery_entries: u64,

    /// This hop's downlink receive window (spec §4.1), the values the next
    /// UPLINK carries as `down_high` / `down_bits`.
    pub down: RecvWindow,
}

/// A snapshot of one reader's downlink at the relay (spec §7.1, §10.2), for
/// the newest admitted hop of that source. Counters are per hop: a renewed
/// hop is a new `Peer` whose counters start from zero, while it keeps the
/// measured loss, the state and their hold timers (spec §4.7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReaderLinkStats {
    /// Whether this hop holds a push subscription (an UPLINK with PUSH within
    /// the 3000 ms lease). The link fields below are zero or default without one.
    pub subscribed: bool,
    /// The downlink's state (spec §5.2), judged by the relay.
    pub downlink: LinkState,
    /// Smoothed downlink loss in per mille (spec §4.3), from the reader's
    /// `UPLINK.down_high` / `down_bits` against the relay's send log.
    pub down_loss_permille: u16,
    /// Severe downlink loss (spec §5.2).
    pub severe: bool,
    /// The current piggyback depth K (spec §7.2): 1 good (2 when JITTERY),
    /// 2 lossy, 3 severe, `min(K, 2)` congested; 0 while offline or
    /// unsubscribed.
    pub piggyback_depth: u8,
    /// Highest `recv_next` this reader acknowledged.
    pub acked_next: u64,
    /// The first record not yet pushed to this reader as a new pack.
    pub sent_next: u64,
    /// Ranges `[from, to)` queued for retransmission (spec §7.3).
    pub retransmit_ranges: usize,
    /// The current PTO backoff exponent `k` (spec §4.6).
    pub pto_backoff: u32,
    /// Datagrams sent to this hop by send-log kind (spec §4.2): 0 non-PACK,
    /// 1 new pack, 2 repeat, 3 retransmission, 4 tail probe.
    pub packets_by_kind: [u64; 5],
    /// Whole datagram bytes (envelope and MAC included) by the same kinds.
    pub bytes_by_kind: [u64; 5],
    /// The relay's `up` receive window for this hop (spec §4.1), the values
    /// its next ACK or STATUS carries as `up_high` / `up_bits`.
    pub up: RecvWindow,
    /// JITTERY from the reader's latest UPLINK (`link` bit 3, spec §7.2):
    /// while good it raises K to 2 and turns the 8 ms repeat on; it does
    /// not change `downlink`.
    pub jittery: bool,
}

impl ClientSession {
    // --- Timed datagrams: each records its send time (spec §4.2) or its
    // socket receive time (spec §4.4). ---

    /// The hop probe at `now_ms`: HELLO before readiness (not counted in any
    /// window, spec §4.1), otherwise a PING keepalive. A HELLO's sequence is
    /// what the CHALLENGE must echo.
    pub fn probe_at(&mut self, now_ms: u64, out: &mut Vec<u8>) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.probe(out)
    }

    /// CONFIRM at `now_ms`: returns the CHALLENGE's 40-byte `cookie`, with a
    /// federated match's roster. The relay's `up` window for this hop starts
    /// with this datagram (spec §4.1).
    pub fn confirm_at(
        &mut self,
        cookie: &[u8],
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.confirm(cookie, out)
    }

    /// A plaintext DATA datagram for `target` (a source or
    /// `BROADCAST`) at `now_ms`; `Unregistered` before the hop is ready.
    pub fn data_at(
        &mut self,
        target: u8,
        payload: &[u8],
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.data(target, payload, out)
    }

    /// READY_REQUEST (request 0, spec §6.0) at `now_ms`: the lobby-signed
    /// `certificate` and the start `agreement`, which must match any earlier
    /// call. Before `started` the caller resends it every 125 ms, with no
    /// PULL beside it; its
    /// STATUS replies give RTT samples and uplink loss feedback. Also sets
    /// `turn_ms` to [`crate::turn_ms`] of the certificate's rate until
    /// `set_ordering_turn_ms` changes it.
    pub fn configure_ordering_at(
        &mut self,
        certificate: &[u8],
        agreement: [u8; 32],
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.configure_ordering(certificate, agreement, out)
    }

    /// PULL (request 2) of the records from `next_seq` at `now_ms`, for the
    /// cases that pull directly (spec §7.7): before the start, while
    /// RESTORING, and catching up. Gap repair during play goes through
    /// `poll_repair_pull` instead.
    pub fn pull_ordering_at(
        &mut self,
        next_seq: u64,
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.pull_ordering(next_seq, out)
    }

    /// PAUSE (request 5) at `now_ms`. Request zero advertises pause support
    /// and asks for current status. Positive IDs are monotonic per source for
    /// this match, including across hop renewal; a retry keeps the exact ID,
    /// expected epoch and desired state.
    pub fn control_ordering_pause_at(
        &mut self,
        request_id: u64,
        expected_epoch: u64,
        paused: bool,
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.control_ordering_pause(request_id, expected_epoch, paused, out)
    }

    /// The terminal LEAVE (request 3) at `now_ms`. A
    /// retry returns the first call's cached bytes with its hop sequence, so
    /// only the first call is a new send. No UPLINK follows a Leave
    /// (spec §11 row 8): `poll_uplink` and `heartbeat_uplink` return false.
    pub fn leave_ordering_at(&mut self, now_ms: u64, out: &mut Vec<u8>) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        self.leave_ordering(out)
    }

    /// Authenticates and decodes one relay datagram; `now_ms` is the moment
    /// the socket read of `bytes` returned. An authenticated datagram updates
    /// the hop's `down` window (spec §4.1) and resets the 1000 ms offline
    /// timer (spec §5.1); an ACK or STATUS also takes an RTT sample (§4.4)
    /// and judges uplink loss (§4.3); an ACK, a BatchGap error or this
    /// source's own BATCH record advances `accepted_through` (§6.1, §6.4);
    /// Backpressure starts the 2000 ms congestion hold (§6.4). Leaving the
    /// offline state arms the immediate recovery UPLINK (§5.5). A STATUS
    /// with `started = 1` starts the match for the scheduler, and a PACK
    /// confirms this hop's push subscription (§6.0).
    pub fn receive_at<'a>(
        &mut self,
        bytes: &'a [u8],
        now_ms: u64,
    ) -> Result<ServerEvent<'a>, RelayError> {
        self.clock_ms = now_ms;
        let event = self.receive(bytes)?;
        self.note_ordering_receipt(
            matches!(
                &event,
                ServerEvent::Ordering(OrderEvent::Status(OrderStatus { started: true, .. }))
            ),
            matches!(&event, ServerEvent::OrderingPack(_)),
        );
        let feedback = match &event {
            ServerEvent::Ordering(OrderEvent::Ack { feedback, .. }) => Some(*feedback),
            ServerEvent::Ordering(OrderEvent::Status(status)) => {
                let (next_seq, feedback) = (status.next_seq, status.feedback);
                self.note_status_next(next_seq);
                Some(feedback)
            }
            ServerEvent::Ordering(OrderEvent::Error {
                code: OrderError::Backpressure,
                ..
            }) => {
                if let Some(ordering) = self.ordering.as_mut() {
                    ordering.link.backpressure_at = Some(now_ms);
                }
                None
            }
            _ => None,
        };
        // Leave the offline state first: its recovery skips the loss cursor
        // past everything sent before (spec §5.5).
        self.evaluate_uplink(now_ms);
        match &event {
            ServerEvent::OrderingPack(events) => self.measure_jitter(events, now_ms),
            ServerEvent::Ordering(OrderEvent::Status(_)) => {
                if let Some(ordering) = self.ordering.as_mut()
                    && std::mem::take(&mut ordering.link.pull_pending)
                {
                    ordering.link.pull_reply_ms = Some(now_ms);
                }
            }
            _ => {}
        }
        if let Some(feedback) = feedback {
            self.take_feedback(feedback, now_ms);
        }
        self.evaluate_uplink(now_ms);
        Ok(event)
    }

    // --- Content from the sequencing layer (spec §6.7). None of these
    // encodes or sends; `poll_uplink` decides when it goes out. ---

    /// Seals `plaintext` as the next batch (spec §6.1) and returns its ID:
    /// consecutive from 1 for this match, carried across hop renewals. The
    /// bytes stay fixed until the batch is acknowledged. The batch goes
    /// out with the next UPLINK (§6.2, §6.3 row 1).
    ///
    /// Errors: `Unregistered` before ordering is configured, before a STATUS
    /// with `started = 1` arrived, while restoring, or after Leave (spec §6.0:
    /// no batches before the start); `Malformed` for an empty plaintext or one
    /// over `ORDERING_BATCH_CAPACITY` bytes; `Full` while
    /// `ORDERING_SUBMISSION_WINDOW` (4) batches are unacknowledged.
    pub fn seal_ordering_batch(&mut self, plaintext: &[u8]) -> Result<u64, RelayError> {
        self.seal_next_batch(plaintext)
    }

    /// Hands over this PC's post-Logic checksums of sampled frames as
    /// `(frame, checksum)`, oldest first: the newest sample and the one
    /// before it (one when only one exists). They replace any samples not
    /// yet sent and ride the next UPLINK (spec §6.3, checksum segment). If no
    /// UPLINK has gone out 100 ms after `now_ms`, `poll_uplink` sends one for
    /// them alone, like row 3. `Malformed` for zero or more than two samples.
    pub fn queue_ordering_checksums(
        &mut self,
        samples: &[(u32, u32)],
        now_ms: u64,
    ) -> Result<(), RelayError> {
        self.queue_checksums(samples, now_ms)
    }

    /// Hands over one pace measurement (about once a second): `pace`, the
    /// frames run in the last second, and the 90th-percentile tick cost in
    /// microseconds (`None`, or `Some(0)`, means none; the wire carries 0).
    /// It rides only the next UPLINK, once, with flags bit1 PACE (spec §6.3,
    /// pace segment); a later measurement replaces one not yet sent.
    pub fn queue_ordering_pace(
        &mut self,
        pace: OrderPace,
        frames_per_second: u16,
        tick_micros: Option<u32>,
    ) {
        self.queue_pace(PaceSegment {
            pace: pace as u8,
            frames_per_second,
            tick_micros: tick_micros.unwrap_or(0),
        });
    }

    /// Sets the contiguous watermark `recv_next` (spec §2): every record
    /// before it is received. Never decreases (a lower value is ignored);
    /// must be at least 1. An advance makes an acknowledgement due after the
    /// state's interval (spec §6.3 row 3: 100 ms good, `turn_ms` lossy,
    /// 200 ms congested).
    pub fn set_ordering_received_next(&mut self, recv_next: u64) {
        if let Some(ordering) = self.ordering.as_mut() {
            let schedule = &mut ordering.link.schedule;
            schedule.recv_next = schedule.recv_next.max(recv_next);
        }
    }

    /// Reports the current gap state; call it whenever it changes, observed
    /// at `now_ms`. Repeating the same value changes nothing; `None` reports
    /// the gap closed. A new gap makes `poll_uplink` send an UPLINK once at
    /// least 16 ms passed since the previous one (spec §6.3 row 2; after the
    /// `turn_ms` grace for [`OrderingGap::BehindStatus`]), and starts the
    /// fallback PULL grace of `poll_repair_pull` (spec §7.7).
    pub fn set_ordering_gap(&mut self, gap: Option<OrderingGap>, now_ms: u64) {
        self.report_gap(gap, now_ms);
    }

    /// Sets the turn duration `turn_ms` in whole milliseconds (spec §2), the
    /// lossy acknowledgement interval (§6.3 row 3) and the `BehindStatus`
    /// grace. Pass [`crate::turn_ms`] of the current sealing rate; call again
    /// when a RATE record changes it.
    pub fn set_ordering_turn_ms(&mut self, turn_ms: u32) {
        if let Some(ordering) = self.ordering.as_mut() {
            ordering.link.schedule.turn_ms = turn_ms;
        }
    }

    // --- Scheduling: the state machine decides when to send. ---

    /// Encodes one UPLINK (request 8, spec §3.2) into `out` if §6.3 says one
    /// is due at `now_ms`, returning true; otherwise leaves `out` alone and
    /// returns false. Covers rows 1-7 of §6.3, the standalone checksum send,
    /// the first UPLINK once a STATUS with `started = 1` arrived and the
    /// subscription resends until this hop's first PACK (§6.0), and the
    /// immediate UPLINK on a renewed hop (§4.7); while offline the only
    /// UPLINK is the 250 ms PROBE. Every UPLINK but a probe carries all
    /// unacknowledged batches that fit from `accepted_through + 1` (§6.2).
    ///
    /// At most one UPLINK per service, with no exception (spec §6.3): call it
    /// once each service (about 16 ms); calling again at the same `now_ms`
    /// returns false and is harmless. When two lossy copies are due at once
    /// only one goes, and the next is due 16 ms after it.
    /// Returns false before the start, after Leave, when the hop is not
    /// ready, or if the hop counter is exhausted.
    pub fn poll_uplink(&mut self, now_ms: u64, out: &mut Vec<u8>) -> bool {
        self.poll_uplink_at(now_ms, out)
    }

    /// The earliest `now_ms` at which `poll_uplink` could return true without
    /// any further input (receive, content or gap report); it may already be
    /// past. `None` before the start or after Leave.
    pub fn next_uplink_deadline(&self) -> Option<u64> {
        self.uplink_deadline()
    }

    /// The transport's IO-thread heartbeat (spec §6.6). If no UPLINK went out
    /// on this hop for 250 ms by `now_ms`, re-encodes the most recent
    /// UPLINK into `out` with a new hop sequence and the current
    /// `down_high` / `down_bits`: its checksum segment repeats, its pace
    /// segment is dropped, and the batches are packed again by §6.2 from the
    /// current `accepted_through + 1`. While the uplink is offline it sends
    /// the probe instead (PROBE set, no batches or checksums). Returns true
    /// if it encoded one; false when not due, when this hop has sent no
    /// UPLINK yet, or after Leave. A heartbeat counts as an UPLINK for every
    /// later interval.
    pub fn heartbeat_uplink(&mut self, now_ms: u64, out: &mut Vec<u8>) -> bool {
        self.heartbeat_uplink_at(now_ms, out)
    }

    /// Encodes the fallback PULL (request 2, `next_seq = recv_next`) into
    /// `out` when a gap reported through `set_ordering_gap` has lasted
    /// `G = clamp(srtt + 4 * rttvar + turn_ms, 100, 1000)` ms without the
    /// watermark advancing, then again after each further `G` multiple, at
    /// most 1000 ms apart (spec §7.7). Before any RTT sample `G` uses srtt
    /// 100 and rttvar 50. While the newest STATUS `next_seq` is more than 64
    /// past `recv_next` it pulls at once and then every `G` instead. Never
    /// before the start or while RESTORING, where the sequencing layer pulls
    /// itself. Returns whether it encoded one.
    pub fn poll_repair_pull(&mut self, now_ms: u64, out: &mut Vec<u8>) -> bool {
        self.poll_repair(now_ms, out)
    }

    // --- Observation ---

    /// A snapshot of this hop's link measurement and the match's §10.1
    /// counters; see [`ClientLinkStats`].
    pub fn link_stats(&self) -> ClientLinkStats {
        let mut stats = ClientLinkStats {
            down: self.hop.down,
            datagrams_received: self.hop.received,
            ..ClientLinkStats::default()
        };
        let Some(ordering) = self.ordering.as_ref() else {
            return stats;
        };
        let link = &ordering.link;
        let (state_ms, delayed_ms) = link.clock.totals(link.clock.last_ms());
        stats.uplink = link.clock.state;
        stats.up_loss_permille = link.loss.permille;
        stats.severe = link.loss.severe;
        stats.srtt_ms = link.rtt.srtt;
        stats.rttvar_ms = link.rtt.srtt.map(|_| link.rtt.rttvar);
        stats.base_ms = link.rtt.base();
        stats.rtt_samples = link.rtt.samples;
        stats.delayed = link.rtt.delayed;
        stats.rto_backoff = link.rto_backoff;
        stats.up_loss_folds = link.loss.folds;
        stats.up_loss_permille_sum = link.loss.permille_sum;
        stats.up_loss_permille_max = link.loss.permille_max;
        stats.uplink_state_ms = state_ms;
        stats.uplink_state_entries = link.clock.entries;
        stats.copies = link.copies;
        stats.rto_timeouts = link.rto_timeouts;
        stats.rto_backoff_max = link.rto_backoff_max;
        stats.datagrams_received += link.earlier_hops_received;
        stats.gaps = link.gaps;
        stats.gaps_repaired_by_push = link.gaps_repaired_by_push;
        stats.repair_pulls = link.repair_pulls;
        stats.srtt_p50_ms = link.rtt.srtt_percentile(50);
        stats.srtt_p99_ms = link.rtt.srtt_percentile(99);
        stats.delayed_ms = delayed_ms;
        stats.jitter_us = link.jitter.jitter_us;
        stats.jitter_max_us = link.jitter.max_us;
        stats.jittery = link.jitter.jittery;
        stats.jittery_ms = link.jitter.jittery_total(link.clock.last_ms());
        stats.jittery_entries = link.jitter.entries;
        stats
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ClientSession {
    /// Test support only, for tests that must start from a good uplink,
    /// such as the replay of spec appendix A: presets the uplink as
    /// measured, one fold done at 0‰, so it is good instead of lossy before
    /// its first fold (spec §5.4). May be called before configuring the
    /// ordering or the start; later folds judge as usual.
    pub fn preset_uplink_measured_good(&mut self) {
        self.measured_good = true;
        if let Some(ordering) = self.ordering.as_mut() {
            ordering.link.loss = LossStats::measured_good();
        }
        if let Some(heard) = self.hop.last_heard_ms {
            self.evaluate_uplink(heard.max(self.clock_ms));
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl RelayServer {
    /// Test support only, for tests that must start from a good downlink,
    /// such as the replay of spec appendix A: presets the downlink of
    /// `source`'s hop in `room` as measured, one fold done at 0‰, so the
    /// reader is good instead of lossy before its first fold (spec §5.4).
    /// The hop must be admitted (after its CONFIRM); it may be before the
    /// reader subscribes. Later folds judge as usual. Returns false when
    /// the relay holds no such hop.
    pub fn preset_reader_measured_good(&mut self, room: [u8; 16], source: u8) -> bool {
        let Some(peer) = self.rooms.get_mut(&room).and_then(|room| {
            room.peers
                .iter_mut()
                .find(|peer| peer.ticket.source == source)
        }) else {
            return false;
        };
        peer.link.loss = LossStats::measured_good();
        let heard = peer.link.last_heard_ms;
        peer.evaluate_reader(heard);
        true
    }
}

impl RelayServer {
    /// A snapshot of the downlink of `source`'s newest admitted hop in
    /// `room`; see [`ReaderLinkStats`]. `None` when the relay holds no such
    /// room or no admitted hop for that source.
    pub fn reader_link_stats(&self, room: [u8; 16], source: u8) -> Option<ReaderLinkStats> {
        let peer = self
            .rooms
            .get(&room)?
            .peers
            .iter()
            .find(|peer| peer.ticket.source == source)?;
        let link = &peer.link;
        let mut stats = ReaderLinkStats {
            subscribed: peer.push.is_some(),
            downlink: link.clock.state,
            down_loss_permille: link.loss.permille,
            severe: link.loss.severe,
            packets_by_kind: link.packets_by_kind,
            bytes_by_kind: link.bytes_by_kind,
            up: link.up,
            jittery: link.jittery,
            ..ReaderLinkStats::default()
        };
        if let Some(push) = peer.push.as_ref() {
            stats.piggyback_depth = link.piggyback_depth();
            push.fill_stats(&mut stats);
        }
        Some(stats)
    }
}

// --- Internal state ---

/// Client hop sequence log length (spec §4.2).
const SENDLOG_CLIENT: usize = 128;
/// Relay per-reader send log length (spec §4.2).
const SENDLOG_READER: usize = 64;
pub(crate) const CLIENT_OFFLINE_MS: u64 = 1000;
pub(crate) const READER_OFFLINE_MS: u64 = 1500;
pub(crate) const BACKPRESSURE_HOLD_MS: u64 = 2000;
/// A congested direction stays congested until its conditions have not
/// held for this long (spec §5.2).
const CONGESTION_HOLD_MS: u64 = 10_000;

/// Send-log kinds (spec §4.2).
pub(crate) const SEND_OTHER: u8 = 0;
pub(crate) const SEND_FRESH: u8 = 1;
pub(crate) const SEND_REPEAT: u8 = 2;
pub(crate) const SEND_RETRANSMIT: u8 = 3;
pub(crate) const SEND_PROBE: u8 = 4;

#[derive(Debug, Clone, Copy, Default)]
struct ClientSend {
    sequence: u64,
    at_ms: u64,
}

/// One client hop's own measurement state (spec §4.1, §4.2, §4.3 steps
/// 1-3, §4.4 steps 1-2). A renewed hop starts with a fresh one (§4.7).
#[derive(Debug)]
pub(crate) struct HopLink {
    pub(crate) down: RecvWindow,
    log: Vec<ClientSend>,
    cursor: LossCursor,
    // The `up_high` of the last reply that gave an RTT sample.
    rtt_high: u64,
    pub(crate) last_heard_ms: Option<u64>,
    pub(crate) received: u64,
    // UPLINKs sent on this hop: the first goes at once (spec §4.7, §6.0).
    pub(crate) uplinks: u64,
    // A PACK arrived on this hop: its subscription is confirmed (spec §6.0).
    pub(crate) first_pack: bool,
    // The last `poll_uplink` send on this hop: one UPLINK per service. A
    // renewed hop's first UPLINK is its own (spec §4.7).
    pub(crate) last_service_ms: Option<u64>,
}

impl Default for HopLink {
    fn default() -> Self {
        Self {
            down: RecvWindow::default(),
            log: vec![ClientSend::default(); SENDLOG_CLIENT],
            cursor: LossCursor::default(),
            rtt_high: 0,
            last_heard_ms: None,
            received: 0,
            uplinks: 0,
            first_pack: false,
            last_service_ms: None,
        }
    }
}

impl HopLink {
    pub(crate) fn record_send(&mut self, sequence: u64, now_ms: u64) {
        self.log[(sequence % SENDLOG_CLIENT as u64) as usize] = ClientSend {
            sequence,
            at_ms: now_ms,
        };
    }

    fn sent(&self, sequence: u64) -> Option<ClientSend> {
        let entry = self.log[(sequence % SENDLOG_CLIENT as u64) as usize];
        (entry.sequence == sequence).then_some(entry)
    }
}

/// The client's path measurement and uplink statistics, carried across hop
/// renewals with the ordering state (spec §4.7).
#[derive(Debug, Clone, Default)]
pub(crate) struct ClientLink {
    pub(crate) loss: LossStats,
    pub(crate) rtt: RttEstimator,
    pub(crate) clock: StateClock,
    pub(crate) backpressure_at: Option<u64>,
    pub(crate) rto_backoff: u32,
    pub(crate) rto_backoff_max: u32,
    pub(crate) rto_timeouts: u64,
    pub(crate) copies: u64,
    // Datagrams counted on hops this session replaced.
    pub(crate) earlier_hops_received: u64,
    pub(crate) gaps: u64,
    pub(crate) gaps_repaired_by_push: u64,
    pub(crate) repair_pulls: u64,
    // Set on leaving the offline state until its recovery UPLINK went.
    pub(crate) recovered: bool,
    // The latest evaluation that found DELAYED or Backpressure (§5.2 hold).
    pub(crate) congested_at: Option<u64>,
    // Downlink new-turn jitter (spec §4.8), with the sealing rate it
    // measures against, a pause in the stream, and a PULL whose reply
    // packs are not measured.
    pub(crate) jitter: JitterMeter,
    pub(crate) jitter_rate: u16,
    pub(crate) stream_paused: bool,
    pub(crate) pull_pending: bool,
    pub(crate) pull_reply_ms: Option<u64>,
    pub(crate) schedule: uplink::Schedule,
}

impl ClientLink {
    pub(crate) fn new(rate: u16) -> Self {
        let mut link = Self {
            loss: LossStats::initial(),
            ..Self::default()
        };
        link.schedule.turn_ms = crate::turn_ms(rate);
        link.jitter_rate = rate;
        link
    }

    /// The `UPLINK.link` byte (spec §3.2).
    pub(crate) fn link_byte(&self) -> u8 {
        self.clock.state as u8
            | if self.rtt.delayed { LINK_DELAYED } else { 0 }
            | if self.jitter.jittery { LINK_JITTERY } else { 0 }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SendRecord {
    pub(crate) sequence: u64,
    pub(crate) first: u64,
    pub(crate) count: u8,
    pub(crate) at_ms: u64,
    pub(crate) kind: u8,
    pub(crate) lost: bool,
}

/// One reader hop's downlink measurement at the relay (spec §4.1, §4.2,
/// §4.3, §5, §7.1). It belongs to the hop's `Peer`; a renewed hop takes
/// over the path measurement with `carry_path` (§4.7).
#[derive(Debug)]
pub(crate) struct ReaderLink {
    pub(crate) up: RecvWindow,
    up_high_at_ms: u64,
    log: Vec<SendRecord>,
    cursor: LossCursor,
    pub(crate) loss: LossStats,
    pub(crate) delayed: bool,
    pub(crate) srtt_c: u16,
    pub(crate) rttvar_c: u16,
    pub(crate) jittery: bool,
    // The latest evaluation that found DELAYED (spec §5.2 hold).
    pub(crate) congested_at: Option<u64>,
    pub(crate) last_heard_ms: u64,
    pub(crate) clock: StateClock,
    pub(crate) packets_by_kind: [u64; 5],
    pub(crate) bytes_by_kind: [u64; 5],
}

impl ReaderLink {
    pub(crate) fn new(now_ms: u64) -> Self {
        let mut clock = StateClock::default();
        clock.enter(LinkState::Good, false, now_ms);
        Self {
            up: RecvWindow::default(),
            up_high_at_ms: now_ms,
            log: vec![SendRecord::default(); SENDLOG_READER],
            cursor: LossCursor::default(),
            loss: LossStats::initial(),
            delayed: false,
            srtt_c: 0,
            rttvar_c: 0,
            jittery: false,
            congested_at: None,
            last_heard_ms: now_ms,
            clock,
            packets_by_kind: [0; 5],
            bytes_by_kind: [0; 5],
        }
    }

    /// A renewed hop keeps the path measurement of the hop it replaces:
    /// loss with its 20 s hold, the state with its 10 s congestion hold,
    /// DELAYED, JITTERY and the reported RTT (spec §4.7). Windows, send
    /// log, judgement cursor and counters stay the new hop's own.
    pub(crate) fn carry_path(&mut self, previous: &ReaderLink) {
        self.loss = previous.loss;
        self.clock = previous.clock;
        self.congested_at = previous.congested_at;
        self.delayed = previous.delayed;
        self.jittery = previous.jittery;
        self.srtt_c = previous.srtt_c;
        self.rttvar_c = previous.rttvar_c;
    }

    /// An authenticated datagram of this hop (spec §4.1, §5.1).
    pub(crate) fn heard(&mut self, sequence: u64, now_ms: u64) {
        if self.up.record(sequence) {
            self.up_high_at_ms = now_ms;
        }
        self.last_heard_ms = self.last_heard_ms.max(now_ms);
    }

    /// Judges nothing sent before hop sequence `next` (spec §5.5).
    pub(crate) fn skip_judged_to(&mut self, next: u64) {
        self.cursor.skip_to(next);
    }

    /// The ACK and STATUS tail, read when the reply is encoded (spec §4.1).
    pub(crate) fn feedback(&self, now_ms: u64) -> HopFeedback {
        HopFeedback {
            up: self.up,
            echo_delay_ms: now_ms
                .saturating_sub(self.up_high_at_ms)
                .min(u64::from(u16::MAX)) as u16,
        }
    }

    /// Logs one datagram sent on this hop (spec §4.2): `pack` is the control
    /// payload when it is a PACK, `bytes` the whole datagram length.
    pub(crate) fn record_send(
        &mut self,
        sequence: u64,
        pack: Option<&[u8]>,
        kind: u8,
        bytes: usize,
        now_ms: u64,
    ) {
        let (first, count) = pack.map_or((0, 0), |pack| {
            crate::sequenced::pack::pack_v2_range(pack).expect("a pack this relay wrote")
        });
        self.log[(sequence % SENDLOG_READER as u64) as usize] = SendRecord {
            sequence,
            first,
            count,
            at_ms: now_ms,
            kind,
            lost: false,
        };
        self.packets_by_kind[usize::from(kind)] += 1;
        self.bytes_by_kind[usize::from(kind)] += bytes as u64;
    }

    /// Judges `UPLINK.down_high` / `down_bits` (spec §4.3, §7.1 step 4) and
    /// returns the logged sends judged lost, oldest first.
    /// `now_ms` is when the feedback arrived (`t_fb`): a feedback is judged
    /// once, and its verdict never changes with time.
    pub(crate) fn judge(&mut self, down: RecvWindow, now_ms: u64) -> Vec<SendRecord> {
        let mut lost = Vec::new();
        let log = &mut self.log;
        // §4.3: wait srtt_c + 4 rttvar_c (§4.6 defaults before a report).
        let (srtt, rttvar) = reader_rtt(self.srtt_c, self.rttvar_c);
        let sent = |sequence: u64| {
            let entry = log[(sequence % SENDLOG_READER as u64) as usize];
            (entry.sequence == sequence).then_some(entry.at_ms)
        };
        let wait = srtt + 4 * rttvar;
        let (judged, missing) =
            self.cursor
                .judge(down.high, down.bits, now_ms, wait, sent, |sequence| {
                    lost.push(sequence)
                });
        self.loss.add(judged, missing, now_ms);
        lost.into_iter()
            .map(|sequence| {
                let entry = &mut self.log[(sequence % SENDLOG_READER as u64) as usize];
                entry.lost = true;
                *entry
            })
            .collect()
    }

    /// The logged sends newer than `sequence`, oldest first.
    pub(crate) fn sent_after(&self, sequence: u64) -> impl Iterator<Item = &SendRecord> {
        let mut later: Vec<_> = self
            .log
            .iter()
            .filter(move |entry| entry.sequence > sequence)
            .collect();
        later.sort_unstable_by_key(|entry| entry.sequence);
        later.into_iter()
    }

    /// The newest logged send of `kind`s matching `keep`.
    pub(crate) fn latest(&self, keep: impl Fn(&SendRecord) -> bool) -> Option<SendRecord> {
        self.log
            .iter()
            .filter(|entry| entry.sequence != 0 && keep(entry))
            .max_by_key(|entry| entry.sequence)
            .copied()
    }

    /// The `n` newest new packs sent, newest first (spec §7.4 step 1).
    pub(crate) fn latest_fresh(&self, n: usize) -> Vec<SendRecord> {
        let mut fresh: Vec<_> = self
            .log
            .iter()
            .filter(|entry| entry.sequence != 0 && entry.kind == SEND_FRESH)
            .copied()
            .collect();
        fresh.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.sequence));
        fresh.truncate(n);
        fresh
    }

    /// Re-evaluates the downlink state at `now_ms` (spec §5.2); returns the
    /// previous state if it changed. Entering offline discards open counts.
    pub(crate) fn evaluate(&mut self, now_ms: u64) -> Option<LinkState> {
        let offline = now_ms.saturating_sub(self.last_heard_ms) >= READER_OFFLINE_MS;
        if self.delayed {
            self.congested_at = Some(now_ms);
        }
        // Congested until DELAYED has not held for 10 s (spec §5.2).
        let congested = self
            .congested_at
            .is_some_and(|at| now_ms.saturating_sub(at) < CONGESTION_HOLD_MS);
        let state = link_state(offline, congested, self.loss.lossy);
        let previous = self.clock.enter(state, self.delayed, now_ms);
        if previous.is_some() && state == LinkState::Offline {
            self.loss.clear_counts();
        }
        previous
    }

    /// K for this reader's state (spec §7.2): JITTERY raises a good
    /// reader's to 2; 0 while offline.
    pub(crate) fn piggyback_depth(&self) -> u8 {
        let by_loss = if self.loss.lossy {
            if self.loss.severe { 3 } else { 2 }
        } else {
            1
        };
        match self.clock.state {
            LinkState::Offline => 0,
            LinkState::Good => 1 + u8::from(self.jittery),
            LinkState::Lossy => by_loss,
            LinkState::Congested => by_loss.min(2),
        }
    }
}

impl ClientSession {
    /// Counts an authenticated datagram of this hop at `clock_ms` (spec
    /// §4.1, §5.1). CHALLENGE and LEAVE_ACK never reach here.
    pub(crate) fn hop_heard(&mut self, sequence: u64) {
        self.hop.down.record(sequence);
        self.hop.last_heard_ms = Some(self.clock_ms);
        self.hop.received += 1;
    }

    /// Logs a datagram this hop sends (spec §4.2); HELLO is not counted by
    /// the relay's window and is never logged.
    pub(crate) fn hop_sent(&mut self, sequence: u64) {
        self.hop.record_send(sequence, self.clock_ms);
    }

    /// One PACK's part in the downlink jitter (spec §4.8): a pack bringing a
    /// FRAME_END at or past `recv_next` is a new-turn arrival, unless it is
    /// part of a PULL reply, the stream is paused or the uplink is offline.
    /// Pause and RATE records in the stream restart the reference and set
    /// the sealing rate.
    pub(crate) fn measure_jitter(&mut self, events: &[OrderEvent], now_ms: u64) {
        let Some(ordering) = self.ordering.as_mut() else {
            return;
        };
        let link = &mut ordering.link;
        let recv_next = link.schedule.recv_next.max(1);
        let newest = events
            .iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(crate::OrderEntry::FrameEnd { sequence, frame })
                    if *sequence >= recv_next =>
                {
                    Some(*frame)
                }
                _ => None,
            })
            .max();
        if let Some(frame) = newest
            && !link.stream_paused
            && link.clock.state != LinkState::Offline
            && link.pull_reply_ms != Some(now_ms)
        {
            let turn_ms = link.schedule.turn_ms;
            link.jitter.sample(now_ms, frame, link.jitter_rate, turn_ms);
        }
        for event in events {
            match event {
                OrderEvent::Entry(crate::OrderEntry::Pause { paused, .. }) => {
                    link.stream_paused = *paused;
                    link.jitter.restart();
                }
                OrderEvent::Entry(crate::OrderEntry::Rate {
                    frames_per_second, ..
                }) => link.jitter_rate = *frames_per_second,
                _ => {}
            }
        }
    }

    /// ACK or STATUS feedback received at `now_ms` (spec §4.3, §4.4).
    pub(crate) fn take_feedback(&mut self, feedback: HopFeedback, now_ms: u64) {
        let Some(ordering) = self.ordering.as_mut() else {
            return;
        };
        let high = feedback.up.high;
        if high == 0 {
            return;
        }
        if high > self.hop.rtt_high
            && feedback.echo_delay_ms != u16::MAX
            && let Some(sent) = self.hop.sent(high)
        {
            self.hop.rtt_high = high;
            let rtt = now_ms
                .saturating_sub(sent.at_ms)
                .saturating_sub(u64::from(feedback.echo_delay_ms));
            ordering
                .link
                .rtt
                .sample(rtt.min(u64::from(u32::MAX)) as u32, now_ms);
        }
        self.judge_uplink(feedback.up, now_ms);
    }

    /// Uplink loss from feedback `up` that arrived at `now_ms` (spec §4.3
    /// `t_fb`): a missing sequence counts once it went srtt + 4 rttvar
    /// before, the §4.6 defaults before a sample. Judged once per feedback.
    fn judge_uplink(&mut self, up: RecvWindow, now_ms: u64) {
        let Some(ordering) = self.ordering.as_mut() else {
            return;
        };
        let rtt = &ordering.link.rtt;
        let wait = rtt
            .srtt
            .map_or(300, |srtt| u64::from(srtt) + 4 * u64::from(rtt.rttvar));
        let hop = &mut self.hop;
        let log = &hop.log;
        let sent = |sequence: u64| {
            let entry = log[(sequence % SENDLOG_CLIENT as u64) as usize];
            (entry.sequence == sequence).then_some(entry.at_ms)
        };
        let (judged, lost) = hop
            .cursor
            .judge(up.high, up.bits, now_ms, wait, sent, |_| {});
        ordering.link.loss.add(judged, lost, now_ms);
    }

    /// Re-evaluates the uplink state at `now_ms` (spec §5.2, §5.5).
    pub(crate) fn evaluate_uplink(&mut self, now_ms: u64) {
        let offline = self
            .hop
            .last_heard_ms
            .is_some_and(|heard| now_ms.saturating_sub(heard) >= CLIENT_OFFLINE_MS);
        let Some(ordering) = self.ordering.as_mut() else {
            return;
        };
        let link = &mut ordering.link;
        if link.rtt.delayed
            || link
                .backpressure_at
                .is_some_and(|at| now_ms.saturating_sub(at) < BACKPRESSURE_HOLD_MS)
        {
            link.congested_at = Some(now_ms);
        }
        // Congested until neither condition has held for 10 s (§5.2).
        let congested = link
            .congested_at
            .is_some_and(|at| now_ms.saturating_sub(at) < CONGESTION_HOLD_MS);
        let state = link_state(offline, congested, link.loss.lossy);
        match link.clock.enter(state, link.rtt.delayed, now_ms) {
            Some(_) if state == LinkState::Offline => link.loss.clear_counts(),
            Some(LinkState::Offline) => {
                link.recovered = true;
                link.rto_backoff = 0;
                // Nothing sent before the recovery is judged (spec §5.5).
                self.hop.cursor.skip_to(self.sent + 1);
                link.jitter.restart();
            }
            _ => {}
        }
    }

    /// The previous hop's datagram count, carried when this hop takes over.
    pub(crate) fn hop_received(&self) -> u64 {
        self.hop.received
    }
}
