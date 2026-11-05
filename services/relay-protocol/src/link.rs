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

use crate::{ClientSession, OrderPace, RelayError, RelayServer, ServerEvent};

/// A direction's link state (spec §5.2), first match wins: offline, then
/// congested, then lossy, then good. The discriminant is the wire value of
/// `UPLINK.link` bits 0..1 (spec §3.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LinkState {
    /// None of the other states holds.
    #[default]
    Good = 0,
    /// `loss‰` is inside the lossy hysteresis band: entered at 30‰ or more,
    /// left at 10‰ or less, only after at least one fold (spec §4.3, §5.4).
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

    /// This hop's downlink receive window (spec §4.1), the values the next
    /// UPLINK carries as `down_high` / `down_bits`.
    pub down: RecvWindow,
}

/// A snapshot of one reader's downlink at the relay (spec §7.1, §10.2), for
/// the newest admitted hop of that source. Counters are per hop: a renewed
/// hop is a new `Peer` and starts from zero and Good (spec §4.7).
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
    /// The current piggyback depth K (spec §7.2): 1 good, 2 lossy, 3 severe,
    /// `min(K, 2)` congested; 0 while offline or unsubscribed.
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
}

impl ClientSession {
    // --- Timed datagrams: each records its send time (spec §4.2) or its
    // socket receive time (spec §4.4). Same wire output as the untimed
    // methods they replace; those are removed once callers migrate. ---

    /// [`ClientSession::probe`] at `now_ms`: HELLO before readiness (not
    /// counted in any window, spec §4.1), otherwise a PING keepalive.
    pub fn probe_at(&mut self, now_ms: u64, out: &mut Vec<u8>) -> Result<(), RelayError> {
        // A1: the send log arrives with A2.
        let _ = now_ms;
        self.probe(out)
    }

    /// [`ClientSession::confirm`] at `now_ms`. The relay's `up` window for
    /// this hop starts with this datagram (spec §4.1).
    pub fn confirm_at(
        &mut self,
        cookie: &[u8],
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        let _ = now_ms;
        self.confirm(cookie, out)
    }

    /// [`ClientSession::data`] at `now_ms`.
    pub fn data_at(
        &mut self,
        target: u8,
        payload: &[u8],
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        let _ = now_ms;
        self.data(target, payload, out)
    }

    /// [`ClientSession::configure_ordering`] (READY_REQUEST, spec §6.0) at
    /// `now_ms`. Before `started` the caller resends it every 125 ms; its
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
        let _ = now_ms;
        self.configure_ordering(certificate, agreement, out)
    }

    /// [`ClientSession::pull_ordering`] (PULL, request 2) at `now_ms`, for the
    /// cases that pull directly (spec §7.7): before the start, while
    /// RESTORING, and catching up. Gap repair during play goes through
    /// `poll_repair_pull` instead.
    pub fn pull_ordering_at(
        &mut self,
        next_seq: u64,
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        let _ = now_ms;
        self.pull_ordering(next_seq, out)
    }

    /// [`ClientSession::control_ordering_pause`] (PAUSE, request 5) at `now_ms`.
    pub fn control_ordering_pause_at(
        &mut self,
        request_id: u64,
        expected_epoch: u64,
        paused: bool,
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        let _ = now_ms;
        self.control_ordering_pause(request_id, expected_epoch, paused, out)
    }

    /// [`ClientSession::leave_ordering`] (LEAVE, request 3) at `now_ms`. A
    /// retry returns the first call's cached bytes with its hop sequence, so
    /// only the first call is a new send. No UPLINK follows a Leave
    /// (spec §11 row 8): `poll_uplink` and `heartbeat_uplink` return false.
    pub fn leave_ordering_at(&mut self, now_ms: u64, out: &mut Vec<u8>) -> Result<(), RelayError> {
        let _ = now_ms;
        self.leave_ordering(out)
    }

    /// [`ClientSession::receive`] with `now_ms`, the moment the socket read of
    /// `bytes` returned. Besides decoding, an authenticated datagram updates
    /// the hop's `down` window (spec §4.1) and resets the 1000 ms offline
    /// timer (spec §5.1); an ACK or STATUS also takes an RTT sample (§4.4)
    /// and judges uplink loss (§4.3); an ACK, a BatchGap error or this
    /// source's own BATCH record advances `accepted_through` (§6.1, §6.4);
    /// Backpressure starts the 2000 ms congestion hold (§6.4). Leaving the
    /// offline state arms the immediate recovery UPLINK (§5.5).
    pub fn receive_at<'a>(
        &mut self,
        bytes: &'a [u8],
        now_ms: u64,
    ) -> Result<ServerEvent<'a>, RelayError> {
        let _ = now_ms;
        self.receive(bytes)
    }

    // --- Content from the sequencing layer (spec §6.7). None of these
    // encodes or sends; `poll_uplink` decides when it goes out. ---

    /// Seals `plaintext` as the next batch (spec §6.1) and returns its ID:
    /// consecutive from 1 for this match, carried across hop renewals. The
    /// ciphertext stays fixed until the batch is acknowledged. The batch goes
    /// out with the next UPLINK (§6.2, §6.3 row 1).
    ///
    /// Errors: `Unregistered` before ordering is configured, before
    /// `set_ordering_started`, while restoring, or after Leave (spec §6.0:
    /// no batches before the start); `Malformed` for an empty plaintext or one
    /// over `ORDERING_BATCH_CAPACITY` bytes; `Full` while
    /// `ORDERING_SUBMISSION_WINDOW` (4) batches are unacknowledged.
    pub fn seal_ordering_batch(&mut self, plaintext: &[u8]) -> Result<u64, RelayError> {
        let _ = plaintext;
        todo!("A3")
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
        let _ = (samples, now_ms);
        todo!("A3")
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
        let _ = (pace, frames_per_second, tick_micros);
        todo!("A3")
    }

    /// Sets the contiguous watermark `recv_next` (spec §2): every record
    /// before it is received. Never decreases (a lower value is ignored);
    /// must be at least 1. An advance makes an acknowledgement due after the
    /// state's interval (spec §6.3 row 3: 100 ms good, `turn_ms` lossy,
    /// 200 ms congested).
    pub fn set_ordering_received_next(&mut self, recv_next: u64) {
        let _ = recv_next;
        todo!("A3")
    }

    /// Reports the current gap state; call it whenever it changes, observed
    /// at `now_ms`. Repeating the same value changes nothing; `None` reports
    /// the gap closed. A new gap makes `poll_uplink` send an UPLINK once at
    /// least 16 ms passed since the previous one (spec §6.3 row 2; after the
    /// `turn_ms` grace for [`OrderingGap::BehindStatus`]), and starts the
    /// fallback PULL grace of `poll_repair_pull` (spec §7.7).
    pub fn set_ordering_gap(&mut self, gap: Option<OrderingGap>, now_ms: u64) {
        let _ = (gap, now_ms);
        todo!("A4")
    }

    /// Sets the turn duration `turn_ms` in whole milliseconds (spec §2), the
    /// lossy acknowledgement interval (§6.3 row 3) and the `BehindStatus`
    /// grace. Pass [`crate::turn_ms`] of the current sealing rate; call again
    /// when a RATE record changes it.
    pub fn set_ordering_turn_ms(&mut self, turn_ms: u32) {
        let _ = turn_ms;
        todo!("A3")
    }

    /// Reports that the match started: a STATUS with `started = 1` arrived
    /// (spec §6.0). The next `poll_uplink`, in the same service, sends the
    /// first UPLINK with PUSH; until the first PACK arrives it resends one
    /// every RTO (§4.5) to confirm the subscription. Batches may be sealed
    /// from now on. Later calls change nothing.
    pub fn set_ordering_started(&mut self) {
        todo!("A3")
    }

    /// Reports that a PACK (event 12) arrived on this hop, which confirms
    /// the push subscription and ends the RTO subscription resends (spec §6.0).
    /// Calling it for every PACK is fine; only the first on a hop counts.
    pub fn note_ordering_pack(&mut self) {
        todo!("A3")
    }

    // --- Scheduling: the state machine decides when to send. ---

    /// Encodes one UPLINK (request 8, spec §3.2) into `out` if §6.3 says one
    /// is due at `now_ms`, returning true; otherwise leaves `out` alone and
    /// returns false. Covers rows 1-7 of §6.3, the standalone checksum send,
    /// the subscription resends of §6.0 and the immediate UPLINK on a renewed
    /// hop (§4.7); while offline the only UPLINK is the 250 ms PROBE. Every
    /// UPLINK but a probe carries all unacknowledged batches that fit from
    /// `accepted_through + 1` (§6.2).
    ///
    /// Call it every service (about 16 ms) and loop until it returns false;
    /// each call makes at most one datagram, and one service normally yields
    /// one UPLINK (lossy copies are their own later sends, §6.3 row 6).
    /// Returns false before `set_ordering_started`, after Leave, when the hop
    /// is not ready, or if the hop counter is exhausted.
    pub fn poll_uplink(&mut self, now_ms: u64, out: &mut Vec<u8>) -> bool {
        let _ = (now_ms, out);
        todo!("A3")
    }

    /// The earliest `now_ms` at which `poll_uplink` could return true without
    /// any further input (receive, content or gap report); it may already be
    /// past. `None` before the start or after Leave.
    pub fn next_uplink_deadline(&self) -> Option<u64> {
        todo!("A3")
    }

    /// The transport's IO-thread heartbeat (spec §6.6). If no UPLINK went out
    /// on this hop for 250 ms by `now_ms`, re-encodes the most recent
    /// UPLINK's contents into `out` with a new hop sequence and the current
    /// `down_high` / `down_bits`, keeping its batch and checksum segments and
    /// dropping its pace segment, and returns true. Returns false when not
    /// due, when this hop has sent no UPLINK yet, or after Leave. A heartbeat
    /// counts as an UPLINK for every later interval.
    pub fn heartbeat_uplink(&mut self, now_ms: u64, out: &mut Vec<u8>) -> bool {
        let _ = (now_ms, out);
        todo!("A3")
    }

    /// Encodes the fallback PULL (request 2, `next_seq = recv_next`) into
    /// `out` when a gap reported through `set_ordering_gap` has lasted
    /// `G = clamp(srtt + 4 * rttvar + turn_ms, 100, 1000)` ms without the
    /// watermark advancing, then again after each further `G` multiple, at
    /// most 1000 ms apart (spec §7.7). Returns whether it encoded one.
    pub fn poll_repair_pull(&mut self, now_ms: u64, out: &mut Vec<u8>) -> bool {
        let _ = (now_ms, out);
        todo!("A4")
    }

    // --- Observation ---

    /// A snapshot of this hop's link measurement and the match's §10.1
    /// counters; see [`ClientLinkStats`].
    pub fn link_stats(&self) -> ClientLinkStats {
        todo!("A2")
    }
}

impl RelayServer {
    /// A snapshot of the downlink of `source`'s newest admitted hop in
    /// `room`; see [`ReaderLinkStats`]. `None` when the relay holds no such
    /// room or no admitted hop for that source.
    pub fn reader_link_stats(&self, room: [u8; 16], source: u8) -> Option<ReaderLinkStats> {
        let _ = (room, source);
        todo!("A2")
    }
}
