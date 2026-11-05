//! Spec appendix A replayed packet by packet (e2e-plan §2.3): two clients
//! at 30 Hz, 20 ms each way without jitter, A starting at t = 0 and B 30 ms
//! later, commands at 480 (A) and 950 (B), A leaving at 1480 and B at 2000;
//! lost are B's second UPLINK and the relay's turn 3 pack to B. Clients
//! serve at once on every datagram, content event and UPLINK deadline,
//! which the appendix's timeline assumes. Both directions of both clients
//! start measured good (appendix A.1), through the test-support presets.
//!
//! Times are the relay clock in ms. The relay seals on whole milliseconds,
//! so the appendix's x.3 / x.7 times arrive up to 1 ms later; every time
//! below allows ±1 ms.

use super::inspect::{Detail, FLAG_PACE, FLAG_PUSH, Feedback, PackRecord};
use super::scenario::Direction;
use super::trace::{Trace, WireRecord};
use super::*;

const A: usize = 0;
const B: usize = 1;

const SCENARIO: &str = r#"{ "name": "appendix-a", "service": "event",
    "players": 2, "tick_hz": 30, "warmup_ms": 0, "duration_ms": 2100,
    "start_offsets_ms": [0, 30], "preset_measured_good": true,
    "network": { "link": { "delay_ms": 20 } },
    "commands": [ { "client": 0, "at_ms": 480 }, { "client": 1, "at_ms": 950 } ],
    "leaves": [ { "client": 0, "at_ms": 1480 }, { "client": 1, "at_ms": 2000 } ],
    "drop_rules": [
        { "direction": "up", "client": 1, "kind": "UPLINK", "nth": 2 },
        { "direction": "down", "client": 1, "kind": "PACK", "nth": 3 } ] }"#;

/// The one datagram of `kind` that `client`'s link carried in `direction`
/// sent within 1 ms of `sent_ms`.
fn packet<'t>(
    trace: &'t Trace,
    direction: Direction,
    client: usize,
    kind: &str,
    sent_ms: f64,
) -> &'t WireRecord {
    let found: Vec<&WireRecord> = trace
        .of(client, direction)
        .filter(|r| r.kind() == kind && (r.sent_ms() - sent_ms).abs() <= 1.0)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{direction:?} {kind} of client {client} near {sent_ms} ms: {:?}",
        found.iter().map(|r| r.sent_ms()).collect::<Vec<_>>()
    );
    found[0]
}

fn arrives(record: &WireRecord, at_ms: f64) {
    let arrived = record.arrived_us().expect("delivered") as f64 / 1000.0;
    assert!(
        (arrived - at_ms).abs() <= 1.0,
        "arrived at {arrived}, appendix {at_ms}"
    );
}

/// The envelope's hop sequence (bandwidth-compaction spec §6.1).
fn hop(record: &WireRecord, hop_seq: u64) {
    assert_eq!(
        record.hop_seq(),
        hop_seq,
        "{} at {} ms",
        record.kind(),
        record.sent_ms()
    );
}

/// Envelope sequence and payload length (bandwidth-compaction spec §6.1).
fn envelope(record: &WireRecord, hop_seq: u64, payload_len: usize) {
    let datagram = record.parsed.as_ref().unwrap();
    assert_eq!(
        (datagram.hop_seq, datagram.payload_len),
        (hop_seq, payload_len),
        "{} at {} ms",
        datagram.kind,
        record.sent_ms()
    );
}

fn status(record: &WireRecord) -> (u32, u64, u64, u32, u16, u32, bool, Feedback) {
    match &record.parsed.as_ref().unwrap().detail {
        Detail::Status {
            head_frame,
            oldest_seq,
            next_seq,
            active_sources,
            tick_hz,
            start_delay_ms,
            started,
            feedback,
        } => (
            *head_frame,
            *oldest_seq,
            *next_seq,
            *active_sources,
            *tick_hz,
            *start_delay_ms,
            *started,
            *feedback,
        ),
        other => panic!("not a STATUS: {other:?}"),
    }
}

fn pack(record: &WireRecord) -> (u64, Vec<PackRecord>) {
    let (base, records) = record.pack().expect("a PACK");
    (base, records.to_vec())
}

/// `(kind, frame)` of each record, FRAME_END = 4, BATCH = 3, DEPARTURE = 5.
fn shape(records: &[PackRecord]) -> Vec<(u64, u8, u32)> {
    records.iter().map(|r| (r.seq, r.kind, r.frame)).collect()
}

#[test]
fn appendix_a_timeline_and_fields() {
    let (_, trace) = Simulation::new(Scenario::from_json(SCENARIO).unwrap()).run_traced();
    use Direction::{Down, Up};

    // A.2, packets 1-4, and B's four 30 ms later.
    for (client, t) in [(A, 0.0), (B, 30.0)] {
        let hello = packet(&trace, Up, client, "HELLO", t);
        hop(hello, 1);
        arrives(hello, t + 20.0);
        let challenge = packet(&trace, Down, client, "CHALLENGE", t + 20.0);
        hop(challenge, 1); // echoes HELLO's sequence
        arrives(challenge, t + 40.0);
        let confirm = packet(&trace, Up, client, "CONFIRM", t + 40.0);
        hop(confirm, 2);
        let ready = packet(&trace, Down, client, "READY", t + 60.0);
        hop(ready, 1); // peer.sent's first use
        arrives(ready, t + 80.0);
    }

    // A.3, packets 5-7: A's configuration and its two replies.
    let request = packet(&trace, Up, A, "READY_REQUEST", 80.0);
    hop(request, 3);
    arrives(request, 100.0);
    let ready = packet(&trace, Down, A, "O_READY", 100.0);
    hop(ready, 2);
    assert!(matches!(
        ready.parsed.as_ref().unwrap().detail,
        Detail::OrderReady {
            ready_mask: 0b01,
            sources_mask: 0b11
        }
    ));
    let reply = packet(&trace, Down, A, "STATUS", 100.0);
    hop(reply, 3);
    arrives(reply, 120.0);
    let (head, oldest, next, active, hz, delay, started, feedback) = status(reply);
    assert_eq!(
        (head, oldest, next, active, hz, delay, started),
        (0, 1, 1, 0b11, 30, 0, false)
    );
    assert_eq!(
        (feedback.high, feedback.bits, feedback.echo_delay_ms),
        (3, 0b11, 0)
    );

    // Packet 8: B's configuration starts the match at 130 + 200.
    let request = packet(&trace, Up, B, "READY_REQUEST", 110.0);
    hop(request, 3);
    let ready = packet(&trace, Down, B, "O_READY", 130.0);
    assert!(matches!(
        ready.parsed.as_ref().unwrap().detail,
        Detail::OrderReady {
            ready_mask: 0b11,
            ..
        }
    ));
    let (.., delay, started, _) = status(packet(&trace, Down, B, "STATUS", 130.0));
    assert_eq!((delay, started), (200, false));

    // Packets 9-12: the 125 ms resends.
    hop(packet(&trace, Up, A, "READY_REQUEST", 205.0), 4);
    hop(packet(&trace, Down, A, "O_READY", 225.0), 4);
    let reply = packet(&trace, Down, A, "STATUS", 225.0);
    hop(reply, 5);
    assert_eq!(status(reply).5, 105);
    hop(packet(&trace, Up, B, "READY_REQUEST", 235.0), 4);
    assert_eq!(status(packet(&trace, Down, B, "STATUS", 255.0)).5, 75);
    // A's second RTT sample (§4.4): srtt 40, rttvar 15.
    let stats = trace.client_stats_at(A, 245_500).unwrap();
    assert_eq!((stats.srtt_ms, stats.rttvar_ms), (Some(40), Some(15)));

    // Packet 13: the reply that says the match started.
    hop(packet(&trace, Up, A, "READY_REQUEST", 330.0), 5);
    let reply = packet(&trace, Down, A, "STATUS", 350.0);
    hop(reply, 7);
    let (head, _, _, _, _, delay, started, _) = status(reply);
    assert_eq!((head, delay, started), (0, 0, true));
    arrives(reply, 370.0);
    // §6.0: no PULL before the start.
    assert!(!trace.wire.iter().any(|r| r.kind() == "PULL"));

    // Packet 14: A's first UPLINK, in the service that read the start.
    let first = packet(&trace, Up, A, "UPLINK", 370.0);
    hop(first, 6);
    arrives(first, 390.0);
    let uplink = first.uplink().unwrap();
    assert_eq!(uplink.flags, FLAG_PUSH);
    assert_eq!(
        (uplink.recv_next, uplink.down_high, uplink.down_bits),
        (1, 7, 0x7F)
    );
    assert_eq!((uplink.srtt_ms, uplink.base_ms, uplink.link), (40, 40, 0));
    assert_eq!(uplink.rttvar_ms, 11);
    assert!(uplink.checksums.is_empty() && uplink.batches.is_empty() && uplink.pace.is_none());
    // B's first, at 400.
    let first = packet(&trace, Up, B, "UPLINK", 400.0);
    hop(first, 6);
    assert_eq!(
        (
            first.uplink().unwrap().recv_next,
            first.uplink().unwrap().down_high
        ),
        (1, 7)
    );

    // A.4, packet 15: turn 1 to A and, once B subscribed at 420, to B.
    let turn = packet(&trace, Down, A, "PACK", 396.7);
    hop(turn, 8);
    arrives(turn, 416.7);
    let (base, records) = pack(turn);
    assert_eq!(base, 1);
    assert_eq!(shape(&records), [(1, 4, 1), (2, 4, 2)]);
    let turn = packet(&trace, Down, B, "PACK", 420.0);
    hop(turn, 8);
    arrives(turn, 440.0);
    assert_eq!(pack(turn).0, 1);

    // Packet 16: turn 2 with turn 1 piggybacked, to both.
    for client in [A, B] {
        let turn = packet(&trace, Down, client, "PACK", 463.3);
        hop(turn, 9);
        let (base, records) = pack(turn);
        assert_eq!(base, 1);
        assert_eq!(
            shape(&records),
            [(1, 4, 1), (2, 4, 2), (3, 4, 3), (4, 4, 4)]
        );
    }

    // Packet 17: A's acknowledgement, 100 ms after its last UPLINK.
    let ack = packet(&trace, Up, A, "UPLINK", 470.0);
    hop(ack, 7);
    let uplink = ack.uplink().unwrap();
    assert_eq!(
        (uplink.recv_next, uplink.down_high, uplink.down_bits),
        (3, 8, 0xFF)
    );
    assert!(uplink.batches.is_empty());

    // Packet 18: A's command, sent in the service that sealed it.
    let command = packet(&trace, Up, A, "UPLINK", 480.0);
    hop(command, 8);
    arrives(command, 500.0);
    let uplink = command.uplink().unwrap();
    assert_eq!(uplink.flags, FLAG_PUSH);
    assert_eq!(
        (uplink.recv_next, uplink.down_high, uplink.down_bits),
        (3, 8, 0xFF)
    );
    assert_eq!((uplink.srtt_ms, uplink.base_ms, uplink.link), (40, 40, 0));
    assert_eq!(uplink.rttvar_ms, 11);
    assert_eq!((uplink.first_batch_id, uplink.batches.len()), (1, 1));

    // Packet 19: its ACK.
    let reply = packet(&trace, Down, A, "ACK", 500.0);
    hop(reply, 10);
    arrives(reply, 520.0);
    match reply.parsed.as_ref().unwrap().detail {
        Detail::Ack {
            batch_id,
            frame,
            feedback,
        } => {
            assert_eq!((batch_id, frame), (1, 5));
            assert_eq!(
                (feedback.high, feedback.bits, feedback.echo_delay_ms),
                (8, 0x7F, 0)
            );
        }
        ref other => panic!("{other:?}"),
    }

    // Packet 20: turn 3 to A, from A's acked_next 3, with the batch in frame 5.
    let turn = packet(&trace, Down, A, "PACK", 530.0);
    hop(turn, 11);
    let (base, records) = pack(turn);
    assert_eq!(base, 3);
    assert_eq!(
        shape(&records),
        [(3, 4, 3), (4, 4, 4), (5, 3, 5), (6, 4, 5), (7, 4, 6)]
    );
    assert_eq!((records[2].source, records[2].batch_id), (Some(0), Some(1)));

    // Packet 21: B's acknowledgement, lost.
    let lost = packet(&trace, Up, B, "UPLINK", 500.0);
    hop(lost, 7);
    assert!(lost.lost());
    assert_eq!(lost.uplink().unwrap().recv_next, 5);

    // Packet 22: turn 3 to B, from B's acked_next 1, lost.
    let turn = packet(&trace, Down, B, "PACK", 530.0);
    assert!(turn.lost());
    assert_eq!(turn.hop_seq(), 10);
    let (base, records) = pack(turn);
    assert_eq!((base, records.len()), (1, 7));

    // Packet 23: turn 4 to B repairs turn 3 by piggyback.
    let turn = packet(&trace, Down, B, "PACK", 596.7);
    hop(turn, 11);
    arrives(turn, 616.7);
    let (base, records) = pack(turn);
    assert_eq!((base, records.len()), (1, 9));
    // No gap was ever reported and nothing pulled (§7.4, §14).
    assert!(!trace.of(B, Up).any(|r| r.kind() == "PULL"));

    // Packet 24: B's acknowledgement shows relay seq 10 missing.
    let ack = packet(&trace, Up, B, "UPLINK", 616.7);
    hop(ack, 8);
    arrives(ack, 636.7);
    let uplink = ack.uplink().unwrap();
    assert_eq!(
        (uplink.recv_next, uplink.down_high, uplink.down_bits),
        (10, 11, 0x7FD)
    );
    assert_eq!((uplink.srtt_ms, uplink.base_ms), (40, 40));

    // Packet 25: B's status heartbeat, 250 ms after its subscription, after
    // turn 5 (B's seq 12).
    hop(packet(&trace, Down, B, "PACK", 663.3), 12);
    let heartbeat = packet(&trace, Down, B, "STATUS", 670.0);
    hop(heartbeat, 13);
    arrives(heartbeat, 690.0);
    let (head, _, next, active, _, _, started, feedback) = status(heartbeat);
    assert_eq!((head, next, active, started), (10, 12, 0b11, true));
    assert_eq!(
        (feedback.high, feedback.bits, feedback.echo_delay_ms),
        (8, 0x7D, 33)
    );
    // Its RTT sample is 40 again (§4.4).
    let stats = trace.client_stats_at(B, 691_000).unwrap();
    assert_eq!(stats.srtt_ms, Some(40));

    // Packet 26: B reports seq 10 missing with down_high 13.
    let ack = packet(&trace, Up, B, "UPLINK", 716.7);
    let uplink = ack.uplink().unwrap();
    assert_eq!(uplink.down_high, 13);
    assert_eq!(uplink.down_bits >> 3 & 1, 0);
    // §7.3: nothing left to retransmit, B stays Good (§5.4).
    let reader = trace.reader_stats_at(B, 740_000).unwrap();
    assert_eq!(reader.packets_by_kind[3], 0);
    assert_eq!(reader.downlink, crate::LinkState::Good);

    // Packets 27-28: frame 15's checksum, alone, from both, in the next
    // UPLINK after frame 15 runs (turn 8 sealed at 863.3 arrives at 883.3),
    // at the latest the standalone one 100 ms later (§6.3).
    for client in [A, B] {
        let carrying: Vec<&WireRecord> = trace
            .of(client, Up)
            .filter(|r| r.uplink().is_some_and(|u| !u.checksums.is_empty()))
            .collect();
        let first = carrying.first().expect("a checksum UPLINK");
        let uplink = first.uplink().unwrap();
        assert_eq!(uplink.checksums.len(), 1);
        assert_eq!(uplink.checksums[0].0, 15);
        assert!(
            (882.0..=984.5).contains(&first.sent_ms()),
            "{}",
            first.sent_ms()
        );
    }

    // Packet 29: B's command into frame 19.
    let command = packet(&trace, Up, B, "UPLINK", 950.0);
    let uplink = command.uplink().unwrap();
    assert_eq!((uplink.first_batch_id, uplink.batches.len()), (1, 1));
    match packet(&trace, Down, B, "ACK", 970.0)
        .parsed
        .as_ref()
        .unwrap()
        .detail
    {
        Detail::Ack {
            batch_id, frame, ..
        } => assert_eq!((batch_id, frame), (1, 19)),
        ref other => panic!("{other:?}"),
    }
    // Turn 10 carries it as seq 20 to both.
    for client in [A, B] {
        let (_, records) = pack(packet(&trace, Down, client, "PACK", 996.7));
        let tail = &records[records.len() - 3..];
        assert_eq!(shape(tail), [(20, 3, 19), (21, 4, 19), (22, 4, 20)]);
        assert_eq!((tail[0].source, tail[0].batch_id), (Some(1), Some(1)));
    }

    // Packet 30: A's first pace segment rides one UPLINK only (§6.3).
    let paced: Vec<&WireRecord> = trace
        .of(A, Up)
        .filter(|r| r.uplink().is_some_and(|u| u.flags & FLAG_PACE != 0))
        .collect();
    assert_eq!(paced.len(), 1);
    let uplink = paced[0].uplink().unwrap();
    assert_eq!(uplink.flags, FLAG_PUSH | FLAG_PACE);
    assert!((1369.0..=1621.0).contains(&paced[0].sent_ms()));
    let (pace, _, tick) = uplink.pace.unwrap();
    assert_eq!((pace, tick), (0, 2100));

    // A.5, packets 31-32: A's LEAVE and its echo.
    let leave = packet(&trace, Up, A, "LEAVE", 1480.0);
    let leave_seq = leave.hop_seq();
    hop(leave, leave_seq);
    let echo = packet(&trace, Down, A, "LEAVE_ACK", 1500.0);
    hop(echo, leave_seq);
    // §11 row 8: no UPLINK after the Leave.
    assert!(
        !trace
            .of(A, Up)
            .any(|r| r.uplink().is_some() && r.sent_us > leave.sent_us)
    );

    // Packet 33: turn 18 to B ends with A's departure in frame 35.
    let (_, records) = pack(packet(&trace, Down, B, "PACK", 1530.0));
    let tail = &records[records.len() - 3..];
    assert_eq!(shape(tail), [(37, 5, 35), (38, 4, 35), (39, 4, 36)]);
    assert_eq!((tail[0].source, tail[0].lost), (Some(0), false));
    let heartbeat = trace
        .of(B, Down)
        .filter(|r| r.kind() == "STATUS" && r.sent_us > 1_530_000)
        .last()
        .expect("a later STATUS");
    assert_eq!(status(heartbeat).3, 0b10);

    // Packets 34-35: B's LEAVE and its echo.
    let leave = packet(&trace, Up, B, "LEAVE", 2000.0);
    hop(
        packet(&trace, Down, B, "LEAVE_ACK", 2020.0),
        leave.hop_seq(),
    );
}

/// A v3 datagram's payload (bandwidth-compaction spec §6.1).
fn payload_v3(record: &WireRecord) -> &[u8] {
    let bytes = record.bytes.as_ref().expect("keep_wire_bytes");
    &bytes[super::inspect::v3::HEADER..bytes.len() - super::inspect::v3::HOP_TAG_LEN]
}

/// Datagram length (no IP/UDP), hop sequence and payload length.
fn sized(record: &WireRecord, len: usize, hop_seq: u64, payload_len: usize) {
    assert_eq!(
        record.len,
        len,
        "{} at {} ms",
        record.kind(),
        record.sent_ms()
    );
    envelope(record, hop_seq, payload_len);
}

/// Appendix A on the compacted wire, every length and byte derived
/// from services/relay/bandwidth-compaction/spec.md alone: envelope v3 of
/// 13 B (§6.1), varint UPLINK / ACK / STATUS (§7, §8), plaintext batches
/// without version or FRAME_INFO (§3, §4: A's move command is 34 - 1 - 14 =
/// 19 B, the default `command_bytes` less 15) and PACK v2 (§5.2, §5.4). Same scenario, same timeline; only bytes
/// change (§0 "发送策略不变"). `appendix_a_timeline_and_fields` holds the
/// rest of the appendix: times, link statistics and record shapes.
#[test]
fn appendix_a_packet_by_packet() {
    let scenario = SCENARIO.replacen(
        r#""name": "appendix-a","#,
        r#""name": "appendix-a-v3", "keep_wire_bytes": true,"#,
        1,
    );
    let (_, trace) = Simulation::new(Scenario::from_json(&scenario).unwrap()).run_traced();
    use Direction::{Down, Up};
    let session = |record: &WireRecord| record.parsed.as_ref().unwrap().session;

    // A.2, packets 1-4: session 0 until READY names the hop's (§6.1, §6.2).
    for (client, t) in [(A, 0.0), (B, 30.0)] {
        let hello = packet(&trace, Up, client, "HELLO", t);
        sized(hello, 111, 1, 98);
        assert_eq!(session(hello), 0);
        let challenge = packet(&trace, Down, client, "CHALLENGE", t + 20.0);
        sized(challenge, 53, 1, 40);
        assert_eq!(session(challenge), 0);
        let confirm = packet(&trace, Up, client, "CONFIRM", t + 40.0);
        sized(confirm, 151, 2, 138);
        assert_eq!(session(confirm), 0);
        let ready = packet(&trace, Down, client, "READY", t + 60.0);
        sized(ready, 13, 1, 0);
        assert_ne!(session(ready), 0);
        // Every later datagram of the hop carries it, both ways.
        let hop_session = session(ready);
        assert!(
            trace
                .of(client, Up)
                .chain(trace.of(client, Down))
                .filter(|r| r.sent_us > ready.sent_us
                    && !matches!(r.kind(), "HELLO" | "CONFIRM" | "CHALLENGE"))
                .all(|r| session(r) == hop_session)
        );
    }

    // A.3, packets 5-7.
    sized(packet(&trace, Up, A, "READY_REQUEST", 80.0), 113, 3, 100);
    let ready = packet(&trace, Down, A, "O_READY", 100.0);
    sized(ready, 22, 2, 9);
    assert_eq!(payload_v3(ready), [0, 1, 0, 0, 0, 3, 0, 0, 0]);
    let reply = packet(&trace, Down, A, "STATUS", 100.0);
    sized(reply, 35, 3, 22);
    assert_eq!(
        payload_v3(reply),
        [
            1, 0, 1, 1, 3, 0, 0, 0, 30, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    // Packet 8: start_delay_ms 200 takes two varint bytes (0xC8 0x01).
    sized(packet(&trace, Up, B, "READY_REQUEST", 110.0), 113, 3, 100);
    let reply = packet(&trace, Down, B, "STATUS", 130.0);
    sized(reply, 36, 3, 23);
    assert_eq!(&payload_v3(reply)[10..12], [0xC8, 0x01]);
    // Packets 9-13: 105 and 75 fit one byte.
    sized(packet(&trace, Down, A, "STATUS", 225.0), 35, 5, 22);
    sized(packet(&trace, Down, B, "STATUS", 255.0), 35, 5, 22);
    let reply = packet(&trace, Down, A, "STATUS", 350.0);
    sized(reply, 35, 7, 22);
    assert_eq!(payload_v3(reply)[11], 1); // started

    // Packet 14: the first UPLINK, 18 B.
    let first = packet(&trace, Up, A, "UPLINK", 370.0);
    sized(first, 31, 6, 18);
    assert_eq!(
        payload_v3(first),
        [8, 1, 1, 7, 0x7F, 0, 0, 0, 0, 0, 0, 0, 40, 40, 11, 0, 0, 0]
    );
    sized(packet(&trace, Up, B, "UPLINK", 400.0), 31, 6, 18);

    // A.4, packets 15-16: two and four FRAME_ENDs, one tag byte each.
    let turn = packet(&trace, Down, A, "PACK", 396.7);
    sized(turn, 19, 8, 6);
    assert_eq!(payload_v3(turn), [12, 1, 1, 2, 0x04, 0x14]);
    sized(packet(&trace, Down, B, "PACK", 420.0), 19, 8, 6);
    for client in [A, B] {
        let turn = packet(&trace, Down, client, "PACK", 463.3);
        sized(turn, 21, 9, 8);
        assert_eq!(payload_v3(turn), [12, 1, 1, 4, 0x04, 0x14, 0x14, 0x14]);
    }

    // Packet 17.
    let ack = packet(&trace, Up, A, "UPLINK", 470.0);
    sized(ack, 31, 7, 18);
    assert_eq!(
        payload_v3(ack),
        [8, 1, 3, 8, 0xFF, 0, 0, 0, 0, 0, 0, 0, 40, 40, 11, 0, 0, 0]
    );

    // Packet 18: the 19 B plaintext rides as [count 1][id 1][len 19][P].
    let plaintext = &trace.plaintexts[A][0].1;
    assert_eq!((trace.plaintexts[A][0].0, plaintext.len()), (1, 19));
    let command = packet(&trace, Up, A, "UPLINK", 480.0);
    sized(command, 52, 8, 39);
    let body = payload_v3(command);
    assert_eq!(
        body[..20],
        [
            8, 1, 3, 8, 0xFF, 0, 0, 0, 0, 0, 0, 0, 40, 40, 11, 0, 0, 1, 1, 19
        ]
    );
    assert_eq!(&body[20..], plaintext.as_slice());

    // Packet 19: the ACK, 13 B.
    let reply = packet(&trace, Down, A, "ACK", 500.0);
    sized(reply, 26, 10, 13);
    assert_eq!(
        payload_v3(reply),
        [2, 1, 5, 8, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0]
    );

    // Packet 20: f3 f4 piggybacked, the BATCH one frame on (tag 0x13, an
    // absolute ID: first of its source in the pack), f5 same frame, f6.
    let turn = packet(&trace, Down, A, "PACK", 530.0);
    sized(turn, 44, 11, 31);
    let body = payload_v3(turn);
    assert_eq!(body[..10], [12, 3, 3, 5, 0x04, 0x14, 0x13, 0, 1, 19]);
    assert_eq!(&body[10..29], plaintext.as_slice());
    assert_eq!(body[29..], [0x04, 0x14]);

    // Packets 21-23 to and from B.
    let lost = packet(&trace, Up, B, "UPLINK", 500.0);
    sized(lost, 31, 7, 18);
    assert!(lost.lost());
    let turn = packet(&trace, Down, B, "PACK", 530.0);
    sized(turn, 46, 10, 33);
    assert!(turn.lost());
    let turn = packet(&trace, Down, B, "PACK", 596.7);
    sized(turn, 48, 11, 35);
    let body = payload_v3(turn);
    assert_eq!(
        body[..12],
        [12, 1, 1, 9, 0x04, 0x14, 0x14, 0x14, 0x13, 0, 1, 19]
    );
    assert_eq!(body[31..], [0x04, 0x14, 0x14, 0x14]);

    // Packet 24: down_bits 0x7FD stays a fixed u64.
    let ack = packet(&trace, Up, B, "UPLINK", 616.7);
    sized(ack, 31, 8, 18);
    assert_eq!(
        payload_v3(ack),
        [
            8, 1, 10, 11, 0xFD, 0x07, 0, 0, 0, 0, 0, 0, 40, 40, 11, 0, 0, 0
        ]
    );

    // Packet 25: turn 5 from B's acked_next 10, then the heartbeat.
    let turn = packet(&trace, Down, B, "PACK", 663.3);
    sized(turn, 19, 12, 6);
    assert_eq!(payload_v3(turn), [12, 10, 9, 2, 0x04, 0x14]);
    let heartbeat = packet(&trace, Down, B, "STATUS", 670.0);
    sized(heartbeat, 35, 13, 22);
    assert_eq!(
        payload_v3(heartbeat),
        [
            1, 10, 1, 12, 3, 0, 0, 0, 30, 0, 0, 1, 8, 0x7D, 0, 0, 0, 0, 0, 0, 0, 33
        ]
    );

    // Packet 26.
    let ack = packet(&trace, Up, B, "UPLINK", 716.7);
    sized(ack, 31, 9, 18);
    assert_eq!(payload_v3(ack)[3], 13);
    assert_eq!(payload_v3(ack)[4..6], [0xF7, 0x1F]);

    // Packets 27-28: one checksum adds [frame 15][checksum u32], 5 B.
    for client in [A, B] {
        let first = trace
            .of(client, Up)
            .find(|r| r.uplink().is_some_and(|u| !u.checksums.is_empty()))
            .expect("a checksum UPLINK");
        assert_eq!((first.len, first.payload_len()), (36, 23));
        assert_eq!(payload_v3(first)[16..18], [1, 15]);
    }

    // Packet 29 and its ACK into frame 19.
    // It may also carry frame 15's checksum (packets 27-28), 5 B more.
    let command = packet(&trace, Up, B, "UPLINK", 950.0);
    let extra = 5 * command.uplink().unwrap().checksums.len();
    sized(command, 52 + extra, command.hop_seq(), 39 + extra);
    let reply = packet(&trace, Down, B, "ACK", 970.0);
    sized(reply, 26, reply.hop_seq(), 13);
    assert_eq!(payload_v3(reply)[..3], [2, 1, 19]);
    // Turn 10: B's BATCH as one 23 B entry with B's plaintext.
    for client in [A, B] {
        let (_, records) = pack(packet(&trace, Down, client, "PACK", 996.7));
        let batch = &records[records.len() - 3];
        assert_eq!(batch.wire_len, 23);
        assert_eq!(&batch.canonical[22..], trace.plaintexts[B][0].1.as_slice());
    }

    // Packet 30: [pace 0][fps 30][tick_micros 2100 = 0xB4 0x10].
    let paced = trace
        .of(A, Up)
        .find(|r| r.uplink().is_some_and(|u| u.flags & FLAG_PACE != 0))
        .expect("a pace UPLINK");
    let extra = 5 * paced.uplink().unwrap().checksums.len();
    assert_eq!((paced.len, paced.payload_len()), (35 + extra, 22 + extra));
    assert_eq!(payload_v3(paced)[1], 3);
    assert_eq!(payload_v3(paced)[17 + extra..], [0, 30, 0xB4, 0x10, 0]);

    // A.5, packets 31-32.
    let leave = packet(&trace, Up, A, "LEAVE", 1480.0);
    sized(leave, 14, leave.hop_seq(), 1);
    sized(
        packet(&trace, Down, A, "LEAVE_ACK", 1500.0),
        13,
        leave.hop_seq(),
        0,
    );
    // Packet 33: DEPARTURE [tag 0x05 or 0x15][source 0][lost 0], f35, f36.
    let turn = packet(&trace, Down, B, "PACK", 1530.0);
    let body = payload_v3(turn);
    assert_eq!(body[body.len() - 4..], [0, 0, 0x04, 0x14]);
    let (_, records) = pack(turn);
    assert_eq!(records[records.len() - 3].wire_len, 3);
    // Packets 34-35.
    let leave = packet(&trace, Up, B, "LEAVE", 2000.0);
    sized(leave, 14, leave.hop_seq(), 1);
    sized(
        packet(&trace, Down, B, "LEAVE_ACK", 2020.0),
        13,
        leave.hop_seq(),
        0,
    );
}
