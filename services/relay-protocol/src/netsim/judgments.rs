//! Phase 2 judgments (e2e-plan §2.2) and CI smoke tests (§2.5) of the
//! adaptive transport. Every expectation comes from the spec
//! (services/relay/adaptive-transport/spec.md), cited next to it; the
//! implementation is reached only through its public interface: the
//! production `ClientSession` drives every send (`adaptive_client.rs`), and
//! `link_stats` / `reader_link_stats` are read as the spec defines them.
//!
//! Each judgment is a `check_*` function over a traced run, so the smoke
//! tests reuse them on shorter runs.

use super::inspect::{Detail, FLAG_PACE, FLAG_PROBE, FLAG_PUSH, FLAG_RESTORING, Uplink};
use super::reader::Record;
use super::report::Report;
use super::scenario::Direction;
use super::trace::{Origin, Trace, WireRecord};
use super::*;
use crate::LinkState;

pub(super) type Check = Result<(), String>;

fn run(json: &str) -> (Report, Trace) {
    Simulation::new(Scenario::from_json(json).expect("scenario")).run_traced()
}

fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

fn uplinks(trace: &Trace, client: usize) -> Vec<&WireRecord> {
    trace
        .of(client, Direction::Up)
        .filter(|record| record.uplink().is_some())
        .collect()
}

fn pushes(trace: &Trace, client: usize) -> Vec<&WireRecord> {
    trace
        .of(client, Direction::Down)
        .filter(|record| record.origin == Origin::Push)
        .collect()
}

/// Spec §6.1: `accepted_through` is the highest of the ACKs, BatchGap's
/// `expected - 1` and the client's own BATCH records it has received by
/// `at_us` (arrivals strictly before it, or also at it).
fn accepted_through(trace: &Trace, client: usize, at_us: u64, inclusive: bool) -> u64 {
    let mut accepted = 0;
    for record in trace.of(client, Direction::Down) {
        let Some(arrived) = record.arrived_us() else {
            continue;
        };
        if arrived > at_us || (!inclusive && arrived == at_us) {
            continue;
        }
        match record.parsed.as_ref().map(|d| &d.detail) {
            Ok(Detail::Ack { batch_id, .. }) => accepted = accepted.max(*batch_id),
            Ok(Detail::Error { code: 4, expected }) => {
                accepted = accepted.max(expected.saturating_sub(1))
            }
            Ok(Detail::Pack { records, .. }) => {
                for entry in records {
                    if entry.kind == 3 && entry.source == Some(client as u8) {
                        accepted = accepted.max(entry.batch_id.unwrap_or(0));
                    }
                }
            }
            _ => {}
        }
    }
    accepted
}

fn ensure(condition: bool, message: impl FnOnce() -> String) -> Check {
    if condition { Ok(()) } else { Err(message()) }
}

/// Spec §5.4: each direction counts as lossy until its first loss fold,
/// and leaves the lossy state only once its estimate is down and its last
/// 8 folds lost nothing. When the client's uplink first left that initial
/// lossy state (µs), or the end.
fn uplink_settled_us(trace: &Trace, client: usize) -> u64 {
    trace
        .client_stats
        .iter()
        .find(|(_, c, stats)| {
            *c == client && stats.up_loss_folds >= 1 && stats.uplink != LinkState::Lossy
        })
        .map_or(u64::MAX, |(at, ..)| *at)
}

/// When the relay first judged this subscribed reader other than lossy,
/// which is no earlier than its first fold (§5.4), or the end.
fn downlink_settled_us(trace: &Trace, client: usize) -> u64 {
    trace
        .reader_stats
        .iter()
        .find(|(_, c, stats)| {
            *c == client && stats.subscribed && stats.downlink != LinkState::Lossy
        })
        .map_or(u64::MAX, |(at, ..)| *at)
}

/// Repeats (send-log kind 2) sent to `client` from `from_us` on.
fn repeats_since(trace: &Trace, client: usize, from_us: u64) -> u64 {
    let at = |t: u64| {
        trace
            .reader_stats_at(client, t)
            .map_or(0, |s| s.packets_by_kind[2])
    };
    at(u64::MAX) - at(from_us)
}

/// Copies `client` sent from `from_us` on.
fn copies_since(trace: &Trace, client: usize, from_us: u64) -> u64 {
    let at = |t: u64| trace.client_stats_at(client, t).map_or(0, |s| s.copies);
    at(u64::MAX) - at(from_us)
}

// --- Judgment 1 (spec §1.2, §3) and the one-UPLINK-per-service rule (§6.3) ---

/// Every datagram parses by spec §3 as the bandwidth compaction encodes it
/// (the inspector applies the six UPLINK decoding rules of §3.2, ACK and
/// STATUS with their feedback tail, §3.3 and §3.4); none of the removed
/// requests or PushState appears (§1.2); every UPLINK has
/// PUSH set (§3.2 flags bit0, "对局开始后始终置 1") unless the client left;
/// a PROBE carries no batch and no checksum (§6.3 row 5, §6.6); and one
/// service sends at most one UPLINK, heartbeat included (§6.3).
pub(super) fn check_wire(trace: &Trace) -> Check {
    for record in &trace.wire {
        let datagram = record.parsed.as_ref().map_err(|error| {
            format!(
                "unparsable {:?} at {} ms: {error}",
                record.direction,
                record.sent_ms()
            )
        })?;
        ensure(
            !matches!(
                datagram.kind,
                "SUBMIT" | "PUSH" | "PACE" | "CHECKSUM" | "PUSH_STATE"
            ),
            || {
                format!(
                    "removed message {} at {} ms (spec §1.2)",
                    datagram.kind,
                    record.sent_ms()
                )
            },
        )?;
        match &datagram.detail {
            Detail::Uplink(uplink) => {
                ensure(uplink.flags & FLAG_PUSH != 0, || {
                    format!("UPLINK without PUSH at {} ms (spec §3.2)", record.sent_ms())
                })?;
                if uplink.flags & FLAG_PROBE != 0 {
                    ensure(
                        uplink.batches.is_empty() && uplink.checksums.is_empty(),
                        || {
                            format!(
                                "PROBE with content at {} ms (spec §6.3 row 5)",
                                record.sent_ms()
                            )
                        },
                    )?;
                }
            }
            _ => {}
        }
    }
    let mut per_service: HashMap<(usize, u64), usize> = HashMap::new();
    for record in &trace.wire {
        if let (Some(service), Some(_)) = (record.service, record.uplink()) {
            let count = per_service.entry((record.client, service)).or_default();
            *count += 1;
            ensure(*count == 1, || {
                format!(
                    "client {} sent two UPLINKs in one service at {} ms (spec §6.3)",
                    record.client,
                    record.sent_ms()
                )
            })?;
        }
    }
    Ok(())
}

/// Spec §6.1, §6.2: every UPLINK but a PROBE, IO heartbeats included
/// (§6.6), carries the unacknowledged batches from `accepted_through + 1`,
/// all of them up to four (the batches here are small, so all fit); a
/// heartbeat keeps the previous UPLINK's checksum segment (§6.6).
pub(super) fn check_batch_segments(trace: &Trace) -> Check {
    for client in 0..trace.seals.len() {
        let mut previous: Option<&Uplink> = None;
        for record in uplinks(trace, client) {
            let uplink = record.uplink().unwrap();
            if uplink.flags & FLAG_PROBE != 0 {
                previous = Some(uplink);
                continue;
            }
            // §6.6: a heartbeat repacks its batches by §6.2 like any other
            // UPLINK and repeats the previous UPLINK's checksum segment.
            if record.heartbeat
                && let Some(previous) = previous
            {
                ensure(previous.checksums == uplink.checksums, || {
                    format!(
                        "heartbeat at {} ms changed the checksums (spec §6.6)",
                        record.sent_ms()
                    )
                })?;
            }
            let sealed = trace.seals[client]
                .iter()
                .filter(|(at, _)| *at <= record.sent_us)
                .map(|(_, id)| *id)
                .max()
                .unwrap_or(0);
            let low = accepted_through(trace, client, record.sent_us, false);
            let high = accepted_through(trace, client, record.sent_us, true);
            let matches = (low..=high).any(|accepted| {
                let expected: Vec<u64> = (accepted + 1..=sealed.min(accepted + 4)).collect();
                uplink.batch_ids().collect::<Vec<_>>() == expected
            });
            ensure(matches, || {
                format!(
                    "client {client} UPLINK at {} ms carries {:?}; accepted through {low}..={high}, sealed through {sealed} (spec §6.2)",
                    record.sent_ms(),
                    uplink.batch_ids().collect::<Vec<_>>()
                )
            })?;
            previous = Some(uplink);
        }
    }
    Ok(())
}

#[test]
fn j01_every_datagram_has_the_spec_layout() {
    // Loss in both directions brings copies, repeats and retransmissions;
    // 300 APM brings batches, checksums every 15 frames and pace each second.
    let (report, trace) = run(r#"{ "name": "j01", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "loss": 0.05 } } }"#);
    assert_eq!(report.malformed, 0);
    check_wire(&trace).unwrap();
    check_batch_segments(&trace).unwrap();
    // §6.3 pace segment: each measurement rides exactly one UPLINK.
    for client in 0..2 {
        let paces = uplinks(&trace, client)
            .iter()
            .filter(|record| record.uplink().unwrap().flags & FLAG_PACE != 0)
            .count();
        assert!(
            (10..=12).contains(&paces),
            "client {client}: {paces} pace segments in 12 s"
        );
    }
}

// --- Judgment 2 (spec §6.3 rows 1 and 6, §13.2 row 1) ---

/// On a good link, once its first loss fold ends the initial lossy state
/// (§5.4), no copy is sent (`copies` stays 0, §6.3 row 6 is lossy only); each batch's first UPLINK goes out in the service that sealed it
/// (row 1, "立即"); and that first send is one datagram with no copy after
/// it (§13.2 row 1): no UPLINK in the next service window (16-32 ms later,
/// where a copy would go) carries the batch unless it has a reason of its
/// own on a good link, a never-sent batch (row 1) or checksums (the
/// checksum segment). Later UPLINKs before the ACK carry it too; §13.2
/// calls that normal redundancy (§6.2).
fn check_good_uplink(trace: &Trace, client: usize) -> Check {
    // §5.4: lossy until the first fold, so judged from then on.
    let settled = uplink_settled_us(trace, client);
    ensure(settled < u64::MAX, || {
        format!("client {client}: no uplink fold")
    })?;
    let copies = copies_since(trace, client, settled);
    ensure(copies == 0, || {
        format!("client {client}: {copies} copies on a good link")
    })?;
    let ups = uplinks(trace, client);
    for &(sealed_at, id) in &trace.seals[client] {
        if sealed_at <= settled {
            continue;
        }
        let first_index = ups
            .iter()
            .position(|record| record.uplink().unwrap().carries(id))
            .ok_or_else(|| format!("batch {id} never sent"))?;
        let first = ups[first_index];
        ensure(first.sent_us == sealed_at, || {
            format!(
                "batch {id} sealed at {} ms, first sent at {} ms (spec §6.3 row 1)",
                ms(sealed_at),
                first.sent_ms()
            )
        })?;
        let mut sent: BTreeSet<u64> = ups[..first_index]
            .iter()
            .flat_map(|record| record.uplink().unwrap().batch_ids().collect::<Vec<_>>())
            .collect();
        sent.extend(first.uplink().unwrap().batch_ids());
        for record in &ups[first_index + 1..] {
            if record.sent_us >= first.sent_us + 32_000 {
                break;
            }
            let uplink = record.uplink().unwrap();
            let own_reason = uplink.batch_ids().any(|other| !sent.contains(&other))
                || !uplink.checksums.is_empty();
            ensure(
                !(record.sent_us >= first.sent_us + 16_000 && uplink.carries(id) && !own_reason),
                || {
                    format!(
                        "batch {id} first sent at {} ms looks copied at {} ms (spec §13.2)",
                        first.sent_ms(),
                        record.sent_ms()
                    )
                },
            )?;
            sent.extend(uplink.batch_ids());
        }
    }
    Ok(())
}

#[test]
fn j02_a_good_link_sends_each_batch_once_without_copies() {
    let (_, trace) = run(r#"{ "name": "j02", "players": 2,
        "warmup_ms": 0, "duration_ms": 15000, "apm": 60 }"#);
    check_wire(&trace).unwrap();
    for client in 0..2 {
        assert!(trace.seals[client].len() >= 5);
        check_good_uplink(&trace, client).unwrap();
    }
}

/// §4.5, §6.3 row 4, §10.1: on a 40 ms RTT without loss every ACK beats
/// the RTO (at least 48 ms), so `rto_timeouts` stays 0.
#[test]
fn j02_a_good_link_never_times_out() {
    let (_, trace) = run(r#"{ "name": "j02-rto", "players": 2,
        "warmup_ms": 0, "duration_ms": 15000, "apm": 300 }"#);
    for client in 0..2 {
        let stats = trace.client_stats_at(client, u64::MAX).unwrap();
        assert_eq!(stats.rto_timeouts, 0, "client {client}");
    }
}

// --- Judgment 3 (spec §6.3 row 6 and its copy-content paragraph) ---

/// For each batch first sent while the uplink is lossy (§5.2, §5.4), one
/// UPLINK carrying it follows 16 ms later (a copy, or an ordinary UPLINK
/// standing in for it, §6.7 item 4) if it is still unacknowledged
/// then, and a second one 32 ms after the first send when the loss is
/// severe (§5.2 sub-band). Copies carry the original's checksum segment
/// and no pace (§6.3, copy content).
fn check_copies(trace: &Trace, client: usize) -> Check {
    let ups = uplinks(trace, client);
    let end_us = trace.wire.iter().map(|r| r.sent_us).max().unwrap_or(0);
    let mut checked = 0;
    for &(sealed_at, id) in &trace.seals[client] {
        let Some(stats) = trace.client_stats_at(client, sealed_at) else {
            continue;
        };
        if stats.uplink != LinkState::Lossy {
            continue;
        }
        let offsets: &[u64] = if stats.severe { &[16, 32] } else { &[16] };
        let first = ups
            .iter()
            .find(|record| record.uplink().unwrap().carries(id))
            .ok_or_else(|| format!("batch {id} never sent"))?;
        for &offset in offsets {
            let due = first.sent_us + offset * 1000;
            // A copy due after the run ended cannot be judged.
            if due + 16_000 > end_us {
                continue;
            }
            if accepted_through(trace, client, due, true) >= id {
                continue;
            }
            // One service later, give or take the client's 16 ms phase.
            let copy = ups.iter().find(|record| {
                record.sent_us >= due
                    && record.sent_us < due + 16_000
                    && record.uplink().unwrap().carries(id)
            });
            let copy = copy.ok_or_else(|| {
                format!(
                    "no copy of batch {id} {offset} ms after {} ms (spec §6.3 row 6)",
                    first.sent_ms()
                )
            })?;
            // A real copy raises `copies`; an ordinary UPLINK standing in
            // (§6.7 item 4) does not and has its own content.
            let counted = |at: u64| trace.client_stats_at(client, at).map_or(0, |s| s.copies);
            let real = counted(copy.sent_us) > counted(copy.sent_us - 1);
            let copy = copy.uplink().unwrap();
            let original = first.uplink().unwrap();
            if !real {
                checked += 1;
                continue;
            }
            ensure(copy.pace.is_none(), || {
                format!("copy of batch {id} carries pace (spec §6.3)")
            })?;
            if !original.checksums.is_empty() {
                ensure(copy.checksums == original.checksums, || {
                    format!("copy of batch {id} lost the original's checksums (spec §6.3)")
                })?;
            }
            checked += 1;
        }
    }
    ensure(checked > 0, || {
        format!("client {client}: no batch was sent while lossy")
    })
}

#[test]
fn j03_a_lossy_uplink_copies_each_new_batch_16_ms_later() {
    let (_, trace) = run(r#"{ "name": "j03", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300,
        "groups": [ { "label": "lossy", "first": 1, "last": 1,
            "network": { "up": { "delay_ms": 20, "loss": 0.06 } } } ] }"#);
    check_wire(&trace).unwrap();
    let last = trace.client_stats_at(1, u64::MAX).unwrap();
    assert!(last.uplink_state_entries[LinkState::Lossy.index()] >= 1);
    // §6.7 item 4: an ordinary UPLINK due at the copy's time stands in for
    // it and is not counted in `copies`, so the copy is judged on the wire.
    check_copies(&trace, 1).unwrap();
}

#[test]
fn j03_a_severely_lossy_uplink_copies_at_16_and_32_ms() {
    let (_, trace) = run(r#"{ "name": "j03-severe", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300,
        "groups": [ { "label": "severe", "first": 1, "last": 1,
            "network": { "up": { "delay_ms": 20, "loss": 0.25 } } } ] }"#);
    check_wire(&trace).unwrap();
    assert!(
        trace
            .client_stats
            .iter()
            .any(|(_, client, stats)| *client == 1 && stats.severe)
    );
    check_copies(&trace, 1).unwrap();
}

// --- Judgment 4 (spec §7.2 row 4, §7.5) ---

/// The 8 ms repeat (send-log kind 2, §4.2) only goes to a lossy reader,
/// or to a good one whose latest UPLINK at the relay set JITTERY
/// (`link` bit3, §7.2 row 4, §7.5): its count rises only then.
fn check_repeats_only_when_lossy(trace: &Trace, client: usize) -> Check {
    let ups = uplinks(trace, client);
    let jittery_at = |at: u64| {
        ups.iter()
            .filter(|r| r.arrived_us().is_some_and(|arrived| arrived <= at))
            .max_by_key(|r| r.arrived_us())
            .is_some_and(|r| r.uplink().unwrap().link & 8 != 0)
    };
    let mut previous: Option<&crate::ReaderLinkStats> = None;
    for (at, c, stats) in &trace.reader_stats {
        if *c != client {
            continue;
        }
        if let Some(before) = previous
            && stats.packets_by_kind[2] > before.packets_by_kind[2]
        {
            let lossy = before.downlink == LinkState::Lossy || stats.downlink == LinkState::Lossy;
            let jittery = stats.downlink == LinkState::Good && jittery_at(*at);
            ensure(lossy || jittery, || {
                format!(
                    "client {client}: a repeat at {} ms while {:?} (spec §7.2)",
                    ms(*at),
                    stats.downlink
                )
            })?;
        }
        previous = Some(stats);
    }
    Ok(())
}

#[test]
fn j04_only_a_lossy_reader_gets_8_ms_repeats() {
    let (_, trace) = run(r#"{ "name": "j04", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 60,
        "groups": [ { "label": "lossy", "first": 1, "last": 1,
            "network": { "down": { "delay_ms": 20, "loss": 0.06 } } } ] }"#);
    check_wire(&trace).unwrap();
    // §5.4: the good reader is lossy only until its first fold.
    assert_eq!(
        repeats_since(&trace, 0, downlink_settled_us(&trace, 0)),
        0,
        "good reader got repeats"
    );
    let lossy = trace.reader_stats_at(1, u64::MAX).unwrap();
    assert!(lossy.packets_by_kind[2] > 0, "lossy reader got no repeat");
    assert!(
        trace
            .reader_stats
            .iter()
            .any(|(_, client, stats)| *client == 1 && stats.downlink == LinkState::Lossy)
    );
    check_repeats_only_when_lossy(&trace, 0).unwrap();
    check_repeats_only_when_lossy(&trace, 1).unwrap();
}

// --- Judgment 5 (spec §7.4, §11 row 12) ---

/// In each push pass, the PACKs that carry new records (sequences never
/// pushed to this reader) number exactly what the new records alone need
/// by PACK v2 packing (bandwidth-compaction spec §5.4); only the first of them carries older records; and no
/// older record is below the reader's `acked_next`.
fn check_piggyback(trace: &Trace, client: usize) -> Check {
    let mut pushed_high = 1;
    let mut passes: BTreeMap<u64, Vec<&WireRecord>> = BTreeMap::new();
    for record in pushes(trace, client) {
        if record.pack().is_some() {
            passes.entry(record.sent_us).or_default().push(record);
        }
    }
    let mut checked = 0;
    for (at, packs) in passes {
        let fresh: Vec<&&WireRecord> = packs
            .iter()
            .filter(|record| {
                let (_, records) = record.pack().unwrap();
                records.iter().any(|entry| entry.seq >= pushed_high)
            })
            .collect();
        if fresh.is_empty() {
            continue;
        }
        let new_records: Vec<&inspect::PackRecord> = fresh
            .iter()
            .flat_map(|record| record.pack().unwrap().1)
            .filter(|entry| entry.seq >= pushed_high)
            .collect();
        // By PACK v2 packing (bandwidth-compaction spec §5.4).
        let canonical: Vec<Vec<u8>> = new_records.iter().map(|r| r.canonical.clone()).collect();
        let needed = inspect::v3::packs_needed(&canonical)?;
        ensure(fresh.len() == needed, || {
            format!(
                "client {client} at {} ms: {} new packs for records needing {needed} (spec §7.4)",
                ms(at),
                fresh.len(),
            )
        })?;
        for (index, record) in fresh.iter().enumerate() {
            let (base, _) = record.pack().unwrap();
            if index > 0 {
                ensure(base >= pushed_high, || {
                    format!(
                        "client {client} at {} ms: piggyback beyond the first new pack (spec §11 row 12)",
                        ms(at)
                    )
                })?;
            } else if let Some(stats) = trace.reader_stats_at(client, at) {
                ensure(base >= stats.acked_next.min(pushed_high), || {
                    format!(
                        "client {client} at {} ms: piggyback from {base} below acked_next {} (spec §7.4)",
                        ms(at),
                        stats.acked_next
                    )
                })?;
            }
        }
        pushed_high = fresh
            .iter()
            .flat_map(|record| record.pack().unwrap().1)
            .map(|entry| entry.seq + 1)
            .max()
            .unwrap_or(pushed_high)
            .max(pushed_high);
        checked += 1;
    }
    ensure(checked > 0, || format!("client {client}: no new pack"))
}

#[test]
fn j05_piggyback_never_adds_a_packet() {
    // 200-byte commands at 300 APM from four players fill more than one
    // pack in many turns, so §7.4 must shed old records.
    let (_, trace) = run(r#"{ "name": "j05", "players": 4,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 300, "command_bytes": 200,
        "groups": [ { "label": "lossy", "first": 3, "last": 3,
            "network": { "link": { "delay_ms": 20, "loss": 0.05 } } } ] }"#);
    check_wire(&trace).unwrap();
    for client in 0..4 {
        check_piggyback(&trace, client).unwrap();
    }
}

// --- Judgment 6 (spec §7.3, §13.2 "连续丢 3 个下行包") ---

/// Every retransmitted record (a non-new PACK in a pass where the reader's
/// kind-3 count rose and kinds 2 and 4 did not) is at or above the reader's
/// `acked_next`, was carried by a PACK the network lost, and by no
/// delivered PACK sent after that loss; the same record is retransmitted
/// at most once per client-reported `srtt` (§7.3 step 4, counted between
/// retransmissions, §7.8 item 3); and each run of losses ends in at most
/// one fallback PULL (§13.2).
fn check_retransmissions(trace: &Trace, client: usize) -> Check {
    let packs: Vec<&WireRecord> = trace
        .of(client, Direction::Down)
        .filter(|record| record.pack().is_some())
        .collect();
    let mut last_retransmitted: HashMap<u64, u64> = HashMap::new();
    let mut checked = 0;
    let mut previous: Option<&crate::ReaderLinkStats> = None;
    for (at, c, stats) in &trace.reader_stats {
        if *c != client {
            continue;
        }
        let Some(before) = previous.replace(stats) else {
            continue;
        };
        let delta = |kind: usize| stats.packets_by_kind[kind] - before.packets_by_kind[kind];
        if delta(3) == 0 || delta(2) > 0 || delta(4) > 0 {
            continue;
        }
        let pushed_before = packs
            .iter()
            .filter(|record| record.sent_us < *at)
            .flat_map(|record| record.pack().unwrap().1)
            .map(|entry| entry.seq)
            .max()
            .unwrap_or(0);
        for record in packs
            .iter()
            .filter(|record| record.sent_us == *at && record.origin == Origin::Push)
        {
            let (_, records) = record.pack().unwrap();
            if records.iter().any(|entry| entry.seq > pushed_before) {
                continue;
            }
            for entry in records {
                let x = entry.seq;
                ensure(x >= stats.acked_next, || {
                    format!(
                        "retransmitted {x} below acked_next {} at {} ms (spec §7.3 step 1)",
                        stats.acked_next,
                        ms(*at)
                    )
                })?;
                let lost = packs
                    .iter()
                    .filter(|p| p.sent_us < *at && p.lost())
                    .filter(|p| p.pack().unwrap().1.iter().any(|e| e.seq == x))
                    .map(|p| p.sent_us)
                    .max()
                    .ok_or_else(|| {
                        format!(
                            "retransmitted {x} at {} ms was never lost (spec §7.3)",
                            ms(*at)
                        )
                    })?;
                ensure(
                    !packs.iter().any(|p| {
                        p.sent_us > lost
                            && p.sent_us < *at
                            && !p.lost()
                            && p.pack().unwrap().1.iter().any(|e| e.seq == x)
                    }),
                    || {
                        format!(
                            "retransmitted {x} at {} ms though a later delivered pack covered it (spec §7.3 step 2)",
                            ms(*at)
                        )
                    },
                )?;
                let srtt = trace
                    .client_stats_at(client, *at)
                    .and_then(|s| s.srtt_ms)
                    .unwrap_or(100);
                if let Some(last) = last_retransmitted.insert(x, *at) {
                    ensure(*at - last >= u64::from(srtt) * 1000, || {
                        format!(
                            "{x} retransmitted twice within srtt {srtt} ms at {} ms (spec §7.3 step 4)",
                            ms(*at)
                        )
                    })?;
                }
                checked += 1;
            }
        }
    }
    ensure(checked > 0, || {
        format!("client {client}: no retransmission seen")
    })?;
    // §13.2: a run of lost packs may end in one fallback PULL (§7.7), at
    // most one: the last lost packs are judged only once a later feedback
    // reaches three past them (§4.3). Runs are losses less than 500 ms
    // apart; each PULL belongs to the run started before it, within 1.5 s.
    let mut runs: Vec<(u64, usize)> = Vec::new();
    for record in &packs {
        if record.lost() {
            match runs.last_mut() {
                Some((start, _)) if record.sent_us < *start + 500_000 => {}
                _ => runs.push((record.sent_us, 0)),
            }
        }
    }
    for pull in trace
        .of(client, Direction::Up)
        .filter(|record| record.kind() == "PULL")
    {
        let run = runs
            .iter_mut()
            .filter(|(start, _)| *start <= pull.sent_us)
            .last()
            .filter(|(start, _)| pull.sent_us < *start + 1_500_000)
            .ok_or_else(|| format!("PULL at {} ms after no loss (spec §13.2)", pull.sent_ms()))?;
        run.1 += 1;
        ensure(run.1 <= 1, || {
            format!(
                "a second PULL at {} ms for the losses from {} ms (spec §13.2)",
                pull.sent_ms(),
                ms(run.0)
            )
        })?;
    }
    Ok(())
}

#[test]
fn j06_retransmissions_cover_only_unacknowledged_uncovered_records() {
    // Three runs of three consecutive packs lost to a good reader (K = 1,
    // no repeats), as spec §13.2's "连续丢 3 个下行包". Four players with
    // 200-byte commands fill the packs, so piggyback cannot carry the lost
    // records and the relay must retransmit; with small records every
    // pack carries everything unacknowledged (§7.4) and nothing is lost.
    let rules: Vec<String> = [60, 61, 62, 160, 161, 162, 260, 261, 262]
        .iter()
        .map(|nth| {
            format!(r#"{{ "direction": "down", "client": 1, "kind": "PACK", "nth": {nth} }}"#)
        })
        .collect();
    let (_, trace) = run(&format!(
        r#"{{ "name": "j06", "players": 4, "warmup_ms": 0, "duration_ms": 15000,
        "apm": 300, "command_bytes": 200, "drop_rules": [{}] }}"#,
        rules.join(",")
    ));
    check_wire(&trace).unwrap();
    check_retransmissions(&trace, 1).unwrap();
}

// --- Judgment 7 (spec §4.1, §3.3) ---

/// Every UPLINK's `down_high` / `down_bits` and every ACK's or STATUS's
/// `up_high` / `up_bits` are exactly the datagrams the network delivered on
/// that hop before encoding: the highest such hop sequence, and bit `i` set
/// iff `high - i` arrived. CHALLENGE and LEAVE_ACK (relay) and HELLO
/// (client) never count. A datagram arriving within 1 ms of the encoding
/// is left unjudged (event order at one instant is not the protocol's).
/// The relay's window counts datagrams it authenticated: those it
/// accepted or refused as a replay; others are unjudged. `echo_delay` is
/// the time from `up_high`'s arrival to the encoding, ±1 ms (§3.3).
pub(super) fn check_bitmaps(trace: &Trace, clients: usize) -> Check {
    for client in 0..clients {
        let down: Vec<&WireRecord> = trace.of(client, Direction::Down).collect();
        let up: Vec<&WireRecord> = trace.of(client, Direction::Up).collect();
        // Client side.
        for record in &up {
            let Some(uplink) = record.uplink() else {
                continue;
            };
            let t = record.sent_us;
            let mut unknown = BTreeSet::new();
            let mut received = BTreeSet::new();
            for d in down.iter().filter(|d| d.hop == record.hop) {
                if matches!(d.kind(), "CHALLENGE" | "LEAVE_ACK") {
                    continue;
                }
                match d.arrived_us() {
                    Some(at) if at + 1000 <= t => {
                        received.insert(d.hop_seq());
                    }
                    Some(at) if at <= t + 1000 => {
                        unknown.insert(d.hop_seq());
                    }
                    _ => {}
                }
            }
            check_window(uplink.down_high, uplink.down_bits, &received, &unknown).map_err(|e| {
                format!(
                    "client {client} UPLINK at {} ms: down {e} (spec §4.1)",
                    record.sent_ms()
                )
            })?;
        }
        // Relay side.
        for record in &down {
            let feedback = match record.parsed.as_ref().map(|d| &d.detail) {
                Ok(Detail::Ack { feedback: f, .. }) | Ok(Detail::Status { feedback: f, .. }) => *f,
                _ => continue,
            };
            let t = record.sent_us;
            let mut unknown = BTreeSet::new();
            let mut received = BTreeSet::new();
            let mut arrival_of = HashMap::new();
            for u in up
                .iter()
                .filter(|u| u.hop == record.hop && u.kind() != "HELLO")
            {
                let Some(at) = u.arrived_us() else {
                    continue;
                };
                let authenticated = match &u.relay_result {
                    Some(Ok(())) => true,
                    Some(Err(error)) => error == "Replay",
                    None => false,
                };
                if at > t + 1000 {
                    continue;
                }
                if at + 1000 > t || !authenticated {
                    unknown.insert(u.hop_seq());
                } else {
                    received.insert(u.hop_seq());
                    arrival_of.insert(u.hop_seq(), at);
                }
            }
            check_window(feedback.high, feedback.bits, &received, &unknown).map_err(|e| {
                format!(
                    "client {client} {} at {} ms: up {e} (spec §4.1)",
                    record.kind(),
                    record.sent_ms()
                )
            })?;
            if let Some(&at) = arrival_of.get(&feedback.high) {
                let echo = (t - at) / 1000;
                ensure(
                    u64::from(feedback.echo_delay_ms).abs_diff(echo) <= 1,
                    || {
                        format!(
                            "client {client} echo_delay {} for {echo} ms at {} ms (spec §3.3)",
                            feedback.echo_delay_ms,
                            record.sent_ms()
                        )
                    },
                )?;
            }
        }
    }
    Ok(())
}

fn check_window(high: u64, bits: u64, received: &BTreeSet<u64>, unknown: &BTreeSet<u64>) -> Check {
    let known_high = received.iter().max().copied().unwrap_or(0);
    let possible_high = unknown.iter().max().copied().unwrap_or(0).max(known_high);
    if high < known_high || high > possible_high {
        return Err(format!(
            "high {high}, delivered {known_high}..={possible_high}"
        ));
    }
    for i in 0..64u64 {
        let Some(seq) = high.checked_sub(i).filter(|seq| *seq >= 1) else {
            break;
        };
        if unknown.contains(&seq) {
            continue;
        }
        let bit = bits >> i & 1 == 1;
        if bit != received.contains(&seq) {
            return Err(format!(
                "bit for {seq} is {bit} (high {high}, bits {bits:#x})"
            ));
        }
    }
    Ok(())
}

#[test]
fn j07_feedback_bitmaps_match_the_network() {
    let (_, trace) = run(r#"{ "name": "j07", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "loss": 0.05 } },
        "drop_rules": [ { "direction": "down", "client": 0, "kind": "CHALLENGE", "nth": 1 } ] }"#);
    check_wire(&trace).unwrap();
    check_bitmaps(&trace, 2).unwrap();
}

// --- Judgment 8 (spec §4.3) ---

/// The protocol's loss estimates against the network's real loss.
///
/// Uplink: the mean over folds (§10.1) of `loss = (3 loss + sample) / 4`,
/// with each sample `l * 1000 / n` over n >= 32 judged packets and the
/// estimate starting at 0. With independent loss at rate p, a sample has
/// mean p and standard deviation `sqrt(p (1 - p) / 32)`; after F folds the
/// estimate's mean is p (1 - (3/4)^k) at fold k, so the fold mean is about
/// `p (1 - 3 / F)`, and its spread is at most that of the mean of F samples,
/// `sqrt(p (1 - p) / (32 F))`. The bound is that expectation ± 3 of those
/// standard deviations, ± 5‰ for the packets each feedback leaves unjudged.
///
/// Downlink: the relay's current estimate. Once settled, the estimate's
/// standard deviation is the sample's times `sqrt(α / (2 - α))` with
/// α = 1/4, i.e. `sqrt(p (1 - p) / 32) / sqrt(7)`; bound ±3 of those.
fn check_loss_estimates(trace: &Trace, client: usize) -> Check {
    let rate = |direction: Direction| {
        let sent: Vec<&WireRecord> = trace
            .of(client, direction)
            .filter(|r| !matches!(r.kind(), "HELLO" | "CHALLENGE" | "LEAVE_ACK"))
            .collect();
        sent.iter().filter(|r| r.lost()).count() as f64 / sent.len().max(1) as f64
    };
    let stats = trace
        .client_stats_at(client, u64::MAX)
        .ok_or("no link stats")?;
    let folds = stats.up_loss_folds as f64;
    ensure(folds >= 8.0, || format!("only {folds} uplink folds"))?;
    let p = rate(Direction::Up);
    let mean = stats.up_loss_permille_sum as f64 / folds;
    let expected = 1000.0 * p * (1.0 - 3.0 / folds);
    let bound = 3000.0 * (p * (1.0 - p) / (32.0 * folds)).sqrt() + 5.0;
    ensure((mean - expected).abs() <= bound, || {
        format!(
            "uplink loss mean {mean:.1}‰, network {:.1}‰, expected {expected:.1} ± {bound:.1}",
            1000.0 * p
        )
    })?;
    let reader = trace
        .reader_stats_at(client, u64::MAX)
        .ok_or("no reader stats")?;
    let p = rate(Direction::Down);
    let bound = 3000.0 * (p * (1.0 - p) / 32.0).sqrt() / 7f64.sqrt();
    let estimate = f64::from(reader.down_loss_permille);
    ensure((estimate - 1000.0 * p).abs() <= bound, || {
        format!(
            "downlink loss {estimate}‰, network {:.1}‰ ± {bound:.1}",
            1000.0 * p
        )
    })
}

#[test]
fn j08_reported_loss_tracks_the_network_loss() {
    let (_, trace) = run(r#"{ "name": "j08", "players": 2,
        "warmup_ms": 0, "duration_ms": 40000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "loss": 0.10 } } }"#);
    check_loss_estimates(&trace, 0).unwrap();
    check_loss_estimates(&trace, 1).unwrap();
}

// --- Judgment 9 (spec §5.1, §5.5, §6.3 row 5, §6.6, §7.2, §13.2) ---

/// An outage of `client` over `[from_ms, to_ms)` in both directions:
/// - once 1000 ms passed without a datagram reaching the client (§5.1),
///   every UPLINK until one arrives again is a PROBE, at least 250 ms after
///   the previous UPLINK (§6.3 row 5, §6.6);
/// - once 1500 ms passed without the relay hearing the client, the relay
///   pushes nothing to it until it hears it again (§7.2, "断开: 不发");
/// - the client sends an UPLINK in the service after its first datagram
///   arrives again (§5.5, "立即发一个 UPLINK");
/// - the relay's first push after the first UPLINK reaches it starts at the
///   reader's `acked_next` (§5.5: `sent_next = acked_next`) and leaves in
///   the pass that read that UPLINK, so it reaches the client within one
///   RTT of that UPLINK (§13.2). The UPLINK, not any datagram, is the
///   start: a 5 s outage outlives the 3000 ms push lease, and only an
///   UPLINK subscribes again (§11 row 22);
/// - the client is caught up to the relay's `next_seq` (from the first
///   STATUS after recovery) within one RTT plus, for each 64 records of
///   backlog (`PUSH_WINDOW`, §7.4), one acknowledgement round (RTT + the
///   100 ms good interval of §6.3 row 3 + a service; §13.2).
fn check_outage(trace: &Trace, client: usize, from_ms: u64, to_ms: u64) -> Check {
    let from = from_ms * 1000;
    let to = to_ms * 1000;
    let down: Vec<&WireRecord> = trace.of(client, Direction::Down).collect();
    let up: Vec<&WireRecord> = trace.of(client, Direction::Up).collect();
    let last_in = down
        .iter()
        .filter_map(|r| r.arrived_us())
        .filter(|at| *at < from)
        .max()
        .ok_or("nothing reached the client before the outage")?;
    let back_in = down
        .iter()
        .filter_map(|r| r.arrived_us())
        .filter(|at| *at >= to)
        .min()
        .ok_or("nothing reached the client after the outage")?;
    let offline = last_in + 1_000_000;
    let mut previous: Option<u64> = None;
    for record in up.iter().filter(|r| r.uplink().is_some()) {
        let t = record.sent_us;
        if t > offline && t < back_in {
            let uplink = record.uplink().unwrap();
            ensure(uplink.flags & FLAG_PROBE != 0, || {
                format!(
                    "non-probe UPLINK at {} ms while offline (spec §6.3 row 5)",
                    record.sent_ms()
                )
            })?;
            if let Some(previous) = previous {
                ensure(t - previous >= 250_000, || {
                    format!(
                        "probes {} ms apart at {} ms (spec §6.3 row 5)",
                        ms(t - previous),
                        record.sent_ms()
                    )
                })?;
            }
        }
        previous = Some(t);
    }
    let heard = up
        .iter()
        .filter_map(|r| r.arrived_us())
        .filter(|at| *at < from)
        .max()
        .ok_or("the relay heard nothing before the outage")?;
    let heard_again = up
        .iter()
        .filter_map(|r| r.arrived_us())
        .filter(|at| *at >= to)
        .min()
        .ok_or("the relay heard nothing after the outage")?;
    let recovery = up
        .iter()
        .find(|r| r.sent_us >= back_in && r.uplink().is_some())
        .ok_or("no UPLINK after the outage")?;
    ensure(recovery.sent_us <= back_in + 16_000, || {
        format!(
            "first UPLINK {} ms after the link came back (spec §5.5)",
            ms(recovery.sent_us - back_in)
        )
    })?;
    let subscribed = up
        .iter()
        .filter(|r| r.uplink().is_some())
        .filter_map(|r| r.arrived_us())
        .filter(|at| *at >= to)
        .min()
        .ok_or("no UPLINK reached the relay after the outage")?;
    // One pass of slack past the 1500 ms threshold.
    let reader_offline = heard + 1_501_000;
    if let Some(push) = down
        .iter()
        .find(|r| r.origin == Origin::Push && r.sent_us > reader_offline && r.sent_us < heard_again)
    {
        return Err(format!(
            "push to an offline reader at {} ms (spec §7.2)",
            push.sent_ms()
        ));
    }
    let first = down
        .iter()
        .find(|r| r.origin == Origin::Push && r.sent_us >= subscribed && r.pack().is_some())
        .ok_or("no push after recovery")?;
    ensure(first.sent_us <= subscribed + 1000, || {
        format!(
            "first push {} ms after the relay read the client's UPLINK",
            ms(first.sent_us - subscribed)
        )
    })?;
    let rtt = 2 * 20_000;
    let uplink_sent = up
        .iter()
        .filter(|r| r.uplink().is_some() && r.arrived_us() == Some(subscribed))
        .map(|r| r.sent_us)
        .min()
        .unwrap();
    let first_in = first.arrived_us().ok_or("the recovery push was lost")?;
    ensure(first_in <= uplink_sent + rtt + 1000, || {
        format!(
            "recovery push reached the client {} ms after its UPLINK (spec §13.2: 1 RTT)",
            ms(first_in - uplink_sent)
        )
    })?;
    let acked = up
        .iter()
        .filter(|r| r.arrived_us().is_some_and(|at| at <= first.sent_us))
        .filter_map(|r| r.uplink().map(|u| u.recv_next))
        .max()
        .unwrap_or(1);
    let (base, _) = first.pack().unwrap();
    ensure(base == acked, || {
        format!("recovery push from {base}, acked_next {acked} (spec §5.5)")
    })?;
    let next_seq = down
        .iter()
        .filter(|r| r.sent_us >= heard_again)
        .find_map(|r| match r.parsed.as_ref().map(|d| &d.detail) {
            Ok(Detail::Status { next_seq, .. }) => Some(*next_seq),
            _ => None,
        })
        .ok_or("no STATUS after recovery")?;
    let rounds = next_seq.saturating_sub(acked).div_ceil(64);
    let deadline = subscribed + rtt + rounds * (rtt + 116_000);
    let caught = up
        .iter()
        .filter(|r| r.sent_us >= subscribed)
        .find(|r| r.uplink().is_some_and(|u| u.recv_next >= next_seq))
        .ok_or("never caught up")?;
    ensure(caught.sent_us <= deadline, || {
        format!(
            "caught up {} ms after recovery; bound {} ms",
            ms(caught.sent_us - subscribed),
            ms(deadline - subscribed)
        )
    })
}

/// The outage adds no loss (§5.5: on recovery `eval_next` skips every
/// datagram sent before it; §13.2): with no loss besides it,
/// `up_loss_permille_max` and the reader's `down_loss_permille` stay 0.
fn check_outage_loss(trace: &Trace, client: usize) -> Check {
    let stats = trace
        .client_stats_at(client, u64::MAX)
        .ok_or("no link stats")?;
    ensure(stats.up_loss_permille_max == 0, || {
        format!(
            "uplink loss {}‰ from the outage (spec §5.5)",
            stats.up_loss_permille_max
        )
    })?;
    let reader = trace
        .reader_stats_at(client, u64::MAX)
        .ok_or("no reader stats")?;
    ensure(reader.down_loss_permille == 0, || {
        format!(
            "downlink loss {}‰ from the outage (spec §5.5)",
            reader.down_loss_permille
        )
    })
}

#[test]
fn j09_offline_sends_only_probes_and_recovers_from_the_watermark() {
    let (_, trace) = run(r#"{ "name": "j09", "players": 2,
        "warmup_ms": 0, "duration_ms": 15000, "apm": 120,
        "groups": [ { "label": "outage", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 20, "outages": [[5000, 10000]] } } } ] }"#);
    check_wire(&trace).unwrap();
    check_outage(&trace, 1, 5000, 10000).unwrap();
    assert!(
        trace
            .client_stats
            .iter()
            .any(|(_, c, s)| *c == 1 && s.uplink == LinkState::Offline)
    );
}

#[test]
fn j09_an_outage_adds_no_loss() {
    let (_, trace) = run(r#"{ "name": "j09-loss", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "network": { "link": { "delay_ms": 20, "outages": [[4000, 9000]] } } }"#);
    for client in 0..2 {
        check_outage_loss(&trace, client).unwrap();
    }
}

// --- Judgment 10 (spec §5.2, §5.3, §6.3 rows 1-2, §7.2) ---

/// Once the client judges DELAYED (§5.3) its uplink is Congested (§5.2)
/// and stays so for the 10 s hold after DELAYED last held, no longer; the
/// UPLINK link byte's bit2 is DELAYED itself, so it may clear inside the
/// hold; no copy is sent while congested
/// (§6.3 row 6 is lossy only), an UPLINK carrying a never-sent batch is at
/// least 33 ms after the previous UPLINK (row 1, 拥堵), any two UPLINKs are
/// at least 16 ms apart (row 2); the relay judges the reader Congested
/// (§5.2 from link bit2) for at most 10 s after the last DELAYED UPLINK
/// reached it, sends it no 8 ms repeat (JITTERY or not) and keeps K <= 2
/// (§7.2).
fn check_congestion(trace: &Trace, client: usize) -> Check {
    let entered = trace
        .client_stats
        .iter()
        .find(|(_, c, s)| *c == client && s.delayed)
        .map(|(at, ..)| *at)
        .ok_or("never DELAYED")?;
    let ups = uplinks(trace, client);
    let mut sent_batches = BTreeSet::new();
    let mut previous: Option<u64> = None;
    let mut copies_at_entry = None;
    for record in &ups {
        let uplink = record.uplink().unwrap();
        let t = record.sent_us;
        let stats = trace.client_stats_at(client, t);
        let congested = stats.is_some_and(|s| s.uplink == LinkState::Congested);
        if t >= entered && congested {
            let copies = stats.unwrap().copies;
            let first = *copies_at_entry.get_or_insert(copies);
            ensure(copies == first, || {
                format!("copy while congested at {} ms", record.sent_ms())
            })?;
            // §5.2: the congested hold may outlast DELAYED; the bit says
            // DELAYED itself (§3.2, §5.3).
            let delayed = stats.unwrap().delayed;
            ensure((uplink.link & 4 != 0) == delayed, || {
                format!(
                    "DELAYED bit {} with DELAYED {delayed} at {} ms (spec §3.2)",
                    uplink.link & 4 != 0,
                    record.sent_ms()
                )
            })?;
            if let Some(previous) = previous {
                ensure(t - previous >= 16_000, || {
                    format!(
                        "UPLINKs {} ms apart at {} ms (spec §6.3 row 2)",
                        ms(t - previous),
                        record.sent_ms()
                    )
                })?;
                if uplink.batch_ids().any(|id| !sent_batches.contains(&id)) {
                    ensure(t - previous >= 33_000, || {
                        format!(
                            "new batch {} ms after the previous UPLINK at {} ms (spec §6.3 row 1)",
                            ms(t - previous),
                            record.sent_ms()
                        )
                    })?;
                }
            }
        }
        sent_batches.extend(uplink.batch_ids());
        previous = Some(t);
    }
    ensure(copies_at_entry.is_some(), || {
        "no UPLINK while congested".into()
    })?;
    // §5.2: the congested state holds 10 s after its condition last held,
    // and no longer (50 ms of slack for services and passes).
    const HOLD_US: u64 = 10_000_000 + 50_000;
    let mut last_delayed: Option<u64> = None;
    for (at, c, stats) in &trace.client_stats {
        if *c != client {
            continue;
        }
        if stats.delayed {
            last_delayed = Some(*at);
        }
        if stats.uplink == LinkState::Congested {
            ensure(last_delayed.is_some_and(|d| *at <= d + HOLD_US), || {
                format!(
                    "uplink Congested at {} ms, more than 10 s after DELAYED (spec §5.2)",
                    ms(*at)
                )
            })?;
        }
    }
    let delayed_arrivals: Vec<u64> = ups
        .iter()
        .filter(|r| r.uplink().unwrap().link & 4 != 0)
        .filter_map(|r| r.arrived_us())
        .collect();
    let mut congested_seen = false;
    let mut previous: Option<&crate::ReaderLinkStats> = None;
    for (at, c, stats) in &trace.reader_stats {
        if *c != client {
            continue;
        }
        if stats.downlink == LinkState::Congested {
            congested_seen = true;
            ensure(
                delayed_arrivals
                    .iter()
                    .any(|d| *d <= *at && *at <= d + HOLD_US),
                || {
                    format!(
                        "reader Congested at {} ms, more than 10 s after a DELAYED UPLINK (spec §5.2)",
                        ms(*at)
                    )
                },
            )?;
            ensure(stats.piggyback_depth <= 2, || {
                format!("K {} while congested", stats.piggyback_depth)
            })?;
            if let Some(before) = previous {
                ensure(
                    stats.packets_by_kind[2] == before.packets_by_kind[2],
                    || format!("repeat to a congested reader at {} ms (spec §7.2)", ms(*at)),
                )?;
            }
        }
        previous = Some(stats);
    }
    ensure(congested_seen, || {
        "the relay never judged the reader congested".into()
    })
}

#[test]
fn j10_a_rising_rtt_congests_without_copies_or_repeats() {
    // RTT 40 ms, then 300 ms from 6 s (spec §13.2).
    let (_, trace) = run(r#"{ "name": "j10", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300,
        "groups": [ { "label": "slow", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 20, "delay_changes": [[6000, 150]] } } } ] }"#);
    check_wire(&trace).unwrap();
    check_congestion(&trace, 1).unwrap();
}

// --- Judgment 11 (spec §4.5, §6.0) ---

#[test]
fn j11_a_lost_first_uplink_is_resent_after_one_rto() {
    let (_, trace) = run(r#"{ "name": "j11", "players": 2,
        "warmup_ms": 0, "duration_ms": 4000,
        "drop_rules": [ { "direction": "up", "client": 1, "kind": "UPLINK", "nth": 1 } ] }"#);
    check_wire(&trace).unwrap();
    let ups = uplinks(&trace, 1);
    let (first, second) = (ups[0], ups[1]);
    assert!(first.lost());
    // §4.5: RTO = clamp(srtt + max(16, 4 rttvar), 48, 1000), 100 without a
    // sample; k = 0 for the first resend.
    let stats = trace.client_stats_at(1, first.sent_us).unwrap();
    let rto = match (stats.srtt_ms, stats.rttvar_ms) {
        (Some(srtt), Some(rttvar)) => {
            (u64::from(srtt) + 16.max(4 * u64::from(rttvar))).clamp(48, 1000)
        }
        _ => 100,
    };
    let waited = second.sent_us - first.sent_us;
    assert!(
        (rto * 1000..rto * 1000 + 16_000).contains(&waited),
        "resent after {} ms, RTO {rto} ms (spec §6.0)",
        ms(waited)
    );
    assert!(second.uplink().unwrap().flags & FLAG_PUSH != 0);
    // The relay starts pushing in the pass that reads it (§7.1, §7.2).
    let arrived = second.arrived_us().unwrap();
    let push = pushes(&trace, 1)
        .into_iter()
        .find(|r| r.pack().is_some())
        .expect("a push");
    assert!(push.sent_us >= arrived && push.sent_us <= arrived + 1000);
    // §6.0: once a PACK arrived, the RTO resends stop: the next UPLINK has
    // another reason, here the watermark's interval of §6.3 row 3, which
    // is `turn_ms` while the uplink is still lossy before its first fold
    // (§5.4) and 100 ms once good.
    if let Some(third) = ups.get(2) {
        let lossy = trace
            .client_stats_at(1, second.sent_us)
            .is_some_and(|s| s.uplink == LinkState::Lossy);
        let interval = if lossy { 67_000 } else { 100_000 };
        assert!(third.sent_us + 16_000 >= second.sent_us + interval);
    }
}

// --- Judgment 12 (spec §4.7, §6.0) ---

#[test]
fn j12_a_renewed_hop_sends_at_once_and_counts_from_zero() {
    let (_, trace) = run(r#"{ "name": "j12", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "renewals": [ { "client": 1, "at_ms": 6000 } ] }"#);
    check_wire(&trace).unwrap();
    check_batch_segments(&trace).unwrap();
    check_bitmaps(&trace, 2).unwrap();
    let ready = trace
        .of(1, Direction::Down)
        .find(|r| r.hop == 1 && r.kind() == "READY")
        .expect("new hop READY");
    let promoted = ready.arrived_us().unwrap();
    let first = uplinks(&trace, 1)
        .into_iter()
        .find(|r| r.hop == 1)
        .expect("an UPLINK on the new hop");
    // §4.7: in the service that switches hops, before any other condition.
    assert!(
        first.sent_us >= promoted && first.sent_us < promoted + 16_000,
        "first new-hop UPLINK {} ms after promotion",
        ms(first.sent_us - promoted)
    );
    let uplink = first.uplink().unwrap();
    assert!(uplink.flags & FLAG_PUSH != 0);
    // A fresh down window: only the new hop's READY so far (§4.7).
    assert_eq!((uplink.down_high, uplink.down_bits), (1, 1));
    // Measurement carries over (§4.7).
    let stats = trace.client_stats_at(1, first.sent_us).unwrap();
    assert!(stats.srtt_ms.is_some() && stats.base_ms.is_some());
    // §4.7: the new subscription pushes from the UPLINK's recv_next.
    let push = pushes(&trace, 1)
        .into_iter()
        .find(|r| r.hop == 1 && r.pack().is_some())
        .expect("a push on the new hop");
    assert_eq!(push.pack().unwrap().0, uplink.recv_next);
    // §4.7, §10.2: the relay's reader counts restart on the new Peer and it
    // starts Good with no loss.
    let reader = trace.reader_stats_at(1, push.sent_us).unwrap();
    let new_hop_sent = trace
        .of(1, Direction::Down)
        .filter(|r| r.hop == 1 && r.sent_us <= push.sent_us)
        .count() as u64;
    assert!(reader.packets_by_kind.iter().sum::<u64>() <= new_hop_sent);
    assert_eq!(reader.downlink, LinkState::Good);
    assert_eq!(reader.down_loss_permille, 0);
}

// --- Judgment 13 (spec §3.2 flags bit3, §11 row 9) ---

#[test]
fn j13_a_restoring_reader_gets_no_new_records_pushed() {
    let (_, trace) = run(r#"{ "name": "j13", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 60,
        "restoring": [ { "client": 1, "from_ms": 5000, "to_ms": 8000 } ] }"#);
    check_wire(&trace).unwrap();
    let ups = uplinks(&trace, 1);
    let restoring = |r: &WireRecord| r.uplink().unwrap().flags & FLAG_RESTORING != 0;
    let first = ups
        .iter()
        .find(|r| restoring(r))
        .expect("an UPLINK with RESTORING");
    assert!(first.sent_us >= 5_000_000);
    let cleared = ups
        .iter()
        .find(|r| r.sent_us > first.sent_us && !restoring(r))
        .expect("an UPLINK after restoring");
    assert!(cleared.sent_us >= 8_000_000);
    assert!(
        ups.iter()
            .filter(|r| r.sent_us < cleared.sent_us && r.sent_us >= first.sent_us)
            .all(|r| restoring(r))
    );
    let from = first.arrived_us().expect("RESTORING reached the relay");
    let to = cleared.arrived_us().expect("the clear reached the relay");
    let pushed = pushes(&trace, 1);
    let high = pushed
        .iter()
        .filter(|r| r.sent_us <= from)
        .filter_map(|r| r.pack())
        .flat_map(|(_, records)| records)
        .map(|e| e.seq + 1)
        .max()
        .unwrap_or(1);
    for record in pushed.iter().filter(|r| r.sent_us > from && r.sent_us < to) {
        if let Some((_, records)) = record.pack() {
            assert!(
                records.iter().all(|e| e.seq < high),
                "new record pushed while restoring at {} ms (spec §11 row 9)",
                record.sent_ms()
            );
        }
    }
    assert!(
        trace
            .of(1, Direction::Down)
            .any(|r| r.origin == Origin::Reply
                && r.pack().is_some()
                && r.sent_us > from
                && r.sent_us < to),
        "no PULL reply while restoring"
    );
    // §11 row 9: pushing resumes from acked_next.
    let resumed = pushed
        .iter()
        .find(|r| r.sent_us >= to && r.pack().is_some())
        .expect("push after restoring");
    let acked = ups
        .iter()
        .filter(|r| r.arrived_us().is_some_and(|at| at <= resumed.sent_us))
        .map(|r| r.uplink().unwrap().recv_next)
        .max()
        .unwrap();
    assert_eq!(resumed.pack().unwrap().0, acked);
}

// --- Judgment 14 (spec §1.1) ---

/// Every client read the same stream; each source's batches are its sealed
/// batches in order from 1; every batch sealed and every command issued
/// three seconds before the end reached every client and ran; nobody
/// stalled; no DESYNC record without a checksum fault.
pub(super) fn check_consistency(report: &Report, trace: &Trace, end_us: u64) -> Check {
    ensure(report.stream_consistent, || "streams differ".into())?;
    ensure(report.desync_records == 0, || "a DESYNC record".into())?;
    ensure(report.stalled_clients == 0, || {
        format!("{} stalled", report.stalled_clients)
    })?;
    let reference = trace.streams.iter().max_by_key(|s| s.len()).unwrap();
    for (client, seals) in trace.seals.iter().enumerate() {
        let ids: Vec<u64> = reference
            .iter()
            .filter_map(|r| match r {
                Record::Batch {
                    source, batch_id, ..
                } if usize::from(*source) == client => Some(*batch_id),
                _ => None,
            })
            .collect();
        ensure(ids.iter().copied().eq(1..=ids.len() as u64), || {
            format!("source {client} batches out of order: {ids:?}")
        })?;
        for (at, id) in seals {
            if *at + 3_000_000 < end_us {
                for (reader, stream) in trace.streams.iter().enumerate() {
                    ensure(
                        stream.iter().any(|r| matches!(r, Record::Batch { source, batch_id, .. } if usize::from(*source) == client && batch_id == id)),
                        || format!("client {reader} never read batch {id} of source {client}"),
                    )?;
                }
            }
        }
        let executed = trace.executed[client].len();
        let issued = trace.issued[client]
            .iter()
            .filter(|at| **at + 3_000_000 < end_us)
            .count();
        ensure(executed >= issued, || {
            format!("client {client}: {issued} commands issued, {executed} ran")
        })?;
    }
    Ok(())
}

const MIXED_FOUR: &str = r#""groups": [
    { "label": "lossy", "first": 1, "last": 1, "network": { "link": { "delay_ms": 20, "loss": 0.05 } } },
    { "label": "burst", "first": 2, "last": 2, "network": { "link": { "delay_ms": 20,
        "burst": { "p_good_to_bad": 0.01, "p_bad_to_good": 0.25, "bad_loss": 0.75 } } } },
    { "label": "far", "first": 3, "last": 3, "network": { "link": { "delay_ms": 100, "jitter_ms": 50 } } } ]"#;

#[test]
fn j14_every_client_runs_the_relays_stream() {
    let (report, trace) = run(&format!(
        r#"{{ "name": "j14", "players": 4,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300, {MIXED_FOUR} }}"#
    ));
    check_wire(&trace).unwrap();
    check_batch_segments(&trace).unwrap();
    check_consistency(&report, &trace, 20_000_000).unwrap();
}

// --- CI smoke tests (e2e-plan §2.5): short runs, judgments and bounds ---

fn group_down(report: &Report, kind: &str) -> f64 {
    report.groups[0]
        .down
        .kinds
        .get(kind)
        .copied()
        .unwrap_or(0.0)
}

#[test]
fn smoke_good_idle() {
    let (report, trace) = run(
        r#"{ "name": "smoke-good-idle",         "players": 2, "warmup_ms": 2000, "duration_ms": 10000 }"#,
    );
    check_wire(&trace).unwrap();
    check_bitmaps(&trace, 2).unwrap();
    // §13.3: an idle good link stays at or below 20 datagrams a second.
    assert!(
        report.groups[0].down.packets_per_s <= 20.0,
        "{:?}",
        report.groups[0].down
    );
    assert_eq!(report.groups[0].pulls, 0.0);
    for client in 0..2 {
        // §5.4: lossy until each direction's first fold.
        assert_eq!(
            repeats_since(&trace, client, downlink_settled_us(&trace, client)),
            0
        );
        assert_eq!(
            copies_since(&trace, client, uplink_settled_us(&trace, client)),
            0
        );
    }
}

#[test]
fn smoke_loss_keeps_latency_within_one_and_a_half_good_links() {
    let (good, good_trace) = run(
        r#"{ "name": "smoke-good-300", "players": 4, "warmup_ms": 2000, "duration_ms": 13000, "apm": 300 }"#,
    );
    check_wire(&good_trace).unwrap();
    check_batch_segments(&good_trace).unwrap();
    // §13.3's rule for a busy good link: new packs plus at most 10 more.
    let down = &good.groups[0].down;
    assert!(
        down.packets_per_s <= group_down(&good, "PACK_NEW") + 10.0,
        "{down:?}"
    );
    for client in 0..4 {
        check_good_uplink(&good_trace, client).unwrap();
    }
    let (lossy, lossy_trace) = run(
        r#"{ "name": "smoke-loss-300", "players": 4, "warmup_ms": 2000, "duration_ms": 13000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "loss": 0.05 } } }"#,
    );
    check_wire(&lossy_trace).unwrap();
    check_batch_segments(&lossy_trace).unwrap();
    // §13.2 "5% 随机丢包": both directions lossy, repeats sent, and command
    // p99 at most 1.5 times the good link's. Four players give some 300
    // commands, so the p99 is not just the largest one. (The copies of
    // §6.3 row 6 are judged by j03.)
    for client in 0..4 {
        let stats = lossy_trace.client_stats_at(client, u64::MAX).unwrap();
        assert!(stats.uplink_state_entries[LinkState::Lossy.index()] >= 1);
        assert!(
            lossy_trace
                .reader_stats_at(client, u64::MAX)
                .unwrap()
                .packets_by_kind[2]
                > 0
        );
        assert!(
            lossy_trace
                .reader_stats
                .iter()
                .any(|(_, c, s)| *c == client && s.downlink == LinkState::Lossy)
        );
    }
    let p99 = |report: &Report| report.groups[0].command_latency_ms.p99;
    assert!(
        p99(&lossy) <= 1.5 * p99(&good),
        "p99 {} vs good {}",
        p99(&lossy),
        p99(&good)
    );
}

#[test]
fn smoke_outage() {
    let (_, trace) = run(r#"{ "name": "smoke-outage", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "network": { "link": { "delay_ms": 20, "outages": [[4000, 9000]] } } }"#);
    check_wire(&trace).unwrap();
    for client in 0..2 {
        check_outage(&trace, client, 4000, 9000).unwrap();
    }
}

#[test]
fn smoke_congestion() {
    let (_, trace) = run(r#"{ "name": "smoke-congestion", "players": 2,
        "warmup_ms": 0, "duration_ms": 14000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "delay_changes": [[4000, 150]] } } }"#);
    check_wire(&trace).unwrap();
    for client in 0..2 {
        check_congestion(&trace, client).unwrap();
    }
}

#[test]
fn smoke_mixed_room() {
    let (report, trace) = run(&format!(
        r#"{{ "name": "smoke-mixed", "players": 4,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 300, {MIXED_FOUR} }}"#
    ));
    check_wire(&trace).unwrap();
    check_batch_segments(&trace).unwrap();
    check_bitmaps(&trace, 4).unwrap();
    check_consistency(&report, &trace, 12_000_000).unwrap();
    for client in 0..4 {
        check_piggyback(&trace, client).unwrap();
        check_repeats_only_when_lossy(&trace, client).unwrap();
    }
}

// --- §5.1 and §13.2: a PING ends the reader's offline state ---

#[test]
fn j09_a_ping_ends_the_readers_offline_state() {
    // After a 5 s outage the client's first datagram to arrive is its 2 s
    // PING; §5.1 ends the reader's offline state on any datagram of the
    // hop, though only an UPLINK subscribes again (§13.2).
    let (_, trace) = run(r#"{ "name": "j09-ping", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "groups": [ { "label": "outage", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 20, "outages": [[5000, 10000]] } } } ] }"#);
    let first = trace
        .of(1, Direction::Up)
        .filter(|r| r.arrived_us().is_some_and(|at| at >= 10_000_000))
        .min_by_key(|r| r.arrived_us())
        .expect("a datagram after the outage");
    assert_eq!(first.kind(), "PING", "the scenario's first datagram back");
    let arrived = first.arrived_us().unwrap();
    let reader = trace.reader_stats_at(1, arrived).expect("reader stats");
    assert_ne!(reader.downlink, LinkState::Offline, "at {} ms", ms(arrived));
}

// --- Downlink jitter (spec §4.8, §5.4, §7.2, §7.5, §13.2) ---

/// The client's JITTERY bit as §4.8 defines it, recomputed from the wire:
/// samples are the PACKs that bring a new turn, a FRAME_END at or past the
/// client's `recv_next` when they arrive, except PULL replies; a repeat or
/// retransmission that first brings it counts too, as §4.8 allows. `D = (t - t_prev) -
/// (f - f_prev) * 1000 / rate` with `f` the pack's highest frame,
/// `J += (|D| - J) / 16`, JITTERY from 16 samples with `j0 = max(10,
/// turn_ms / 4)` unrounded, set at `J >= j0`, cleared at `J <= j0 / 2`. Each UPLINK's
/// `link` bit3 must match the state as of its sending, except within 1.5 ms
/// of either threshold or 20 ms of a change (rounding and service delay).
/// Returns whether any UPLINK set the bit.
fn check_jitter(trace: &Trace, client: usize, tick_hz: u16) -> Result<bool, String> {
    let rate = f64::from(tick_hz);
    // §4.8: exact, not rounded (16.75 ms at 30 Hz).
    let j0 = (f64::from(crate::turn_ms(tick_hz)) / 4.0).max(10.0);
    let down: Vec<&WireRecord> = trace.of(client, Direction::Down).collect();
    let mut arrivals: Vec<(u64, usize)> = down
        .iter()
        .enumerate()
        .filter_map(|(index, record)| record.arrived_us().map(|at| (at, index)))
        .collect();
    arrivals.sort();
    let mut next = 1;
    let mut ahead = BTreeSet::new();
    let mut previous: Option<(u64, u32)> = None;
    let (mut j, mut samples, mut jittery) = (0.0f64, 0u32, false);
    // (time µs, J, JITTERY) after each sample.
    let mut states: Vec<(u64, f64, bool)> = Vec::new();
    for (at, index) in arrivals {
        let Some((_, records)) = down[index].pack() else {
            continue;
        };
        let turn = records
            .iter()
            .filter(|e| e.kind == 4 && e.seq >= next)
            .map(|e| e.frame)
            .max();
        if down[index].origin != Origin::Reply
            && let Some(frame) = turn
        {
            if let Some((t_prev, f_prev)) = previous {
                let frames = f64::from(frame) - f64::from(f_prev);
                let d = ms(at - t_prev) - frames * 1000.0 / rate;
                j += (d.abs() - j) / 16.0;
                samples += 1;
                if samples >= 16 {
                    jittery = if jittery { j > j0 / 2.0 } else { j >= j0 };
                }
                states.push((at, j, jittery));
            }
            previous = Some((at, frame));
        }
        for entry in records {
            if entry.seq >= next {
                ahead.insert(entry.seq);
            }
        }
        while ahead.remove(&next) {
            next += 1;
        }
    }
    let mut set = false;
    for record in uplinks(trace, client) {
        let bit = record.uplink().unwrap().link & 8 != 0;
        set |= bit;
        let t = record.sent_us;
        let Some(&(_, j, expected)) = states.iter().filter(|(at, ..)| *at <= t).last() else {
            ensure(!bit, || {
                format!("JITTERY at {} ms before any sample", record.sent_ms())
            })?;
            continue;
        };
        let near = (j - j0).abs() < 1.5 || (j - j0 / 2.0).abs() < 1.5;
        let changed = states
            .iter()
            .filter(|(at, ..)| *at <= t && *at + 20_000 > t)
            .any(|(_, _, state)| *state != expected)
            || states
                .windows(2)
                .any(|w| w[1].0 <= t && w[1].0 + 20_000 > t && w[0].2 != w[1].2);
        if near || changed {
            continue;
        }
        ensure(bit == expected, || {
            format!(
                "client {client} UPLINK at {} ms: JITTERY {bit}, J {j:.1} ms (j0 {j0}) says {expected} (spec §4.8)",
                record.sent_ms()
            )
        })?;
    }
    Ok(set)
}

/// The current protocol's results on the same scenario files with seeds
/// 1-5, run at commit 720f12400 (the phase 1 runner, seed overridden):
/// every client's hitches summed over the room, per seed, and the room's
/// command p99 in ms, per seed.
const BASELINE_30P_LOSS_1PCT_300APM_HITCHES: [u64; 5] = [6, 7, 5, 4, 2];
const BASELINE_30P_LOSS_1PCT_300APM_P99_MS: [f64; 5] =
    [127.191, 127.237, 127.218, 127.160, 127.189];
const BASELINE_2P_JITTER_IDLE_HITCHES: [u64; 5] = [29, 32, 35, 31, 35];
const BASELINE_2P_JITTER_300APM_HITCHES: [u64; 5] = [26, 38, 34, 30, 34];
const SEEDS: [u64; 5] = [1, 2, 3, 4, 5];
/// §13.2: latency comparisons with a baseline allow 5 ms besides the
/// statistical bound, less than one 16 ms service.
const LATENCY_TOLERANCE_MS: f64 = 5.0;

/// Hitches of the new protocol against the baseline over the same seeds
/// and clients, so both counts cover the same client-minutes. Taking each
/// count as Poisson, the difference `new - old` has variance `new + old`;
/// the new protocol is no worse when `new <= old + 2 sqrt(new + old)`, a
/// one-sided bound of about 97.7 %. Hitches come in bursts, so their real
/// variance is larger and this bound is, if anything, strict.
fn hitches_no_worse(name: &str, new: u64, old: u64) {
    let bound = old as f64 + 2.0 * ((new + old) as f64).sqrt();
    assert!(
        new as f64 <= bound,
        "{name}: {new} hitches over five seeds, baseline {old}, bound {bound:.1}"
    );
}

/// Mean and sample variance.
fn mean_variance(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, variance)
}

fn baseline_scenario(file: &str, name: &str, seed: u64) -> Scenario {
    let mut scenario = Scenario::list_from_json(file)
        .unwrap()
        .into_iter()
        .find(|scenario| scenario.name == name)
        .unwrap();
    scenario.seed = seed;
    scenario
}

#[test]
fn jt1_a_jittery_good_link_gets_repeats_and_two_deep_piggyback() {
    // RTT 200 ms ± 50 ms jitter, no loss (spec §13.2).
    let (_, trace) = run(r#"{ "name": "jt1", "players": 2,
        "warmup_ms": 0, "duration_ms": 30000, "apm": 60,
        "groups": [ { "label": "jittery", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 100, "jitter_ms": 50 } } } ] }"#);
    check_wire(&trace).unwrap();
    assert!(check_jitter(&trace, 1, 30).unwrap(), "never JITTERY");
    assert!(
        !check_jitter(&trace, 0, 30).unwrap(),
        "the steady client was JITTERY"
    );
    // §7.2: a Good and JITTERY reader gets K = 2. (That it stays Good is
    // jt1b.)
    let ups = uplinks(&trace, 1);
    for (at, c, stats) in &trace.reader_stats {
        if *c != 1 || !stats.subscribed {
            continue;
        }
        let jittery = ups
            .iter()
            .filter(|r| r.arrived_us().is_some_and(|arrived| arrived <= *at))
            .max_by_key(|r| r.arrived_us())
            .is_some_and(|r| r.uplink().unwrap().link & 8 != 0);
        if jittery && stats.downlink == LinkState::Good {
            assert_eq!(stats.piggyback_depth, 2, "at {} ms", ms(*at));
        }
    }
    // §7.5: repeats go to it, and only while lossy or jittery.
    assert!(trace.reader_stats_at(1, u64::MAX).unwrap().packets_by_kind[2] > 0);
    check_repeats_only_when_lossy(&trace, 0).unwrap();
    check_repeats_only_when_lossy(&trace, 1).unwrap();
}

/// §7.2: a reader with large jitter is either Good and JITTERY or, when
/// reordering reads as loss (§7.2's note), Lossy; both get repeats and
/// K >= 2. Judged from the first JITTERY UPLINK reaching the relay after
/// the initial lossy state (§5.4), since JITTERY needs 16 samples (§4.8):
/// every reader snapshot from then on is one of the two, and every second
/// of it adds repeats.
#[test]
fn jt1b_a_jittery_reader_is_jittery_good_or_lossy_with_repeats() {
    let (_, trace) = run(r#"{ "name": "jt1", "players": 2,
        "warmup_ms": 0, "duration_ms": 30000, "apm": 60,
        "groups": [ { "label": "jittery", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 100, "jitter_ms": 50 } } } ] }"#);
    let ups = uplinks(&trace, 1);
    let jittery_at = |at: u64| {
        ups.iter()
            .filter(|r| r.arrived_us().is_some_and(|arrived| arrived <= at))
            .max_by_key(|r| r.arrived_us())
            .is_some_and(|r| r.uplink().unwrap().link & 8 != 0)
    };
    let first_jittery = ups
        .iter()
        .filter(|r| r.uplink().unwrap().link & 8 != 0)
        .filter_map(|r| r.arrived_us())
        .min()
        .expect("never JITTERY");
    let from = first_jittery.max(downlink_settled_us(&trace, 1));
    let end = trace.wire.iter().map(|r| r.sent_us).max().unwrap();
    for (at, c, stats) in &trace.reader_stats {
        if *c != 1 || *at < from || !stats.subscribed {
            continue;
        }
        match stats.downlink {
            LinkState::Good => {
                assert!(jittery_at(*at), "Good without JITTERY at {} ms", ms(*at));
                assert_eq!(stats.piggyback_depth, 2, "at {} ms", ms(*at));
            }
            LinkState::Lossy => {
                assert!(
                    stats.piggyback_depth >= 2,
                    "K {} at {} ms",
                    stats.piggyback_depth,
                    ms(*at)
                );
            }
            other => panic!("{other:?} at {} ms", ms(*at)),
        }
    }
    let mut second = from;
    while second + 1_000_000 <= end {
        let repeats = |t: u64| {
            trace
                .reader_stats_at(1, t)
                .map_or(0, |s| s.packets_by_kind[2])
        };
        assert!(
            repeats(second + 1_000_000) > repeats(second),
            "no repeat in the second from {} ms",
            ms(second)
        );
        second += 1_000_000;
    }
}

#[test]
fn jt2_a_steady_link_is_not_jittery() {
    let (_, trace) = run(r#"{ "name": "jt2", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300 }"#);
    for client in 0..2 {
        assert!(
            !check_jitter(&trace, client, 30).unwrap(),
            "client {client} JITTERY"
        );
        assert_eq!(
            repeats_since(&trace, client, downlink_settled_us(&trace, client)),
            0
        );
    }
}

#[test]
#[ignore = "slow; five 30-player minutes, run with --ignored when the loss or repeat rules change"]
fn jt3_one_percent_loss_in_a_full_room_is_no_worse_than_the_baseline() {
    let file = include_str!("../../netsim/loss-1pct.json");
    let mut hitches = 0;
    let mut p99s = Vec::new();
    for seed in SEEDS {
        let scenario = baseline_scenario(file, "30p-loss-1pct-300apm", seed);
        let (report, trace) = Simulation::new(scenario).run_traced();
        // §5.4: both directions of every client go lossy at 1% loss.
        for client in 0..30 {
            let stats = trace.client_stats_at(client, u64::MAX).unwrap();
            assert!(
                stats.uplink_state_entries[LinkState::Lossy.index()] >= 1,
                "seed {seed} client {client} uplink"
            );
            assert!(
                trace
                    .reader_stats
                    .iter()
                    .any(|(_, c, s)| *c == client && s.downlink == LinkState::Lossy),
                "seed {seed} client {client} downlink"
            );
        }
        hitches += report.clients.iter().map(|c| c.hitches as u64).sum::<u64>();
        p99s.push(report.groups[0].command_latency_ms.p99);
    }
    hitches_no_worse(
        "30p-loss-1pct-300apm",
        hitches,
        BASELINE_30P_LOSS_1PCT_300APM_HITCHES.iter().sum(),
    );
    // The p99 per seed: the new mean is no worse when within two standard
    // errors of the difference of the two means (Welch, five seeds each)
    // plus the 5 ms absolute tolerance of §13.2 for latencies.
    let (new_mean, new_var) = mean_variance(&p99s);
    let (old_mean, old_var) = mean_variance(&BASELINE_30P_LOSS_1PCT_300APM_P99_MS);
    let bound = old_mean + 2.0 * ((new_var + old_var) / 5.0).sqrt() + LATENCY_TOLERANCE_MS;
    assert!(
        new_mean <= bound,
        "p99 per seed {p99s:?}, mean {new_mean:.1}, baseline {old_mean:.1}, bound {bound:.1}"
    );
}

#[test]
#[ignore = "slow; ten 2-player minutes, run with --ignored when the jitter or repeat rules change"]
fn jt4_pure_jitter_hitches_no_more_than_the_baseline() {
    // §5.3 judges DELAYED on `srtt - 2 rttvar`, so ±50 ms jitter is no
    // congestion and the jittery reader gets its repeats (§7.2).
    let file = include_str!("../../netsim/rtt200-jitter50.json");
    for (name, baseline) in [
        ("2p-rtt200-jitter50-idle", BASELINE_2P_JITTER_IDLE_HITCHES),
        (
            "2p-rtt200-jitter50-300apm",
            BASELINE_2P_JITTER_300APM_HITCHES,
        ),
    ] {
        let hitches: u64 = SEEDS
            .iter()
            .map(|seed| {
                let report = Simulation::new(baseline_scenario(file, name, *seed)).run();
                report.clients.iter().map(|c| c.hitches as u64).sum::<u64>()
            })
            .sum();
        hitches_no_worse(name, hitches, baseline.iter().sum());
    }
}

#[test]
fn jt5_a_congested_jittery_reader_gets_no_repeats() {
    // RTT 40 ms, then 300 ms ± 50 ms from 6 s: DELAYED and jittery at once.
    let (_, trace) = run(r#"{ "name": "jt5", "players": 2,
        "warmup_ms": 0, "duration_ms": 25000, "apm": 300,
        "groups": [ { "label": "slow", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 20, "jitter_ms": 50,
                "delay_changes": [[6000, 150]] } } } ] }"#);
    check_wire(&trace).unwrap();
    // §7.2: congested beats JITTERY, so no repeat while congested.
    check_congestion(&trace, 1).unwrap();
    assert!(
        uplinks(&trace, 1)
            .iter()
            .any(|r| r.uplink().unwrap().link & 0x0C == 0x0C),
        "never DELAYED and JITTERY together"
    );
}

// --- Spec §5.4: lossy until the first fold ---

#[test]
fn j15_each_direction_starts_lossy_until_its_first_fold() {
    let (_, trace) = run(r#"{ "name": "j15", "players": 2,
        "warmup_ms": 0, "duration_ms": 10000, "apm": 300 }"#);
    for client in 0..2 {
        // Uplink: Lossy from the start to the first fold, with copies.
        let settled = uplink_settled_us(&trace, client);
        assert!(settled < u64::MAX, "client {client}: no fold");
        let started = uplinks(&trace, client)[0].sent_us;
        for (at, c, stats) in &trace.client_stats {
            if *c == client && *at >= started && *at < settled {
                assert_eq!(
                    stats.uplink,
                    LinkState::Lossy,
                    "client {client} at {} ms",
                    ms(*at)
                );
            }
        }
        assert!(
            trace.client_stats_at(client, settled).unwrap().copies > 0,
            "client {client}: no copy before the first fold"
        );
        // §5.4: the initial lossy state ends at the first fold by the
        // threshold alone, so on this lossless link the fold that makes
        // `up_loss_folds` 1 is the one that turns the uplink Good.
        let (_, _, first_fold) = trace
            .client_stats
            .iter()
            .find(|(_, c, stats)| *c == client && stats.up_loss_folds >= 1)
            .unwrap();
        assert_eq!(first_fold.up_loss_folds, 1);
        assert_eq!(
            first_fold.uplink,
            LinkState::Good,
            "client {client} after its first fold"
        );
        check_copies(&trace, client).unwrap();
        // Downlink: from the subscription, Lossy with K = 2 and repeats
        // until the relay's first fold.
        let reader_settled = downlink_settled_us(&trace, client);
        assert!(
            reader_settled < u64::MAX,
            "client {client}: reader never settled"
        );
        let mut subscribed = false;
        for (at, c, stats) in &trace.reader_stats {
            if *c != client || *at >= reader_settled {
                continue;
            }
            if stats.subscribed {
                subscribed = true;
                assert_eq!(
                    stats.downlink,
                    LinkState::Lossy,
                    "reader {client} at {} ms",
                    ms(*at)
                );
                assert_eq!(
                    stats.piggyback_depth,
                    2,
                    "reader {client} at {} ms",
                    ms(*at)
                );
            }
        }
        assert!(subscribed);
        let repeats = trace
            .reader_stats_at(client, reader_settled)
            .unwrap()
            .packets_by_kind[2];
        assert!(
            repeats > 0,
            "reader {client}: no repeat before the first fold"
        );
    }
}
