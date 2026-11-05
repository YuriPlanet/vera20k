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

const WAITS: &str = "waits for the adaptive transport implementation";

type Check = Result<(), String>;

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

// --- Judgment 1 (spec §1.2, §3) and the one-UPLINK-per-service rule (§6.3) ---

/// Every datagram parses by spec §3 (the inspector applies the six UPLINK
/// decoding rules of §3.2 and the 681-byte payload cap of §3.1); none of the
/// removed requests or PushState appears (§1.2); every ACK is the 31-byte
/// layout (§3.3) and every STATUS the 50-byte one (§3.4); every UPLINK has
/// PUSH set (§3.2 flags bit0, "对局开始后始终置 1") unless the client left;
/// a PROBE carries no batch and no checksum (§6.3 row 5, §6.6); and one
/// service sends at most one UPLINK, heartbeat included (§6.3).
fn check_wire(trace: &Trace) -> Check {
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
            Detail::Ack { feedback, .. } => ensure(feedback.is_some(), || {
                format!("13-byte ACK at {} ms (spec §3.3)", record.sent_ms())
            })?,
            Detail::Status { feedback, .. } => ensure(feedback.is_some(), || {
                format!("32-byte STATUS at {} ms (spec §3.4)", record.sent_ms())
            })?,
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

/// Spec §6.1, §6.2: every UPLINK but a PROBE or an IO heartbeat carries
/// the unacknowledged batches from `accepted_through + 1`, all of them up
/// to four (the batches here are small, so all fit); an IO heartbeat
/// repeats the batch segment of the UPLINK before it (§6.6).
fn check_batch_segments(trace: &Trace) -> Check {
    for client in 0..trace.seals.len() {
        let mut previous: Option<&Uplink> = None;
        for record in uplinks(trace, client) {
            let uplink = record.uplink().unwrap();
            if uplink.flags & FLAG_PROBE != 0 {
                previous = Some(uplink);
                continue;
            }
            if record.heartbeat {
                if let Some(previous) = previous {
                    ensure(
                        previous.first_batch_id == uplink.first_batch_id
                            && previous.batches == uplink.batches,
                        || {
                            format!(
                                "heartbeat at {} ms changed the batches (spec §6.6)",
                                record.sent_ms()
                            )
                        },
                    )?;
                }
                previous = Some(uplink);
                continue;
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
#[ignore = "waits for the adaptive transport implementation"]
fn j01_every_datagram_has_the_spec_layout() {
    let _ = WAITS;
    // Loss in both directions brings copies, repeats and retransmissions;
    // 300 APM brings batches, checksums every 15 frames and pace each second.
    let (report, trace) = run(r#"{ "name": "j01", "client": "adaptive", "players": 2,
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

/// On a good link no copy is sent (`copies` stays 0, §6.3 row 6 is lossy
/// only), each batch's first UPLINK goes out in the service that sealed it
/// (row 1, "立即"), and a batch with no other batch sealed within 200 ms of
/// it rides exactly one UPLINK (§13.2 row 1). Batches near another one also
/// ride that one's UPLINK while unacknowledged (§6.2), so §13.2's "1 个上行包"
/// is asserted only for isolated batches.
fn check_good_uplink(trace: &Trace, client: usize) -> Check {
    let copies = trace
        .client_stats
        .iter()
        .filter(|(_, c, _)| *c == client)
        .map(|(_, _, stats)| stats.copies)
        .max()
        .unwrap_or(0);
    ensure(copies == 0, || {
        format!("client {client}: {copies} copies on a good link")
    })?;
    let seals = &trace.seals[client];
    let ups = uplinks(trace, client);
    for &(sealed_at, id) in seals {
        let carrying: Vec<&&WireRecord> = ups
            .iter()
            .filter(|record| record.uplink().unwrap().carries(id))
            .collect();
        let first = carrying
            .first()
            .ok_or_else(|| format!("batch {id} never sent"))?;
        ensure(first.sent_us == sealed_at, || {
            format!(
                "batch {id} sealed at {} ms, first sent at {} ms (spec §6.3 row 1)",
                ms(sealed_at),
                first.sent_ms()
            )
        })?;
        let isolated = seals
            .iter()
            .all(|&(other, other_id)| other_id == id || other.abs_diff(sealed_at) > 200_000);
        if isolated {
            ensure(carrying.len() == 1, || {
                format!(
                    "isolated batch {id} rode {} UPLINKs (spec §13.2)",
                    carrying.len()
                )
            })?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn j02_a_good_link_sends_each_batch_once_without_copies() {
    let (_, trace) = run(r#"{ "name": "j02", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 15000, "apm": 60 }"#);
    check_wire(&trace).unwrap();
    for client in 0..2 {
        assert!(trace.seals[client].len() >= 5);
        check_good_uplink(&trace, client).unwrap();
    }
}

// --- Judgment 3 (spec §6.3 row 6 and its copy-content paragraph) ---

/// For each batch first sent while the uplink is lossy (§5.2, §5.4), one
/// UPLINK carrying it follows 16 ms later if it is still unacknowledged
/// then, and a second one 32 ms after the first send when the loss is
/// severe (§5.2 sub-band). Copies carry the original's checksum segment
/// and no pace (§6.3, copy content).
fn check_copies(trace: &Trace, client: usize) -> Check {
    let ups = uplinks(trace, client);
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
            let copy = copy.uplink().unwrap();
            let original = first.uplink().unwrap();
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
#[ignore = "waits for the adaptive transport implementation"]
fn j03_a_lossy_uplink_copies_each_new_batch_16_ms_later() {
    let (_, trace) = run(r#"{ "name": "j03", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300,
        "groups": [ { "label": "lossy", "first": 1, "last": 1,
            "network": { "up": { "delay_ms": 20, "loss": 0.06 } } } ] }"#);
    check_wire(&trace).unwrap();
    let last = trace.client_stats_at(1, u64::MAX).unwrap();
    assert!(last.uplink_state_entries[LinkState::Lossy.index()] >= 1);
    assert!(last.copies > 0);
    check_copies(&trace, 1).unwrap();
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn j03_a_severely_lossy_uplink_copies_at_16_and_32_ms() {
    let (_, trace) = run(
        r#"{ "name": "j03-severe", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300,
        "groups": [ { "label": "severe", "first": 1, "last": 1,
            "network": { "up": { "delay_ms": 20, "loss": 0.25 } } } ] }"#,
    );
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

/// The 8 ms repeat (send-log kind 2, §4.2) only goes to a lossy reader:
/// its count rises only from a snapshot whose downlink is Lossy.
fn check_repeats_only_when_lossy(trace: &Trace, client: usize) -> Check {
    let mut previous: Option<&crate::ReaderLinkStats> = None;
    for (at, c, stats) in &trace.reader_stats {
        if *c != client {
            continue;
        }
        if let Some(before) = previous
            && stats.packets_by_kind[2] > before.packets_by_kind[2]
        {
            ensure(
                before.downlink == LinkState::Lossy || stats.downlink == LinkState::Lossy,
                || {
                    format!(
                        "client {client}: a repeat at {} ms while {:?} (spec §7.2)",
                        ms(*at),
                        stats.downlink
                    )
                },
            )?;
        }
        previous = Some(stats);
    }
    Ok(())
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn j04_only_a_lossy_reader_gets_8_ms_repeats() {
    let (_, trace) = run(r#"{ "name": "j04", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 60,
        "groups": [ { "label": "lossy", "first": 1, "last": 1,
            "network": { "down": { "delay_ms": 20, "loss": 0.06 } } } ] }"#);
    check_wire(&trace).unwrap();
    let good = trace.reader_stats_at(0, u64::MAX).unwrap();
    assert_eq!(good.packets_by_kind[2], 0, "good reader got repeats");
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
/// by §3.5 packing; only the first of them carries older records; and no
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
        let new_lens: Vec<usize> = fresh
            .iter()
            .flat_map(|record| record.pack().unwrap().1)
            .filter(|entry| entry.seq >= pushed_high)
            .map(|entry| entry.len)
            .collect();
        ensure(fresh.len() == inspect::packs_needed(&new_lens), || {
            format!(
                "client {client} at {} ms: {} new packs for records needing {} (spec §7.4)",
                ms(at),
                fresh.len(),
                inspect::packs_needed(&new_lens)
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
#[ignore = "waits for the adaptive transport implementation"]
fn j05_piggyback_never_adds_a_packet() {
    // 200-byte commands at 300 APM from four players fill more than one
    // pack in many turns, so §7.4 must shed old records.
    let (_, trace) = run(r#"{ "name": "j05", "client": "adaptive", "players": 4,
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
/// at most once per client-reported `srtt` (§7.3 step 4); and the gaps
/// close without a PULL.
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
    let pulls = trace
        .of(client, Direction::Up)
        .filter(|record| record.kind() == "PULL")
        .count();
    ensure(pulls == 0, || {
        format!("client {client}: {pulls} PULLs (spec §13.2)")
    })
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn j06_retransmissions_cover_only_unacknowledged_uncovered_records() {
    // Three runs of three consecutive packs lost to a good reader (K = 1,
    // no repeats), as spec §13.2's "连续丢 3 个下行包".
    let rules: Vec<String> = [40, 41, 42, 90, 91, 92, 140, 141, 142]
        .iter()
        .map(|nth| {
            format!(r#"{{ "direction": "down", "client": 1, "kind": "PACK", "nth": {nth} }}"#)
        })
        .collect();
    let (_, trace) = run(&format!(
        r#"{{ "name": "j06", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 15000, "apm": 120,
        "drop_rules": [{}] }}"#,
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
fn check_bitmaps(trace: &Trace, clients: usize) -> Check {
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
                Ok(Detail::Ack {
                    feedback: Some(f), ..
                })
                | Ok(Detail::Status {
                    feedback: Some(f), ..
                }) => *f,
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
#[ignore = "waits for the adaptive transport implementation"]
fn j07_feedback_bitmaps_match_the_network() {
    let (_, trace) = run(r#"{ "name": "j07", "client": "adaptive", "players": 2,
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
#[ignore = "waits for the adaptive transport implementation"]
fn j08_reported_loss_tracks_the_network_loss() {
    let (_, trace) = run(r#"{ "name": "j08", "client": "adaptive", "players": 2,
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
/// - the first push after the relay hears it again starts at the
///   reader's `acked_next` (§5.5: `sent_next = acked_next`), and leaves the
///   relay within one pass of hearing it;
/// - the client is caught up to the relay's `next_seq` (from the first
///   STATUS after recovery) within one RTT plus, for each 64 records of
///   backlog (`PUSH_WINDOW`, §7.4), one acknowledgement round (RTT + the
///   100 ms good interval of §6.3 row 3 + a service);
/// - the outage loses no loss statistics (§5.5, §13.2): with no loss besides
///   it, `up_loss_permille_max` and the reader's `down_loss_permille` stay 0.
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
        .find(|r| r.origin == Origin::Push && r.sent_us >= heard_again && r.pack().is_some())
        .ok_or("no push after recovery")?;
    ensure(first.sent_us <= heard_again + 1000, || {
        format!(
            "first push {} ms after the relay heard the client",
            ms(first.sent_us - heard_again)
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
    let rtt = 2 * 20_000;
    let rounds = next_seq.saturating_sub(acked).div_ceil(64);
    let deadline = heard_again + rtt + rounds * (rtt + 116_000);
    let caught = up
        .iter()
        .filter(|r| r.sent_us >= heard_again)
        .find(|r| r.uplink().is_some_and(|u| u.recv_next >= next_seq))
        .ok_or("never caught up")?;
    ensure(caught.sent_us <= deadline, || {
        format!(
            "caught up {} ms after recovery; bound {} ms",
            ms(caught.sent_us - heard_again),
            ms(deadline - heard_again)
        )
    })?;
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
#[ignore = "waits for the adaptive transport implementation"]
fn j09_offline_sends_only_probes_and_recovers_from_the_watermark() {
    let (_, trace) = run(r#"{ "name": "j09", "client": "adaptive", "players": 2,
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

// --- Judgment 10 (spec §5.2, §5.3, §6.3 rows 1-2, §7.2) ---

/// Once the client judges DELAYED (§5.3) its uplink is Congested (§5.2),
/// the UPLINK link byte carries bit2, no copy is sent while congested
/// (§6.3 row 6 is lossy only), an UPLINK carrying a never-sent batch is at
/// least 33 ms after the previous UPLINK (row 1, 拥堵), any two UPLINKs are
/// at least 16 ms apart (row 2); the relay judges the reader Congested
/// (§5.2 from link bit2), sends it no 8 ms repeat and keeps K <= 2 (§7.2).
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
            ensure(uplink.link & 4 != 0, || {
                format!("DELAYED bit clear at {} ms (spec §3.2)", record.sent_ms())
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
    let mut congested_seen = false;
    let mut previous: Option<&crate::ReaderLinkStats> = None;
    for (at, c, stats) in &trace.reader_stats {
        if *c != client {
            continue;
        }
        if stats.downlink == LinkState::Congested {
            congested_seen = true;
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
#[ignore = "waits for the adaptive transport implementation"]
fn j10_a_rising_rtt_congests_without_copies_or_repeats() {
    // RTT 40 ms, then 300 ms from 6 s (spec §13.2).
    let (_, trace) = run(r#"{ "name": "j10", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 20000, "apm": 300,
        "groups": [ { "label": "slow", "first": 1, "last": 1,
            "network": { "link": { "delay_ms": 20, "delay_changes": [[6000, 150]] } } } ] }"#);
    check_wire(&trace).unwrap();
    check_congestion(&trace, 1).unwrap();
}

// --- Judgment 11 (spec §4.5, §6.0) ---

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn j11_a_lost_first_uplink_is_resent_after_one_rto() {
    let (_, trace) = run(r#"{ "name": "j11", "client": "adaptive", "players": 2,
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
    // another reason (here the watermark's 100 ms interval of row 3).
    if let Some(third) = ups.get(2) {
        assert!(third.sent_us >= second.sent_us + 100_000 - 16_000);
    }
}

// --- Judgment 12 (spec §4.7, §6.0) ---

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn j12_a_renewed_hop_sends_at_once_and_counts_from_zero() {
    let (_, trace) = run(r#"{ "name": "j12", "client": "adaptive", "players": 2,
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
#[ignore = "waits for the adaptive transport implementation"]
fn j13_a_restoring_reader_gets_no_new_records_pushed() {
    let (_, trace) = run(r#"{ "name": "j13", "client": "adaptive", "players": 2,
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
fn check_consistency(report: &Report, trace: &Trace, end_us: u64) -> Check {
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
#[ignore = "waits for the adaptive transport implementation"]
fn j14_every_client_runs_the_relays_stream() {
    let (report, trace) = run(&format!(
        r#"{{ "name": "j14", "client": "adaptive", "players": 4,
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
#[ignore = "waits for the adaptive transport implementation"]
fn smoke_good_idle() {
    let (report, trace) = run(r#"{ "name": "smoke-good-idle", "client": "adaptive",
        "players": 2, "warmup_ms": 2000, "duration_ms": 10000 }"#);
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
        assert_eq!(
            trace
                .reader_stats_at(client, u64::MAX)
                .unwrap()
                .packets_by_kind[2],
            0
        );
        assert_eq!(trace.client_stats_at(client, u64::MAX).unwrap().copies, 0);
    }
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn smoke_loss_keeps_latency_within_one_and_a_half_good_links() {
    let (good, good_trace) = run(r#"{ "name": "smoke-good-300", "client": "adaptive",
        "players": 2, "warmup_ms": 2000, "duration_ms": 13000, "apm": 300 }"#);
    check_wire(&good_trace).unwrap();
    check_batch_segments(&good_trace).unwrap();
    // §13.3's rule for a busy good link: new packs plus at most 10 more.
    let down = &good.groups[0].down;
    assert!(
        down.packets_per_s <= group_down(&good, "PACK_NEW") + 10.0,
        "{down:?}"
    );
    for client in 0..2 {
        check_good_uplink(&good_trace, client).unwrap();
    }
    let (lossy, lossy_trace) = run(r#"{ "name": "smoke-loss-300", "client": "adaptive",
        "players": 2, "warmup_ms": 2000, "duration_ms": 13000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "loss": 0.05 } } }"#);
    check_wire(&lossy_trace).unwrap();
    check_batch_segments(&lossy_trace).unwrap();
    // §13.2 "5% 随机丢包": both directions lossy, copies and repeats sent,
    // and command p99 at most 1.5 times the good link's.
    for client in 0..2 {
        let stats = lossy_trace.client_stats_at(client, u64::MAX).unwrap();
        assert!(stats.uplink_state_entries[LinkState::Lossy.index()] >= 1);
        assert!(stats.copies > 0);
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
#[ignore = "waits for the adaptive transport implementation"]
fn smoke_outage() {
    let (_, trace) = run(
        r#"{ "name": "smoke-outage", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "network": { "link": { "delay_ms": 20, "outages": [[4000, 9000]] } } }"#,
    );
    check_wire(&trace).unwrap();
    for client in 0..2 {
        check_outage(&trace, client, 4000, 9000).unwrap();
    }
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn smoke_congestion() {
    let (_, trace) = run(
        r#"{ "name": "smoke-congestion", "client": "adaptive", "players": 2,
        "warmup_ms": 0, "duration_ms": 14000, "apm": 300,
        "network": { "link": { "delay_ms": 20, "delay_changes": [[4000, 150]] } } }"#,
    );
    check_wire(&trace).unwrap();
    for client in 0..2 {
        check_congestion(&trace, client).unwrap();
    }
}

#[test]
#[ignore = "waits for the adaptive transport implementation"]
fn smoke_mixed_room() {
    let (report, trace) = run(&format!(
        r#"{{ "name": "smoke-mixed", "client": "adaptive", "players": 4,
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
