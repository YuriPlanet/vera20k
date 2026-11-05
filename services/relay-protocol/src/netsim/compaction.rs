//! End-to-end judgments of the bandwidth compaction
//! (services/relay/bandwidth-compaction/spec.md, "the spec" below; the
//! transport spec is services/relay/adaptive-transport/spec.md). Every
//! expectation comes from the spec, cited next to it; the wire is read by
//! the independent inspector (`inspect::v3`) and the implementation only
//! through its public interface.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::inspect::v3;
use super::inspect::{Datagram, Detail};
use super::judgments::{Check, check_batch_segments, check_bitmaps, check_consistency, check_wire};
use super::reader::Record;
use super::report::Report;
use super::scenario::Direction;
use super::trace::{Trace, WireRecord};
use super::*;

fn run(json: &str) -> (Report, Trace) {
    Simulation::new(Scenario::from_json(json).expect("scenario")).run_traced()
}

fn ensure(condition: bool, message: impl FnOnce() -> String) -> Check {
    if condition { Ok(()) } else { Err(message()) }
}

fn parsed(record: &WireRecord) -> &Datagram {
    record.parsed.as_ref().expect("parsed")
}

/// Each (client, hop)'s session: the one its first relay READY (relay
/// sequence 1) names (§6.1 READY row, §6.2 "分配").
fn sessions(trace: &Trace) -> HashMap<(usize, usize), u32> {
    trace
        .wire
        .iter()
        .filter_map(|record| match record.parsed.as_ref().ok()?.detail {
            Detail::HopReady { session } => Some(((record.client, record.hop), session)),
            _ => None,
        })
        .collect()
}

/// §6.1, §6.3, §6.4: every datagram (all parsed as envelope v3) has a 4-byte tag that
/// verifies under its hop's key and direction's domain; its payload is
/// within 720 B; HELLO, CONFIRM and CHALLENGE carry session 0 and every
/// other datagram its hop's session; each hop's sequences count from 1,
/// the client's from its HELLO and the relay's from its READY (CHALLENGE
/// and LEAVE_ACK echo the client's). A LEAVE resent byte for byte keeps
/// its sequence (§6.2 "客户端重发的 LEAVE 是原样的").
fn check_envelopes(trace: &Trace) -> Check {
    let sessions = sessions(trace);
    let mut last: HashMap<(usize, usize, Direction), u64> = HashMap::new();
    let mut last_leave: HashMap<(usize, usize), &Vec<u8>> = HashMap::new();
    for record in &trace.wire {
        let datagram = parsed(record);
        let at = record.sent_ms();
        ensure(datagram.payload_len <= v3::MAX_PAYLOAD, || {
            format!(
                "{} at {at} ms: {} B payload (spec §6.1)",
                datagram.kind, datagram.payload_len
            )
        })?;
        let bytes = record.bytes.as_ref().expect("keep_wire_bytes");
        ensure(bytes.len() == datagram.payload_len + v3::OVERHEAD, || {
            format!(
                "{} at {at} ms: not 13 B of envelope (spec §6.1)",
                datagram.kind
            )
        })?;
        let key = &trace.hop_keys[record.client][record.hop];
        ensure(
            v3::tag_ok(key, record.direction == Direction::Up, bytes),
            || {
                format!(
                    "{} at {at} ms: tag does not verify (spec §6.3)",
                    datagram.kind
                )
            },
        )?;
        let session = datagram.session;
        let handshake = matches!(datagram.kind, "HELLO" | "CONFIRM" | "CHALLENGE");
        let expected = if handshake {
            0
        } else {
            *sessions
                .get(&(record.client, record.hop))
                .ok_or_else(|| format!("{} at {at} ms on a hop with no READY", datagram.kind))?
        };
        ensure(session == expected, || {
            format!(
                "client {} hop {} {} at {at} ms carries session {session:#x}, not {expected:#x} (spec §6.1, §6.2)",
                record.client, record.hop, datagram.kind
            )
        })?;
        if matches!(datagram.kind, "CHALLENGE" | "LEAVE_ACK") {
            continue;
        }
        if datagram.kind == "LEAVE" {
            let resent = last_leave.insert((record.client, record.hop), bytes) == Some(bytes);
            if resent {
                continue;
            }
        }
        let previous = last
            .entry((record.client, record.hop, record.direction))
            .or_insert(0);
        ensure(datagram.hop_seq == *previous + 1, || {
            format!(
                "client {} hop {} {:?} {} at {at} ms: sequence {} after {} (spec §6.1)",
                record.client,
                record.hop,
                record.direction,
                datagram.kind,
                datagram.hop_seq,
                previous
            )
        })?;
        *previous = datagram.hop_seq;
    }
    Ok(())
}

/// §5.1, §11 row 1: each PACK decodes alone, from its own bytes; and every
/// copy of a record, to any client, by any route (new, piggyback, repeat,
/// retransmission, PULL reply), is the same canonical bytes. Returns the
/// canonical bytes by sequence.
fn check_packs_alone(trace: &Trace) -> Result<BTreeMap<u64, Vec<u8>>, String> {
    let mut by_seq: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    let mut packs = 0;
    for record in trace.wire.iter().filter(|r| r.pack().is_some()) {
        let bytes = record.bytes.as_ref().expect("keep_wire_bytes");
        let payload = &bytes[v3::HEADER..bytes.len() - v3::HOP_TAG_LEN];
        let (_, _, records) = v3::decode_pack(payload)
            .map_err(|error| format!("PACK at {} ms: {error} (spec §5.3)", record.sent_ms()))?;
        for entry in records {
            match by_seq.get(&entry.seq) {
                Some(known) => ensure(*known == entry.canonical, || {
                    format!(
                        "seq {} at {} ms to client {} differs from an earlier copy (spec §5.1)",
                        entry.seq,
                        record.sent_ms(),
                        record.client
                    )
                })?,
                None => {
                    by_seq.insert(entry.seq, entry.canonical);
                }
            }
        }
        packs += 1;
    }
    ensure(packs > 0, || "no PACK".into())?;
    Ok(by_seq)
}

/// §5.4: each PACK is the one encoding of its records, so the inspector's
/// own encoder writes the same bytes; and none is over 681 B (§5.3 rule 5).
fn check_packs_canonical(trace: &Trace) -> Check {
    for record in trace.wire.iter().filter(|r| r.pack().is_some()) {
        let bytes = record.bytes.as_ref().expect("keep_wire_bytes");
        let payload = &bytes[v3::HEADER..bytes.len() - v3::HOP_TAG_LEN];
        let (_, records) = record.pack().unwrap();
        let canonical: Vec<Vec<u8>> = records.iter().map(|r| r.canonical.clone()).collect();
        let encoded = v3::encode_pack(&canonical)?;
        ensure(encoded == payload, || {
            format!(
                "PACK at {} ms to client {}: {:02x?} is not the §5.4 encoding {:02x?}",
                record.sent_ms(),
                record.client,
                payload,
                encoded
            )
        })?;
        ensure(payload.len() <= v3::PACK_CAP, || {
            format!(
                "PACK at {} ms: {} B (spec §5.3 rule 5)",
                record.sent_ms(),
                payload.len()
            )
        })?;
    }
    Ok(())
}

/// The stream comparison's view of a canonical record (transport spec §3.5).
fn record_of(canonical: &[u8]) -> Record {
    let u32_at = |at: usize| u32::from_le_bytes(canonical[at..at + 4].try_into().unwrap());
    let frame = u32_at(9);
    match canonical[0] {
        3 => Record::Batch {
            frame,
            source: canonical[13],
            batch_id: u64::from_le_bytes(canonical[14..22].try_into().unwrap()),
        },
        4 => Record::FrameEnd { frame },
        5 => Record::Departure {
            frame,
            source: canonical[13],
        },
        9 => Record::Pause {
            frame,
            paused: canonical[21] != 0,
        },
        10 => Record::Rate {
            frame,
            frames_per_second: u16::from_le_bytes([canonical[13], canonical[14]]),
        },
        11 => Record::Desync {
            frame,
            sample_frame: u32_at(13),
        },
        kind => panic!("canonical kind {kind}"),
    }
}

/// §12.2 "每个客户端读到的规范记录流，和中继日志里的规范记录逐字节相同":
/// what each client read (its production decode) is, record for record,
/// the canonical bytes the wire carried from seq 1, and those bytes are the
/// relay's canonical history; every BATCH's plaintext is the one its
/// source sealed (§3.1: `opaque` holds `P`).
fn check_streams_match_history(trace: &Trace, wire: &BTreeMap<u64, Vec<u8>>) -> Check {
    let history = trace.relay_history.as_ref().ok_or("no relay history")?;
    // `ordering_history` starts at the oldest record the relay keeps, 1
    // with the journal the simulation always runs.
    ensure(
        history
            .first()
            .is_some_and(|first| first[1..9] == 1u64.to_le_bytes()),
        || "relay history does not start at seq 1".into(),
    )?;
    for (seq, canonical) in wire {
        let index = usize::try_from(*seq - 1).unwrap();
        ensure(history.get(index) == Some(canonical), || {
            format!("seq {seq} on the wire differs from the relay's history (spec §12.2)")
        })?;
    }
    for (client, stream) in trace.streams.iter().enumerate() {
        for (index, read) in stream.iter().enumerate() {
            let seq = index as u64 + 1;
            let canonical = &history[index];
            ensure(record_of(canonical) == *read, || {
                format!(
                    "client {client} read {read:?} as seq {seq}, the relay holds {:?}",
                    record_of(canonical)
                )
            })?;
        }
    }
    let sealed: HashMap<(u8, u64), &Vec<u8>> = trace
        .plaintexts
        .iter()
        .enumerate()
        .flat_map(|(source, sealed)| sealed.iter().map(move |(id, p)| ((source as u8, *id), p)))
        .collect();
    for canonical in history.iter().filter(|c| c[0] == 3) {
        let source = canonical[13];
        let id = u64::from_le_bytes(canonical[14..22].try_into().unwrap());
        ensure(
            sealed
                .get(&(source, id))
                .is_some_and(|p| **p == canonical[22..]),
            || format!("BATCH {id} of source {source} is not its sealed plaintext (spec §3.1)"),
        )?;
    }
    Ok(())
}

/// Loss both ways (repeats, piggyback, retransmissions), a renewal, a
/// restoring window (PULL replies) and 300 APM.
const BUSY: &str = r#"{ "name": "compaction", "players": 3, "keep_wire_bytes": true,
    "warmup_ms": 0, "duration_ms": 15000, "apm": 300,
    "network": { "link": { "delay_ms": 20, "loss": 0.05 } },
    "renewals": [ { "client": 1, "at_ms": 6000 } ],
    "restoring": [ { "client": 2, "from_ms": 8000, "to_ms": 10000 } ] }"#;

#[test]
fn bc01_every_datagram_has_the_v3_layout() {
    let (report, trace) = run(BUSY);
    assert_eq!(report.malformed, 0);
    // The transport spec's own judgments over the new layout (§12.2 "原有
    // 判定全部通过").
    check_wire(&trace).unwrap();
    check_batch_segments(&trace).unwrap();
    check_envelopes(&trace).unwrap();
}

#[test]
fn bc02_every_pack_decodes_alone_and_is_the_one_encoding() {
    let (_, trace) = run(BUSY);
    check_packs_alone(&trace).unwrap();
    check_packs_canonical(&trace).unwrap();
    // The §11 row 1 routes were all exercised.
    assert!(trace.wire.iter().any(|r| r.kind() == "PULL"));
    assert!(trace.wire.iter().any(|r| r.hop == 1 && r.pack().is_some()));
}

#[test]
fn bc03_every_client_reads_the_relays_canonical_history() {
    let (report, trace) = run(BUSY);
    check_consistency(&report, &trace, 15_000_000).unwrap();
    let wire = check_packs_alone(&trace).unwrap();
    check_streams_match_history(&trace, &wire).unwrap();
}

/// Forged datagrams: none is accepted or answered, and no relay datagram
/// goes to a forger's address (§6.4 item 4: nothing changes before the tag
/// verifies, so neither an endpoint, §6.2 "换地址").
fn check_forgeries_inert(trace: &Trace) -> Check {
    ensure(!trace.forged.is_empty(), || "nothing forged".into())?;
    for forged in &trace.forged {
        ensure(forged.result.is_err() && forged.replies == 0, || {
            format!(
                "forged datagram at {} ms accepted (spec §6.3)",
                forged.at_us / 1000
            )
        })?;
    }
    let forgers: HashSet<SocketAddrV4> = trace.forged.iter().map(|f| f.endpoint).collect();
    ensure(
        !trace.misrouted.iter().any(|(_, to)| forgers.contains(to)),
        || "a relay datagram went to a forger (spec §6.4 item 4)".into(),
    )
}

/// Client `client`'s datagrams the relay refused in `[from_us, to_us)`, by
/// arrival.
fn refused(trace: &Trace, client: usize, from_us: u64, to_us: u64) -> Vec<u64> {
    trace
        .of(client, Direction::Up)
        .filter_map(|r| Some((r.arrived_us()?, r.relay_result.as_ref()?)))
        .filter(|(at, result)| (from_us..to_us).contains(at) && result.is_err())
        .map(|(at, _)| at)
        .collect()
}

/// §6.4 item 2: "合法包永远照常校验、照常接受" — no limit ever drops a
/// datagram with a valid tag, so not one of any client is refused; and the
/// forged ones change nothing (item 5): exact feedback windows, one stream.
fn check_forged_flood(report: &Report, trace: &Trace, clients: usize, end_us: u64) {
    check_forgeries_inert(trace).unwrap();
    for client in 0..clients {
        assert_eq!(
            refused(trace, client, 0, u64::MAX),
            Vec::<u64>::new(),
            "client {client}"
        );
    }
    check_bitmaps(trace, clients).unwrap();
    check_consistency(report, trace, end_us).unwrap();
}

#[test]
fn bc04_forgeries_from_another_address_change_nothing() {
    // 300 forged a second from one IP on A's session.
    let (report, trace) = run(r#"{ "name": "bc04", "players": 2, "keep_wire_bytes": true,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "forgeries": [ { "client": 0, "from_ms": 3000, "to_ms": 8000, "per_second": 300 } ] }"#);
    check_forged_flood(&report, &trace, 2, 12_000_000);
    // §6.4 item 2: each one a failure on a known session; 1500 stay under
    // the 4096-a-window alert. Neither client saw a bad tag (item 6).
    let counts = trace.mac_counts.unwrap();
    assert_eq!(
        (
            counts.failures,
            counts.unknown_sessions,
            counts.session_alerts
        ),
        (trace.forged.len() as u64, 0, 0)
    );
    assert_eq!(trace.client_mac_failures, [0, 0]);
}

#[test]
fn bc06_forgeries_from_many_ips_drop_none_of_the_sessions_datagrams() {
    // §6.4 item 2: eight IPs, 2000 forged a second on A's one session,
    // 6000 in 3 s, past the 4096-a-minute alert; each fails alone and A's
    // own datagrams all pass. (The failure and alert counts are asserted
    // through `MacFailureCounts` once that interface is merged.)
    let (report, trace) = run(r#"{ "name": "bc06", "players": 2, "keep_wire_bytes": true,
        "warmup_ms": 0, "duration_ms": 10000, "apm": 300,
        "forgeries": [ { "client": 0, "from_ms": 3000, "to_ms": 6000, "per_second": 2000,
            "addresses": 8 } ] }"#);
    assert!(trace.forged.len() > 4096);
    check_forged_flood(&report, &trace, 2, 10_000_000);
    // §6.4 item 2: 3-6 s lies in one fixed 60 s window (unix seconds; the
    // run starts at 1_000_000, a boundary at 20 s): one alert.
    let counts = trace.mac_counts.unwrap();
    assert_eq!(
        (
            counts.failures,
            counts.unknown_sessions,
            counts.session_alerts
        ),
        (trace.forged.len() as u64, 0, 1)
    );
}

#[test]
fn bc07_sessions_are_allocated_per_hop_and_released_after_leave() {
    // A's first LEAVE_ACK is lost, so A sends its LEAVE again.
    let (_, trace) = run(r#"{ "name": "bc07", "players": 2, "keep_wire_bytes": true,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "renewals": [ { "client": 1, "at_ms": 5000 } ],
        "leaves": [ { "client": 0, "at_ms": 9000 } ],
        "drop_rules": [ { "direction": "down", "client": 0, "kind": "LEAVE_ACK", "nth": 1 } ] }"#);
    check_envelopes(&trace).unwrap();
    let sessions = sessions(&trace);
    // §6.2 "分配": one nonzero session per hop, unique on the relay, the
    // renewal's included (§6.2 "续期": two hops, two sessions).
    assert_eq!(sessions.len(), 3, "{sessions:?}");
    let distinct: HashSet<u32> = sessions.values().copied().collect();
    assert_eq!(distinct.len(), 3);
    assert!(!distinct.contains(&0));
    // §6.2 "续期": the new hop counts its own sequences from 1 both ways
    // (check_envelopes); both hops were in use.
    assert!(
        trace
            .of(1, Direction::Up)
            .any(|r| r.hop == 0 && r.uplink().is_some())
    );
    assert!(
        trace
            .of(1, Direction::Up)
            .any(|r| r.hop == 1 && r.uplink().is_some())
    );
    // §6.2 "释放": in the leaving state each LEAVE, the resent one too,
    // gets a LEAVE_ACK echoing its sequence, and the relay sends nothing
    // else on A's session.
    let leaves: Vec<&WireRecord> = trace
        .of(0, Direction::Up)
        .filter(|r| r.kind() == "LEAVE")
        .collect();
    assert!(leaves.len() >= 2, "the lost LEAVE_ACK brought no resend");
    let acks: Vec<&WireRecord> = trace
        .of(0, Direction::Down)
        .filter(|r| r.kind() == "LEAVE_ACK")
        .collect();
    assert!(acks[0].lost());
    let answered = acks
        .iter()
        .find(|ack| !ack.lost() && ack.sent_us >= leaves[1].arrived_us().unwrap())
        .expect("no LEAVE_ACK for the resent LEAVE");
    assert_eq!(answered.hop_seq(), leaves[1].hop_seq());
    let released = sessions[&(0, 0)];
    let first_leave = leaves[0].arrived_us().unwrap();
    assert!(
        !trace.wire.iter().any(|r| r.direction == Direction::Down
            && r.sent_us > first_leave
            && r.kind() != "LEAVE_ACK"
            && parsed(r).session == released),
        "a relay datagram other than LEAVE_ACK on a leaving session"
    );
}

#[test]
fn bc08_a_rebound_client_is_followed_but_a_forger_or_replay_is_not() {
    // §6.2 "换地址": the first datagram from the new address with a valid
    // tag and a fresh sequence moves the hop's endpoint; §6.4 item 5: a
    // forged one from another address does not, nor (§6.2, revised) an old
    // valid datagram replayed from another address.
    let (report, trace) = run(r#"{ "name": "bc08", "players": 2, "keep_wire_bytes": true,
        "warmup_ms": 0, "duration_ms": 12000, "apm": 120,
        "rebinds": [ { "client": 0, "at_ms": 5000 } ],
        "forgeries": [ { "client": 0, "from_ms": 7000, "to_ms": 8000, "per_second": 50 } ],
        "replays": [ { "client": 0, "at_ms": 9000 }, { "client": 0, "at_ms": 9500 } ] }"#);
    check_forgeries_inert(&trace).unwrap();
    assert_eq!(trace.replayed.len(), 2);
    for replay in &trace.replayed {
        assert_eq!(
            replay.replies,
            0,
            "a replay at {} ms was answered",
            replay.at_us / 1000
        );
    }
    let replayers: HashSet<SocketAddrV4> = trace.replayed.iter().map(|r| r.endpoint).collect();
    assert!(
        !trace.misrouted.iter().any(|(_, to)| replayers.contains(to)),
        "a replay moved the endpoint"
    );
    let old = trace.endpoints[0][0];
    let moved = trace
        .of(0, Direction::Up)
        .filter(|r| r.sent_us >= 5_000_000)
        .find_map(|r| r.arrived_us())
        .expect("a datagram from the new address");
    let late: Vec<u64> = trace
        .misrouted
        .iter()
        .filter(|(at, to)| *to == old && *at > moved)
        .map(|(at, _)| at / 1000)
        .collect();
    assert!(
        late.is_empty(),
        "relay still sent to the old address at {late:?} ms"
    );
    check_consistency(&report, &trace, 12_000_000).unwrap();
}

/// Deterministic fuzz inputs (§12.2 "模糊测试"): random lengths and bytes,
/// and the same with every length or count field at its widest, which an
/// allocation sized from the field would turn into an abort.
fn fuzz_payloads(random: &mut network::Rng, first: u8, cases: usize) -> Vec<Vec<u8>> {
    let mut payloads = Vec::new();
    for case in 0..cases {
        let mut payload = vec![first];
        let len = (random.next_u64() % 760) as usize;
        payload.extend((0..len).map(|_| random.next_u64() as u8));
        if case % 4 == 0 {
            // Widest varints (§8: ten bytes, the last 1) and counts.
            let at = 1 + (random.next_u64() % 8) as usize;
            payload.truncate(at);
            payload.extend_from_slice(&[0xFF; 9]);
            payload.push(0x01);
            payload.push(0xFF);
            payload.extend_from_slice(&[0xFF; 9]);
            payload.push(0x01);
        }
        payloads.push(payload);
    }
    payloads
}

#[test]
fn bc09_decoders_survive_random_input() {
    // §12.2: random bytes to the envelope v3 decoders of both sides, and
    // random UPLINK (§7.1) and PACK v2 (§5.2) bodies under a valid tag, so
    // they pass the envelope and reach those decoders. Bounded and
    // reproducible: one seed, 2000 inputs each.
    let mut sim = Simulation::new(
        Scenario::from_json(
            r#"{ "name": "bc09", "players": 2, "warmup_ms": 0, "duration_ms": 3000 }"#,
        )
        .unwrap(),
    );
    sim.step_until(1_500_000);
    let session = sim.clients[0].sessions[0].expect("a session by 1.5 s");
    let key = sim.clients[0].hop_keys[0];
    let endpoint = sim.clients[0].endpoint;
    let attacker = SocketAddrV4::new(Ipv4Addr::new(10, 8, 0, 1), 1);
    let mut random = network::Rng::new(9, 9);
    let now_ms = 1_500;
    for (index, raw) in fuzz_payloads(&mut random, 0x64, 2000)
        .into_iter()
        .enumerate()
    {
        let _ = inspect::client_datagram(&raw);
        let _ = inspect::server_datagram(&raw);
        let _ = sim.relay.process_at(attacker, &raw, unix(now_ms), now_ms);
        sim.clients[0].driver.deliver(&raw, now_ms, now_ms * 1000);
        // Sequences far above the hop's, so the replay window lets them in.
        let seq = 1_000_000 + index as u32;
        let uplink = fuzz_payloads(&mut random, 8, 1).remove(0);
        let _ = v3::client_datagram(&v3::seal(&key, true, v3::CONTROL, session, seq, &uplink));
        let _ = sim.relay.process_at(
            endpoint,
            &v3::seal(&key, true, v3::CONTROL, session, seq, &uplink),
            unix(now_ms),
            now_ms,
        );
        let pack = fuzz_payloads(&mut random, v3::PACK, 1).remove(0);
        let _ = v3::decode_pack(&pack);
        sim.clients[0].driver.deliver(
            &v3::seal(&key, false, v3::CONTROL, session, seq, &pack),
            now_ms,
            now_ms * 1000,
        );
        sim.clients[0].driver.take_output();
    }
}
