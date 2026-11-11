use super::*;
use network::Loss;
use scenario::LinkSpec;

fn short_match(apm: f64) -> Scenario {
    Scenario::from_json(&format!(
        r#"{{ "name": "test", "players": 2, "warmup_ms": 1000, "duration_ms": 4000, "apm": {apm} }}"#
    ))
    .unwrap()
}

/// Every datagram of a good two-player match parses by spec §3, both
/// players read one stream and execute every command, and one seed
/// reproduces the run.
#[test]
fn a_good_match_parses_reads_one_stream_and_replays_by_seed() {
    let report = Simulation::new(short_match(300.0)).run();
    assert_eq!(report.malformed, 0);
    assert!(
        report.relay_rejects.is_empty(),
        "{:?}",
        report.relay_rejects
    );
    assert!(report.stream_consistent);
    assert_eq!(report.stalled_clients, 0);
    let group = &report.groups[0];
    assert!(group.command_latency_ms.count > 0);
    assert_eq!(group.repair_pulls, 0.0);
    assert!(group.down.kinds.contains_key("PACK_NEW"));
    let again = Simulation::new(short_match(300.0)).run();
    assert_eq!(
        serde_json::to_string(&report).unwrap(),
        serde_json::to_string(&again).unwrap()
    );
}

#[test]
fn outages_and_a_full_queue_lose_datagrams_on_the_link() {
    let mut link = Link::new(
        LinkSpec {
            outages: vec![[10, 20]],
            ..LinkSpec::default()
        },
        Rng::new(1, 0),
    );
    assert_eq!(link.transmit(9_999, 100), Ok(vec![29_999]));
    assert_eq!(link.transmit(10_000, 100), Err(Loss::Outage));
    assert_eq!(link.transmit(20_000, 100), Ok(vec![40_000]));
    // 8 kbit/s: a 100-byte datagram takes 100 ms; a 150 ms queue holds one
    // more behind it.
    let mut link = Link::new(
        LinkSpec {
            delay_ms: 0.0,
            bandwidth_bps: 8_000,
            queue_ms: 150.0,
            ..LinkSpec::default()
        },
        Rng::new(1, 0),
    );
    assert_eq!(link.transmit(0, 100), Ok(vec![100_000]));
    assert_eq!(link.transmit(0, 100), Ok(vec![200_000]));
    assert_eq!(link.transmit(0, 100), Err(Loss::Queue));
}

#[test]
fn the_inspector_rejects_a_hello_of_the_wrong_length() {
    let key = SecretKey::new([3; 32]);
    let ticket = Ticket {
        room: [1; 16],
        source: 0,
        expires: 100,
        nonce: [2; 16],
        player: [4; 16],
        generation: 1,
    }
    .sign(&key)
    .unwrap();
    let mut session = ClientSession::new(
        &ticket,
        *derive_packet_key(&key, &ticket).expose_for_https(),
        [9; 32],
    )
    .unwrap();
    let mut bytes = Vec::new();
    session.probe(&mut bytes).unwrap();
    assert_eq!(inspect::client_datagram(&bytes).unwrap().kind, "HELLO");
    bytes.push(0);
    assert!(inspect::client_datagram(&bytes).is_err());
}

/// The inspector's own compacted-wire code against the spec's rules
/// (bandwidth-compaction spec §5.2-§5.4, §8): its encoder's PACK decodes
/// back to the same canonical records, and §5.3 rule 6's non-shortest
/// forms and §8's are refused.
#[test]
fn the_v3_inspector_round_trips_and_refuses_non_shortest_forms() {
    use inspect::v3;
    let mut at = 0;
    assert!(v3::read_varint(&[0x80, 0x00], &mut at).is_err());
    let mut ten = vec![0xFF; 9];
    ten.push(0x02);
    assert!(v3::read_varint(&ten, &mut { 0 }).is_err());
    ten[9] = 0x01;
    assert_eq!(v3::read_varint(&ten, &mut { 0 }), Ok(u64::MAX));
    let record = |kind: u8, seq: u64, frame: u32, rest: &[u8]| {
        let mut bytes = vec![kind];
        bytes.extend_from_slice(&seq.to_le_bytes());
        bytes.extend_from_slice(&frame.to_le_bytes());
        bytes.extend_from_slice(rest);
        bytes
    };
    let batch = |id: u64| {
        let mut rest = vec![2];
        rest.extend_from_slice(&id.to_le_bytes());
        rest.extend_from_slice(&[7; 19]);
        rest
    };
    let records = vec![
        record(3, 300, 5, &batch(9)),
        record(3, 301, 5, &batch(10)),
        record(4, 302, 5, &[]),
        record(4, 303, 9, &[]),
    ];
    let pack = v3::encode_pack(&records).unwrap();
    // [12][300 = AC 02][5][4], BATCH absolute, BATCH by bit6, FRAME_END
    // same frame, FRAME_END by delta 4.
    assert_eq!(pack[..8], [12, 0xAC, 0x02, 5, 4, 0x03, 2, 9]);
    assert_eq!(pack[28..32], [0x43, 2, 19, 7]);
    assert_eq!(pack[pack.len() - 3..], [0x04, 0x24, 4]);
    let (base, _, decoded) = v3::decode_pack(&pack).unwrap();
    assert_eq!(base, 300);
    let canonical: Vec<Vec<u8>> = decoded.into_iter().map(|r| r.canonical).collect();
    assert_eq!(canonical, records);
    // §5.3 rule 6: an absolute ID where bit6 applies.
    let mut unchained = pack[..28].to_vec();
    unchained.extend_from_slice(&[0x03, 2, 10, 19]);
    unchained.extend_from_slice(&pack[32..]);
    assert!(v3::decode_pack(&unchained).is_err());
    // §5.3 rule 6: delta 1 written as code 2.
    let mut slow = pack[..pack.len() - 2].to_vec();
    slow.extend_from_slice(&[0x24, 1]);
    assert!(v3::decode_pack(&slow).is_err());
    let key = [5; 32];
    let sealed = v3::seal(&key, true, v3::PING, 77, 3, &[]);
    assert!(v3::tag_ok(&key, true, &sealed) && !v3::tag_ok(&key, false, &sealed));
    assert_eq!(inspect::client_datagram(&sealed).unwrap().kind, "PING");
}
