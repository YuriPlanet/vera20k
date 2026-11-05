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
fn the_inspector_rejects_a_length_that_disagrees_with_the_envelope() {
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
