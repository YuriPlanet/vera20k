//! The client's uplink scheduler (spec §6): which UPLINK goes out when, and
//! what it carries. All times are the caller's `now_ms`.

use super::codec::{FLAG_PACE, FLAG_PROBE, FLAG_PUSH, FLAG_RESTORING, PaceSegment, Uplink};
use std::collections::VecDeque;

use super::{LinkState, OrderingGap, ack_interval};
use crate::envelope::PACK_LIMIT;
use crate::sequenced::ClientOrdering;
use crate::{CONTROL, ClientSession, OrderPace, RelayError};

const UPLINK_HEARTBEAT_MS: u64 = 250;
const COPY_SPACING_MS: u64 = 16;
const CONGESTED_SEND_SPACING_MS: u64 = 33;
const GAP_UPLINK_SPACING_MS: u64 = 16;
/// A checksum sample waits this long for another UPLINK (spec §6.3).
const CHECKSUM_SEND_MS: u64 = 100;
const MAX_BATCHES: usize = super::codec::MAX_UPLINK_BATCHES;
const PULL_GRACE_MIN: u64 = 100;
const PULL_GRACE_MAX: u64 = 1000;
/// A STATUS this far ahead of the watermark is pulled at once (spec §7.7).
const FAR_BEHIND: u64 = 64;

/// The uplink scheduler's state, carried across hop renewals with the
/// ordering state; the per-hop part is in `HopLink`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Schedule {
    /// A STATUS with `started = 1` arrived (spec §6.0).
    pub(crate) started: bool,
    pub(crate) recv_next: u64,
    // The watermark the last UPLINK carried (row 3).
    acked_recv_next: u64,
    pub(crate) turn_ms: u32,
    last_uplink_ms: Option<u64>,
    // Every batch up to this ID went out at least once (row 1).
    sent_batches_through: u64,
    // The last UPLINK that carried unacknowledged batches (row 4).
    batches_carried_ms: Option<u64>,
    // Checksum samples not yet sent, and when the first of them arrived.
    checksums: Option<(Vec<(u32, u32)>, u64)>,
    pace: Option<PaceSegment>,
    // One plan per UPLINK that first sent batches while lossy.
    copies: VecDeque<Copies>,
    pub(crate) gap: Option<Gap>,
    last: Option<LastUplink>,
    /// The newest STATUS `next_seq` (spec §7.7).
    pub(crate) status_next_seq: u64,
    // When the next far-behind PULL is due.
    far_pull_ms: Option<u64>,
}

/// Lossy-state copies still owed for a first transmission (row 6).
#[derive(Debug, Clone)]
struct Copies {
    batch: u64,
    due_ms: u64,
    remaining: u8,
    checksums: Vec<(u32, u32)>,
}

/// A gap the sequencing layer reported (row 2, spec §7.7).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gap {
    pub(crate) kind: OrderingGap,
    pub(crate) since_ms: u64,
    // When the first UPLINK went while the gap was open.
    pub(crate) uplinked: Option<u64>,
    // An UPLINK went max(16, 4 rttvar) ms after that one (§6.7 item 1).
    pub(crate) resent: bool,
    // Fallback PULLs sent for it, when the next is due, and the watermark
    // it last saw: an advance restarts the grace (spec §7.7).
    pub(crate) pulls: u32,
    pub(crate) next_pull_ms: Option<u64>,
    pub(crate) watermark: u64,
}

/// What the IO-thread heartbeat repeats (spec §6.6).
#[derive(Debug, Clone)]
struct LastUplink {
    flags: u8,
    recv_next: u64,
    checksums: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Normal,
    Copy,
    Probe,
}

struct Due {
    kind: Kind,
    // Row 4 expired: the RTO backs off.
    timeout: bool,
    // A lossy copy is due and this send serves as it.
    copy: bool,
}

/// `max(16, 4 rttvar)`: how long into a gap its second UPLINK waits.
fn gap_resend_ms(link: &super::ClientLink) -> u64 {
    GAP_UPLINK_SPACING_MS.max(4 * u64::from(link.rtt.rttvar))
}

fn since(at: Option<u64>, now_ms: u64) -> u64 {
    at.map_or(u64::MAX, |at| now_ms.saturating_sub(at))
}

/// The batches an UPLINK may carry, from `accepted_through + 1`
/// (spec §6.2): their first ID and contents, at most four.
/// `encode_uplink` keeps those that fit.
fn fitting_batches(ordering: &ClientOrdering) -> (u64, Vec<Vec<u8>>) {
    let first = ordering.accepted_through + 1;
    let mut batches = Vec::new();
    for id in first..ordering.next_batch {
        let Some(batch) = ordering.sealed.iter().find(|batch| batch.id == id) else {
            break;
        };
        if batches.len() == MAX_BATCHES {
            break;
        }
        batches.push(batch.opaque.clone());
    }
    (first, batches)
}

/// `G = clamp(srtt + 4 * rttvar + turn_ms, 100, 1000)` (spec §7.7), with
/// the §4.6 defaults before any sample.
fn pull_grace(link: &super::ClientLink) -> u64 {
    let (srtt, rttvar) = link.rtt.rtt_or_default();
    (srtt + 4 * rttvar + u64::from(link.schedule.turn_ms)).clamp(PULL_GRACE_MIN, PULL_GRACE_MAX)
}

impl ClientSession {
    /// The ordering of a session that may send UPLINKs: ready, started, not
    /// left, and a player's: a watcher only pulls (README "Watchers").
    fn open_uplink(&self) -> Option<&ClientOrdering> {
        let open = self.ready && !self.is_watching();
        self.ordering
            .as_ref()
            .filter(|ordering| open && ordering.link.schedule.started && ordering.leave.is_none())
    }

    /// [`Self::open_uplink`], to change.
    fn open_uplink_mut(&mut self) -> Option<&mut ClientOrdering> {
        let open = self.ready && !self.is_watching();
        self.ordering
            .as_mut()
            .filter(|ordering| open && ordering.link.schedule.started && ordering.leave.is_none())
    }

    /// The §6.3 decision at `now_ms` for the open `ordering`, or none.
    fn due_uplink(&self, ordering: &ClientOrdering, now_ms: u64) -> Option<Due> {
        let link = &ordering.link;
        let schedule = &link.schedule;
        let state = link.clock.state;
        let since_last = since(schedule.last_uplink_ms, now_ms);
        if state == LinkState::Offline {
            return (self.hop.uplinks == 0 || since_last >= UPLINK_HEARTBEAT_MS).then_some(Due {
                kind: Kind::Probe,
                timeout: false,
                copy: false,
            });
        }
        let newest = ordering.next_batch - 1;
        let fresh = newest > schedule.sent_batches_through.max(ordering.accepted_through);
        // Row 4 times only batches some UPLINK already carried (§6.7 item
        // 2): those still unacknowledged since the last one carrying them.
        let timeout = ordering.accepted_through < schedule.sent_batches_through
            && schedule
                .batches_carried_ms
                .is_some_and(|at| now_ms.saturating_sub(at) >= link.rtt.rto(link.rto_backoff));
        let copy = state == LinkState::Lossy
            && schedule
                .copies
                .iter()
                .any(|copies| copies.due_ms <= now_ms && copies.batch > ordering.accepted_through);
        // While congested, nothing carries a never-sent batch until 33 ms
        // after the previous UPLINK (row 1): the wait merges new batches.
        if fresh && state == LinkState::Congested && since_last < CONGESTED_SEND_SPACING_MS {
            return None;
        }
        let resend = gap_resend_ms(link);
        let normal = self.hop.uplinks == 0
            || link.recovered
            || (fresh
                && (state != LinkState::Congested || since_last >= CONGESTED_SEND_SPACING_MS))
            || schedule.gap.is_some_and(|gap| {
                since_last >= GAP_UPLINK_SPACING_MS
                    && if let Some(first) = gap.uplinked {
                        // Once more max(16, 4 rttvar) after the first: a
                        // snapshot the relay can judge (§6.7 item 1).
                        !gap.resent && now_ms.saturating_sub(first) >= resend
                    } else {
                        gap.kind == OrderingGap::Reordered
                            || now_ms.saturating_sub(gap.since_ms) >= u64::from(schedule.turn_ms)
                    }
            })
            || (schedule.recv_next > schedule.acked_recv_next
                && since_last >= ack_interval(state, schedule.turn_ms))
            || schedule
                .checksums
                .as_ref()
                .is_some_and(|(_, at)| now_ms.saturating_sub(*at) >= CHECKSUM_SEND_MS)
            || timeout
            || (!self.hop.first_pack && since_last >= link.rtt.rto(0))
            || since_last >= UPLINK_HEARTBEAT_MS;
        if normal {
            Some(Due {
                kind: Kind::Normal,
                timeout,
                copy,
            })
        } else if copy {
            Some(Due {
                kind: Kind::Copy,
                timeout: false,
                copy: true,
            })
        } else {
            None
        }
    }

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
        if self.open_uplink().is_none() {
            return false;
        }
        self.clock_ms = now_ms;
        self.evaluate_uplink(now_ms);
        if self.hop.last_service_ms == Some(now_ms) {
            return false;
        }
        let Some(due) = self
            .open_uplink()
            .and_then(|ordering| self.due_uplink(ordering, now_ms))
        else {
            return false;
        };
        if self.send_uplink(due.kind, due.copy, now_ms, out).is_err() {
            return false;
        }
        self.hop.last_service_ms = Some(now_ms);
        if due.timeout
            && let Some(ordering) = self.ordering.as_mut()
        {
            let link = &mut ordering.link;
            link.rto_backoff += 1;
            link.rto_backoff_max = link.rto_backoff_max.max(link.rto_backoff);
            link.rto_timeouts += 1;
        }
        true
    }

    /// Encodes and logs one UPLINK of `kind` at `now_ms` (spec §3.2, §6.2,
    /// §6.3) and books it in the schedule.
    /// `copy`: this send serves the earliest copy due (row 6).
    fn send_uplink(
        &mut self,
        kind: Kind,
        copy: bool,
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        // Copies of acknowledged batches go first, so the copy found
        // below is the one booked after the send.
        let ordering = self.ordering.as_mut().ok_or(RelayError::Unregistered)?;
        let accepted = ordering.accepted_through;
        ordering
            .link
            .schedule
            .copies
            .retain(|copies| copies.batch > accepted);
        let ordering = &*ordering;
        let link = &ordering.link;
        let schedule = &link.schedule;
        let served = copy
            .then(|| {
                schedule
                    .copies
                    .iter()
                    .enumerate()
                    .filter(|(_, copies)| copies.due_ms <= now_ms && copies.batch > accepted)
                    .min_by_key(|(_, copies)| copies.due_ms)
                    .map(|(index, _)| index)
            })
            .flatten();
        let mut flags = FLAG_PUSH;
        if ordering.restoring {
            flags |= FLAG_RESTORING;
        }
        let (checksums, pace) = match kind {
            Kind::Normal => (
                schedule
                    .checksums
                    .as_ref()
                    .map_or(Vec::new(), |(samples, _)| samples.clone()),
                schedule.pace,
            ),
            Kind::Copy => (
                served.map_or(Vec::new(), |index| schedule.copies[index].checksums.clone()),
                None,
            ),
            Kind::Probe => {
                flags |= FLAG_PROBE;
                (Vec::new(), None)
            }
        };
        let (first_batch, mut batches) = if kind == Kind::Probe {
            (0, Vec::new())
        } else {
            fitting_batches(ordering)
        };
        let recv_next = schedule.recv_next.max(1);
        self.encode_uplink(
            flags,
            recv_next,
            &checksums,
            pace,
            first_batch,
            &mut batches,
            out,
        )?;
        self.hop.uplinks += 1;
        let ordering = self.ordering.as_mut().ok_or(RelayError::Unregistered)?;
        // A stream transport delivers the original first, so it takes no copies.
        let lossy = !self.stream_transport && ordering.link.clock.state == LinkState::Lossy;
        let severe = ordering.link.loss.severe;
        let link = &mut ordering.link;
        let schedule = &mut link.schedule;
        schedule.last_uplink_ms = Some(now_ms);
        schedule.acked_recv_next = recv_next;
        link.recovered = false;
        let resend = gap_resend_ms(link);
        let schedule = &mut link.schedule;
        if let Some(gap) = schedule.gap.as_mut() {
            match gap.uplinked {
                None => gap.uplinked = Some(now_ms),
                Some(first) => gap.resent |= now_ms.saturating_sub(first) >= resend,
            }
        }
        if kind == Kind::Normal {
            schedule.checksums = None;
            schedule.pace = None;
        }
        // The copy served: its next one 16 ms on; any other copy due now
        // waits as long, so copies stay 16 ms apart (§6.3).
        if let Some(index) = served {
            if kind == Kind::Copy {
                link.copies += 1;
            }
            let copies = &mut schedule.copies[index];
            copies.remaining -= 1;
            copies.due_ms = now_ms + COPY_SPACING_MS;
            for copies in schedule.copies.iter_mut() {
                copies.due_ms = copies.due_ms.max(now_ms + COPY_SPACING_MS);
            }
            schedule.copies.retain(|copies| copies.remaining > 0);
        }
        if !batches.is_empty() {
            schedule.batches_carried_ms = Some(now_ms);
            let newest = first_batch + batches.len() as u64 - 1;
            if newest > schedule.sent_batches_through {
                schedule.sent_batches_through = newest;
                if kind == Kind::Normal && lossy {
                    schedule.copies.push_back(Copies {
                        batch: newest,
                        due_ms: now_ms + COPY_SPACING_MS,
                        remaining: if severe { 2 } else { 1 },
                        checksums: checksums.clone(),
                    });
                }
            }
        }
        if kind != Kind::Probe {
            schedule.last = Some(LastUplink {
                flags: flags & !FLAG_PACE,
                recv_next,
                checksums,
            });
        }
        Ok(())
    }

    fn encode_uplink(
        &mut self,
        flags: u8,
        recv_next: u64,
        checksums: &[(u32, u32)],
        pace: Option<PaceSegment>,
        first_batch: u64,
        batches: &mut Vec<Vec<u8>>,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        let link = &self.ordering.as_ref().ok_or(RelayError::Unregistered)?.link;
        let saturate = |value: Option<u32>| value.map_or(0, |value| value.min(65535) as u16);
        let uplink = Uplink {
            flags,
            recv_next,
            down: self.hop.down.feedback_window(),
            srtt_ms: saturate(link.rtt.srtt),
            base_ms: saturate(link.rtt.base()),
            // 0 with srtt (spec §3.2).
            rttvar_ms: saturate(link.rtt.srtt.map(|_| link.rtt.rttvar)),
            link: link.link_byte(),
            checksums: checksums.to_vec(),
            pace,
            first_batch_id: first_batch,
            batches: batches.iter().map(Vec::as_slice).collect(),
        };
        let mut uplink = uplink;
        uplink.fit(PACK_LIMIT);
        let fitting = uplink.batches.len();
        let mut payload = Vec::with_capacity(PACK_LIMIT);
        uplink.encode(&mut payload);
        batches.truncate(fitting);
        self.encode(CONTROL, &payload, out)
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
        if self.open_uplink().is_none() || self.hop.uplinks == 0 {
            return false;
        }
        self.clock_ms = now_ms;
        self.evaluate_uplink(now_ms);
        let Some(ordering) = self.open_uplink() else {
            return false;
        };
        let schedule = &ordering.link.schedule;
        if since(schedule.last_uplink_ms, now_ms) < UPLINK_HEARTBEAT_MS {
            return false;
        }
        if ordering.link.clock.state == LinkState::Offline {
            return self.send_uplink(Kind::Probe, false, now_ms, out).is_ok();
        }
        let Some(last) = schedule.last.clone() else {
            return self.send_uplink(Kind::Probe, false, now_ms, out).is_ok();
        };
        // Batches as §6.2 packs them now; the checksum segment repeats.
        let (first_batch, mut batches) = fitting_batches(ordering);
        if self
            .encode_uplink(
                last.flags,
                last.recv_next,
                &last.checksums,
                None,
                first_batch,
                &mut batches,
                out,
            )
            .is_err()
        {
            return false;
        }
        self.hop.uplinks += 1;
        if let Some(ordering) = self.ordering.as_mut() {
            let schedule = &mut ordering.link.schedule;
            schedule.last_uplink_ms = Some(now_ms);
            if !batches.is_empty() {
                schedule.batches_carried_ms = Some(now_ms);
            }
        }
        true
    }

    /// The earliest `now_ms` at which `poll_uplink` could return true without
    /// any further input (receive, content or gap report); it may already be
    /// past. `None` before the start or after Leave.
    pub fn next_uplink_deadline(&self) -> Option<u64> {
        let ordering = self.open_uplink()?;
        let link = &ordering.link;
        let schedule = &link.schedule;
        let state = link.clock.state;
        let last = schedule.last_uplink_ms;
        let after = |interval: u64| last.map_or(0, |at| at.saturating_add(interval));
        let heartbeat = after(UPLINK_HEARTBEAT_MS);
        if state == LinkState::Offline {
            return Some(if self.hop.uplinks == 0 { 0 } else { heartbeat });
        }
        if self.hop.uplinks == 0 || link.recovered {
            return Some(0);
        }
        let mut deadline = heartbeat;
        let mut consider = |at: u64| deadline = deadline.min(at);
        let newest = ordering.next_batch - 1;
        if newest > schedule.sent_batches_through.max(ordering.accepted_through) {
            consider(if state == LinkState::Congested {
                after(CONGESTED_SEND_SPACING_MS)
            } else {
                0
            });
        }
        if let Some(gap) = schedule.gap.filter(|gap| !gap.resent) {
            let grace = if let Some(first) = gap.uplinked {
                first + gap_resend_ms(link)
            } else {
                match gap.kind {
                    OrderingGap::Reordered => 0,
                    OrderingGap::BehindStatus => gap.since_ms + u64::from(schedule.turn_ms),
                }
            };
            consider(after(GAP_UPLINK_SPACING_MS).max(grace));
        }
        if schedule.recv_next > schedule.acked_recv_next {
            consider(after(ack_interval(state, schedule.turn_ms)));
        }
        if let Some((_, at)) = &schedule.checksums {
            consider(at + CHECKSUM_SEND_MS);
        }
        if newest > ordering.accepted_through {
            consider(
                schedule
                    .batches_carried_ms
                    .map_or(0, |at| at + link.rtt.rto(link.rto_backoff)),
            );
        }
        if !self.hop.first_pack {
            consider(after(link.rtt.rto(0)));
        }
        if state == LinkState::Lossy {
            for copies in &schedule.copies {
                if copies.batch > ordering.accepted_through {
                    consider(copies.due_ms);
                }
            }
        }
        Some(deadline)
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
        let ordering = self.ordering.as_ref().ok_or(RelayError::Unregistered)?;
        if !ordering.link.schedule.started
            || ordering.restoring
            || ordering.leave.is_some()
            || self.is_watching()
        {
            return Err(RelayError::Unregistered);
        }
        self.seal_batch(plaintext)
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
        if samples.is_empty() || samples.len() > super::codec::MAX_UPLINK_CHECKSUMS {
            return Err(RelayError::Malformed);
        }
        let ordering = self.ordering.as_mut().ok_or(RelayError::Unregistered)?;
        let schedule = &mut ordering.link.schedule;
        let since = schedule.checksums.as_ref().map_or(now_ms, |(_, at)| *at);
        schedule.checksums = Some((samples.to_vec(), since));
        Ok(())
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
        if let Some(ordering) = self.ordering.as_mut() {
            ordering.link.schedule.pace = Some(PaceSegment {
                pace,
                frames_per_second,
                tick_micros: tick_micros.unwrap_or(0),
            });
        }
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
        let Some(ordering) = self.ordering.as_mut() else {
            return;
        };
        let link = &mut ordering.link;
        match (link.schedule.gap.as_mut(), gap) {
            (None, Some(kind)) => {
                link.gaps += 1;
                link.schedule.gap = Some(Gap {
                    kind,
                    since_ms: now_ms,
                    uplinked: None,
                    resent: false,
                    pulls: 0,
                    next_pull_ms: None,
                    watermark: link.schedule.recv_next.max(1),
                });
            }
            (Some(open), Some(kind)) => open.kind = kind,
            (Some(open), None) => {
                if open.uplinked.is_some() && open.pulls == 0 {
                    link.gaps_repaired_by_push += 1;
                }
                link.schedule.gap = None;
            }
            (None, None) => {}
        }
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
        let Some(ordering) = self
            .open_uplink_mut()
            .filter(|ordering| !ordering.restoring)
        else {
            return false;
        };
        let grace = pull_grace(&ordering.link);
        let schedule = &mut ordering.link.schedule;
        let recv_next = schedule.recv_next.max(1);
        let far = schedule.status_next_seq > recv_next + FAR_BEHIND;
        let due = if far {
            schedule.far_pull_ms.is_none_or(|at| at <= now_ms)
        } else {
            schedule.far_pull_ms = None;
            match schedule.gap.as_mut() {
                Some(gap) => {
                    if recv_next > gap.watermark {
                        gap.watermark = recv_next;
                        gap.since_ms = now_ms;
                        gap.pulls = 0;
                        gap.next_pull_ms = None;
                    }
                    gap.next_pull_ms.unwrap_or(gap.since_ms + grace) <= now_ms
                }
                None => false,
            }
        };
        if !due {
            return false;
        }
        self.clock_ms = now_ms;
        if self.pull_ordering(recv_next, out).is_err() {
            return false;
        }
        if let Some(ordering) = self.ordering.as_mut() {
            let link = &mut ordering.link;
            link.repair_pulls += 1;
            let schedule = &mut link.schedule;
            if far {
                schedule.far_pull_ms = Some(now_ms + grace);
            } else if let Some(gap) = schedule.gap.as_mut() {
                gap.pulls += 1;
                let backoff = grace
                    .checked_shl(gap.pulls)
                    .unwrap_or(PULL_GRACE_MAX)
                    .min(PULL_GRACE_MAX);
                gap.next_pull_ms = Some(now_ms + backoff);
            }
        }
        true
    }

    /// Receipt bookkeeping of `receive_at`: the start (spec §6.0) and a PACK
    /// confirming this hop's subscription.
    pub(crate) fn note_status_next(&mut self, next_seq: u64) {
        if let Some(ordering) = self.ordering.as_mut() {
            let schedule = &mut ordering.link.schedule;
            schedule.status_next_seq = schedule.status_next_seq.max(next_seq);
        }
    }

    pub(crate) fn note_ordering_receipt(&mut self, started: bool, pack: bool) {
        if pack {
            self.hop.first_pack = true;
        }
        if started && let Some(ordering) = self.ordering.as_mut() {
            ordering.link.schedule.started = true;
        }
    }
}

#[cfg(test)]
impl ClientSession {
    /// Test support: an UPLINK with the queued content and only batch `id`
    /// (sealed or retained), sent regardless of the schedule.
    pub(crate) fn test_uplink(
        &mut self,
        batch: Option<u64>,
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        let ordering = self.ordering.as_ref().ok_or(RelayError::Unregistered)?;
        let schedule = &ordering.link.schedule;
        let checksums = schedule
            .checksums
            .as_ref()
            .map_or(Vec::new(), |(samples, _)| samples.clone());
        let pace = schedule.pace;
        let mut flags = FLAG_PUSH;
        if ordering.restoring {
            flags |= FLAG_RESTORING;
        }
        let mut batches: Vec<Vec<u8>> = batch
            .and_then(|id| ordering.sealed.iter().find(|sealed| sealed.id == id))
            .map(|sealed| sealed.opaque.clone())
            .into_iter()
            .collect();
        let recv_next = schedule.recv_next.max(1);
        self.encode_uplink(
            flags,
            recv_next,
            &checksums,
            pace,
            batch.unwrap_or(0),
            &mut batches,
            out,
        )?;
        let schedule = &mut self
            .ordering
            .as_mut()
            .ok_or(RelayError::Unregistered)?
            .link
            .schedule;
        schedule.checksums = None;
        schedule.pace = None;
        Ok(())
    }

    /// Test support for the removed SUBMIT: seals `plaintext` as batch `id`,
    /// or resends batch `id` while it is retained, alone in an UPLINK.
    pub(crate) fn test_submit(
        &mut self,
        id: u64,
        plaintext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        let retained = self
            .ordering
            .as_ref()
            .and_then(|ordering| ordering.sealed.iter().find(|batch| batch.id == id))
            .map(|batch| batch.opaque.clone());
        match retained {
            Some(opaque) => assert_eq!(opaque, plaintext, "a retry resends the sealed bytes"),
            None => assert_eq!(self.seal_batch(plaintext)?, id, "batch IDs are consecutive"),
        }
        self.test_uplink(Some(id), self.clock_ms, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::decode_uplink;
    use crate::{HEADER_LEN, HOP_TAG_LEN, OrderPace, OrderingCertificate, SecretKey, Ticket};

    /// A ready, started session at 0 whose first PACK arrived, its uplink
    /// measured good.
    fn started() -> ClientSession {
        let key = SecretKey::new([3; 32]);
        let ticket = Ticket {
            room: [5; 16],
            source: 0,
            expires: 1200,
            nonce: [1; 16],
            player: [1; 16],
            generation: 1,
        }
        .sign(&key)
        .unwrap();
        let mut session = ClientSession::new(
            &ticket,
            *crate::derive_packet_key(&key, &ticket).expose_for_https(),
            [9; 32],
        )
        .unwrap();
        session.ready = true;
        session.session = Some(1);
        let certificate = OrderingCertificate {
            room: [5; 16],
            sources_mask: 3,
            tick_hz: 30,
            expires: 1400,
            pacing: Default::default(),
            ordered_alone: false,
        }
        .sign(&key)
        .unwrap();
        session
            .configure_ordering_at(&certificate, [7; 32], 0, &mut Vec::new())
            .unwrap();
        session.hop.last_heard_ms = Some(0);
        session.note_ordering_receipt(true, true);
        session.preset_uplink_measured_good();
        session
    }

    #[test]
    fn a_measured_good_preset_before_configuring_starts_the_uplink_good() {
        let key = SecretKey::new([4; 32]);
        let ticket = Ticket {
            room: [5; 16],
            source: 0,
            expires: 1200,
            nonce: [1; 16],
            player: [1; 16],
            generation: 1,
        }
        .sign(&key)
        .unwrap();
        let mut session = ClientSession::new(
            &ticket,
            *crate::derive_packet_key(&key, &ticket).expose_for_https(),
            [9; 32],
        )
        .unwrap();
        session.preset_uplink_measured_good();
        session.ready = true;
        session.session = Some(1);
        let certificate = OrderingCertificate {
            room: [5; 16],
            sources_mask: 3,
            tick_hz: 30,
            expires: 1400,
            pacing: Default::default(),
            ordered_alone: false,
        }
        .sign(&key)
        .unwrap();
        session
            .configure_ordering_at(&certificate, [7; 32], 0, &mut Vec::new())
            .unwrap();
        session.hop.last_heard_ms = Some(0);
        session.evaluate_uplink(0);
        let stats = session.link_stats();
        assert_eq!((stats.uplink, stats.up_loss_folds), (LinkState::Good, 1));
    }

    #[test]
    fn the_uplink_copies_from_the_start_until_its_first_fold_measures_it() {
        let mut session = started();
        session.ordering.as_mut().unwrap().link.loss = crate::link::LossStats::initial();
        poll(&mut session, 0).unwrap();
        assert_eq!(session.link_stats().uplink, LinkState::Lossy);
        session.seal_ordering_batch(b"early").unwrap();
        poll(&mut session, 10).unwrap();
        assert!(poll(&mut session, 26).is_some(), "a copy before any fold");
        // 32 judged, none lost: the first fold decides good.
        link(&mut session).loss.add(32, 0, 30);
        assert!(!link(&mut session).loss.lossy);
        poll(&mut session, 40);
        assert_eq!(session.link_stats().uplink, LinkState::Good);
    }

    /// One service at `now_ms` with the hop still hearing the relay.
    fn poll(session: &mut ClientSession, now_ms: u64) -> Option<Vec<u8>> {
        session.hop.last_heard_ms = Some(now_ms);
        poll_silent(session, now_ms)
    }

    fn poll_silent(session: &mut ClientSession, now_ms: u64) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        session.poll_uplink(now_ms, &mut out).then_some(out)
    }

    fn decoded(datagram: &[u8]) -> Uplink<'_> {
        decode_uplink(&datagram[HEADER_LEN..datagram.len() - HOP_TAG_LEN]).unwrap()
    }

    fn link(session: &mut ClientSession) -> &mut crate::link::ClientLink {
        &mut session.ordering.as_mut().unwrap().link
    }

    #[test]
    fn the_first_uplink_goes_at_the_start_and_repeats_every_rto_until_a_pack() {
        let mut session = started();
        session.hop.first_pack = false;
        let first = poll(&mut session, 0).expect("the start sends at once");
        assert_eq!(decoded(&first).flags, FLAG_PUSH);
        // No sample: srtt_ms 0 and rttvar_ms 0 (spec §3.2).
        assert_eq!((decoded(&first).srtt_ms, decoded(&first).rttvar_ms), (0, 0));
        assert!(poll(&mut session, 0).is_none(), "one UPLINK per service");
        assert!(poll(&mut session, 99).is_none());
        assert!(
            poll(&mut session, 100).is_some(),
            "RTO 100 without a sample"
        );
        session.note_ordering_receipt(false, true);
        assert!(poll(&mut session, 349).is_none());
        assert!(poll(&mut session, 350).is_some(), "the 250 ms heartbeat");
        assert_eq!(session.next_uplink_deadline(), Some(600));
    }

    #[test]
    fn the_uplink_stays_congested_ten_seconds_after_its_last_backpressure() {
        let mut session = started();
        link(&mut session).backpressure_at = Some(1000);
        link(&mut session).loss.lossy = true;
        poll(&mut session, 1000);
        assert_eq!(session.link_stats().uplink, LinkState::Congested);
        // The 2 s Backpressure condition ended at 3000; the hold runs from
        // its last evaluation, 2999, so lossy copies wait until 12_999.
        session.hop.last_heard_ms = Some(12_998);
        session.evaluate_uplink(2999);
        session.evaluate_uplink(12_998);
        assert_eq!(session.link_stats().uplink, LinkState::Congested);
        session.seal_ordering_batch(b"held").unwrap();
        session.hop.last_heard_ms = Some(12_998);
        poll_silent(&mut session, 12_998).unwrap();
        assert!(
            poll(&mut session, 13_014).is_none(),
            "no copy while congested"
        );
        session.evaluate_uplink(12_999);
        assert_eq!(session.link_stats().uplink, LinkState::Lossy);
    }

    #[test]
    fn a_new_batch_goes_at_once_but_waits_33_ms_and_merges_while_congested() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.seal_ordering_batch(b"first").unwrap();
        assert_eq!(decoded(&poll(&mut session, 1).unwrap()).batches.len(), 1);
        link(&mut session).backpressure_at = Some(5);
        session.seal_ordering_batch(b"second").unwrap();
        assert!(poll(&mut session, 33).is_none());
        session.seal_ordering_batch(b"third").unwrap();
        let merged = poll(&mut session, 34).unwrap();
        assert_eq!(decoded(&merged).first_batch_id, 1);
        assert_eq!(decoded(&merged).batches.len(), 3);
        assert_eq!(decoded(&merged).link, LinkState::Congested as u8);
    }

    #[test]
    fn a_gap_uplinks_16_ms_on_or_after_a_turn_behind_a_status() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.set_ordering_gap(Some(OrderingGap::Reordered), 5);
        assert!(poll(&mut session, 15).is_none());
        assert!(poll(&mut session, 16).is_some());
        session.set_ordering_gap(None, 17);
        session.set_ordering_gap(Some(OrderingGap::BehindStatus), 20);
        // turn_ms at 30 Hz is 67.
        assert!(poll(&mut session, 86).is_none());
        assert!(poll(&mut session, 87).is_some());
    }

    #[test]
    fn a_lasting_gap_uplinks_once_more_after_four_rttvar_then_waits_for_the_pull() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        link(&mut session).rtt.sample(40, 0); // rttvar 20: again at 80 ms
        session.set_ordering_gap(Some(OrderingGap::Reordered), 20);
        assert!(poll(&mut session, 36).is_some(), "the first, 16 ms on");
        assert!(poll(&mut session, 115).is_none());
        assert!(
            poll(&mut session, 116).is_some(),
            "the second, max(16, 4 * 20) after the first"
        );
        assert!(poll(&mut session, 300).is_none(), "at most two per gap");
    }

    #[test]
    fn a_gap_behind_a_status_resends_only_after_its_first_uplink() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.set_ordering_gap(Some(OrderingGap::BehindStatus), 20);
        // max(16, 4 * 0) from the gap would be 36, before the turn's grace.
        assert!(poll(&mut session, 86).is_none());
        assert!(poll(&mut session, 87).is_some(), "the first, after turn_ms");
        assert!(poll(&mut session, 102).is_none());
        assert!(poll(&mut session, 103).is_some(), "the second, 16 ms later");
    }

    #[test]
    fn a_watermark_advance_is_acknowledged_after_its_state_interval() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.set_ordering_received_next(5);
        assert!(poll(&mut session, 99).is_none());
        assert_eq!(decoded(&poll(&mut session, 100).unwrap()).recv_next, 5);
        link(&mut session).loss.lossy = true;
        session.set_ordering_received_next(9);
        assert!(poll(&mut session, 166).is_none());
        assert!(poll(&mut session, 167).is_some());
        link(&mut session).backpressure_at = Some(167);
        session.set_ordering_received_next(12);
        assert!(poll(&mut session, 366).is_none());
        assert!(poll(&mut session, 367).is_some());
    }

    #[test]
    fn unacknowledged_batches_resend_after_a_doubling_rto() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.seal_ordering_batch(b"lost").unwrap();
        poll(&mut session, 10).unwrap();
        assert!(poll(&mut session, 109).is_none());
        assert!(poll(&mut session, 110).is_some());
        assert_eq!(session.link_stats().rto_backoff, 1);
        assert!(poll(&mut session, 309).is_none());
        assert!(poll(&mut session, 310).is_some());
        let stats = session.link_stats();
        assert_eq!((stats.rto_backoff, stats.rto_timeouts), (2, 2));
    }

    #[test]
    fn lossy_copies_follow_16_ms_apart_without_pace() {
        let mut session = started();
        link(&mut session).loss.lossy = true;
        poll(&mut session, 0).unwrap();
        session.seal_ordering_batch(b"copied").unwrap();
        session.queue_ordering_pace(OrderPace::KeepingUp, 30, None);
        assert!(decoded(&poll(&mut session, 10).unwrap()).pace.is_some());
        assert!(poll(&mut session, 25).is_none());
        let copy = poll(&mut session, 26).unwrap();
        assert_eq!(decoded(&copy).batches.len(), 1);
        assert!(decoded(&copy).pace.is_none());
        assert!(
            poll(&mut session, 60).is_none(),
            "one copy outside severe loss"
        );
        // Severe loss: two copies; a late service sends one, the next 16 ms on.
        session.ordering.as_mut().unwrap().accepted_through = 1;
        link(&mut session).loss.severe = true;
        session.seal_ordering_batch(b"severe").unwrap();
        poll(&mut session, 100).unwrap();
        assert!(poll(&mut session, 140).is_some());
        assert!(poll(&mut session, 155).is_none());
        assert!(poll(&mut session, 156).is_some());
        assert!(poll(&mut session, 200).is_none());
        assert_eq!(session.link_stats().copies, 3);
    }

    #[test]
    fn a_stream_transport_sends_no_lossy_copies_but_still_resends_by_rto() {
        let mut session = started();
        session.use_stream_transport();
        link(&mut session).loss.lossy = true;
        link(&mut session).loss.severe = true;
        poll(&mut session, 0).unwrap();
        session.seal_ordering_batch(b"streamed").unwrap();
        assert_eq!(decoded(&poll(&mut session, 10).unwrap()).batches.len(), 1);
        assert!(poll(&mut session, 26).is_none(), "no copy 16 ms on");
        assert!(poll(&mut session, 42).is_none(), "no severe second copy");
        let resent = poll(&mut session, 200).expect("the RTO resend");
        assert_eq!(decoded(&resent).batches.len(), 1);
        assert_eq!(session.link_stats().copies, 0);
    }

    #[test]
    fn a_renewed_hop_keeps_the_stream_transport() {
        let key = SecretKey::new([3; 32]);
        let mut session = started();
        session.use_stream_transport();
        let ticket = Ticket {
            room: [5; 16],
            source: 0,
            expires: 1500,
            nonce: [2; 16],
            player: [1; 16],
            generation: 1,
        }
        .sign(&key)
        .unwrap();
        let packet_key = *crate::derive_packet_key(&key, &ticket).expose_for_https();
        assert!(
            session
                .renewal(&ticket, packet_key, [9; 32])
                .unwrap()
                .stream_transport
        );
    }

    #[test]
    fn a_batch_first_sent_by_a_stand_in_for_a_copy_gets_its_own_copy() {
        let mut session = started();
        link(&mut session).loss.lossy = true;
        poll(&mut session, 0).unwrap();
        session.seal_ordering_batch(b"first").unwrap();
        poll(&mut session, 10).unwrap();
        // Batch 2 is new when batch 1's copy is due: one UPLINK serves both.
        session.seal_ordering_batch(b"second").unwrap();
        assert_eq!(decoded(&poll(&mut session, 26).unwrap()).batches.len(), 2);
        assert_eq!(session.link_stats().copies, 0, "a stand-in is no copy");
        let copy = poll(&mut session, 42).expect("batch 2's own copy");
        assert_eq!(decoded(&copy).batches.len(), 2);
        assert_eq!(session.link_stats().copies, 1);
    }

    #[test]
    fn jittery_new_turn_arrivals_set_link_bit3_but_pull_replies_are_not_measured() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        let frame_end = |sequence: u64, frame: u32| {
            vec![crate::OrderEvent::Entry(crate::OrderEntry::FrameEnd {
                sequence,
                frame,
            })]
        };
        // 30 Hz two-frame turns, alternately on time and 60 ms late.
        let mut at = 0;
        for turn in 1..=18u64 {
            at += if turn % 2 == 0 { 127 } else { 7 };
            let recv_next = 2 * turn - 1;
            session.set_ordering_received_next(recv_next);
            session.measure_jitter(&frame_end(recv_next + 1, 2 * turn as u32), at);
        }
        assert!(session.link_stats().jittery);
        session.hop.last_heard_ms = Some(at);
        let jittery = poll_silent(&mut session, at + 300).unwrap();
        assert_eq!(decoded(&jittery).link, crate::link::LINK_JITTERY);
        // A PULL's reply packs, after its STATUS, take no sample.
        let samples = session.link_stats().jitter_us;
        let link = link(&mut session);
        link.pull_reply_ms = Some(at + 400);
        session.measure_jitter(&frame_end(100, 100), at + 400);
        session.measure_jitter(&frame_end(101, 101), at + 400);
        assert_eq!(session.link_stats().jitter_us, samples);
    }

    #[test]
    fn acknowledged_batches_never_time_out_a_later_first_send() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.seal_ordering_batch(b"one").unwrap();
        poll(&mut session, 10).unwrap();
        session.ordering.as_mut().unwrap().accepted_through = 1;
        session.seal_ordering_batch(b"two").unwrap();
        poll(&mut session, 500).unwrap();
        let stats = session.link_stats();
        assert_eq!((stats.rto_timeouts, stats.rto_backoff), (0, 0));
    }

    #[test]
    fn while_congested_no_uplink_carries_a_new_batch_within_33_ms() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        link(&mut session).backpressure_at = Some(0);
        session.seal_ordering_batch(b"held").unwrap();
        session.set_ordering_gap(Some(OrderingGap::Reordered), 1);
        assert!(poll(&mut session, 16).is_none(), "the gap waits with it");
        assert!(poll(&mut session, 32).is_none());
        assert_eq!(decoded(&poll(&mut session, 33).unwrap()).batches.len(), 1);
    }

    #[test]
    fn offline_sends_only_probes_and_recovery_resends_every_batch_at_once() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        let probe = poll_silent(&mut session, 1000).unwrap();
        assert_eq!(decoded(&probe).flags, FLAG_PUSH | FLAG_PROBE);
        assert_eq!(decoded(&probe).link & 3, LinkState::Offline as u8);
        session.seal_ordering_batch(b"held").unwrap();
        assert!(poll_silent(&mut session, 1249).is_none());
        assert!(
            decoded(&poll_silent(&mut session, 1250).unwrap())
                .batches
                .is_empty()
        );
        let recovery = poll(&mut session, 1300).unwrap();
        assert_eq!(decoded(&recovery).batches.len(), 1);
        assert_eq!(decoded(&recovery).link & 3, LinkState::Good as u8);
    }

    #[test]
    fn checksums_wait_100_ms_for_an_uplink_and_pace_rides_one_only() {
        let mut session = started();
        poll(&mut session, 0).unwrap();
        session.queue_ordering_checksums(&[(15, 7)], 10).unwrap();
        assert!(poll(&mut session, 109).is_none());
        assert_eq!(
            decoded(&poll(&mut session, 110).unwrap()).checksums,
            [(15, 7)]
        );
        session.queue_ordering_pace(OrderPace::Behind, 25, Some(2100));
        let paced = poll(&mut session, 360).unwrap();
        assert_eq!(decoded(&paced).pace.unwrap().tick_micros, 2100);
        assert!(decoded(&paced).checksums.is_empty());
        assert!(decoded(&poll(&mut session, 610).unwrap()).pace.is_none());
    }

    #[test]
    fn a_gap_pulls_after_its_grace_and_backs_off_unless_the_watermark_moves() {
        let mut session = started();
        let mut out = Vec::new();
        poll(&mut session, 0).unwrap();
        session.set_ordering_gap(Some(OrderingGap::Reordered), 5);
        // No sample: G = 100 + 4 * 50 + turn_ms 67.
        assert!(!session.poll_repair_pull(371, &mut out));
        assert!(session.poll_repair_pull(372, &mut out));
        assert!(!session.poll_repair_pull(1105, &mut out), "twice G next");
        assert!(session.poll_repair_pull(1106, &mut out));
        // Progress restarts the grace from the service that sees it; the
        // closed gap was not repaired by push.
        session.set_ordering_received_next(4);
        assert!(!session.poll_repair_pull(1200, &mut out));
        assert!(!session.poll_repair_pull(1566, &mut out));
        assert!(session.poll_repair_pull(1567, &mut out));
        session.set_ordering_gap(None, 1600);
        let stats = session.link_stats();
        assert_eq!(
            (stats.gaps, stats.repair_pulls, stats.gaps_repaired_by_push),
            (1, 3, 0)
        );
        // A STATUS 64 or more past the watermark pulls at once, then every G.
        session.note_status_next(68);
        assert!(
            !session.poll_repair_pull(1600, &mut out),
            "64 ahead is near"
        );
        session.note_status_next(69);
        assert!(session.poll_repair_pull(1600, &mut out));
        assert!(!session.poll_repair_pull(1966, &mut out));
        assert!(session.poll_repair_pull(1967, &mut out));
    }

    #[test]
    fn the_io_heartbeat_repeats_the_last_uplink_without_pace_or_probes_offline() {
        let mut session = started();
        session.seal_ordering_batch(b"kept").unwrap();
        session.queue_ordering_checksums(&[(15, 7)], 0).unwrap();
        session.queue_ordering_pace(OrderPace::KeepingUp, 30, None);
        poll(&mut session, 0).unwrap();
        let mut out = Vec::new();
        assert!(!session.heartbeat_uplink(249, &mut out));
        assert!(session.heartbeat_uplink(250, &mut out));
        let repeated = decoded(&out);
        assert_eq!((repeated.batches.len(), repeated.checksums.len()), (1, 1));
        assert!(repeated.pace.is_none());
        assert!(
            poll(&mut session, 251).is_none(),
            "the heartbeat was an UPLINK"
        );
        // Silent since 251: offline at 1251, so the heartbeat probes.
        assert!(session.heartbeat_uplink(1251, &mut out));
        assert_eq!(decoded(&out).flags, FLAG_PUSH | FLAG_PROBE);
    }
}
