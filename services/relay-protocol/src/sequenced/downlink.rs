//! The relay's per-reader downlink (spec §7): state-driven new packs with
//! piggyback, retransmission on loss, 8 ms repeats in the lossy state and
//! tail probes. The reader's link measurement is its hop's `ReaderLink`.

use std::collections::VecDeque;

use super::*;
use crate::link::{
    LinkState, SEND_FRESH, SEND_OTHER, SEND_PROBE, SEND_REPEAT, SEND_RETRANSMIT, SendRecord, pto,
};

/// The acknowledgement interval of a congested reader (spec §6.3 row 3).
const CONGESTED_ACK_DELAY_MS: u64 = 200;
const GOOD_ACK_DELAY_MS: u64 = 100;
/// `srtt_c` until a reader reports one (spec §4.6, §7.3).
const DEFAULT_SRTT_MS: u64 = 100;
/// Records the retransmission queue may owe a reader at most; the rest is
/// left to the fallback PULL (spec §7.3, §7.7).
const RETRANSMIT_QUEUE_RECORDS: u64 = PUSH_WINDOW + ORDERING_PULL_LIMIT as u64;

/// `ranges` minus `cut`, in order. Ranges are `[from, to)` of record seqs.
pub(super) fn subtract(ranges: &[(u64, u64)], cut: (u64, u64)) -> Vec<(u64, u64)> {
    let mut out = Vec::with_capacity(ranges.len() + 1);
    for &(from, to) in ranges {
        if cut.1 <= from || cut.0 >= to {
            out.push((from, to));
            continue;
        }
        if from < cut.0 {
            out.push((from, cut.0));
        }
        if cut.1 < to {
            out.push((cut.1, to));
        }
    }
    out
}

/// Merges `range` into the sorted, disjoint `queue`, joining touching ranges
/// (spec §7.3 step 3).
pub(super) fn merge(queue: &mut VecDeque<(u64, u64)>, range: (u64, u64)) {
    if range.0 >= range.1 {
        return;
    }
    let (mut from, mut to) = range;
    let mut kept = VecDeque::with_capacity(queue.len() + 1);
    for &(start, end) in queue.iter() {
        if end < from || start > to {
            kept.push_back((start, end));
        } else {
            from = from.min(start);
            to = to.max(end);
        }
    }
    let at = kept.partition_point(|&(start, _)| start < from);
    kept.insert(at, (from, to));
    *queue = kept;
}

/// New packs with piggyback (spec §7.4 step 2): the packs of `new` that fit
/// `datagrams`, preceded by as many of the newest `old` records as those
/// packs can also hold. Returns the packs, the new records packed and the
/// old records piggybacked.
pub(super) fn piggyback_packs(
    old: &[Vec<u8>],
    new: &[Vec<u8>],
    datagrams: usize,
) -> (Vec<(Vec<u8>, usize)>, usize, usize) {
    let fresh = super::pack::encode_pack_v2(new, datagrams);
    let count: usize = fresh.iter().map(|(_, entries)| entries).sum();
    if fresh.is_empty() || old.is_empty() {
        return (fresh, count, 0);
    }
    let limit = fresh.len();
    // The fewest old records to drop, counted as PACK v2 packs, with
    // one copy for the packs that go out.
    let new = &new[..count];
    let all: Vec<&[u8]> = old.iter().chain(new).map(Vec::as_slice).collect();
    let Some(skip) = (0..old.len()).find(|&skip| super::pack::fits_packs_v2(&all[skip..], limit))
    else {
        return (fresh, count, 0);
    };
    let records: Vec<Vec<u8>> = old[skip..].iter().chain(new).cloned().collect();
    (
        super::pack::encode_pack_v2(&records, usize::MAX),
        count,
        old.len() - skip,
    )
}

/// Whether a reader in `state` gets the 8 ms repeat (spec §7.2, §7.5):
/// lossy, or good and JITTERY.
pub(super) fn repeats_on(state: LinkState, jittery: bool) -> bool {
    state == LinkState::Lossy || (state == LinkState::Good && jittery)
}

impl PushSubscription {
    /// The §5.5 side effects of the reader's state change on this
    /// subscription.
    pub(super) fn on_transition(&mut self, previous: LinkState, state: LinkState, jittery: bool) {
        if previous == LinkState::Offline {
            // Push again from the watermark.
            self.sent_next = self.acknowledged_next;
            self.repeat_next = self.acknowledged_next;
            self.unrepeated_since = None;
            self.retransmit.clear();
            self.retransmitted.clear();
            self.pto_backoff = 0;
        } else if repeats_on(previous, jittery) && !repeats_on(state, jittery) {
            self.unrepeated_since = None;
            self.repeat_next = self.sent_next;
        }
    }

    /// Merges an UPLINK watermark; progress ends the PTO backoff (§4.6).
    pub(super) fn acknowledge(&mut self, cursor: u64, now_ms: u64) {
        if cursor > self.acknowledged_next {
            self.acknowledged_next = cursor;
            self.last_ack_progress_ms = now_ms;
            self.pto_backoff = 0;
        }
        self.sent_next = self.sent_next.max(cursor);
        self.retransmit.retain(|&(_, to)| to > cursor);
    }

    /// Queues what a lost send still owes (spec §7.3 steps 1-4): its range,
    /// less the acknowledged records, what later sends not judged lost
    /// cover, and what went as a retransmission within `srtt_c`. Returns
    /// the records queued.
    pub(super) fn enqueue_lost<'a>(
        &mut self,
        lost: &SendRecord,
        later: impl Iterator<Item = &'a SendRecord>,
        srtt_c: u16,
        now_ms: u64,
    ) -> u64 {
        let end = lost.first + u64::from(lost.count);
        let mut owed = vec![(lost.first.max(self.acknowledged_next), end)];
        for send in later.filter(|send| send.count > 0 && !send.lost) {
            owed = subtract(&owed, (send.first, send.first + u64::from(send.count)));
        }
        let srtt = if srtt_c == 0 {
            DEFAULT_SRTT_MS
        } else {
            u64::from(srtt_c)
        };
        self.retransmitted
            .retain(|&(_, _, at)| now_ms.saturating_sub(at) < srtt);
        for &(from, to, _) in &self.retransmitted {
            owed = subtract(&owed, (from, to));
        }
        let mut queued = 0;
        for range in owed.into_iter().filter(|(from, to)| from < to) {
            queued += range.1 - range.0;
            merge(&mut self.retransmit, range);
        }
        self.cap_retransmit();
        queued
    }

    /// Keeps the oldest `RETRANSMIT_QUEUE_RECORDS` records of the queue.
    fn cap_retransmit(&mut self) {
        let mut left = RETRANSMIT_QUEUE_RECORDS;
        self.retransmit.retain_mut(|(from, to)| {
            if left == 0 {
                return false;
            }
            *to = (*to).min(*from + left);
            left -= *to - *from;
            true
        });
    }
}

impl Peer {
    /// Queues what the sends judged `lost` still owe for retransmission
    /// (spec §7.3). Returns how many sends were lost.
    pub(crate) fn owe_lost(&mut self, lost: &[SendRecord], now_ms: u64) -> u64 {
        if let Some(push) = self.push.as_mut() {
            for send in lost.iter().filter(|send| send.count > 0) {
                push.enqueue_lost(
                    send,
                    self.link.sent_after(send.sequence),
                    self.link.srtt_c,
                    now_ms,
                );
            }
        }
        lost.len() as u64
    }

    /// Re-evaluates this reader's downlink state (spec §5.2) and applies the
    /// §5.5 side effects of a change. Returns the current state.
    pub(crate) fn evaluate_reader(&mut self, now_ms: u64) -> LinkState {
        if let Some(previous) = self.link.evaluate(now_ms) {
            let state = self.link.clock.state;
            if previous == LinkState::Offline {
                // Nothing sent before the recovery is judged.
                self.link.skip_judged_to(self.sent + 1);
            }
            if let Some(push) = self.push.as_mut() {
                push.on_transition(previous, state, self.link.jittery);
            }
        }
        self.link.clock.state
    }
}

/// One reader's new-data part of a pass (spec §7.2 items 1-3), computed
/// before any budget is spent.
struct FreshPlan {
    ticket: Ticket,
    // The status heartbeat, then retransmission packs.
    first: Vec<Vec<u8>>,
    status: Option<(bool, u32)>,
    // Retransmitted ranges: `[from, from + entries)`.
    retransmitted: Vec<(u64, u64)>,
    fresh: Vec<Vec<u8>>,
    fresh_entries: usize,
    piggybacked: usize,
    terminal: bool,
}

impl RelayServer {
    /// Drain a bounded fair share of already committed history. Canonical
    /// advancement remains solely in `advance_clock`; a slow reader consumes
    /// only its own delivery credit and never stalls another participant.
    /// Each reader gets, in order (spec §7.2): its status heartbeat,
    /// retransmissions and new packs from the new-data budget, then lossy
    /// repeats and a tail probe from the repeat budget; an offline reader
    /// gets nothing.
    pub fn drain_ordering_push(&mut self, now_ms: u64, now: u64) -> Vec<ForwardDatagram> {
        self.prune(now);
        let mut recipients = Vec::new();
        for (id, room) in &mut self.rooms {
            for peer in &mut room.peers {
                if room.ordering.is_none()
                    || peer.ticket.expires <= now
                    || peer.push.as_ref().is_some_and(|push| {
                        now_ms.saturating_sub(push.refreshed_at) >= PUSH_LEASE_MS
                    })
                {
                    peer.push = None;
                }
                if peer.push.is_some() {
                    recipients.push((*id, peer.ticket.source));
                }
            }
        }
        recipients.sort_unstable();
        if recipients.is_empty() {
            return Vec::new();
        }
        if let Some(after) = self.push_after {
            let start = recipients.partition_point(|recipient| *recipient <= after);
            let count = recipients.len();
            recipients.rotate_left(start % count);
        }
        self.ordering_counts.reader_passes += 1;
        let mut output = Vec::new();
        let mut output_bytes = 0;
        let mut room_packets = HashMap::new();
        // Each reader reached, with its stream end before this pass and the
        // new-data packets it was sent.
        let mut reached = Vec::new();
        for (id, source) in recipients {
            if output.len() == PUSH_DRAIN_PACKETS || output_bytes >= PUSH_DRAIN_BYTES {
                break;
            }
            let room_used = *room_packets.get(&id).unwrap_or(&0);
            let limit = PUSH_PEER_PACKETS
                .min(PUSH_ROOM_PACKETS - room_used)
                .min(PUSH_DRAIN_PACKETS - output.len());
            if limit == 0 {
                continue;
            }
            self.push_after = Some((id, source));
            let Some((datagrams, before)) = self.push_fresh(
                id,
                source,
                limit,
                PUSH_DRAIN_BYTES - output_bytes,
                now_ms,
                now,
            ) else {
                continue;
            };
            *room_packets.entry(id).or_insert(0) += datagrams.len();
            output_bytes += datagrams
                .iter()
                .map(|datagram| datagram.bytes.len())
                .sum::<usize>();
            reached.push((id, source, before, datagrams.len()));
            output.extend(datagrams);
        }
        // Repeats and probes use only what this pass has left and a budget
        // of their own, so they never delay or rate-limit a fresh entry or a
        // request reply. They copy entries sent before, which take no credit.
        for (id, source, before, fresh) in reached {
            if output.len() == PUSH_DRAIN_PACKETS || output_bytes >= PUSH_DRAIN_BYTES {
                break;
            }
            let room_used = *room_packets.get(&id).unwrap_or(&0);
            let capacity = (PUSH_PEER_PACKETS - fresh)
                .min(PUSH_ROOM_PACKETS - room_used)
                .min(PUSH_DRAIN_PACKETS - output.len())
                .min(self.repeat_packets.remaining(now, PUSH_REPEAT_PACKETS) as usize);
            let datagrams = self.push_repair(
                id,
                source,
                before,
                capacity,
                PUSH_DRAIN_BYTES - output_bytes,
                now_ms,
            );
            self.repeat_packets.charge(now, datagrams.len() as u64);
            *room_packets.entry(id).or_insert(0) += datagrams.len();
            output_bytes += datagrams
                .iter()
                .map(|datagram| datagram.bytes.len())
                .sum::<usize>();
            output.extend(datagrams);
        }
        output
    }

    /// Items 1-3 of spec §7.2 for one reader: none while it is offline or
    /// when the new-data budget refuses them. Returns the datagrams and the
    /// reader's stream end before them.
    fn push_fresh(
        &mut self,
        id: [u8; 16],
        source: u8,
        limit: usize,
        bytes_left: usize,
        now_ms: u64,
        now: u64,
    ) -> Option<(Vec<ForwardDatagram>, u64)> {
        let room = self.rooms.get_mut(&id)?;
        let peer = room
            .peers
            .iter_mut()
            .find(|peer| peer.ticket.source == source)?;
        let state = peer.evaluate_reader(now_ms);
        self.ordering_counts.reader_states[state.index()] += 1;
        let ordering = room.ordering.as_mut()?;
        let push = peer.push.as_mut()?;
        if state == LinkState::Offline {
            // About one new pack per new head the reader misses.
            if ordering.next_seq > push.sent_next.max(push.offline_seen_next) {
                self.ordering_counts.offline_unsent_packets += 1;
                push.offline_seen_next = ordering.next_seq;
            }
            return None;
        }
        let before = push.sent_next;
        let status_state = ordering.push_status_state(now_ms);
        let status_due = status_state != push.status_state
            || now_ms.saturating_sub(push.status_at) >= PUSH_STATUS_MS;
        let mut plan = FreshPlan {
            ticket: peer.ticket,
            first: Vec::new(),
            status: status_due.then_some(status_state),
            retransmitted: Vec::new(),
            fresh: Vec::new(),
            fresh_entries: 0,
            piggybacked: 0,
            terminal: false,
        };
        if status_due {
            plan.first.push(ordering.status(now_ms));
        }
        let mut datagrams = limit - usize::from(status_due);
        if !push.restoring {
            // Item 2: retransmissions, oldest range first (§7.3).
            for (from, to) in push.retransmit.iter().copied().collect::<Vec<_>>() {
                let from = from.max(push.acknowledged_next);
                if datagrams == 0 || from >= to {
                    continue;
                }
                // Read no more than this pass's packs can hold.
                let fits = datagrams * usize::from(u8::MAX);
                let Ok(records) = ordering.history_page(from, ((to - from) as usize).min(fits))
                else {
                    break;
                };
                let packs = super::pack::encode_pack_v2(&records, datagrams);
                let entries: usize = packs.iter().map(|(_, entries)| entries).sum();
                datagrams -= packs.len();
                plan.first.extend(packs.into_iter().map(|(pack, _)| pack));
                plan.retransmitted.push((from, from + entries as u64));
            }
            // Item 3: new packs with piggyback (§7.4). Credit counts only
            // new records.
            let credit = PUSH_WINDOW.saturating_sub(push.sent_next - push.acknowledged_next);
            if datagrams > 0 && credit > 0 {
                match ordering.history_page(push.sent_next, credit as usize) {
                    Ok(new) if !new.is_empty() => {
                        let depth = peer.link.piggyback_depth();
                        let start = peer
                            .link
                            .latest_fresh(usize::from(depth))
                            .iter()
                            .map(|send| send.first)
                            .min()
                            .map_or(push.sent_next, |first| {
                                first.max(push.acknowledged_next).min(push.sent_next)
                            });
                        let old = if start < push.sent_next {
                            ordering
                                .history_page(start, (push.sent_next - start) as usize)
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        let (packs, entries, piggybacked) = piggyback_packs(&old, &new, datagrams);
                        plan.fresh = packs.into_iter().map(|(pack, _)| pack).collect();
                        plan.fresh_entries = entries;
                        plan.piggybacked = piggybacked;
                    }
                    Ok(_) => {}
                    Err((code, expected)) => {
                        plan.first = error(code, expected);
                        plan.status = None;
                        plan.retransmitted.clear();
                        plan.terminal = true;
                    }
                }
            }
        }
        let bytes = plan
            .first
            .iter()
            .chain(&plan.fresh)
            .map(|payload| {
                envelope::OVERHEAD
                    + payload.len()
                    + if payload[0] == STATUS_EVENT {
                        crate::link::FEEDBACK_LEN
                    } else {
                        0
                    }
            })
            .sum::<usize>();
        if bytes > bytes_left {
            return None;
        }
        // Reuse the request-reply authentication and all three rate-budget
        // owners. Failed budget admission cannot move the stream cursor.
        let mut output = Vec::new();
        let ticket = plan.ticket;
        if !plan.first.is_empty() {
            let kinds: Vec<u8> = plan
                .first
                .iter()
                .map(|payload| {
                    if payload[0] == PACK_EVENT {
                        SEND_RETRANSMIT
                    } else {
                        SEND_OTHER
                    }
                })
                .collect();
            let first = std::mem::take(&mut plan.first);
            let Ok(datagrams) = self.ordering_reply(ticket, first, now, now_ms, SEND_RETRANSMIT)
            else {
                return None;
            };
            self.count_pushed(&datagrams, &kinds);
            output.extend(datagrams);
        }
        let mut fresh_sent = false;
        if !plan.fresh.is_empty() {
            let fresh = std::mem::take(&mut plan.fresh);
            let kinds = vec![SEND_FRESH; fresh.len()];
            if let Ok(datagrams) = self.ordering_reply(ticket, fresh, now, now_ms, SEND_FRESH) {
                self.count_pushed(&datagrams, &kinds);
                output.extend(datagrams);
                fresh_sent = true;
            }
        }
        let lossy = self.rooms.get(&id).and_then(|room| {
            room.peers
                .iter()
                .find(|peer| peer.ticket == ticket)
                .map(|peer| repeats_on(peer.link.clock.state, peer.link.jittery))
        })?;
        let push = self
            .rooms
            .get_mut(&id)?
            .peers
            .iter_mut()
            .find(|peer| peer.ticket == ticket)?
            .push
            .as_mut()?;
        if plan.terminal {
            self.rooms
                .get_mut(&id)?
                .peers
                .iter_mut()
                .find(|peer| peer.ticket == ticket)?
                .push = None;
            return Some((output, before));
        }
        if let Some(state) = plan.status {
            push.status_at = now_ms;
            push.status_state = state;
        }
        for &(from, to) in &plan.retransmitted {
            self.ordering_counts.retransmitted_records += to - from;
            push.retransmitted.push_back((from, to, now_ms));
            // Ranges go in order, so everything before `to` went.
            let queue: Vec<_> = push.retransmit.iter().copied().collect();
            push.retransmit = subtract(&queue, (0, to)).into();
        }
        if fresh_sent {
            let entries = plan.fresh_entries as u64;
            if lossy {
                // With no older entry owed a copy, this pass starts the delay.
                if entries > 0 && push.repeat_next.max(push.acknowledged_next) >= push.sent_next {
                    push.unrepeated_since = Some(now_ms);
                }
            }
            push.sent_next += entries;
            if !lossy {
                push.repeat_next = push.sent_next;
            }
            self.ordering_counts.piggybacked_records += plan.piggybacked as u64;
        }
        Some((output, before))
    }

    /// Items 4-5 of spec §7.2 for one reader, from the repeat budget: the
    /// 8 ms repeat of what this reader was sent before `before`, only while
    /// lossy (§7.5), then a tail probe (§7.6).
    fn push_repair(
        &mut self,
        id: [u8; 16],
        source: u8,
        before: u64,
        capacity: usize,
        bytes_left: usize,
        now_ms: u64,
    ) -> Vec<ForwardDatagram> {
        let mut output = Vec::new();
        let Some(room) = self.rooms.get_mut(&id) else {
            return output;
        };
        let (Some(peer), Some(ordering)) = (
            room.peers
                .iter_mut()
                .find(|peer| peer.ticket.source == source),
            room.ordering.as_ref(),
        ) else {
            return output;
        };
        let Some(push) = peer.push.as_ref() else {
            return output;
        };
        if push.restoring || capacity == 0 {
            return output;
        }
        let state = peer.link.clock.state;
        let hot = ordering.hot_oldest_seq();
        let acked = push.acknowledged_next;
        // Item 4: repeats, less what the retransmission queue holds.
        let mut repeats = Vec::new();
        let mut repeated_to = None;
        if repeats_on(state, peer.link.jittery)
            && push
                .unrepeated_since
                .is_some_and(|at| now_ms.saturating_sub(at) >= PUSH_REPEAT_MS)
        {
            let from = push.repeat_next.max(acked).max(hot);
            let mut segments = vec![(from, before.max(from))];
            for &range in &push.retransmit {
                segments = subtract(&segments, range);
            }
            let mut reached = before.max(from);
            'segments: for (start, end) in segments.into_iter().filter(|(a, b)| a < b) {
                let records = ordering.hot_page(start, (end - start) as usize);
                let mut next = start;
                for (pack, entries) in
                    super::pack::encode_pack_v2(&records, capacity - repeats.len())
                {
                    repeats.push(pack);
                    next += entries as u64;
                }
                if next < end {
                    reached = next;
                    break 'segments;
                }
            }
            repeated_to = Some(reached);
        }
        // Item 5: a tail probe of the newest new pack (§7.6).
        let mut probes = Vec::new();
        if repeats.len() < capacity && acked < push.sent_next {
            let ack_delay = match state {
                LinkState::Lossy => u64::from(turn_ms(ordering.pace.rate)),
                LinkState::Congested => CONGESTED_ACK_DELAY_MS,
                _ => GOOD_ACK_DELAY_MS,
            };
            let timeout = pto(
                peer.link.srtt_c,
                peer.link.rttvar_c,
                ack_delay,
                push.pto_backoff,
            );
            // A probe also waits a PTO after the previous probe, or a capped
            // PTO would probe every pass.
            let quiet = peer
                .link
                .latest(|send| matches!(send.kind, SEND_FRESH | SEND_RETRANSMIT | SEND_PROBE))
                .is_none_or(|send| now_ms.saturating_sub(send.at_ms) >= timeout);
            if quiet
                && now_ms.saturating_sub(push.last_ack_progress_ms) >= timeout
                && let Some(newest) = peer.link.latest(|send| send.kind == SEND_FRESH)
            {
                let from = newest.first.max(acked).max(hot);
                let end = newest.first + u64::from(newest.count);
                if from < end {
                    let records = ordering.hot_page(from, (end - from) as usize);
                    probes = super::pack::encode_pack_v2(&records, capacity - repeats.len())
                        .into_iter()
                        .map(|(pack, _)| pack)
                        .collect();
                }
            }
        }
        let mut bytes = 0;
        let mut sent_probe = false;
        for (pack, kind) in repeats
            .iter()
            .map(|pack| (pack, SEND_REPEAT))
            .chain(probes.iter().map(|pack| (pack, SEND_PROBE)))
        {
            let length = envelope::OVERHEAD + pack.len();
            if bytes + length > bytes_left {
                break;
            }
            let Ok(datagram) = server_packet(peer, CONTROL, source, pack, now_ms, kind) else {
                break;
            };
            bytes += length;
            sent_probe |= kind == SEND_PROBE;
            self.ordering_counts.push_packets[usize::from(kind)] += 1;
            self.ordering_counts.push_bytes[usize::from(kind)] += datagram.bytes.len() as u64;
            output.push(datagram);
        }
        let push = peer.push.as_mut().expect("subscribed reader");
        if let Some(reached) = repeated_to {
            push.repeat_next = reached;
            // Once every entry sent before this pass went twice, this
            // pass's entries start the next wait.
            if reached >= before {
                push.unrepeated_since = (push.sent_next > before).then_some(now_ms);
            }
        }
        if sent_probe {
            push.pto_backoff += 1;
        }
        output
    }

    fn count_pushed(&mut self, datagrams: &[ForwardDatagram], kinds: &[u8]) {
        for (datagram, kind) in datagrams.iter().zip(kinds) {
            self.ordering_counts.push_packets[usize::from(*kind)] += 1;
            self.ordering_counts.push_bytes[usize::from(*kind)] += datagram.bytes.len() as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{certificate, client, configure, connect, endpoint, subscribe, unix};
    use super::*;

    fn lost_send(sequence: u64, first: u64, count: u8) -> SendRecord {
        SendRecord {
            sequence,
            first,
            count,
            at_ms: 0,
            kind: SEND_FRESH,
            lost: true,
        }
    }

    #[test]
    fn a_lost_pack_owes_what_neither_the_watermark_nor_a_later_pack_covers_once_per_rtt() {
        let mut push = PushSubscription::new(5, false, (true, 3), 0);
        // Lost [3, 12); acknowledged before 5; a later pack not judged lost
        // covers [8, 10); a later lost one covers nothing.
        let later = [
            SendRecord {
                lost: false,
                ..lost_send(11, 8, 2)
            },
            lost_send(12, 10, 2),
        ];
        assert_eq!(
            push.enqueue_lost(&lost_send(10, 3, 9), later.iter(), 40, 100),
            5
        );
        assert_eq!(push.retransmit, [(5, 8), (10, 12)]);
        // Overlapping losses merge.
        push.enqueue_lost(&lost_send(13, 7, 4), [].iter(), 40, 100);
        assert_eq!(push.retransmit, [(5, 12)]);
        // Retransmitted at 100: within srtt_c 40 it is not queued again.
        push.retransmit.clear();
        push.retransmitted.push_back((5, 12, 100));
        assert_eq!(
            push.enqueue_lost(&lost_send(14, 5, 7), [].iter(), 40, 139),
            0
        );
        assert_eq!(
            push.enqueue_lost(&lost_send(14, 5, 7), [].iter(), 40, 140),
            7
        );
    }

    /// One reader of a two-player room, subscribed at 0.
    fn reader_room(seed: u8) -> (RelayServer, ClientSession) {
        let key = SecretKey::new([seed; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut reader, mut other) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut reader, &mut other] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        subscribe(&mut server, &mut reader, 1, 0);
        // Measured good: the initial lossy state has its own test.
        for source in 0..2 {
            assert!(server.preset_reader_measured_good([5; 16], source));
        }
        (server, reader)
    }

    /// Spec §5.4: a reader is lossy from its subscription until its first
    /// fold, and a renewed hop keeps the measured loss.
    #[test]
    fn a_new_reader_starts_lossy_and_a_renewed_hop_keeps_its_measurement() {
        let key = SecretKey::new([86; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut reader, mut other) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut reader, &mut other] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        // The other reader, preset before it subscribes, starts good.
        assert!(server.preset_reader_measured_good([5; 16], 1));
        subscribe(&mut server, &mut other, 1, 0);
        let preset = server.reader_link_stats([5; 16], 1).unwrap();
        assert_eq!(
            (preset.downlink, preset.piggyback_depth),
            (LinkState::Good, 1)
        );
        subscribe(&mut server, &mut reader, 1, 0);
        let fresh = stats(&server);
        assert_eq!(
            (fresh.downlink, fresh.piggyback_depth),
            (LinkState::Lossy, 2)
        );
        let peer = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers[0];
        peer.link.loss.add(32, 0, 10);
        peer.evaluate_reader(10);
        assert_eq!(stats(&server).downlink, LinkState::Good);
        // Congested before the renewal: its hold carries to the new hop.
        let peer = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers[0];
        peer.link.delayed = true;
        peer.evaluate_reader(19_000);
        super::super::tests::renew(&mut server, &mut reader, 20_000);
        let renewed = &server.rooms[&[5; 16]].peers[0].link;
        assert_eq!((renewed.loss.folds, renewed.loss.lossy), (1, false));
        assert_eq!(renewed.clock.state, LinkState::Congested);
        // DELAYED carried too, so the new hop renews the hold itself.
        assert!(renewed.delayed && renewed.congested_at >= Some(19_000));
    }

    fn stats(server: &RelayServer) -> crate::ReaderLinkStats {
        server.reader_link_stats([5; 16], 0).unwrap()
    }

    fn packs(packets: &[ForwardDatagram]) -> Vec<(u64, u8)> {
        packets
            .iter()
            .map(|packet| &packet.bytes[SERVER_HEADER..packet.bytes.len() - HOP_TAG_LEN])
            .filter(|payload| payload[0] == PACK_EVENT)
            .map(|payload| super::super::pack::pack_v2_range(payload).unwrap())
            .collect()
    }

    /// Spec §7.4 and appendix A packets 15, 16 and 20: each new pack carries
    /// the K newest packs' unacknowledged records first; credit counts only
    /// new records.
    #[test]
    fn new_packs_piggyback_the_k_newest_unacknowledged_packs() {
        let (mut server, mut reader) = reader_room(84);
        let turn = |server: &mut RelayServer, at: u64| {
            server.advance_clock(at, unix(at));
            packs(&server.drain_ordering_push(at, unix(at)))
        };
        let [(base, first)] = turn(&mut server, 300)[..] else {
            panic!("one new pack");
        };
        assert_eq!(base, 1);
        let end = 1 + u64::from(first);
        let [(base, count)] = turn(&mut server, 370)[..] else {
            panic!("one new pack");
        };
        assert_eq!(base, 1, "K = 1 carries the previous pack");
        let sent = stats(&server).sent_next;
        assert_eq!(sent, 1 + u64::from(count), "credit counts new records only");
        subscribe(&mut server, &mut reader, end, 371);
        assert_eq!(
            turn(&mut server, 440)[0].0,
            end,
            "acknowledged records drop"
        );
        // Lossy: K = 2 reaches back two packs, still above the watermark.
        server.rooms.get_mut(&[5; 16]).unwrap().peers[0]
            .link
            .loss
            .lossy = true;
        subscribe(&mut server, &mut reader, end, 441);
        assert_eq!(stats(&server).piggyback_depth, 2);
        assert_eq!(turn(&mut server, 510)[0].0, end);
    }

    /// Spec §7.2, §7.5: a good but JITTERY reader gets K = 2 and the 8 ms
    /// repeat, and stays good; clearing JITTERY turns the repeat off.
    #[test]
    fn a_jittery_good_reader_gets_repeats_until_its_jitter_clears() {
        let (mut server, mut reader) = reader_room(85);
        let mut uplink = |server: &mut RelayServer, link: u8, at: u64| {
            let mut payload = Vec::new();
            crate::link::Uplink {
                flags: crate::link::FLAG_PUSH,
                recv_next: 1,
                down: crate::RecvWindow::default(),
                srtt_ms: 0,
                base_ms: 0,
                rttvar_ms: 0,
                link,
                checksums: Vec::new(),
                pace: None,
                first_batch_id: 0,
                batches: Vec::new(),
            }
            .encode(&mut payload);
            let mut bytes = Vec::new();
            reader.encode(CONTROL, &payload, &mut bytes).unwrap();
            server
                .process_at(super::super::tests::endpoint(&reader), &bytes, unix(at), at)
                .unwrap();
        };
        uplink(&mut server, crate::link::LINK_JITTERY, 290);
        let jittery = stats(&server);
        assert_eq!((jittery.downlink, jittery.jittery), (LinkState::Good, true));
        assert_eq!(jittery.piggyback_depth, 2);
        server.advance_clock(300, unix(300));
        server.drain_ordering_push(300, unix(300));
        server.drain_ordering_push(308, unix(308));
        assert_eq!(stats(&server).packets_by_kind[2], 1);
        // Calm again: the next turn is not repeated.
        uplink(&mut server, 0, 320);
        server.advance_clock(370, unix(370));
        server.drain_ordering_push(370, unix(370));
        server.drain_ordering_push(378, unix(378));
        assert_eq!(stats(&server).packets_by_kind[2], 1);
    }

    /// Spec §4.3: a feedback judges a missing send only if it went
    /// srtt_c + 4 rttvar_c before the feedback arrived; later push passes
    /// never judge it again, the next feedback does.
    #[test]
    fn only_a_feedback_old_enough_judges_a_send_lost() {
        let (mut server, _reader) = reader_room(87);
        server.advance_clock(300, unix(300));
        server.drain_ordering_push(300, unix(300));
        let peer = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers[0];
        let pack = peer.link.latest(|send| send.kind == SEND_FRESH).unwrap();
        (peer.link.srtt_c, peer.link.rttvar_c) = (40, 10);
        // A later datagram arrived; the pack did not. Wait: 40 + 4 * 10.
        let (high, bits) = (pack.sequence + 3, !(1u64 << 3));
        let young = pack.at_ms + 79;
        assert!(peer.link.judge(RecvWindow { high, bits }, young).is_empty());
        server.drain_ordering_push(young + 200, unix(young + 200));
        assert_eq!(stats(&server).packets_by_kind[3], 0, "no judging by time");
        let peer = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers[0];
        assert_eq!(
            peer.link.judge(RecvWindow { high, bits }, young + 1).len(),
            1
        );
    }

    /// Spec §5.2: a congested reader stays congested until DELAYED has not
    /// held for 10 s, so neither loss nor JITTERY repeats in that time;
    /// DELAYED again restarts the 10 s.
    #[test]
    fn a_congested_reader_holds_congestion_for_ten_seconds() {
        let (mut server, _reader) = reader_room(88);
        fn peer(server: &mut RelayServer) -> &mut Peer {
            &mut server.rooms.get_mut(&[5; 16]).unwrap().peers[0]
        }
        let at = |server: &mut RelayServer, now: u64, delayed: bool| {
            let reader = peer(server);
            reader.link.delayed = delayed;
            (reader.link.jittery, reader.link.loss.lossy) = (true, true);
            reader.link.last_heard_ms = now;
            reader.evaluate_reader(now);
            (reader.link.clock.state, reader.link.piggyback_depth())
        };
        assert_eq!(at(&mut server, 1000, true), (LinkState::Congested, 2));
        // Lossy and JITTERY, but DELAYED 9999 ms ago: still congested.
        assert_eq!(at(&mut server, 10_999, false), (LinkState::Congested, 2));
        assert_eq!(at(&mut server, 11_000, false), (LinkState::Lossy, 2));
        at(&mut server, 12_000, true);
        assert_eq!(at(&mut server, 21_999, false).0, LinkState::Congested);
        assert_eq!(at(&mut server, 22_000, false).0, LinkState::Lossy);
    }

    /// Review fix: lost PULL replies of adjacent pages cannot merge into an
    /// unbounded range; the queue keeps PUSH_WINDOW + ORDERING_PULL_LIMIT
    /// records from the watermark and the rest is the fallback PULL's.
    #[test]
    fn the_retransmission_queue_is_bounded() {
        let mut push = PushSubscription::new(1, false, (true, 3), 0);
        for page in 0..100u64 {
            let send = SendRecord {
                kind: crate::link::SEND_OTHER,
                ..lost_send(page + 1, 1 + 32 * page, 32)
            };
            push.enqueue_lost(&send, [].iter(), 1, 1000 * page);
        }
        assert_eq!(push.retransmit, [(1, 1 + RETRANSMIT_QUEUE_RECORDS)]);
    }

    #[test]
    fn queued_ranges_go_first_as_retransmissions() {
        let (mut server, _reader) = reader_room(81);
        server.advance_clock(300, unix(300));
        server.drain_ordering_push(300, unix(300));
        let peer = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers[0];
        merge(&mut peer.push.as_mut().unwrap().retransmit, (1, 3));
        server.advance_clock(310, unix(310));
        let packets = server.drain_ordering_push(310, unix(310));
        let pack = &packets[0].bytes[SERVER_HEADER..packets[0].bytes.len() - HOP_TAG_LEN];
        assert_eq!(super::super::pack::pack_v2_range(pack), Some((1, 2)));
        assert_eq!(stats(&server).packets_by_kind[3], 1);
        assert_eq!(stats(&server).retransmit_ranges, 0);
    }

    #[test]
    fn a_quiet_tail_is_probed_after_a_pto_that_doubles_until_acknowledged() {
        let (mut server, mut reader) = reader_room(82);
        // Credit runs out after 64 records: then no new packs follow.
        server.advance_clock(2000, unix(2000));
        subscribe(&mut server, &mut reader, 1, 1999);
        server.drain_ordering_push(2000, unix(2000));
        assert_eq!(stats(&server).sent_next, 65);
        // No srtt yet: PTO = 100 + max(16, 200) + 100 ms in the good state.
        server.drain_ordering_push(2399, unix(2399));
        assert_eq!(stats(&server).packets_by_kind[4], 0);
        server.drain_ordering_push(2400, unix(2400));
        assert_eq!(stats(&server).packets_by_kind[4], 1);
        assert_eq!(stats(&server).pto_backoff, 1);
        subscribe(&mut server, &mut reader, 1, 3000);
        server.drain_ordering_push(3199, unix(3199));
        assert_eq!(stats(&server).packets_by_kind[4], 1);
        server.drain_ordering_push(3200, unix(3200));
        assert_eq!(stats(&server).packets_by_kind[4], 2);
        subscribe(&mut server, &mut reader, 30, 3300);
        assert_eq!(stats(&server).pto_backoff, 0);
    }

    #[test]
    fn an_offline_reader_gets_nothing_and_resumes_from_its_watermark() {
        let (mut server, mut reader) = reader_room(83);
        server.advance_clock(300, unix(300));
        assert!(!server.drain_ordering_push(300, unix(300)).is_empty());
        let sent = stats(&server).sent_next;
        assert!(sent > 1);
        // Silent 1500 ms: offline, not even a status heartbeat.
        server.advance_clock(1500, unix(1500));
        assert!(server.drain_ordering_push(1500, unix(1500)).is_empty());
        assert_eq!(stats(&server).downlink, LinkState::Offline);
        assert_eq!(server.ordering_counts.offline_unsent_packets, 1);
        // Any datagram of the hop ends the offline state (spec §5.1).
        let mut bytes = Vec::new();
        reader.probe_at(1550, &mut bytes).unwrap();
        server
            .process_at(endpoint(&reader), &bytes, unix(1550), 1550)
            .unwrap();
        assert_eq!(stats(&server).downlink, LinkState::Good);
        // Back: pushing restarts at the watermark the reader acknowledged.
        subscribe(&mut server, &mut reader, 1, 1600);
        assert_eq!(stats(&server).downlink, LinkState::Good);
        assert_eq!(stats(&server).sent_next, 1);
        let packets = server.drain_ordering_push(1600, unix(1600));
        let first_pack = packets
            .iter()
            .map(|packet| &packet.bytes[SERVER_HEADER..packet.bytes.len() - HOP_TAG_LEN])
            .find(|payload| payload[0] == PACK_EVENT)
            .unwrap();
        assert_eq!(super::super::pack::pack_v2_range(first_pack).unwrap().0, 1);
    }

    #[test]
    fn ranges_subtract_and_merge_into_a_sorted_disjoint_queue() {
        assert_eq!(subtract(&[(1, 10)], (3, 5)), [(1, 3), (5, 10)]);
        assert_eq!(subtract(&[(1, 10)], (0, 20)), []);
        assert_eq!(subtract(&[(1, 4), (6, 9)], (3, 7)), [(1, 3), (7, 9)]);
        let mut queue = VecDeque::from([(10, 12)]);
        merge(&mut queue, (1, 3));
        merge(&mut queue, (3, 5));
        merge(&mut queue, (11, 20));
        assert_eq!(queue, [(1, 5), (10, 20)]);
    }

    fn frame_end(sequence: u64) -> Vec<u8> {
        OrderingRoom::encoded_entry(sequence, sequence as u32, FRAME_END_EVENT, &[])
    }

    #[test]
    fn piggyback_drops_the_oldest_records_that_would_add_a_pack() {
        let old: Vec<_> = (1..=4).map(frame_end).collect();
        let new = vec![frame_end(5)];
        let (packs, entries, piggybacked) = piggyback_packs(&old, &new, 8);
        assert_eq!((packs.len(), entries, piggybacked), (1, 1, 4));
        assert_eq!(super::super::pack::pack_v2_range(&packs[0].0), Some((1, 5)));
        // PACK v2: a full batch (517 B) and 159 frame ends (1 B each) fill
        // 680 of 681 bytes with its 4 B header: one more frame end fits, two
        // would add a pack.
        let mut opaque = vec![0; 9];
        opaque.extend_from_slice(&[7; 512]);
        let mut busy = vec![OrderingRoom::encoded_entry(5, 5, BATCH_EVENT, &opaque)];
        busy.extend((6..165).map(frame_end));
        let (packs, entries, piggybacked) = piggyback_packs(&old, &busy, 8);
        assert_eq!((packs.len(), entries, piggybacked), (1, 160, 1));
        assert_eq!(packs[0].0.len(), 681);
        assert_eq!(
            super::super::pack::pack_v2_range(&packs[0].0),
            Some((4, 161))
        );
        busy.push(frame_end(165));
        let (packs, _, piggybacked) = piggyback_packs(&old, &busy, 8);
        assert_eq!((packs.len(), piggybacked), (1, 0));
    }
}
