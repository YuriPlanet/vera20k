use super::*;

pub(super) fn client(key: &SecretKey, source: u8) -> ClientSession {
    client_for(
        key,
        Ticket {
            room: [5; 16],
            source,
            expires: 1200,
            nonce: [source + 1; 16],
            player: [source + 1; 16],
            generation: 1,
        },
    )
}

/// A session on `ticket`, signed by the relay's `key`.
fn client_for(key: &SecretKey, ticket: Ticket) -> ClientSession {
    let signed = ticket.sign(key).unwrap();
    ClientSession::new(
        &signed,
        *derive_packet_key(key, &signed).expose_for_https(),
        [89; 32],
    )
    .unwrap()
}

pub(super) struct TestDirectory(pub(super) std::path::PathBuf);
impl TestDirectory {
    pub(super) fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        Self(std::env::temp_dir().join(format!("vera-relay-journal-{name}")))
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(super) fn unix(now_ms: u64) -> u64 {
    1000 + now_ms / 1000
}

pub(super) fn renew(server: &mut RelayServer, client: &mut ClientSession, now_ms: u64) {
    let mut ticket = client.ticket;
    ticket.expires = (unix(now_ms) + 200).max(client.ticket.expires + 1);
    ticket.nonce[15] = ticket.nonce[15].wrapping_add(1);
    let signed = ticket.sign(&server.key).unwrap();
    let mut candidate = client
        .renewal(
            &signed,
            *derive_packet_key(&server.key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
    connect(server, &mut candidate, now_ms);
    candidate.adopt_ordering_from(client).unwrap();
    *client = candidate;
}

pub(super) fn endpoint(client: &ClientSession) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31000 + u16::from(client.source()))
}

pub(super) fn connect(server: &mut RelayServer, client: &mut ClientSession, now_ms: u64) {
    let mut bytes = Vec::new();
    client.probe(&mut bytes).unwrap();
    let challenge = server
        .process_at(endpoint(client), &bytes, unix(now_ms), now_ms)
        .unwrap()
        .remove(0);
    let ServerEvent::Challenge(cookie) = client.receive(&challenge.bytes).unwrap() else {
        panic!("challenge");
    };
    client.confirm(cookie, &mut bytes).unwrap();
    let ready = server
        .process_at(endpoint(client), &bytes, unix(now_ms), now_ms)
        .unwrap()
        .remove(0);
    assert!(matches!(
        client.receive(&ready.bytes),
        Ok(ServerEvent::Ready)
    ));
}

pub(super) fn events(
    server: &mut RelayServer,
    client: &mut ClientSession,
    bytes: &[u8],
    now_ms: u64,
) -> Vec<OrderEvent> {
    server
        .process_at(endpoint(client), bytes, unix(now_ms), now_ms)
        .unwrap()
        .into_iter()
        .flat_map(|packet| received(client, &packet.bytes))
        .collect()
}

/// One server datagram's ordering events, each entry of a pack.
pub(super) fn received(client: &mut ClientSession, bytes: &[u8]) -> Vec<OrderEvent> {
    match client.receive(bytes).unwrap() {
        ServerEvent::Ordering(event) => vec![event],
        ServerEvent::OrderingPack(events) => events,
        _ => panic!("ordering"),
    }
}

pub(super) fn configure(
    server: &mut RelayServer,
    client: &mut ClientSession,
    cert: &[u8],
    now_ms: u64,
) -> Vec<OrderEvent> {
    let mut bytes = Vec::new();
    client
        .configure_ordering(cert, [7; 32], &mut bytes)
        .unwrap();
    events(server, client, &bytes, now_ms)
}

pub(super) fn pull(
    server: &mut RelayServer,
    client: &mut ClientSession,
    cursor: u64,
    now_ms: u64,
) -> Vec<OrderEvent> {
    let mut bytes = Vec::new();
    client.pull_ordering(cursor, &mut bytes).unwrap();
    events(server, client, &bytes, now_ms)
}

fn ordering(server: &RelayServer) -> &OrderingRoom {
    server.rooms[&[5; 16]].ordering.as_ref().unwrap()
}

pub(super) fn certificate(key: &SecretKey, mask: u32, expires: u64) -> Vec<u8> {
    OrderingCertificate {
        room: [5; 16],
        sources_mask: mask,
        tick_hz: 45,
        expires,
        pacing: Default::default(),
        ordered_alone: false,
    }
    .sign(key)
    .unwrap()
}

/// The batches of an encoded client UPLINK datagram.
fn uplink_batches(datagram: &[u8]) -> Vec<Vec<u8>> {
    crate::link::decode_uplink(&datagram[HEADER_LEN..datagram.len() - HOP_TAG_LEN])
        .unwrap()
        .batches
        .iter()
        .map(|batch| batch.to_vec())
        .collect()
}

/// An UPLINK payload with `flags` carrying `batches` from `first`.
fn raw_uplink(flags: u8, first: u64, batches: &[&[u8]]) -> Vec<u8> {
    let mut payload = Vec::new();
    crate::link::Uplink {
        flags,
        recv_next: 1,
        down: RecvWindow::default(),
        srtt_ms: 0,
        base_ms: 0,
        rttvar_ms: 0,
        link: 0,
        checksums: Vec::new(),
        pace: None,
        first_batch_id: first,
        batches: batches.to_vec(),
    }
    .encode(&mut payload);
    payload
}

/// An UPLINK with PUSH acknowledging `cursor`, without batches.
pub(super) fn subscribe(
    server: &mut RelayServer,
    client: &mut ClientSession,
    cursor: u64,
    now_ms: u64,
) -> Vec<OrderEvent> {
    let mut bytes = Vec::new();
    client.ordering.as_mut().unwrap().link.schedule.recv_next = cursor;
    client.test_uplink(None, now_ms, &mut bytes).unwrap();
    events(server, client, &bytes, now_ms)
}

fn pause_control(
    server: &mut RelayServer,
    client: &mut ClientSession,
    id: u64,
    epoch: u64,
    paused: bool,
    now_ms: u64,
) -> OrderEvent {
    let mut bytes = Vec::new();
    client
        .control_ordering_pause(id, epoch, paused, &mut bytes)
        .unwrap();
    events(server, client, &bytes, now_ms).remove(0)
}

fn pace_report(
    server: &mut RelayServer,
    client: &mut ClientSession,
    pace: OrderPace,
    frames_per_second: u16,
    now_ms: u64,
) {
    tick_report(server, client, pace, frames_per_second, None, now_ms);
}

fn tick_report(
    server: &mut RelayServer,
    client: &mut ClientSession,
    pace: OrderPace,
    frames_per_second: u16,
    tick_micros: Option<u32>,
    now_ms: u64,
) {
    let mut bytes = Vec::new();
    client.queue_ordering_pace(pace, frames_per_second, tick_micros);
    client.test_uplink(None, now_ms, &mut bytes).unwrap();
    assert!(events(server, client, &bytes, now_ms).is_empty());
}

fn paced_room(key: &SecretKey) -> (RelayServer, ClientSession, ClientSession) {
    paced_room_with(key, CertificatePacing::Legacy)
}

fn paced_room_with(
    key: &SecretKey,
    pacing: CertificatePacing,
) -> (RelayServer, ClientSession, ClientSession) {
    paced_room_for(
        key,
        OrderingCertificate {
            pacing,
            ..OrderingCertificate::parse(&certificate(key, 3, 1400)).unwrap()
        },
    )
}

/// Sources 0 and 1 connected and configured for ordering under `certificate`.
fn paced_room_for(
    key: &SecretKey,
    certificate: OrderingCertificate,
) -> (RelayServer, ClientSession, ClientSession) {
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(key, 0), client(key, 1));
    let cert = certificate.sign(key).unwrap();
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    (server, first, second)
}

/// Under VO2C a silent member does not hold the clock, a player that only
/// draws slowly catches up on its own, and one whose simulation cannot
/// hold the rate slows it to a rate it can; the host can switch it off.
#[test]
fn a_vo2c_room_slows_only_for_a_simulation_that_cannot_hold_the_rate() {
    use OrderPace::*;
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room_with(&key, CertificatePacing::On);
    // 10 ms ticks hold 80 Hz at 80%: running 30 is only a slow draw.
    tick_report(&mut server, &mut second, Behind, 30, Some(10_000), 1000);
    server.advance_clock(1000, unix(1000));
    assert_eq!(
        ordering(&server).pace.rate,
        45,
        "a slow draw alone holds the rate, and the silent first player holds nothing"
    );
    // 30 ms ticks hold 26 Hz: slow to the fastest paced rate at most 26.
    tick_report(&mut server, &mut second, Behind, 25, Some(30_000), 2000);
    server.advance_clock(2000, unix(2000));
    assert_eq!(ordering(&server).pace.rate, 20);
    // The slow player keeps up, but 30 ms ticks cannot hold 30 Hz: no
    // raise. A player that only draws slowly holds nothing back.
    for at in (3000..=40_000).step_by(2000) {
        tick_report(&mut server, &mut first, CatchingUp, 20, Some(5_000), at);
        tick_report(&mut server, &mut second, KeepingUp, 20, Some(30_000), at);
        server.advance_clock(at, unix(at));
    }
    assert_eq!(ordering(&server).pace.rate, 20);
    tick_report(&mut server, &mut first, CatchingUp, 20, Some(5_000), 41_000);
    tick_report(
        &mut server,
        &mut second,
        KeepingUp,
        20,
        Some(20_000),
        41_000,
    );
    server.advance_clock(41_000, unix(41_000));
    assert_eq!(ordering(&server).pace.rate, 30);

    let (mut server, _first, mut second) = paced_room_with(&key, CertificatePacing::Off);
    tick_report(&mut server, &mut second, Behind, 5, Some(200_000), 1000);
    server.advance_clock(1000, unix(1000));
    assert_eq!(
        ordering(&server).pace.rate,
        45,
        "the host switched pacing off"
    );
}

#[test]
fn pace_history_a_transient_dip_does_not_block_recovery() {
    use OrderPace::*;
    let (mut server, mut first, mut second) =
        paced_room_with(&SecretKey::new([73; 32]), CertificatePacing::On);
    tick_report(&mut server, &mut second, Behind, 25, Some(30_000), 1000);
    server.advance_clock(1000, unix(1000));
    assert_eq!(ordering(&server).pace.rate, 20);
    tick_report(&mut server, &mut first, Behind, 15, Some(8_000), 30_000);
    tick_report(&mut server, &mut first, KeepingUp, 20, Some(8_000), 31_100);
    server.advance_clock(31_100, unix(31_100));
    assert_eq!(
        ordering(&server).pace.rate,
        30,
        "one dip cannot hold the slower clock"
    );
}

#[test]
fn pace_history_needs_fresh_running_samples_and_releases_at_ten_percent() {
    use OrderPace::*;
    let (mut server, _, mut client) =
        paced_room_with(&SecretKey::new([73; 32]), CertificatePacing::On);
    for (at, fps) in [(1000, 35), (5000, 35), (6000, 0), (8000, 35)] {
        tick_report(&mut server, &mut client, Behind, fps, Some(8_000), at);
        server.advance_clock(at, unix(at));
        assert_eq!(ordering(&server).pace.rate, 45, "no sustained run at {at}");
    }
    tick_report(&mut server, &mut client, Behind, 35, Some(8_000), 10_000);
    server.advance_clock(10_000, unix(10_000));
    assert_eq!(ordering(&server).pace.rate, 30);
    // Keep a fresh report on the clock before its first raise is due.
    tick_report(&mut server, &mut client, KeepingUp, 30, Some(7_201), 39_000);
    tick_report(&mut server, &mut client, KeepingUp, 30, Some(7_201), 41_000);
    server.advance_clock(41_000, unix(41_000));
    assert_eq!(ordering(&server).pace.rate, 30);
    tick_report(&mut server, &mut client, KeepingUp, 30, Some(7_200), 42_000);
    server.advance_clock(42_000, unix(42_000));
    assert_eq!(ordering(&server).pace.rate, 45);
}

/// The extra preset keeps the same fifteen-turn cadence and may slow
/// only within the two rows below its certified ceiling.
#[test]
fn ninety_hz_certificate_starts_a_six_frame_clock_and_bounded_pacing() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let certificate = OrderingCertificate {
        room: [5; 16],
        sources_mask: 3,
        tick_hz: 90,
        expires: 1400,
        pacing: CertificatePacing::On,
        ordered_alone: false,
    };
    let signed = certificate.sign(&key).unwrap();
    assert_eq!(OrderingCertificate::parse(&signed).unwrap(), certificate);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &signed, 0);
    }
    let start = ordering(&server).starts_at.unwrap();
    server.advance_clock(start + 66, unix(start + 66));
    assert_eq!(ordering(&server).head, 0);
    server.advance_clock(start + 67, unix(start + 67));
    assert_eq!(ordering(&server).head, 6);
    assert_eq!(paced_rates(90), &[90, 60, 45]);
    assert_eq!(paced_rates(60), &[60, 45, 30]);
    tick_report(
        &mut server,
        &mut first,
        OrderPace::Behind,
        45,
        Some(15_000),
        1000,
    );
    server.advance_clock(1000, unix(1000));
    assert_eq!(ordering(&server).pace.rate, 45);
    assert!(
        OrderingCertificate {
            tick_hz: 91,
            ..certificate
        }
        .sign(&key)
        .is_err()
    );
}

/// The clock seals whole turns: three frames at a time at 45 Hz, about
/// fifteen turns a second at every rate.
#[test]
fn a_45_hz_room_seals_in_three_frame_turns() {
    assert_eq!(
        [60, 45, 30, 20, 15, 10].map(turn_frames),
        [4, 3, 2, 2, 1, 1]
    );
    let key = SecretKey::new([73; 32]);
    let (mut server, _first, _second) = paced_room(&key);
    let start = ordering(&server).starts_at.unwrap();
    let mut heads = Vec::new();
    // Frame 3 is due 66.7 ms after the start, frame 6 at 133.3 ms.
    for at in [66, 67, 100, 133, 134] {
        server.advance_clock(start + at, unix(start + at));
        heads.push(ordering(&server).head);
    }
    assert_eq!(heads, [0, 3, 3, 3, 6]);
}

/// A pull carries its page packed after the status, and the client
/// rebuilds every entry's exact canonical bytes and sequence.
/// Spec appendix A packets 5 to 7: the relay's window starts at CONFIRM,
/// its STATUS reply carries it, and the client takes its first RTT
/// sample from the socket times.
#[test]
fn a_status_reply_carries_the_hop_window_and_gives_the_first_rtt_sample() {
    let key = SecretKey::new([75; 32]);
    let mut server = RelayServer::new(key.clone(), 1);
    let mut first = client(&key, 0);
    connect(&mut server, &mut first, 0);
    let mut bytes = Vec::new();
    first
        .configure_ordering_at(&certificate(&key, 3, 1400), [7; 32], 80, &mut bytes)
        .unwrap();
    let replies = server
        .process_at(endpoint(&first), &bytes, unix(100), 100)
        .unwrap();
    first.receive_at(&replies[0].bytes, 120).unwrap();
    let ServerEvent::Ordering(OrderEvent::Status(status)) =
        first.receive_at(&replies[1].bytes, 120).unwrap()
    else {
        panic!("status");
    };
    assert_eq!(
        status.feedback.up,
        RecvWindow {
            high: 3,
            bits: 0b11
        }
    );
    assert_eq!(status.feedback.echo_delay_ms, 0);
    let stats = first.link_stats();
    assert_eq!((stats.srtt_ms, stats.rttvar_ms), (Some(40), Some(20)));
    // READY (1), ordering READY (2) and STATUS (3).
    assert_eq!(
        stats.down,
        RecvWindow {
            high: 3,
            bits: 0b111
        }
    );
    assert_eq!(stats.datagrams_received, 3);

    // Deliver a keepalive (5) before the older pull (4). Both count,
    // but STATUS measures the delay since 5 first arrived.
    let mut pull = Vec::new();
    first.pull_ordering_at(1, 130, &mut pull).unwrap();
    first.probe_at(135, &mut bytes).unwrap();
    server
        .process_at(endpoint(&first), &bytes, unix(150), 150)
        .unwrap();
    let replies = server
        .process_at(endpoint(&first), &pull, unix(175), 175)
        .unwrap();
    let ServerEvent::Ordering(OrderEvent::Status(status)) =
        first.receive_at(&replies[0].bytes, 190).unwrap()
    else {
        panic!("status");
    };
    assert_eq!(
        status.feedback.up,
        RecvWindow {
            high: 5,
            bits: 0b1111
        }
    );
    assert_eq!(status.feedback.echo_delay_ms, 25);
}

#[test]
fn rtt_samples_subtract_the_echo_delay_once_per_logged_sequence() {
    let key = SecretKey::new([76; 32]);
    let mut session = client(&key, 0);
    session.ready = true;
    session.session = Some(1);
    let mut bytes = Vec::new();
    session
        .configure_ordering_at(&certificate(&key, 3, 1400), [7; 32], 1000, &mut bytes)
        .unwrap();
    let feedback = |high, echo_delay_ms| HopFeedback {
        up: RecvWindow { high, bits: 1 },
        echo_delay_ms,
    };
    session.take_feedback(feedback(1, 25), 1100);
    assert_eq!(session.link_stats().srtt_ms, Some(75));
    // The same up_high again, or a saturated echo delay, gives no sample.
    session.take_feedback(feedback(1, 0), 1200);
    for _ in 0..129 {
        session.pull_ordering_at(1, 1300, &mut bytes).unwrap();
    }
    // Sequence 2 was overwritten in the 128-entry log by 130.
    session.take_feedback(feedback(2, 0), 1400);
    session.take_feedback(feedback(129, u16::MAX), 1400);
    assert_eq!(session.link_stats().rtt_samples, 1);
    session.take_feedback(feedback(130, 0), 1400);
    assert_eq!(session.link_stats().rtt_samples, 2);
}

#[test]
fn the_uplink_goes_offline_after_a_silent_second_and_recovers_on_receipt() {
    let key = SecretKey::new([77; 32]);
    let mut server = RelayServer::new(key.clone(), 1);
    let mut first = client(&key, 0);
    connect(&mut server, &mut first, 0);
    let mut bytes = Vec::new();
    first
        .configure_ordering_at(&certificate(&key, 3, 1400), [7; 32], 0, &mut bytes)
        .unwrap();
    let replies = server
        .process_at(endpoint(&first), &bytes, unix(0), 0)
        .unwrap();
    first.preset_uplink_measured_good();
    for reply in &replies {
        first.receive_at(&reply.bytes, 0).unwrap();
    }
    first.evaluate_uplink(999);
    assert_eq!(first.link_stats().uplink, LinkState::Good);
    first.evaluate_uplink(1000);
    assert_eq!(first.link_stats().uplink, LinkState::Offline);
    first.ordering.as_mut().unwrap().link.rto_backoff = 3;
    // A Backpressure error ends the silence and holds congestion 2 s.
    let mut error = vec![ERROR_EVENT, OrderError::Backpressure as u8];
    error.extend_from_slice(&[0; 8]);
    let datagram = server_packet(
        server
            .rooms
            .get_mut(&[5; 16])
            .unwrap()
            .peers
            .get_mut(0)
            .unwrap(),
        crate::CONTROL,
        0,
        &error,
        1500,
        crate::link::SEND_OTHER,
    )
    .unwrap();
    first.receive_at(&datagram.bytes, 1500).unwrap();
    let link = &first.ordering.as_ref().unwrap().link;
    assert!(link.recovered);
    assert_eq!(link.rto_backoff, 0);
    assert_eq!(first.link_stats().uplink, LinkState::Congested);
    first.evaluate_uplink(3499);
    assert_eq!(first.link_stats().uplink, LinkState::Offline);
    let stats = first.link_stats();
    assert_eq!(stats.uplink_state_entries, [0, 0, 1, 2]);
    assert_eq!(stats.uplink_state_ms, [1000, 0, 1999, 500]);
}

/// Spec appendix A packets 20 and 25 as PACKs: a pack with two piggybacked frame
/// ends before a batch, and a status heartbeat's feedback.
#[test]
fn appendix_pack_and_status_heartbeat_have_their_layouts() {
    let opaque = [9; 74];
    let mut batch = vec![0];
    batch.extend_from_slice(&1u64.to_le_bytes());
    batch.extend_from_slice(&opaque);
    let records = [
        OrderingRoom::encoded_entry(3, 3, FRAME_END_EVENT, &[]),
        OrderingRoom::encoded_entry(4, 4, FRAME_END_EVENT, &[]),
        OrderingRoom::encoded_entry(5, 5, BATCH_EVENT, &batch),
        OrderingRoom::encoded_entry(6, 5, FRAME_END_EVENT, &[]),
        OrderingRoom::encoded_entry(7, 6, FRAME_END_EVENT, &[]),
    ];
    // Compaction spec §5.2: 86 B.
    let packs = pack::encode_pack(&records, 1);
    let (pack, count) = &packs[0];
    assert_eq!((pack.len(), *count), (86, 5));
    assert_eq!(&pack[..6], &[PACK_EVENT, 3, 3, 5, 0x04, 0x14]);
    assert_eq!(&pack[6..10], &[0x13, 0, 1, 74]);
    assert_eq!(&pack[84..], &[0x04, 0x14]);
    assert_eq!(pack::decode_pack(pack).unwrap(), records);

    let key = SecretKey::new([78; 32]);
    let mut reader = client(&key, 1);
    reader.ready = true;
    reader.session = Some(1);
    reader
        .configure_ordering(&certificate(&key, 3, 1400), [7; 32], &mut Vec::new())
        .unwrap();
    // Compaction spec §7.3: 22 B.
    #[rustfmt::skip]
    let status = [
        STATUS_EVENT, 10, 1, 12, 0b11, 0, 0, 0, 30, 0, 0, 1,
        8, 0x7D, 0, 0, 0, 0, 0, 0, 0, 33,
    ];
    let Ok(OrderEvent::Status(status)) = reader.receive_ordering(&status) else {
        panic!("status");
    };
    assert_eq!(
        (status.head_frame, status.next_seq, status.started),
        (10, 12, true)
    );
    assert_eq!(
        status.feedback.up,
        RecvWindow {
            high: 8,
            bits: 0x7D
        }
    );
    assert_eq!(status.feedback.echo_delay_ms, 33);
}

#[test]
fn a_pull_packs_its_page_and_the_client_reads_each_canonical_entry() {
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    let mut bytes = Vec::new();
    second.test_submit(1, b"packed input", &mut bytes).unwrap();
    events(&mut server, &mut second, &bytes, 0);
    server.advance_clock(400, unix(400));
    first.pull_ordering(1, &mut bytes).unwrap();
    let packets = server
        .process_at(endpoint(&first), &bytes, unix(400), 400)
        .unwrap();
    assert_eq!(packets.len(), 2, "a status and one pack");
    let payload = &packets[1].bytes[HEADER_LEN..packets[1].bytes.len() - HOP_TAG_LEN];
    let history: Vec<_> = ordering(&server)
        .history
        .iter()
        .map(|entry| entry.bytes.clone())
        .collect();
    assert_eq!(pack::decode_pack(payload).unwrap(), history);
    let entries: Vec<_> = received(&mut first, &packets[1].bytes)
        .into_iter()
        .map(|event| match event {
            OrderEvent::Entry(OrderEntry::Batch {
                sequence, bytes, ..
            }) => (sequence, Some(bytes)),
            OrderEvent::Entry(OrderEntry::FrameEnd { sequence, .. }) => (sequence, None),
            _ => panic!("a batch or frame end"),
        })
        .collect();
    assert_eq!(entries.len(), 10, "a batch and nine frame ends");
    assert_eq!(entries[0], (1, Some(b"packed input".to_vec())));
    assert!(
        entries
            .iter()
            .enumerate()
            .all(|(index, (sequence, _))| *sequence == index as u64 + 1)
    );
}

fn report_checksums(
    server: &mut RelayServer,
    client: &mut ClientSession,
    samples: &[(u32, u32)],
    now_ms: u64,
) {
    let mut bytes = Vec::new();
    client.queue_ordering_checksums(samples, now_ms).unwrap();
    client.test_uplink(None, now_ms, &mut bytes).unwrap();
    assert!(events(server, client, &bytes, now_ms).is_empty());
}

/// The DESYNC records of a room's history, decoded, each with the entry
/// before it.
fn desyncs(server: &RelayServer, reader: &mut ClientSession) -> Vec<(u8, OrderEntry)> {
    let history = &ordering(server).history;
    (1..history.len())
        .filter(|&index| history[index].bytes[0] == DESYNC_EVENT)
        .map(|index| {
            let OrderEvent::Entry(entry) = reader.receive_ordering(&history[index].bytes).unwrap()
            else {
                panic!("a desync entry");
            };
            (history[index - 1].bytes[0], entry)
        })
        .collect()
}

/// The relay compares every player's checksum of a sampled frame and
/// ends the match once, with a DESYNC record after the head's frame
/// end: of three players it names the one off the majority, of two the
/// later reporter.
#[test]
fn the_relay_names_the_player_whose_checksum_differs_once() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut players: Vec<_> = (0..3).map(|source| client(&key, source)).collect();
    let cert = certificate(&key, 7, 1400);
    for player in &mut players {
        connect(&mut server, player, 0);
        configure(&mut server, player, &cert, 0);
    }
    server.advance_clock(1000, unix(1000));
    for player in &mut players {
        report_checksums(&mut server, player, &[(15, 7)], 1000);
    }
    // Repeats of a report change nothing; one against one of three waits.
    report_checksums(&mut server, &mut players[0], &[(30, 9), (15, 7)], 1100);
    report_checksums(&mut server, &mut players[1], &[(30, 8), (15, 7)], 1100);
    report_checksums(&mut server, &mut players[1], &[(30, 9)], 1100);
    assert!(desyncs(&server, &mut players[0]).is_empty());
    report_checksums(&mut server, &mut players[2], &[(30, 9), (15, 7)], 1200);
    let head = ordering(&server).head;
    // Nothing is compared again, though the relay has sealed frame 45.
    server.advance_clock(1300, unix(1300));
    report_checksums(&mut server, &mut players[0], &[(45, 1)], 1300);
    report_checksums(&mut server, &mut players[2], &[(45, 2)], 1300);
    assert!(matches!(
        &desyncs(&server, &mut players[0])[..],
        [(
            FRAME_END_EVENT,
            OrderEntry::Desync {
                frame,
                sample_frame: 30,
                outlier: 1,
                outlier_checksum: 8,
                other: 0,
                other_checksum: 9,
                ..
            }
        )] if *frame == head
    ));
    assert_eq!(server.take_ordering_counts().desyncs, 1);

    let (mut server, mut first, mut second) = paced_room(&key);
    server.advance_clock(1000, unix(1000));
    report_checksums(&mut server, &mut first, &[(15, 9)], 1000);
    report_checksums(&mut server, &mut second, &[(15, 8)], 1000);
    assert!(matches!(
        &desyncs(&server, &mut first)[..],
        [(
            _,
            OrderEntry::Desync {
                outlier: 1,
                outlier_checksum: 8,
                other: 0,
                other_checksum: 9,
                ..
            }
        )]
    ));
}

/// A player reports only sampled frames the relay has reached, so a flood
/// of later ones cannot push the others' reports out of the window.
#[test]
fn far_future_checksum_reports_cannot_switch_desync_detection_off() {
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    server.advance_clock(1000, unix(1000));
    for pair in 0..CHECKSUM_FRAMES as u32 / 2 {
        let frame = 15_000 + 30 * pair;
        report_checksums(
            &mut server,
            &mut second,
            &[(frame + 15, 1), (frame, 1)],
            1000,
        );
    }
    report_checksums(&mut server, &mut first, &[(15, 9)], 1000);
    report_checksums(&mut server, &mut second, &[(15, 8)], 1000);
    assert_eq!(desyncs(&server, &mut first).len(), 1);
}

/// A player's reports stop counting when it leaves: one that reports a
/// false checksum and then leaves cannot end the other's match.
#[test]
fn a_departed_players_checksum_names_no_outlier() {
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    server.advance_clock(1000, unix(1000));
    report_checksums(&mut server, &mut second, &[(30, 8)], 1000);
    let mut bytes = Vec::new();
    second.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut second, &bytes, 1000);
    report_checksums(&mut server, &mut first, &[(30, 9)], 1000);
    assert!(desyncs(&server, &mut first).is_empty());
}

/// A follower whose healed link delivers two rate changes at once applies
/// both in order, each from its frame's due time on the old schedule.
#[test]
fn queued_federated_rate_changes_apply_in_order_on_one_timeline() {
    let key = SecretKey::new([73; 32]);
    let (mut server, _first, _second) = paced_room(&key);
    let room = server.rooms.get_mut(&[5; 16]).unwrap();
    let ordering = room.ordering.as_mut().unwrap();
    let start = ordering.starts_at.unwrap();
    ordering.pace.scheduled.extend([(90, 30, 1), (180, 20, 1)]);
    // 90 frames at 45 Hz are due 2 s in; 90 more at 30 Hz, 3 s later.
    ordering.apply_scheduled_rate(start + 4_999, start);
    assert_eq!(
        (ordering.pace.rate, ordering.pace.anchor),
        (30, Some((90, 2_000)))
    );
    ordering.apply_scheduled_rate(start + 5_000, start);
    assert_eq!(
        (ordering.pace.rate, ordering.pace.anchor),
        (20, Some((180, 5_000)))
    );
    assert!(ordering.pace.scheduled.is_empty());
}

/// VO2C adds the host's pacing flag under its own signing domain; the
/// vectors are an independent HMAC-SHA256 the lobby also checks against.
#[test]
fn a_vo2c_certificate_signs_its_pacing_flag_under_its_own_domain() {
    let key = SecretKey::new([73; 32]);
    let cert = |pacing| OrderingCertificate {
        room: [5; 16],
        sources_mask: 3,
        tick_hz: 45,
        expires: 2000,
        pacing,
        ordered_alone: false,
    };
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let on = cert(CertificatePacing::On).sign(&key).unwrap();
    let off = cert(CertificatePacing::Off).sign(&key).unwrap();
    assert_eq!(
        hex(&on),
        "564f324305050505050505050505050505050505030000002d00d007000000000000003e4c04db8da83d3705b259262fafbcf274224f6ba8d59b743e14db3d5b10dc48"
    );
    assert_eq!(
        hex(&off),
        "564f324305050505050505050505050505050505030000002d00d00700000000000001252f63eae21061da149b08caacb3bbecf61985bf996d0932b3e101330bbc461d"
    );
    assert_eq!(
        OrderingCertificate::verify(&off, &key, 1000)
            .unwrap()
            .pacing,
        CertificatePacing::Off
    );
    let legacy = cert(CertificatePacing::Legacy).sign(&key).unwrap();
    assert_eq!(legacy.len(), ORDERING_CERTIFICATE_LEN);
    assert_eq!(
        OrderingCertificate::verify(&legacy, &key, 1000)
            .unwrap()
            .pacing,
        CertificatePacing::Legacy
    );
    // The relay orders a one-source match the lobby marked; only VO2C
    // carries the mark.
    let alone = OrderingCertificate {
        sources_mask: 1,
        ordered_alone: true,
        ..cert(CertificatePacing::On)
    };
    assert_eq!(
        hex(&alone.sign(&key).unwrap()),
        "564f324305050505050505050505050505050505010000002d00d0070000000000000276df2bf6da6804b5e845d57954af1692120f0b35b4fd4e2a605cc07151a73a81"
    );
    assert!(alone.relay_orders());
    assert!(
        !OrderingCertificate {
            ordered_alone: false,
            ..alone
        }
        .relay_orders()
    );
    assert!(
        OrderingCertificate {
            pacing: CertificatePacing::Legacy,
            ..alone
        }
        .sign(&key)
        .is_err()
    );
    // A VO1C body relabelled VO2C does not verify, and unknown flags fail.
    let mut relabelled = on.clone();
    relabelled[34] = 8;
    assert!(matches!(
        OrderingCertificate::parse(&relabelled),
        Err(RelayError::Malformed)
    ));
    let mut forged = legacy.clone();
    forged[..4].copy_from_slice(CERTIFICATE_V2_MAGIC);
    forged.insert(34, 0);
    assert!(matches!(
        OrderingCertificate::verify(&forged, &key, 1000),
        Err(RelayError::Authentication)
    ));
}

#[test]
fn a_player_who_cannot_keep_up_slows_the_clock_at_most_two_rows() {
    use OrderPace::*;
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    pace_report(&mut server, &mut second, Behind, 40, 1000);
    server.advance_clock(1000, unix(1000));
    assert_eq!(
        ordering(&server).pace.rate,
        45,
        "a player who has not reported may not read a rate marker"
    );
    pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
    server.advance_clock(1000, unix(1000));
    server.advance_clock(2000, unix(2000));
    assert_eq!(ordering(&server).head, 36 + 30);
    pace_report(&mut server, &mut second, Behind, 25, 3000);
    server.advance_clock(3000, unix(3000));
    assert_eq!(
        ordering(&server).pace.rate,
        30,
        "one change per five seconds"
    );
    pace_report(&mut server, &mut second, Behind, 25, 6000);
    pace_report(&mut server, &mut second, Behind, 5, 11_000);
    server.advance_clock(11_000, unix(11_000));
    let markers: Vec<_> = ordering(&server)
        .history
        .iter()
        .filter(|entry| entry.bytes[0] == RATE_EVENT)
        .map(
            |entry| match first.receive_ordering(&entry.bytes).unwrap() {
                OrderEvent::Entry(OrderEntry::Rate {
                    frame,
                    frames_per_second,
                    source,
                    ..
                }) => (frame, frames_per_second, source),
                _ => panic!("rate marker"),
            },
        )
        .collect();
    // 40 a second leaves 30 a tenth of slack; 25 leaves 20 none to spare.
    assert_eq!(markers, [(36, 30, Some(1)), (186, 20, Some(1))]);
}

#[test]
fn the_clock_speeds_up_once_nobody_lags_and_waits_twice_as_long_after_a_relapse() {
    use OrderPace::*;
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
    pace_report(&mut server, &mut second, Behind, 40, 1000);
    server.advance_clock(1000, unix(1000));
    // Frames still wait on the slow player: the wait starts once they don't.
    pace_report(&mut server, &mut second, CatchingUp, 33, 2000);
    pace_report(&mut server, &mut second, CatchingUp, 33, 5000);
    pace_report(&mut server, &mut second, KeepingUp, 30, 8000);
    server.advance_clock(37_999, unix(37_999));
    assert_eq!(ordering(&server).pace.rate, 30);
    server.advance_clock(38_000, unix(38_000));
    assert_eq!(ordering(&server).pace.rate, 45);
    pace_report(&mut server, &mut second, Behind, 40, 46_000);
    server.advance_clock(46_000, unix(46_000));
    pace_report(&mut server, &mut second, KeepingUp, 30, 47_000);
    server.advance_clock(106_999, unix(106_999));
    assert_eq!(ordering(&server).pace.rate, 30);
    server.advance_clock(107_000, unix(107_000));
    assert_eq!(ordering(&server).pace.rate, 45);
    pull(&mut server, &mut first, 1, 107_000);
    let counts = server.take_ordering_counts();
    assert_eq!(
        (counts.slowdowns, counts.raises, counts.pull_replies),
        (2, 2, 1)
    );
    assert_eq!(server.take_ordering_counts(), Default::default());
}

#[test]
fn a_pause_holds_the_wait_before_speeding_up() {
    use OrderPace::*;
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    for peer in [&mut first, &mut second] {
        pause_control(&mut server, peer, 0, 0, false, 500);
    }
    pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
    pace_report(&mut server, &mut second, Behind, 40, 1000);
    server.advance_clock(1000, unix(1000));
    pace_report(&mut server, &mut second, CatchingUp, 35, 20_000);
    server.advance_clock(20_000, unix(20_000));
    pause_control(&mut server, &mut first, 1, 0, true, 40_000);
    pause_control(&mut server, &mut second, 1, 1, false, 140_000);
    server.advance_clock(149_999, unix(149_999));
    assert_eq!(ordering(&server).pace.rate, 30);
    server.advance_clock(150_000, unix(150_000));
    assert_eq!(ordering(&server).pace.rate, 45);
}

#[test]
fn the_journal_count_has_each_slowdown_once_and_no_speedup() {
    use OrderPace::*;
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
    pace_report(&mut server, &mut second, Behind, 40, 1000);
    server.advance_clock(1000, unix(1000));
    pace_report(&mut server, &mut second, KeepingUp, 30, 2000);
    server.advance_clock(32_000, unix(32_000));
    assert_eq!(ordering(&server).pace.rate, 45, "slowed, then sped up");
    assert_eq!(server.take_ordering_counts().slowdowns, 1);
    assert_eq!(server.take_ordering_counts().slowdowns, 0);
}

#[test]
fn the_journal_count_has_a_second_for_each_catching_up_or_behind_report() {
    use OrderPace::*;
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
    pace_report(&mut server, &mut second, CatchingUp, 44, 1000);
    pace_report(&mut server, &mut second, Behind, 40, 2000);
    pace_report(&mut server, &mut second, KeepingUp, 45, 3000);
    assert_eq!(server.take_ordering_counts().catching_up_s, 2);
    assert_eq!(server.take_ordering_counts().catching_up_s, 0);
}

/// Released clients cannot be changed, so a newer relay may only append
/// to replies. A reply decodes by its known prefix; a newer error code or
/// record is reported as unrecognized, and a canonical entry decodes only
/// in full.
#[test]
fn newer_replies_decode_by_prefix_and_newer_records_are_unrecognized() {
    let key = SecretKey::new([74; 32]);
    let mut server = RelayServer::new(key.clone(), 1);
    let mut reader = client(&key, 0);
    connect(&mut server, &mut reader, 0);
    reader
        .configure_ordering(&certificate(&key, 3, 1400), [7; 32], &mut Vec::new())
        .unwrap();
    // STATUS (compaction spec §7.3), all zero but `started`, its feedback
    // tail and one byte more.
    let mut status = vec![STATUS_EVENT, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    crate::link::append_feedback(&mut status, crate::link::HopFeedback::default());
    status.push(0);
    assert!(matches!(
        reader.receive_ordering(&status),
        Ok(OrderEvent::Status(OrderStatus { started: true, .. }))
    ));
    let mut error = vec![ERROR_EVENT, OrderError::NotStarted as u8, 0, 0];
    assert!(matches!(
        reader.receive_ordering(&error),
        Ok(OrderEvent::Error {
            code: OrderError::NotStarted,
            ..
        })
    ));
    error[1] = 200;
    let departure = |reason: &[u8]| {
        let mut payload = vec![DEPARTURE_EVENT];
        payload.extend_from_slice(&[0; 12]);
        payload.push(0);
        payload.extend_from_slice(reason);
        payload
    };
    let mut frame_end = vec![FRAME_END_EVENT];
    frame_end.resize(14, 0);
    for payload in [error, departure(&[2]), departure(&[1, 0]), frame_end] {
        assert!(matches!(
            reader.receive_ordering(&payload),
            Ok(OrderEvent::Unrecognized)
        ));
    }
    assert!(matches!(
        reader.receive_ordering(&departure(&[1])),
        Ok(OrderEvent::Entry(OrderEntry::Departure { lost: true, .. }))
    ));
}

#[test]
fn nobody_pauses_again_until_the_match_has_run_after_a_resume() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
        pause_control(&mut server, peer, 0, 0, false, 300);
    }
    pause_control(&mut server, &mut first, 1, 0, true, 300);
    pause_control(&mut server, &mut second, 1, 1, false, 400);
    let paused = |event| matches!(event, OrderEvent::PauseStatus { paused: true, .. });
    let early = 400 + PAUSE_COOLDOWN_MS - 1;
    assert!(!paused(pause_control(
        &mut server,
        &mut first,
        2,
        2,
        true,
        early
    )));
    assert!(paused(pause_control(
        &mut server,
        &mut first,
        3,
        2,
        true,
        early + 1
    )));
}

#[test]
fn a_one_source_certificate_opens_no_ordering_room() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut solo = client(&key, 0);
    connect(&mut server, &mut solo, 0);
    let mut bytes = Vec::new();
    solo.configure_ordering(&certificate(&key, 1, 1400), [7; 32], &mut bytes)
        .unwrap();
    assert!(matches!(
        server.process_at(endpoint(&solo), &bytes, unix(0), 0),
        Err(RelayError::Authentication)
    ));
    assert!(server.rooms[&[5; 16]].ordering.is_none());
}

/// One person watching AI play, whose match the lobby marked so that
/// others can watch it, orders through the relay alone and starts.
#[test]
fn a_marked_one_source_match_starts_alone() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut solo = client(&key, 0);
    connect(&mut server, &mut solo, 0);
    let alone = OrderingCertificate {
        room: [5; 16],
        sources_mask: 1,
        tick_hz: 45,
        expires: 1400,
        pacing: CertificatePacing::On,
        ordered_alone: true,
    }
    .sign(&key)
    .unwrap();
    configure(&mut server, &mut solo, &alone, 0);
    let start = ordering(&server)
        .starts_at
        .expect("its only source is ready");
    server.advance_clock(start + 67, unix(start + 67));
    assert_eq!(ordering(&server).head, 3, "the first turn is sealed");
}

#[test]
fn pause_requires_all_capabilities_and_rejects_stale_control_epochs() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    assert!(matches!(
        pause_control(&mut server, &mut first, 0, 0, false, 300),
        OrderEvent::PauseStatus {
            supported_sources: 1,
            request_id: 0,
            paused: false,
            ..
        }
    ));
    assert!(matches!(
        pause_control(&mut server, &mut first, 1, 0, true, 300),
        OrderEvent::PauseStatus {
            request_id: 1,
            epoch: 0,
            paused: false,
            ..
        }
    ));
    pause_control(&mut server, &mut second, 0, 0, false, 300);
    assert!(
        matches!(
            pause_control(&mut server, &mut first, 1, 0, true, 310),
            OrderEvent::PauseStatus {
                request_id: 1,
                epoch: 0,
                paused: false,
                ..
            }
        ),
        "a previously rejected request cannot become an action after quorum"
    );
    assert!(matches!(
        pause_control(&mut server, &mut first, 2, 0, true, 310),
        OrderEvent::PauseStatus {
            supported_sources: 3,
            request_id: 2,
            epoch: 1,
            paused: true,
            frame: 3,
            ..
        }
    ));
    // Fresh envelopes carrying the same request cannot append twice.
    assert!(matches!(
        pause_control(&mut server, &mut first, 2, 0, true, 1000),
        OrderEvent::PauseStatus {
            epoch: 1,
            frame: 3,
            paused: true,
            ..
        }
    ));
    assert!(matches!(
        pause_control(&mut server, &mut first, 2, 0, false, 1000),
        OrderEvent::Error {
            code: OrderError::ModifiedRetry,
            expected: 2
        }
    ));
    assert!(matches!(
        pause_control(&mut server, &mut second, 1, 0, false, 1000),
        OrderEvent::PauseStatus {
            request_id: 1,
            epoch: 1,
            paused: true,
            ..
        }
    ));
    assert!(matches!(
        pause_control(&mut server, &mut second, 2, 1, false, 2000),
        OrderEvent::PauseStatus {
            epoch: 2,
            paused: false,
            frame: 3,
            ..
        }
    ));
    server.advance_clock(2023, unix(2023));
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 3);
    server.advance_clock(2024, unix(2024));
    assert_eq!(
        server.rooms[&[5; 16]].ordering.as_ref().unwrap().head,
        6,
        "resume preserves the next frame deadline instead of catching up paused time"
    );
    let markers: Vec<_> = pull(&mut server, &mut first, 1, 2024)
        .into_iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Pause {
                epoch,
                paused,
                frame,
                ..
            }) => Some((epoch, paused, frame)),
            _ => None,
        })
        .collect();
    assert_eq!(markers, [(1, true, 3), (2, false, 3)]);
}

#[test]
fn lost_pause_marker_replays_after_renewal_and_preserves_pending_input() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
        pause_control(&mut server, peer, 0, 0, false, 0);
    }
    subscribe(&mut server, &mut first, 1, 300);
    let mut bytes = Vec::new();
    second
        .test_submit(1, b"accepted before pause", &mut bytes)
        .unwrap();
    events(&mut server, &mut second, &bytes, 300);
    pause_control(&mut server, &mut first, 1, 0, true, 300);
    let mut lost = None;
    for packet in server.drain_ordering_push(300, unix(300)) {
        let payload = &packet.bytes[HEADER_LEN..packet.bytes.len() - HOP_TAG_LEN];
        if payload[0] == PACK_EVENT {
            lost = pack::decode_pack(payload)
                .unwrap()
                .into_iter()
                .find(|entry| entry[0] == PAUSE_EVENT);
        } else {
            first.receive(&packet.bytes).unwrap();
        }
    }
    let lost = lost.expect("the pause marker was in the intentionally lost push");
    server.advance_clock(1000, unix(1000));
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 3);
    renew(&mut server, &mut first, 1000);
    configure(&mut server, &mut first, &cert, 1000);
    assert!(matches!(
        pause_control(&mut server, &mut first, 1, 0, true, 1000),
        OrderEvent::PauseStatus {
            request_id: 1,
            epoch: 1,
            paused: true,
            ..
        }
    ));
    assert!(matches!(
        pause_control(&mut server, &mut second, 1, 1, false, 1000),
        OrderEvent::PauseStatus {
            epoch: 2,
            paused: false,
            frame: 3,
            ..
        }
    ));
    server.advance_clock(250_000, unix(250_000));
    assert!(
        server.rooms[&[5; 16]]
            .ordering
            .as_ref()
            .unwrap()
            .hot_oldest_seq()
            > 1
    );
    renew(&mut server, &mut first, 250_000);
    configure(&mut server, &mut first, &cert, 250_000);
    first.pull_ordering(1, &mut bytes).unwrap();
    let packets = server
        .process_at(endpoint(&first), &bytes, unix(250_000), 250_000)
        .unwrap();
    assert!(
        packets.iter().any(|packet| {
            let payload = &packet.bytes[HEADER_LEN..packet.bytes.len() - HOP_TAG_LEN];
            payload[0] == PACK_EVENT && pack::decode_pack(payload).unwrap().contains(&lost)
        }),
        "disk replay retains exactly the lost canonical marker bytes"
    );
    let mut markers = Vec::new();
    let mut inputs = Vec::new();
    for packet in packets {
        for event in received(&mut first, &packet.bytes) {
            match event {
                OrderEvent::Entry(OrderEntry::Pause { epoch, paused, .. }) => {
                    markers.push((epoch, paused))
                }
                OrderEvent::Entry(OrderEntry::Batch { frame, bytes, .. }) => {
                    inputs.push((frame, bytes))
                }
                _ => {}
            }
        }
    }
    assert_eq!(markers, [(1, true), (2, false)]);
    assert_eq!(inputs, [(4, b"accepted before pause".to_vec())]);
}

/// Compaction spec §6.2 "续期": the relay accepted the renewal's CONFIRM
/// but its READY never arrived, and the player quits from the old hop.
/// The old session still takes the LEAVE, answers it, and the stream
/// records a Leave, not a lost connection.
#[test]
fn a_player_quitting_before_its_renewal_ready_still_leaves() {
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, mut second) = paced_room(&key);
    server.advance_clock(300, unix(300));
    let mut ticket = first.ticket;
    ticket.expires += 50;
    ticket.nonce[15] ^= 0x40;
    let signed = ticket.sign(&key).unwrap();
    let mut candidate = first
        .renewal(
            &signed,
            *derive_packet_key(&key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
    let mut bytes = Vec::new();
    candidate.probe(&mut bytes).unwrap();
    let challenge = server
        .process_at(endpoint(&first), &bytes, unix(300), 300)
        .unwrap()
        .remove(0);
    let ServerEvent::Challenge(cookie) = candidate.receive(&challenge.bytes).unwrap() else {
        panic!("challenge");
    };
    candidate.confirm(cookie, &mut bytes).unwrap();
    // The relay takes the renewal; its READY is lost.
    server
        .process_at(endpoint(&first), &bytes, unix(300), 300)
        .unwrap();
    first.leave_ordering(&mut bytes).unwrap();
    for _ in 0..2 {
        let ack = server
            .process_at(endpoint(&first), &bytes, unix(300), 300)
            .unwrap();
        assert!(matches!(
            first.receive(&ack[0].bytes),
            Ok(ServerEvent::Ordering(OrderEvent::Left))
        ));
    }
    server.advance_clock(700, unix(700));
    let departures: Vec<_> = pull(&mut server, &mut second, 1, 700)
        .into_iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Departure { source, lost, .. }) => Some((source, lost)),
            _ => None,
        })
        .collect();
    assert_eq!(departures, [(0, false)]);
}

#[test]
fn last_explicit_leave_resumes_the_paused_canonical_departure_tail() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
        pause_control(&mut server, peer, 0, 0, false, 0);
    }
    pause_control(&mut server, &mut first, 1, 0, true, 200);
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 0);
    let mut bytes = Vec::new();
    first.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut first, &bytes, 500);
    server.advance_clock(600, unix(600));
    assert!(
        server.rooms[&[5; 16]]
            .ordering
            .as_ref()
            .unwrap()
            .paused_at
            .is_some()
    );
    second.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut second, &bytes, 600);
    server.advance_clock(700, unix(700));
    // The hop that left takes only LEAVE (compaction spec §6.2).
    renew(&mut server, &mut first, 700);
    let events = pull(&mut server, &mut first, 1, 700);
    let mut resumed = false;
    let mut departed = Vec::new();
    for event in events {
        match event {
            OrderEvent::Entry(OrderEntry::Pause {
                epoch: 2,
                paused: false,
                source: 1,
                frame: 0,
                ..
            }) => resumed = true,
            OrderEvent::Entry(OrderEntry::Departure {
                source, frame: 1, ..
            }) => departed.push(source),
            _ => {}
        }
    }
    assert!(
        resumed,
        "the just-departing source remains a valid original roster identity"
    );
    assert_eq!(departed, [0, 1]);
    let ordering = server.rooms[&[5; 16]].ordering.as_ref().unwrap();
    assert_eq!(ordering.head, 1);
    assert!(ordering.finished_at.is_some());
}

#[test]
fn opted_in_push_follows_the_clock_and_keeps_legacy_pull_unchanged() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut pushed, mut legacy) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut pushed, &mut legacy] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    // The UPLINK subscribes without any reply (spec §3.4).
    assert!(subscribe(&mut server, &mut pushed, 1, 0).is_empty());
    server.advance_clock(350, unix(350));
    let packets = server.drain_ordering_push(350, unix(350));
    assert!(!packets.is_empty());
    assert!(
        packets
            .iter()
            .all(|packet| packet.endpoint == endpoint(&pushed))
    );
    let mut pushed_ends = Vec::new();
    let mut started = false;
    for packet in packets {
        for event in received(&mut pushed, &packet.bytes) {
            match event {
                OrderEvent::Status(status) => started |= status.started,
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                    pushed_ends.push((sequence, frame))
                }
                _ => {}
            }
        }
    }
    assert!(
        started,
        "the startup transition does not wait for a client Pull"
    );
    let legacy_ends: Vec<_> = pull(&mut server, &mut legacy, 1, 350)
        .into_iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => Some((sequence, frame)),
            _ => None,
        })
        .collect();
    assert_eq!(pushed_ends, legacy_ends);
    assert_eq!(pushed_ends.last(), Some(&(6, 6)));
    // Status still describes a missing tail while delivery credit is idle.
    let heartbeat = server.drain_ordering_push(600, unix(600));
    assert!(heartbeat.into_iter().any(|packet| matches!(
        pushed.receive(&packet.bytes),
        Ok(ServerEvent::Ordering(OrderEvent::Status(_)))
    )));
    // A reordered older UPLINK cannot move the watermark back.
    let (mut older, mut newer) = (Vec::new(), Vec::new());
    pushed.ordering.as_mut().unwrap().link.schedule.recv_next = 3;
    pushed.test_uplink(None, 601, &mut older).unwrap();
    pushed.ordering.as_mut().unwrap().link.schedule.recv_next = 7;
    pushed.test_uplink(None, 601, &mut newer).unwrap();
    events(&mut server, &mut pushed, &newer, 601);
    events(&mut server, &mut pushed, &older, 601);
    let stats = server.reader_link_stats([5; 16], 0).unwrap();
    assert_eq!((stats.acked_next, stats.sent_next), (7, 7));
    server.advance_clock(3601, unix(3601));
    assert!(
        server.drain_ordering_push(3601, unix(3601)).is_empty(),
        "a paused reader's subscription expires without retiring its source"
    );
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 3);
    server.advance_clock(120_000, unix(120_000));
    let cursor = server.rooms[&[5; 16]].ordering.as_ref().unwrap().next_seq;
    subscribe(&mut server, &mut pushed, cursor, 120_000);
    assert!(
        !server
            .drain_ordering_push(120_250, unix(120_250))
            .is_empty(),
        "the next UPLINK subscribes again"
    );
}

/// A new process meets the batches of the one it replaces in the stream.
/// As receipts of its own they would be refused, and its cursor stay on the
/// first of them for good.
#[test]
fn a_replacing_process_takes_its_batch_counter_from_the_stream() {
    let key = SecretKey::new([74; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut back = client(&key, 0);
    connect(&mut server, &mut back, 0);
    configure(&mut server, &mut back, &certificate(&key, 3, 1400), 0);
    let ordering = back.ordering.as_mut().unwrap();
    assert!(matches!(
        ordering.accept_through(5),
        Err(RelayError::Authentication)
    ));

    back.set_ordering_restore(true);
    back.ordering.as_mut().unwrap().accept_through(5).unwrap();
    back.set_ordering_restore(false);

    assert_eq!(back.seal_batch(b"next"), Ok(6));
    // Once restored, a batch this process did not send is refused again.
    assert!(matches!(
        back.ordering.as_mut().unwrap().accept_through(7),
        Err(RelayError::Authentication)
    ));
}

/// Credit counts entries and caps count datagrams: one pack carries a
/// reader's whole credit of frame ends.
#[test]
fn push_credit_counts_entries_and_existing_pull_repairs_a_lost_pack() {
    use std::collections::BTreeMap;
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut clients: Vec<_> = (0..6).map(|source| client(&key, source)).collect();
    let cert = certificate(&key, 63, 1400);
    for peer in &mut clients {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
        subscribe(&mut server, peer, 1, 0);
    }
    // 81 frame ends.
    server.advance_clock(2000, unix(2000));
    // Heard within the 1500 ms offline limit (spec §5.1).
    for peer in &mut clients {
        subscribe(&mut server, peer, 1, 1999);
    }
    let mut lost = false;
    for _ in 0..20 {
        let mut per_peer = BTreeMap::new();
        for packet in server.drain_ordering_push(2000, unix(2000)) {
            let index = clients
                .iter()
                .position(|peer| endpoint(peer) == packet.endpoint)
                .unwrap();
            *per_peer.entry(index).or_insert(0) += 1;
            if packet.bytes[HEADER_LEN] == PACK_EVENT {
                if index == 0 && !lost {
                    lost = true;
                    continue; // a real missing authenticated datagram
                }
            }
            received(&mut clients[index], &packet.bytes);
        }
        assert!(
            per_peer.values().all(|count| *count <= 3),
            "a status, a pack and a tail probe"
        );
    }
    assert!(lost);
    for source in 0..6 {
        assert_eq!(
            server.reader_link_stats([5; 16], source).unwrap().sent_next,
            65,
            "unacknowledged readers cannot exceed their individual credit"
        );
    }
    // A healthy reader continues without granting the stalled readers credit.
    subscribe(&mut server, &mut clients[1], 65, 2001);
    let next = server.drain_ordering_push(2001, unix(2001));
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].endpoint, endpoint(&clients[1]));
    let sequences: Vec<_> = received(&mut clients[1], &next[0].bytes)
        .into_iter()
        .map(|event| match event {
            OrderEvent::Entry(OrderEntry::FrameEnd { sequence, .. }) => sequence,
            _ => panic!("a frame end"),
        })
        .collect();
    assert_eq!(sequences, (65..=81).collect::<Vec<_>>());
    assert!(
        pull(&mut server, &mut clients[0], 1, 2002)
            .iter()
            .any(|event| matches!(
                event,
                OrderEvent::Entry(OrderEntry::FrameEnd {
                    sequence: 1,
                    frame: 1
                })
            )),
        "the unchanged bounded Pull recovers the lost pack"
    );
}

/// Spec §7.5: only a lossy reader gets the 8 ms repeat, once.
#[test]
fn push_repeats_unacknowledged_entries_once_8_ms_on_only_while_lossy() {
    fn frame_ends(reader: &mut ClientSession, packets: Vec<ForwardDatagram>) -> Vec<u64> {
        packets
            .into_iter()
            .flat_map(|packet| received(reader, &packet.bytes))
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, .. }) => Some(sequence),
                _ => None,
            })
            .collect()
    }
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut lossy, mut good) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut lossy, &mut good] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
        subscribe(&mut server, peer, 1, 0);
    }
    let peers = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers;
    peers[0].link.loss.lossy = true;
    peers[1].link.loss = crate::link::LossStats::measured_good();
    server.advance_clock(300, unix(300));
    let first = server.drain_ordering_push(300, unix(300));
    let (lossy_at, good_at) = (endpoint(&lossy), endpoint(&good));
    let to = |at: SocketAddrV4, packets: &[ForwardDatagram]| -> Vec<ForwardDatagram> {
        packets
            .iter()
            .filter(|packet| packet.endpoint == at)
            .map(|packet| ForwardDatagram {
                endpoint: packet.endpoint,
                bytes: packet.bytes.clone(),
            })
            .collect()
    };
    assert_eq!(frame_ends(&mut lossy, to(lossy_at, &first)), [1, 2, 3]);
    assert_eq!(frame_ends(&mut good, to(good_at, &first)), [1, 2, 3]);
    assert!(server.drain_ordering_push(307, unix(307)).is_empty());
    let repeat = server.drain_ordering_push(308, unix(308));
    assert!(
        to(good_at, &repeat).is_empty(),
        "a good reader gets no repeat"
    );
    assert_eq!(frame_ends(&mut lossy, to(lossy_at, &repeat)), [1, 2, 3]);
    assert!(
        server.drain_ordering_push(330, unix(330)).is_empty(),
        "each entry is repeated once"
    );
    let stats = server.reader_link_stats([5; 16], 0).unwrap();
    assert_eq!(stats.packets_by_kind[2], 1);
    assert_eq!(stats.piggyback_depth, 2);
}

#[test]
fn repeats_take_only_what_fresh_pushes_leave() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut readers: Vec<_> = (0..5).map(|source| client(&key, source)).collect();
    let cert = certificate(&key, 31, 1400);
    for reader in &mut readers {
        connect(&mut server, reader, 0);
        configure(&mut server, reader, &cert, 0);
        subscribe(&mut server, reader, 1, 0);
    }
    // Only lossy readers get repeats (spec §7.5).
    for peer in &mut server.rooms.get_mut(&[5; 16]).unwrap().peers {
        peer.link.loss.lossy = true;
    }
    // Seven full batches in frame 1: each fills a pack of its own.
    let mut bytes = Vec::new();
    for (source, id) in [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1), (0, 2), (1, 2)] {
        readers[source]
            .test_submit(id, &[source as u8; ORDERING_BATCH_CAPACITY], &mut bytes)
            .unwrap();
        events(&mut server, &mut readers[source], &bytes, 0);
    }
    // Every reader holds frames 1-6 in seven packs, due for a repeat when
    // the next turn commits. Its fresh pack plus seven repeats fill one
    // reader's pass share, so four readers could otherwise take the
    // room's whole pass.
    server.advance_clock(360, unix(360));
    while !server.drain_ordering_push(360, unix(360)).is_empty() {}
    server.advance_clock(400, unix(400));
    let spent = server.rooms[&[5; 16]].order_packets.spent;
    let packets = server.drain_ordering_push(400, unix(400));
    assert_eq!(packets.len(), PUSH_ROOM_PACKETS);
    let fresh = packets
        .iter()
        .filter(|packet| {
            let reader = readers
                .iter_mut()
                .find(|reader| endpoint(reader) == packet.endpoint)
                .unwrap();
            received(reader, &packet.bytes).iter().any(|event| {
                matches!(
                    event,
                    OrderEvent::Entry(OrderEntry::FrameEnd { frame: 7, .. })
                )
            })
        })
        .count();
    assert_eq!(
        fresh,
        readers.len(),
        "repeats never take a fresh entry's place"
    );
    assert_eq!(
        server.rooms[&[5; 16]].order_packets.spent - spent,
        5,
        "repeats spend no fresh budget"
    );
}

#[test]
fn one_room_may_spend_past_half_the_relay_ordering_output() {
    fn spend(server: &mut RelayServer, packets: u64) {
        let room = server.rooms.get_mut(&[5; 16]).unwrap();
        assert!(server.order_packets.admit_part(
            &mut room.order_packets,
            unix(0),
            packets,
            INGRESS_PER_SECOND
        ));
        server
            .order_packets
            .charge_part(&mut room.order_packets, packets);
    }
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut reader, mut other) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut reader, &mut other] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    // A busy room may push more than half the total.
    spend(&mut server, INGRESS_PER_SECOND / 2);
    let mut bytes = Vec::new();
    reader.pull_ordering(1, &mut bytes).unwrap();
    assert!(
        server
            .process_at(endpoint(&reader), &bytes, unix(0), 0)
            .is_ok()
    );
    let left = ROOM_ORDER_PACKETS - server.rooms[&[5; 16]].order_packets.spent;
    spend(&mut server, left);
    reader.pull_ordering(1, &mut bytes).unwrap();
    assert!(
        matches!(
            server.process_at(endpoint(&reader), &bytes, unix(0), 0),
            Err(RelayError::RateLimited)
        ),
        "a room leaves the rest of the total to other matches"
    );
}

#[test]
fn renewed_push_replays_journal_batches_and_follows_a_rebound_hop() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut reader, mut sender) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1500);
    for peer in [&mut reader, &mut sender] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    subscribe(&mut server, &mut reader, 1, 0);
    let mut bytes = Vec::new();
    sender
        .test_submit(1, b"immutable pushed input", &mut bytes)
        .unwrap();
    events(&mut server, &mut sender, &bytes, 0);
    server.advance_clock(350, unix(350));
    let mut canonical = None;
    // The sender's UPLINK subscribed it too.
    let reader_endpoint = endpoint(&reader);
    for packet in server
        .drain_ordering_push(350, unix(350))
        .into_iter()
        .filter(|packet| packet.endpoint == reader_endpoint)
    {
        let payload = &packet.bytes[HEADER_LEN..packet.bytes.len() - HOP_TAG_LEN];
        if payload[0] == PACK_EVENT {
            canonical = canonical.or(pack::decode_pack(payload)
                .unwrap()
                .into_iter()
                .find(|entry| entry[0] == BATCH_EVENT));
        }
        for event in received(&mut reader, &packet.bytes) {
            if let OrderEvent::Entry(OrderEntry::Batch { bytes, .. }) = event {
                assert_eq!(bytes, b"immutable pushed input");
            }
        }
    }
    let canonical = canonical.expect("the original batch was pushed");
    server.advance_clock(250_000, unix(250_000));
    assert!(
        server.rooms[&[5; 16]]
            .ordering
            .as_ref()
            .unwrap()
            .hot_oldest_seq()
            > 1
    );
    renew(&mut server, &mut reader, 250_000);
    configure(&mut server, &mut reader, &cert, 250_000);
    assert!(
        server
            .drain_ordering_push(250_000, unix(250_000))
            .is_empty(),
        "new hop keys do not inherit an old subscription"
    );
    subscribe(&mut server, &mut reader, 1, 250_000);
    let replay = server.drain_ordering_push(250_000, unix(250_000));
    assert!(replay.iter().any(|packet| {
        let payload = &packet.bytes[HEADER_LEN..packet.bytes.len() - HOP_TAG_LEN];
        payload[0] == PACK_EVENT && pack::decode_pack(payload).unwrap().contains(&canonical)
    }));
    for packet in replay {
        reader.receive(&packet.bytes).unwrap();
    }

    // An authenticated datagram from a new address moves the hop
    // (compaction spec §6.2 "换地址").
    let rebound = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32000);
    reader.probe(&mut bytes).unwrap();
    let ready = server
        .process_at(rebound, &bytes, unix(250_001), 250_001)
        .unwrap()
        .remove(0);
    assert_eq!(ready.endpoint, rebound);
    reader.receive(&ready.bytes).unwrap();
    reader.ordering.as_mut().unwrap().link.schedule.recv_next = 9;
    reader.test_uplink(None, 250_001, &mut bytes).unwrap();
    for packet in server
        .process_at(rebound, &bytes, unix(250_001), 250_001)
        .unwrap()
    {
        reader.receive(&packet.bytes).unwrap();
    }
    let rebound_packets = server.drain_ordering_push(250_001, unix(250_001));
    assert!(!rebound_packets.is_empty());
    assert!(
        rebound_packets
            .iter()
            .all(|packet| packet.endpoint == rebound)
    );
    // The reader acknowledges the credit pushed, so the next pass reads disk.
    reader.ordering.as_mut().unwrap().link.schedule.recv_next = 73;
    reader.test_uplink(None, 250_001, &mut bytes).unwrap();
    server
        .process_at(rebound, &bytes, unix(250_001), 250_001)
        .unwrap();
    // Disk failure keeps the archive and becomes the existing terminal error.
    std::fs::OpenOptions::new()
        .write(true)
        .open(directory.0.join("05".repeat(16)).join("stream.index"))
        .unwrap()
        .set_len(0)
        .unwrap();
    let failure = server.drain_ordering_push(250_002, unix(250_002));
    assert!(failure.into_iter().any(|packet| matches!(
        reader.receive(&packet.bytes),
        Ok(ServerEvent::Ordering(OrderEvent::Error {
            code: OrderError::StorageUnavailable,
            ..
        }))
    )));
    assert!(
        server
            .drain_ordering_push(250_003, unix(250_003))
            .is_empty()
    );
    assert!(
        directory
            .0
            .join("05".repeat(16))
            .join("stream.data")
            .exists()
    );
}

#[test]
fn terminal_leave_survives_loss_expired_admission_and_deleted_room() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 2000);
    for participant in [&mut first, &mut second] {
        connect(&mut server, participant, 0);
        configure(&mut server, participant, &cert, 0);
    }
    server.advance_clock(1000, unix(1000));
    let mut leave = Vec::new();
    first.leave_ordering(&mut leave).unwrap();
    // Lose the first Leave, then let both hop tickets and admissions
    // expire. Silence shorter than the absence limit removes neither player.
    server.advance_clock(350_000, unix(350_000));
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 3);
    let mut forged = leave.clone();
    *forged.last_mut().unwrap() ^= 1;
    assert!(matches!(
        server.process_at(endpoint(&first), &forged, unix(350_000), 350_000),
        Err(RelayError::Authentication)
    ));
    // Past its ticket the hop takes only LEAVE (compaction spec §6.2).
    let mut bytes = Vec::new();
    first.test_submit(1, b"expired input", &mut bytes).unwrap();
    assert!(matches!(
        server.process_at(endpoint(&first), &bytes, unix(350_000), 350_000),
        Err(RelayError::Unregistered)
    ));
    first.probe(&mut bytes).unwrap();
    assert!(matches!(
        server.process_at(endpoint(&first), &bytes, unix(350_000), 350_000),
        Err(RelayError::Unregistered)
    ));
    first.forget_readiness();
    first.leave_ordering(&mut bytes).unwrap();
    assert_eq!(bytes, leave, "terminal retry preserves exact authority");
    let lost_ack = server
        .process_at(endpoint(&first), &leave, unix(350_000), 350_000)
        .unwrap();
    assert_eq!(lost_ack.len(), 1);
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 2);
    second.leave_ordering(&mut bytes).unwrap();
    server
        .process_at(endpoint(&second), &bytes, unix(351_000), 351_000)
        .unwrap();
    // Lose all ACKs until after the completed archive's existing grace.
    server.advance_clock(351_100, unix(351_100));
    server.advance_clock(652_000, unix(652_000));
    let room = directory.0.join("05".repeat(16));
    assert!(!room.exists());
    assert_eq!(server.room_count(), 0);
    let ack = server
        .process_at(endpoint(&first), &leave, unix(652_000), 652_000)
        .unwrap();
    assert!(matches!(
        first.receive(&ack[0].bytes),
        Ok(ServerEvent::Ordering(OrderEvent::Left))
    ));
    assert_eq!(server.room_count(), 0, "terminal ACK never recreates state");
    assert!(!room.exists());
    // An expired ticket's replays stop at the player's own budget instead
    // of spending the relay-wide order budget everyone's input needs.
    for _ in 1..PLAYER_HANDSHAKES_PER_SECOND {
        assert!(
            server
                .process_at(endpoint(&first), &leave, unix(652_000), 652_000)
                .is_ok()
        );
    }
    assert_eq!(
        server
            .process_at(endpoint(&first), &leave, unix(652_000), 652_000)
            .err(),
        Some(RelayError::RateLimited)
    );
}

#[test]
fn cancelled_start_reclaims_the_empty_archive_without_reopening_stale_tickets() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 16, &directory.0).unwrap();
    // Each cancellation frees its archive reservation at once, without
    // waiting for its tombstone's short-ticket grace to elapse. The first
    // loses its Ready; the second is a plain cancellation.
    for attempt in 1..=2 {
        let now_ms = u64::from(attempt) * 1000;
        let mut ticket = client(&key, 0).ticket;
        ticket.room = [attempt; 16];
        ticket.generation = u64::from(attempt);
        let mut participant = client_for(&key, ticket);
        connect(&mut server, &mut participant, now_ms);
        let cert = OrderingCertificate {
            room: ticket.room,
            sources_mask: 3,
            tick_hz: 45,
            expires: 2000,
            pacing: Default::default(),
            ordered_alone: false,
        }
        .sign(&key)
        .unwrap();
        let ready = if attempt == 1 {
            // Lose this source's first Ready while its peer creates the
            // room. Local retirement must still cancel that full quorum.
            let mut lost_ready = Vec::new();
            participant
                .configure_ordering(&cert, [7; 32], &mut lost_ready)
                .unwrap();
            let mut peer_ticket = client(&key, 1).ticket;
            peer_ticket.room = ticket.room;
            let mut peer = client_for(&key, peer_ticket);
            connect(&mut server, &mut peer, now_ms);
            let ready = configure(&mut server, &mut peer, &cert, now_ms);
            assert!(
                ready
                    .iter()
                    .any(|event| matches!(event, OrderEvent::Ready { ready_mask: 2, .. }))
            );
            ready
        } else {
            configure(&mut server, &mut participant, &cert, now_ms)
        };
        assert!(ready.iter().any(|event| matches!(event,
            OrderEvent::Status(status) if !status.started && status.head_frame == 0)));
        let room = directory.0.join(format!("{attempt:02x}").repeat(16));
        assert_eq!(
            std::fs::metadata(room.join("stream.data")).unwrap().len(),
            0
        );
        let mut bytes = Vec::new();
        participant.leave_ordering(&mut bytes).unwrap();
        assert!(
            events(&mut server, &mut participant, &bytes, now_ms)
                .iter()
                .any(|event| matches!(event, OrderEvent::Left))
        );
        server.advance_clock(now_ms, unix(now_ms));
        assert!(!room.exists(), "empty archive reservation must be reusable");
        // The hop that left takes only LEAVE (compaction spec §6.2).
        renew(&mut server, &mut participant, now_ms);
        assert!(matches!(
            configure(&mut server, &mut participant, &cert, now_ms).as_slice(),
            [OrderEvent::Error {
                code: OrderError::Inactive,
                ..
            }]
        ));
        assert!(!room.exists(), "stale Ready must not recreate the archive");
    }
    assert_eq!(
        server.room_count(),
        2,
        "inactive tombstones retain their grace"
    );
}

#[test]
fn a_newer_admission_cancels_the_unstarted_room_whose_leave_was_lost() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    // An HTTP cancellation can lose its final UDP Leave. A newer admission
    // of the same player cancels the unstarted room before the loading
    // timeout starts it without the peer that never arrived.
    let mut abandoned = client(&key, 0);
    connect(&mut server, &mut abandoned, 0);
    let abandoned_cert = certificate(&key, 3, 2000);
    configure(&mut server, &mut abandoned, &abandoned_cert, 0);
    let abandoned_path = directory.0.join("05".repeat(16));
    let superseded = LOADING_SILENCE_MS - 1;
    server.advance_clock(superseded, unix(superseded));
    assert!(abandoned_path.exists());
    let mut ticket = abandoned.ticket;
    ticket.room = [6; 16];
    ticket.generation = 2;
    ticket.expires = unix(superseded) + 200;
    let mut next = client_for(&key, ticket);
    connect(&mut server, &mut next, superseded);
    assert!(
        !abandoned_path.exists(),
        "new admission reclaims the lost Leave"
    );
    let mut bytes = Vec::new();
    abandoned
        .configure_ordering(&abandoned_cert, [7; 32], &mut bytes)
        .unwrap();
    // The newer generation released the old hop's session (compaction
    // spec §6.2).
    assert!(matches!(
        server.process_at(endpoint(&abandoned), &bytes, unix(superseded), superseded),
        Err(RelayError::Unregistered)
    ));
    assert!(!abandoned_path.exists());
    let mut cert = OrderingCertificate::parse(&abandoned_cert).unwrap();
    cert.room = ticket.room;
    assert!(
        configure(
            &mut server,
            &mut next,
            &cert.sign(&key).unwrap(),
            superseded
        )
        .iter()
        .any(|event| matches!(event, OrderEvent::Status(_)))
    );
    next.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut next, &bytes, superseded);
    let finished = superseded + MAX_TICKET_LIFETIME * 1000;
    server.advance_clock(finished, unix(finished));
    assert_eq!(server.room_count(), 0);
}

#[test]
fn disconnected_host_renews_after_ticket_expiry_and_replays_the_full_disk_stream() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut host, mut active, mut third) = (client(&key, 0), client(&key, 1), client(&key, 2));
    for client in [&mut host, &mut active, &mut third] {
        connect(&mut server, client, 0);
    }
    let cert = certificate(&key, 7, 2000);
    configure(&mut server, &mut host, &cert, 0);
    configure(&mut server, &mut active, &cert, 0);
    server.advance_clock(1000, unix(1000));
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 0);
    configure(&mut server, &mut third, &cert, 1000);
    // Reverse UDP order is canonicalized by source, including empty frames.
    let mut bytes = Vec::new();
    third.test_submit(1, b"third input", &mut bytes).unwrap();
    events(&mut server, &mut third, &bytes, 1000);
    active.test_submit(1, b"active input", &mut bytes).unwrap();
    events(&mut server, &mut active, &bytes, 1000);
    let received = pull(&mut server, &mut active, 1, 1300);
    let batch: Vec<_> = received
        .iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Batch {
                sequence,
                frame,
                source,
                bytes,
                ..
            }) => Some((*sequence, *frame, *source, bytes.as_slice())),
            _ => None,
        })
        .collect();
    assert_eq!(
        batch,
        [
            (1, 1, 1, b"active input".as_slice()),
            (2, 1, 2, b"third input".as_slice())
        ]
    );
    let ends: Vec<_> = received
        .iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                Some((*sequence, *frame))
            }
            _ => None,
        })
        .collect();
    // 100 ms at 45 Hz: one 3-frame turn.
    assert_eq!(ends, [(3, 1), (4, 2), (5, 3)]);
    active
        .test_submit(2, b"healthy input during outage", &mut bytes)
        .unwrap();
    events(&mut server, &mut active, &bytes, 30_000);
    server.advance_clock(250_000, unix(250_000));
    assert!(
        server.rooms[&[5; 16]].peers.is_empty(),
        "every short hop lease has expired"
    );
    let ordering = server.rooms[&[5; 16]].ordering.as_ref().unwrap();
    assert!(ordering.head > 4096);
    assert!(ordering.hot_oldest_seq() > 1);
    assert_eq!(ordering.active, 7);
    renew(&mut server, &mut active, 250_000);
    configure(&mut server, &mut active, &cert, 250_000);
    active
        .test_submit(3, b"healthy input after lease renewal", &mut bytes)
        .unwrap();
    let acked = events(&mut server, &mut active, &bytes, 250_000);
    assert!(matches!(acked[0], OrderEvent::Ack { batch_id: 3, frame, .. } if frame > 4096));
    // The host returns after >300 seconds, retains its original principal,
    // and starts from sequence1 even though the hot cache evicted it.
    server.advance_clock(350_000, unix(350_000));
    host.probe(&mut bytes).unwrap();
    assert!(matches!(
        server.process_at(endpoint(&host), &bytes, unix(350_000), 350_000),
        Err(RelayError::Unregistered)
    ));
    renew(&mut server, &mut host, 350_000);
    configure(&mut server, &mut host, &cert, 350_000);
    let mut cursor = 1;
    let mut last_frame = 0;
    let mut saw_old_input = false;
    let mut saw_new_input = false;
    let mut caught_up = false;
    let mut now_ms = 350_000;
    for attempt in 0..600 {
        now_ms += 25;
        if attempt == 100 {
            active
                .test_submit(4, b"healthy input while host catches up", &mut bytes)
                .unwrap();
            assert!(matches!(
                events(&mut server, &mut active, &bytes, now_ms)[0],
                OrderEvent::Ack { batch_id: 4, .. }
            ));
        }
        let replay = pull(&mut server, &mut host, cursor, now_ms);
        let OrderEvent::Status(status) = replay[0] else {
            panic!("full-history replay must remain available");
        };
        assert_eq!(status.oldest_seq, 1);
        assert_eq!(status.active_sources, 7);
        for event in &replay[1..] {
            match event {
                OrderEvent::Entry(OrderEntry::Batch {
                    sequence, bytes, ..
                }) => {
                    assert_eq!(*sequence, cursor);
                    cursor += 1;
                    let bytes = bytes.as_slice();
                    saw_old_input |= bytes == b"active input";
                    saw_new_input |= bytes == b"healthy input after lease renewal";
                }
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                    assert_eq!(*sequence, cursor);
                    cursor += 1;
                    assert_eq!(*frame, last_frame + 1);
                    last_frame = *frame;
                }
                _ => panic!("an outage is never a departure"),
            }
        }
        if cursor == status.next_seq {
            caught_up = true;
            break;
        }
    }
    assert!(caught_up && saw_old_input && saw_new_input);
    assert!(last_frame > 15_000);
    host.test_submit(1, b"recovered host input", &mut bytes)
        .unwrap();
    assert!(
        matches!(events(&mut server, &mut host, &bytes, now_ms)[0], OrderEvent::Ack { batch_id: 1, frame, .. } if frame > last_frame)
    );
    assert!(
        server.rooms[&[5; 16]]
            .ordering
            .as_ref()
            .unwrap()
            .history
            .len()
            <= MAX_HISTORY_ENTRIES
    );
}

#[test]
fn accepted_future_batches_deduplicate_and_renewal_recovers_a_lost_ack_from_own_stream() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    for client in [&mut first, &mut second] {
        connect(&mut server, client, 0);
    }
    let cert = certificate(&key, 3, 1300);
    configure(&mut server, &mut first, &cert, 0);
    configure(&mut server, &mut second, &cert, 0);
    let mut bytes = Vec::new();
    first
        .test_submit(1, b"private ordered command", &mut bytes)
        .unwrap();
    // Plaintext (compaction spec §3.1).
    let opaque = uplink_batches(&bytes).remove(0);
    assert_eq!(opaque, b"private ordered command");
    let accepted = server
        .process_at(endpoint(&first), &bytes, 1000, 0)
        .unwrap();
    assert_eq!(accepted.len(), 1); // deliberately lose the ACK
    first
        .test_submit(1, b"private ordered command", &mut bytes)
        .unwrap();
    assert_eq!(uplink_batches(&bytes), [opaque.clone()]);
    let duplicate = server
        .process_at(endpoint(&first), &bytes, 1000, 0)
        .unwrap();
    assert_eq!(duplicate.len(), 1);
    let mut modified = opaque.clone();
    *modified.last_mut().unwrap() ^= 1;
    first
        .encode(
            CONTROL,
            &raw_uplink(crate::link::FLAG_PUSH, 1, &[&modified]),
            &mut bytes,
        )
        .unwrap();
    assert!(matches!(
        events(&mut server, &mut first, &bytes, 0)[0],
        OrderEvent::Error {
            code: OrderError::ModifiedRetry,
            ..
        }
    ));
    first
        .encode(
            CONTROL,
            &raw_uplink(crate::link::FLAG_PUSH, 3, &[&modified]),
            &mut bytes,
        )
        .unwrap();
    assert!(matches!(
        events(&mut server, &mut first, &bytes, 0)[0],
        OrderEvent::Error {
            code: OrderError::BatchGap,
            expected: 2
        }
    ));
    let mut renewed_ticket = first.ticket;
    renewed_ticket.nonce = [42; 16];
    renewed_ticket.expires = 1250;
    let signed = renewed_ticket.sign(&key).unwrap();
    let mut candidate = first
        .renewal(
            &signed,
            *derive_packet_key(&key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
    assert!(candidate.ordering.is_none());
    connect(&mut server, &mut candidate, 300);
    candidate.adopt_ordering_from(&mut first).unwrap();
    assert!(first.ordering.is_none());
    first = candidate;
    let fresh_cert = certificate(&key, 3, 1400);
    configure(&mut server, &mut first, &fresh_cert, 300);
    first
        .test_submit(1, b"private ordered command", &mut bytes)
        .unwrap();
    assert_eq!(uplink_batches(&bytes), [opaque]);
    server
        .process_at(endpoint(&first), &bytes, 1000, 300)
        .unwrap(); // lose ACK again
    let entries = pull(&mut server, &mut first, 1, 300);
    assert_eq!(
        entries
            .iter()
            .filter(|event| matches!(
                event,
                OrderEvent::Entry(OrderEntry::Batch { batch_id: 1, .. })
            ))
            .count(),
        1
    );
    first.test_submit(2, b"next batch", &mut bytes).unwrap();
    assert!(matches!(
        events(&mut server, &mut first, &bytes, 300)[0],
        OrderEvent::Ack {
            batch_id: 2,
            frame: 4,
            ..
        }
    ));
    first.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut first, &bytes, 300);
    server.advance_clock(400, 1000);
    let tail = pull(&mut server, &mut second, 5, 400);
    let at_four: Vec<_> = tail
        .iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Batch { frame: 4, .. }) => Some("batch"),
            OrderEvent::Entry(OrderEntry::Departure {
                frame: 4,
                source: 0,
                ..
            }) => Some("departure"),
            _ => None,
        })
        .collect();
    assert_eq!(at_four, ["batch", "departure"]);
    second.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut second, &bytes, 400);
    server.advance_clock(600, unix(600));
    let room = directory.0.join("05".repeat(16));
    assert!(
        room.join("stream.data").exists(),
        "terminal replay retains the short-ticket grace"
    );
    server.advance_clock(301_000, unix(301_000));
    assert!(
        !room.exists(),
        "explicit-all-left retires only the completed journal"
    );
}

/// Spec §8: each row of the batch table, and the reply rules.
#[test]
fn uplink_batches_follow_the_relay_table_and_duplicates_still_get_an_ack() {
    let key = SecretKey::new([79; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1300);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
    }
    configure(&mut server, &mut first, &cert, 0);
    for (id, plaintext) in [(1, b"one"), (2, b"two"), (3, b"thr")] {
        assert_eq!(first.seal_batch(plaintext), Ok(id));
    }
    let batch = |client: &ClientSession, id: u64| {
        let ordering = client.ordering.as_ref().unwrap();
        ordering
            .sealed
            .iter()
            .find(|batch| batch.id == id)
            .unwrap()
            .opaque
            .clone()
    };
    let (one, two, three) = (batch(&first, 1), batch(&first, 2), batch(&first, 3));
    fn send(
        server: &mut RelayServer,
        client: &mut ClientSession,
        flags: u8,
        first_id: u64,
        batches: &[&[u8]],
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        let mut bytes = Vec::new();
        client
            .encode(CONTROL, &raw_uplink(flags, first_id, batches), &mut bytes)
            .unwrap();
        events(server, client, &bytes, now_ms)
    }
    let push = crate::link::FLAG_PUSH;
    let error = |events: &[OrderEvent]| match events {
        [OrderEvent::Error { code, expected }] => Some((*code, *expected)),
        _ => None,
    };
    let acked = |events: &[OrderEvent]| match events {
        [OrderEvent::Ack { batch_id, .. }] => Some(*batch_id),
        _ => None,
    };
    // Not all ready: no start is scheduled.
    let early = send(&mut server, &mut first, push, 1, &[&one], 0);
    assert_eq!(error(&early), Some((OrderError::NotStarted, 0)));
    configure(&mut server, &mut second, &cert, 0);
    assert!(
        send(&mut server, &mut first, push, 1, &[], 300).is_empty(),
        "no batch, no reply"
    );
    assert_eq!(
        acked(&send(&mut server, &mut first, push, 1, &[&one], 300)),
        Some(1)
    );
    assert_eq!(
        acked(&send(&mut server, &mut first, push, 1, &[&one], 300)),
        Some(1)
    );
    let changed = [&two[..]];
    assert_eq!(
        error(&send(&mut server, &mut first, push, 1, &changed, 300)),
        Some((OrderError::ModifiedRetry, 1))
    );
    assert_eq!(
        error(&send(&mut server, &mut first, push, 3, &[&three], 300)),
        Some((OrderError::BatchGap, 2))
    );
    // A duplicate prefix is skipped and the rest accepted in order.
    assert_eq!(
        acked(&send(
            &mut server,
            &mut first,
            push,
            1,
            &[&one, &two, &three],
            300
        )),
        Some(3)
    );
    assert_eq!(
        acked(&send(&mut server, &mut first, push, 1, &[&one], 300)),
        Some(3)
    );
    let probe = send(
        &mut server,
        &mut first,
        push | crate::link::FLAG_PROBE,
        1,
        &[],
        300,
    );
    assert!(matches!(probe[..], [OrderEvent::Status(_)]));
    let mut bytes = Vec::new();
    assert_eq!(second.seal_batch(b"gone"), Ok(1));
    let gone = batch(&second, 1);
    second.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut second, &bytes, 300);
    // The hop that left takes only LEAVE (compaction spec §6.2).
    second
        .encode(CONTROL, &raw_uplink(push, 1, &[&gone]), &mut bytes)
        .unwrap();
    assert_eq!(
        server
            .process_at(endpoint(&second), &bytes, unix(300), 300)
            .err(),
        Some(RelayError::Unregistered)
    );
    renew(&mut server, &mut second, 300);
    assert_eq!(
        error(&send(&mut server, &mut second, push, 1, &[&gone], 300)),
        Some((OrderError::Inactive, 0))
    );
}

#[test]
fn pipelined_batches_recover_reordering_and_lost_ack_without_duplicate_entries() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut sender, mut peer) = (client(&key, 0), client(&key, 1));
    for client in [&mut sender, &mut peer] {
        connect(&mut server, client, 0);
    }
    let cert = certificate(&key, 3, 1300);
    configure(&mut server, &mut sender, &cert, 0);
    configure(&mut server, &mut peer, &cert, 0);
    let mut bytes = Vec::new();
    sender.test_submit(1, b"checksum", &mut bytes).unwrap();
    sender.test_submit(2, b"move", &mut bytes).unwrap();
    assert!(matches!(
        events(&mut server, &mut sender, &bytes, 0)[0],
        OrderEvent::Error {
            code: OrderError::BatchGap,
            expected: 1
        }
    ));
    sender.test_submit(1, b"checksum", &mut bytes).unwrap();
    let lost_ack = server
        .process_at(endpoint(&sender), &bytes, 1000, 0)
        .unwrap();
    sender.test_submit(2, b"move", &mut bytes).unwrap();
    assert!(matches!(
        events(&mut server, &mut sender, &bytes, 0)[0],
        OrderEvent::Ack { batch_id: 2, .. }
    ));
    // A delayed ACK cannot move the accepted prefix backwards.
    sender.receive(&lost_ack[0].bytes).unwrap();
    assert_eq!(sender.ordering.as_ref().unwrap().accepted_through, 2);
    // An older batch is skipped and its UPLINK still gets the ACK (§8).
    sender.test_submit(1, b"checksum", &mut bytes).unwrap();
    assert!(matches!(
        events(&mut server, &mut sender, &bytes, 0)[0],
        OrderEvent::Ack { batch_id: 2, .. }
    ));
    server.advance_clock(400, 1000);
    let batches: Vec<_> = pull(&mut server, &mut peer, 1, 400)
        .into_iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Batch {
                batch_id, bytes, ..
            }) => Some((batch_id, bytes.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(batches, [(1, b"checksum".to_vec()), (2, b"move".to_vec())]);
}

#[test]
fn full_submission_window_survives_renewal_with_immutable_retry_bytes() {
    let key = SecretKey::new([74; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut sender, mut peer) = (client(&key, 0), client(&key, 1));
    for client in [&mut sender, &mut peer] {
        connect(&mut server, client, 0);
    }
    let cert = certificate(&key, 3, 1300);
    configure(&mut server, &mut sender, &cert, 0);
    configure(&mut server, &mut peer, &cert, 0);
    let mut bytes = Vec::new();
    let mut sealed = Vec::new();
    for id in 1..=4 {
        sender.test_submit(id, &[id as u8], &mut bytes).unwrap();
        sealed.push(uplink_batches(&bytes).remove(0));
    }
    assert_eq!(
        sender.test_submit(5, b"fifth", &mut bytes),
        Err(RelayError::Full)
    );
    renew(&mut server, &mut sender, 1000);
    let mut final_ack = None;
    for id in 1..=4 {
        sender.test_submit(id, &[id as u8], &mut bytes).unwrap();
        assert_eq!(uplink_batches(&bytes), [sealed[(id - 1) as usize].clone()]);
        final_ack = Some(
            server
                .process_at(endpoint(&sender), &bytes, 1001, 1000)
                .unwrap(),
        );
    }
    // Only the final receipt arrives; strict server admission proves all
    // four predecessors, and one new slot becomes available immediately.
    sender.receive(&final_ack.unwrap()[0].bytes).unwrap();
    sender.test_submit(5, b"fifth", &mut bytes).unwrap();
    assert!(matches!(
        events(&mut server, &mut sender, &bytes, 1000)[0],
        OrderEvent::Ack { batch_id: 5, .. }
    ));
    assert_eq!(sender.ordering.as_ref().unwrap().sealed.len(), 4);
}

#[test]
fn archive_quota_admits_rooms_by_reserved_bytes() {
    let directory = TestDirectory::new();
    crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now()).unwrap();
    // A journal left by an earlier process holds only its size on disk.
    let leftover = directory.0.join("0b".repeat(16));
    std::fs::create_dir(&leftover).unwrap();
    std::fs::write(leftover.join("stream.data"), [0; 64]).unwrap();
    let mut journals =
        crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now()).unwrap();
    assert_eq!(
        journals.create([11; 16], 45, 2).err().unwrap().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    let mut admitted = Vec::new();
    let full = loop {
        match journals.create([0x80 + admitted.len() as u8; 16], 45, 2) {
            Ok(journal) => admitted.push(journal),
            Err(error) => break error,
        }
    };
    assert_eq!(full.kind(), std::io::ErrorKind::StorageFull);
    // Each two-player 45 FPS room reserves its first hour of clock
    // records and 32 MiB of commands per player, and leaves its next
    // hour free: not a whole day of clock or a fixed eighth of the quota.
    assert_eq!(admitted.len(), 114);
    // A finished room keeps only the bytes it wrote.
    journals.settle(&mut admitted[0], 0);
    assert!(journals.create([0x70; 16], 45, 2).is_ok());
}

/// A journaled two-player 10 FPS match, started at 200 ms.
fn slow_match(directory: &TestDirectory) -> RelayServer {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let cert = OrderingCertificate {
        room: [5; 16],
        sources_mask: 3,
        tick_hz: 10,
        expires: 2000,
        pacing: Default::default(),
        ordered_alone: false,
    }
    .sign(&key)
    .unwrap();
    for source in 0..2 {
        let mut player = client(&key, source);
        connect(&mut server, &mut player, 0);
        configure(&mut server, &mut player, &cert, 0);
    }
    server
}

/// Runs the match five minutes at a time, both players in contact.
fn play(server: &mut RelayServer, from_ms: u64, until_ms: u64) {
    for now_ms in (from_ms..=until_ms).step_by(300_000) {
        let room = server.rooms.get_mut(&[5; 16]).unwrap();
        for source in 0..2 {
            let ordering = room.ordering.as_mut().unwrap();
            ordering.heard(source, ([source + 1; 16], 1), now_ms);
        }
        server.advance_clock(now_ms, unix(now_ms));
    }
}

#[test]
fn a_journal_reserves_the_next_hour_of_clock_as_the_match_runs() {
    let directory = TestDirectory::new();
    let mut server = slow_match(&directory);
    play(&mut server, 0, 3_900_000);
    assert!(!ordering(&server).storage_failed);
    assert_eq!(ordering(&server).head, 38_998);
}

#[test]
fn a_full_relay_refuses_new_rooms_and_a_running_one_still_takes_its_next_hour() {
    let directory = TestDirectory::new();
    let mut server = slow_match(&directory);
    // Other rooms fill the relay until it refuses the smallest one.
    let journals = server.journal_directory.as_mut().unwrap();
    let mut others = Vec::new();
    for (tick_hz, sources) in [(45, 2), (10, 0)] {
        let refused = loop {
            let mut room = [0x80; 16];
            room[..8].copy_from_slice(&(others.len() as u64).to_le_bytes());
            match journals.create(room, tick_hz, sources) {
                Ok(journal) => others.push(journal),
                Err(error) => break error,
            }
        };
        assert_eq!(refused.kind(), std::io::ErrorKind::StorageFull);
    }
    play(&mut server, 0, 3_900_000);
    assert!(!ordering(&server).storage_failed);
    assert_eq!(ordering(&server).head, 38_998);
}

#[test]
fn a_started_match_outlives_its_certificate_until_its_players_go_silent() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1300);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    server.advance_clock(301_000, unix(301_000));
    let archive = directory.0.join("05".repeat(16));
    assert!(archive.exists() && ordering(&server).head > 300 * 45);
    // Silence still ends the abandoned match and frees its archive.
    server.advance_clock(ABSENT_MS, unix(ABSENT_MS));
    let freed = ABSENT_MS + MAX_TICKET_LIFETIME * 1000;
    server.advance_clock(freed, unix(freed));
    assert!(!archive.exists());
}

#[test]
fn flooding_matches_cannot_take_the_ordering_output_another_match_steadily_uses() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 16);
    // Room 1's one steady player waits for a second player, so each Pull
    // returns one status. Rooms 2 and 3 hold four flooders each, and ten
    // idle rooms leave the steady player more than an equal share.
    let mut players = Vec::new();
    let rooms = [(1, 1, 0b11), (2, 4, 0b1111), (3, 4, 0b1111)];
    for (room, sources, sources_mask) in rooms.into_iter().chain((4..14).map(|room| (room, 1, 1))) {
        let cert = OrderingCertificate {
            room: [room; 16],
            sources_mask,
            tick_hz: 45,
            expires: 1400,
            pacing: Default::default(),
            ordered_alone: false,
        }
        .sign(&key)
        .unwrap();
        for source in 0..sources {
            let mut player = client_for(
                &key,
                Ticket {
                    room: [room; 16],
                    source,
                    expires: 1200,
                    nonce: [room; 16],
                    player: [room * 16 + source; 16],
                    generation: 1,
                },
            );
            connect(&mut server, &mut player, 0);
            if room < 4 {
                configure(&mut server, &mut player, &cert, 0);
                players.push(player);
            }
        }
    }
    let pull_at = |server: &mut RelayServer, player: &mut ClientSession, now_ms| {
        let mut bytes = Vec::new();
        player.pull_ordering(1, &mut bytes).unwrap();
        server
            .process_at(endpoint(player), &bytes, unix(now_ms), now_ms)
            .is_ok()
    };
    let (steady, flooders) = players.split_first_mut().unwrap();
    assert!((0..1500).all(|_| pull_at(&mut server, steady, 1000)));
    // The next second, the flooders pull until the relay refuses them.
    for flooder in flooders {
        while pull_at(&mut server, flooder, 2000) {}
    }
    assert!(
        (0..1500).all(|_| pull_at(&mut server, steady, 2000)),
        "the steady match keeps what it used the second before"
    );
}

#[test]
fn leave_refuses_a_ticket_older_than_any_room_it_named() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut player = client(&key, 0);
    connect(&mut server, &mut player, 0);
    player
        .configure_ordering(&certificate(&key, 1, 1400), [7; 32], &mut Vec::new())
        .unwrap();
    let mut leave = Vec::new();
    player.leave_ordering(&mut leave).unwrap();
    let last = player.ticket.expires + LEAVE_GRACE - 1;
    assert!(
        server
            .process_at(endpoint(&player), &leave, last, last * 1000)
            .is_ok()
    );
    assert_eq!(
        server
            .process_at(endpoint(&player), &leave, last + 1, (last + 1) * 1000)
            .err(),
        Some(RelayError::Unregistered)
    );
}

#[test]
fn newer_admission_departs_the_player_from_an_older_started_match() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1400);
    for peer in [&mut first, &mut second] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    server.advance_clock(300, unix(300));
    // The lobby admits the first player to its next match; the old
    // generation can no longer send Leave for itself.
    let mut ticket = first.ticket;
    ticket.room = [6; 16];
    ticket.generation = 2;
    let mut next = client_for(&key, ticket);
    connect(&mut server, &mut next, 300);
    server.advance_clock(400, unix(400));
    let departed: Vec<_> = pull(&mut server, &mut second, 1, 400)
        .into_iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Departure { source, .. }) => Some(source),
            _ => None,
        })
        .collect();
    assert_eq!(departed, [0]);
}

#[test]
fn ready_refuses_a_source_outside_the_certificate_and_a_forged_certificate() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let cert = certificate(&key, 3, 1300);
    let mut bytes = Vec::new();
    // A registered source outside the signed manifest cannot join the
    // ready quorum even with a legitimate hop ticket for this room.
    let mut outsider = client(&key, 2);
    connect(&mut server, &mut outsider, 0);
    assert!(matches!(
        outsider.configure_ordering(&cert, [7; 32], &mut bytes),
        Err(RelayError::Authentication)
    ));
    let mut first = client(&key, 0);
    connect(&mut server, &mut first, 0);
    let mut forged = cert.clone();
    forged[34] ^= 1;
    first
        .configure_ordering(&forged, [7; 32], &mut bytes)
        .unwrap();
    assert!(matches!(
        server.process_at(endpoint(&first), &bytes, 1000, 0),
        Err(RelayError::Authentication)
    ));
    assert!(
        OrderingCertificate {
            sources_mask: 1 << MAX_PEERS,
            ..OrderingCertificate::parse(&cert).unwrap()
        }
        .sign(&key)
        .is_err()
    );
}

#[test]
fn archive_restart_and_storage_failure_preserve_the_committed_stream() {
    let directory = TestDirectory::new();
    let (mut server, mut first, mut second) = started_journal_room(&directory);
    let key = server.key.clone();
    let cert = certificate(&key, 3, 1300);
    let mut bytes = Vec::new();
    first
        .test_submit(1, b"authenticated plaintext", &mut bytes)
        .unwrap();
    events(&mut server, &mut first, &bytes, 0);
    second.pull_ordering(1, &mut bytes).unwrap();
    let packets = server
        .process_at(endpoint(&second), &bytes, 1000, 300)
        .unwrap();
    // Batches travel plaintext (compaction spec §3): what the relay
    // committed is what every reader gets.
    assert!(packets.iter().any(|packet| {
        matches!(
            received(&mut second, &packet.bytes)[..],
            [OrderEvent::Entry(OrderEntry::Batch { ref bytes, .. }), ..]
                if bytes == b"authenticated plaintext"
        )
    }));
    server.advance_clock(112_000, unix(112_000));
    let replay = pull(&mut server, &mut second, 1, 112_000);
    assert!(matches!(
        replay[0],
        OrderEvent::Status(OrderStatus { oldest_seq: 1, .. })
    ));
    assert!(matches!(
        replay[1],
        OrderEvent::Entry(OrderEntry::Batch {
            sequence: 1,
            frame: 1,
            ..
        })
    ));
    assert!(
        server.rooms[&[5; 16]]
            .ordering
            .as_ref()
            .unwrap()
            .history
            .len()
            <= MAX_HISTORY_ENTRIES
    );
    assert!(server.ordering_memory() <= GLOBAL_ORDERING_BYTES);
    let cursor = server.rooms[&[5; 16]].ordering.as_ref().unwrap().next_seq;
    assert!(
        matches!(pull(&mut server, &mut second, cursor + 1, 112_000)[0], OrderEvent::Error { code: OrderError::InvalidCursor, expected } if expected == cursor)
    );
    let room = directory.0.join("05".repeat(16));
    let before = std::fs::read(room.join("stream.data")).unwrap();
    // Restart migration is unsupported: an existing journal is never
    // overwritten, even by a valid freshly connected principal.
    let mut restarted = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let mut newcomer = client(&key, 0);
    connect(&mut restarted, &mut newcomer, 0);
    assert!(matches!(
        configure(&mut restarted, &mut newcomer, &cert, 0)[0],
        OrderEvent::Error {
            code: OrderError::StorageUnavailable,
            ..
        }
    ));
    assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
    // A broken offset index yields a storage failure, not a fabricated
    // history gap or implicit departure. Preserve the data for inspection.
    std::fs::OpenOptions::new()
        .write(true)
        .open(room.join("stream.index"))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(matches!(
        pull(&mut server, &mut second, 1, 112_000)[0],
        OrderEvent::Error {
            code: OrderError::StorageUnavailable,
            ..
        }
    ));
    let head = server.rooms[&[5; 16]].ordering.as_ref().unwrap().head;
    server.advance_clock(113_000, unix(113_000));
    assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, head);
    assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
}

/// Past the reservation estimate every room is counted: reservations
/// that hold nothing refuse no batch, and bytes no reservation covered
/// are found by the next pass, which evicts before the next batch is
/// refused.
#[test]
fn ordering_reservations_past_the_estimate_count_every_room() {
    let key = SecretKey::new([73; 32]);
    let (mut server, mut first, _second) = paced_room(&key);
    let mut submit = |server: &mut RelayServer, id, now_ms| {
        let mut bytes = Vec::new();
        first.test_submit(id, b"input", &mut bytes).unwrap();
        events(server, &mut first, &bytes, now_ms).remove(0)
    };
    server.ordering_estimate = GLOBAL_ORDERING_BYTES;
    assert!(matches!(
        submit(&mut server, 1, 0),
        OrderEvent::Ack { batch_id: 1, .. }
    ));
    server
        .rooms
        .get_mut(&[5; 16])
        .unwrap()
        .ordering
        .as_mut()
        .unwrap()
        .sources[1]
        .last_batch = Some((0, 0, vec![0; GLOBAL_ORDERING_BYTES]));
    server.advance_clock(1000, unix(1000));
    assert!(ordering(&server).history.is_empty());
    assert!(matches!(
        submit(&mut server, 2, 1000),
        OrderEvent::Error {
            code: OrderError::Backpressure,
            expected: 2
        }
    ));
}

fn started_journal_room(directory: &TestDirectory) -> (RelayServer, ClientSession, ClientSession) {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 1300);
    for client in [&mut first, &mut second] {
        connect(&mut server, client, 0);
        configure(&mut server, client, &cert, 0);
    }
    (server, first, second)
}

#[test]
fn a_pull_past_its_senders_budget_is_refused_before_it_reads() {
    let directory = TestDirectory::new();
    let (mut server, _first, mut second) = started_journal_room(&directory);
    server.advance_clock(112_000, unix(112_000));
    for _ in 0..PLAYER_ARCHIVE_PULLS_PER_SECOND {
        assert!(matches!(
            pull(&mut server, &mut second, 1, 112_000)[0],
            OrderEvent::Status(_)
        ));
    }
    // A read now finds a broken index and fails the room's storage.
    std::fs::OpenOptions::new()
        .write(true)
        .open(directory.0.join("05".repeat(16)).join("stream.index"))
        .unwrap()
        .set_len(0)
        .unwrap();
    let mut bytes = Vec::new();
    second.pull_ordering(1, &mut bytes).unwrap();
    assert_eq!(
        server
            .process_at(endpoint(&second), &bytes, unix(112_000), 112_000)
            .err(),
        Some(RelayError::RateLimited)
    );
    assert!(!ordering(&server).storage_failed);
}

#[test]
fn archive_pages_read_back_records_of_any_length_up_to_the_last() {
    let directory = TestDirectory::new();
    let (mut server, mut first, _second) = started_journal_room(&directory);
    let mut bytes = Vec::new();
    first
        .test_submit(1, b"longer than a frame end", &mut bytes)
        .unwrap();
    events(&mut server, &mut first, &bytes, 0);
    server.advance_clock(3000, unix(3000));
    let room = server.rooms.get_mut(&[5; 16]).unwrap();
    let ordering = room.ordering.as_mut().unwrap();
    let committed: Vec<_> = ordering
        .history
        .iter()
        .map(|entry| entry.bytes.clone())
        .collect();
    assert!(committed.len() > ORDERING_PULL_LIMIT);
    while ordering.pop_oldest() {}
    assert_eq!(ordering.full_history(), committed);
}

#[test]
fn a_player_silent_for_ten_minutes_departs_through_the_stream() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut present, mut absent) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 2000);
    for participant in [&mut present, &mut absent] {
        connect(&mut server, participant, 0);
        configure(&mut server, participant, &cert, 0);
    }
    // Only one player keeps contact, renewing its short hop ticket.
    let mut bytes = Vec::new();
    for now_ms in (100_000..ABSENT_MS).step_by(100_000) {
        renew(&mut server, &mut present, now_ms);
        present.probe(&mut bytes).unwrap();
        server
            .process_at(endpoint(&present), &bytes, unix(now_ms), now_ms)
            .unwrap();
    }
    server.advance_clock(ABSENT_MS - 1, unix(ABSENT_MS - 1));
    assert_eq!(
        ordering(&server).active,
        3,
        "a shorter outage keeps its place"
    );
    server.advance_clock(ABSENT_MS, unix(ABSENT_MS));
    assert_eq!(ordering(&server).active, 1);
    server.advance_clock(ABSENT_MS + 1000, unix(ABSENT_MS + 1000));
    assert!(
        ordering(&server)
            .history
            .iter()
            .any(|entry| entry.bytes[0] == DEPARTURE_EVENT && entry.bytes[13..] == [1, 1]),
        "source 1 departs with connection lost"
    );
}

/// The silent sources in the STATUS answering `player`'s ordering probe.
pub(super) fn told_silent(
    server: &mut RelayServer,
    player: &mut ClientSession,
    now_ms: u64,
) -> u32 {
    let mut bytes = Vec::new();
    let probe = raw_uplink(crate::link::FLAG_PROBE, 1, &[]);
    player.encode(CONTROL, &probe, &mut bytes).unwrap();
    events(server, player, &bytes, now_ms)
        .into_iter()
        .find_map(|event| match event {
            OrderEvent::Status(status) => Some(status.silent_sources),
            _ => None,
        })
        .expect("a STATUS")
}

#[test]
fn a_player_silent_for_ten_seconds_is_named_to_the_others_until_heard_again() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 2000);
    for player in [&mut first, &mut second] {
        connect(&mut server, player, 0);
        configure(&mut server, player, &cert, 0);
    }
    let start = ordering(&server).starts_at.unwrap();
    assert_eq!(told_silent(&mut server, &mut second, start), 0);
    let silent_at = start + SILENT_NOTICE_MS;
    assert_eq!(told_silent(&mut server, &mut first, silent_at - 1), 0);
    assert_eq!(told_silent(&mut server, &mut first, silent_at), 0b10);
    // Any packet ends it: here the silent player's own probe.
    assert_eq!(told_silent(&mut server, &mut second, silent_at + 1), 0);
    assert_eq!(told_silent(&mut server, &mut first, silent_at + 2), 0);
}

#[test]
fn players_who_never_finish_loading_depart_in_the_first_frame() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut players: Vec<_> = (0..3).map(|source| client(&key, source)).collect();
    for player in &mut players {
        connect(&mut server, player, 0);
    }
    let cert = certificate(&key, 0b111, 2000);
    // The second player's game crashed while loading, before the first
    // became ready: its silence counts from its last packet. The third
    // still answers but never becomes ready.
    let ready_at = 10_000;
    let mut bytes = Vec::new();
    for now_ms in (ready_at..ready_at + LOADING_LIMIT_MS).step_by(10_000) {
        players[2].probe(&mut bytes).unwrap();
        server
            .process_at(endpoint(&players[2]), &bytes, unix(now_ms), now_ms)
            .unwrap();
        if now_ms == ready_at {
            configure(&mut server, &mut players[0], &cert, now_ms);
        }
        server.advance_clock(now_ms, unix(now_ms));
        let expected = if now_ms < LOADING_SILENCE_MS {
            0b111
        } else {
            0b101
        };
        assert_eq!(ordering(&server).active, expected, "at {now_ms} ms");
        assert!(ordering(&server).starts_at.is_none());
    }
    let limit = ready_at + LOADING_LIMIT_MS;
    server.advance_clock(limit, unix(limit));
    assert_eq!(ordering(&server).active, 1);
    let first = pull(&mut server, &mut players[0], 1, limit + 1000);
    let departed: Vec<_> = first
        .iter()
        .filter_map(|event| match event {
            OrderEvent::Entry(OrderEntry::Departure {
                frame: 1,
                source,
                lost: true,
                ..
            }) => Some(*source),
            _ => None,
        })
        .collect();
    assert_eq!(departed, [1, 2]);
}

#[test]
fn ready_accepts_a_renewed_expiry_and_refuses_a_different_pacing() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    connect(&mut server, &mut first, 0);
    connect(&mut server, &mut second, 0);
    let cert = OrderingCertificate {
        pacing: CertificatePacing::On,
        ..OrderingCertificate::parse(&certificate(&key, 3, 2000)).unwrap()
    };
    configure(&mut server, &mut first, &cert.sign(&key).unwrap(), 1000);
    let renewed = OrderingCertificate {
        expires: 2100,
        ..cert
    };
    let retry = configure(&mut server, &mut first, &renewed.sign(&key).unwrap(), 1500);
    assert!(matches!(retry.first(), Some(OrderEvent::Ready { .. })));
    let different = OrderingCertificate {
        pacing: CertificatePacing::Off,
        ..renewed
    };
    let reply = configure(
        &mut server,
        &mut second,
        &different.sign(&key).unwrap(),
        2000,
    );
    assert!(matches!(
        reply.as_slice(),
        [OrderEvent::Error {
            code: OrderError::AgreementMismatch,
            ..
        }]
    ));
}

#[test]
fn a_loading_players_different_ready_refuses_the_start_for_everyone() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let mut players: Vec<_> = (0..2).map(|source| client(&key, source)).collect();
    for player in &mut players {
        connect(&mut server, player, 0);
    }
    let cert = certificate(&key, 0b11, 2000);
    configure(&mut server, &mut players[0], &cert, 1000);
    let ready = |player: &mut ClientSession, agreement| {
        let mut bytes = Vec::new();
        player
            .configure_ordering(&cert, agreement, &mut bytes)
            .unwrap();
        bytes
    };
    let refused = |events: Vec<OrderEvent>| {
        matches!(
            events.as_slice(),
            [OrderEvent::Error {
                code: OrderError::AgreementMismatch,
                ..
            }]
        )
    };
    // Games resend Ready until the start.
    let other = ready(&mut players[1], [8; 32]);
    assert!(refused(events(&mut server, &mut players[1], &other, 2000)));
    // The first Ready's player no longer waits, and both retries agree.
    let first = ready(&mut players[0], [7; 32]);
    assert!(refused(events(&mut server, &mut players[0], &first, 3000)));
    let other = ready(&mut players[1], [8; 32]);
    assert!(refused(events(&mut server, &mut players[1], &other, 3000)));
    // Nobody starts alone once the refused player goes silent.
    let late = 3000 + LOADING_LIMIT_MS;
    server.advance_clock(late, unix(late));
    assert!(ordering(&server).starts_at.is_none());
    assert_eq!(server.take_ordering_counts().start_mismatches, 1);
}

#[test]
fn a_leave_before_any_ready_keeps_the_start_closed_once_its_room_is_gone() {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let cert = certificate(&key, 3, 2000);
    // The first player quits while loading; its Ready never arrived.
    let mut leaver = client(&key, 0);
    connect(&mut server, &mut leaver, 0);
    let mut bytes = Vec::new();
    leaver
        .configure_ordering(&cert, [7; 32], &mut bytes)
        .unwrap();
    leaver.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut leaver, &bytes, 0);
    // Its ticket expires, and the room with it.
    let expired = (leaver.ticket.expires - unix(0)) * 1000;
    server.advance_clock(expired, unix(expired));
    assert_eq!(server.room_count(), 0);
    // The other player arrives later on a renewed ticket.
    let mut ticket = client(&key, 1).ticket;
    ticket.expires = unix(expired) + 200;
    let mut late = client_for(&key, ticket);
    connect(&mut server, &mut late, expired);
    assert!(matches!(
        configure(&mut server, &mut late, &cert, expired).as_slice(),
        [OrderEvent::Error {
            code: OrderError::Inactive,
            ..
        }]
    ));
}

#[test]
fn a_late_returner_cannot_restart_a_cleaned_up_match_under_its_id() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let cert = certificate(&key, 3, 2000);
    for source in 0..2 {
        let mut player = client(&key, source);
        connect(&mut server, &mut player, 0);
        configure(&mut server, &mut player, &cert, 0);
        let mut bytes = Vec::new();
        player.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut player, &bytes, 1000);
    }
    server.advance_clock(1000, unix(1000));
    let cleaned = 1000 + MAX_TICKET_LIFETIME * 1000;
    server.advance_clock(cleaned, unix(cleaned));
    assert!(!directory.0.join("05".repeat(16)).exists());
    // The first player returns with a fresh ticket and its certificate.
    let mut returner = client_for(
        &key,
        Ticket {
            room: [5; 16],
            source: 0,
            expires: unix(cleaned) + 200,
            nonce: [9; 16],
            player: [1; 16],
            generation: 1,
        },
    );
    connect(&mut server, &mut returner, cleaned);
    assert!(matches!(
        configure(&mut server, &mut returner, &cert, cleaned).as_slice(),
        [OrderEvent::Error {
            code: OrderError::Inactive,
            ..
        }]
    ));
    assert!(!directory.0.join("05".repeat(16)).exists());
    // The marker outlasts any certificate verified before the cleanup.
    let expiry = unix(cleaned) + MAX_MATCH_SECONDS;
    server.advance_clock(1_000_000, expiry - 1);
    assert!(server.closed_startups.contains(&[5; 16]));
    server.advance_clock(1_000_000, expiry);
    assert!(!server.closed_startups.contains(&[5; 16]));
}

/// A started journaled match whose archive fails at 112 seconds, when a
/// pull from the start, past the hot cache, reads a broken index.
fn failed_match(directory: &TestDirectory) -> (RelayServer, ClientSession) {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let (mut first, mut second) = (client(&key, 0), client(&key, 1));
    let cert = certificate(&key, 3, 2000);
    for participant in [&mut first, &mut second] {
        connect(&mut server, participant, 0);
        configure(&mut server, participant, &cert, 0);
    }
    server.advance_clock(112_000, unix(112_000));
    std::fs::OpenOptions::new()
        .write(true)
        .open(directory.0.join("05".repeat(16)).join("stream.index"))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(matches!(
        pull(&mut server, &mut second, 1, 112_000)[0],
        OrderEvent::Error {
            code: OrderError::StorageUnavailable,
            ..
        }
    ));
    server.advance_clock(112_000, unix(112_000));
    (server, first)
}

#[test]
fn leave_after_a_journal_failure_is_acknowledged_and_keeps_the_archive() {
    let directory = TestDirectory::new();
    let (mut server, mut first) = failed_match(&directory);
    let room = directory.0.join("05".repeat(16));
    let before = std::fs::read(room.join("stream.data")).unwrap();
    // The client waits for this acknowledgement before it may join
    // another match.
    let mut bytes = Vec::new();
    first.leave_ordering(&mut bytes).unwrap();
    assert!(matches!(
        events(&mut server, &mut first, &bytes, 112_000).as_slice(),
        [OrderEvent::Left]
    ));
    assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
}

#[test]
fn a_failed_archive_frees_its_room_once_every_ticket_expired_and_keeps_its_files() {
    let directory = TestDirectory::new();
    let (mut server, _) = failed_match(&directory);
    let room = directory.0.join("05".repeat(16));
    let before = std::fs::read(room.join("stream.data")).unwrap();
    // Both hop tickets expire at 1200, 200 seconds in.
    server.advance_clock(199_000, unix(199_000));
    assert!(server.rooms.contains_key(&[5; 16]));
    server.advance_clock(200_000, unix(200_000));
    assert!(!server.rooms.contains_key(&[5; 16]));
    assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
    // Its match ID stays taken, holding only its bytes on disk.
    let on_disk = [
        room.clone(),
        room.join("stream.data"),
        room.join("stream.index"),
    ]
    .iter()
    .map(|path| std::fs::metadata(path).unwrap().len())
    .sum::<u64>();
    let journals = server.journal_directory.as_ref().unwrap();
    assert_eq!(journals.reserved(&[5; 16]), Some(on_disk));
    assert_eq!(journals.total(), on_disk);
}

#[test]
fn a_failed_archive_frees_its_room_ten_minutes_on_while_a_player_renews() {
    let directory = TestDirectory::new();
    let (mut server, mut first) = failed_match(&directory);
    let freed = 112_000 + ABSENT_MS;
    for now_ms in (150_000..freed).step_by(100_000) {
        renew(&mut server, &mut first, now_ms);
    }
    server.advance_clock(freed - 1, unix(freed - 1));
    assert!(server.rooms[&[5; 16]].ordering.is_some());
    server.advance_clock(freed, unix(freed));
    assert!(server.rooms[&[5; 16]].ordering.is_none());
    // A renewed Ready cannot start a new timeline under its match ID: the
    // ended-match marker answers while the certificate lasts, the files
    // after it.
    let cert = certificate(&server.key, 3, 2000);
    assert!(matches!(
        configure(&mut server, &mut first, &cert, freed)[0],
        OrderEvent::Error {
            code: OrderError::Inactive,
            ..
        }
    ));
}

/// A burst merge, such as a healed federation link's backlog, draws later
/// hours instead of failing the room, and nothing stays counted after it.
#[test]
fn a_burst_of_frame_ends_draws_later_hours_and_releases_them() {
    let directory = TestDirectory::new();
    let mut journals =
        crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now()).unwrap();
    // One frame a second makes an hour 3600 frame ends of 25 bytes.
    let mut journal = journals.create([0x0d; 16], 1, 2).unwrap();
    let (total, growth) = (journals.total(), journals.growth());
    let mut frame_end = vec![0; 13];
    frame_end[0] = FRAME_END_EVENT;
    journal.commit(&vec![frame_end; 3700]).unwrap();
    journals.reserve_frames(&mut journal);
    assert_eq!(journals.total(), total + 3600 * 25);
    assert_eq!(journals.growth(), growth);
    journals.settle(&mut journal, 0);
    assert_eq!(journals.growth(), 0);
    journals.cleanup(journal, false);
    assert_eq!(journals.total(), 0);
}

#[test]
fn startup_sets_aside_only_leftovers_of_ended_matches() {
    let directory = TestDirectory::new();
    let now = std::time::SystemTime::now();
    crate::journal::JournalDirectory::open(&directory.0, now).unwrap();
    for room in ["0b", "0c"] {
        let leftover = directory.0.join(room.repeat(16));
        std::fs::create_dir(&leftover).unwrap();
        std::fs::write(leftover.join("stream.data"), [0; 64]).unwrap();
    }
    // The second was written until an hour after the first.
    let hour = std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(directory.0.join("0c".repeat(16)).join("stream.data"))
        .unwrap()
        .set_modified(now + hour)
        .unwrap();
    let ended = std::time::Duration::from_millis(ROOM_LIFETIME_MS + MAX_TICKET_LIFETIME * 1000);
    let mut journals =
        crate::journal::JournalDirectory::open(&directory.0, now + ended + hour / 2).unwrap();
    assert_eq!(
        std::fs::read(
            directory
                .0
                .join("archive")
                .join("0b".repeat(16))
                .join("stream.data")
        )
        .unwrap(),
        [0; 64]
    );
    // Only the recent leftover still counts against the quota.
    assert_eq!(journals.reserved(&[0x0b; 16]), None);
    assert_eq!(Some(journals.total()), journals.reserved(&[0x0c; 16]));
    assert!(journals.create([0x0b; 16], 45, 2).is_ok());
    assert_eq!(
        journals.create([0x0c; 16], 45, 2).err().unwrap().kind(),
        std::io::ErrorKind::AlreadyExists
    );
}

#[test]
fn a_failed_removal_waits_for_the_timed_retry_and_a_later_room_does_not_retry_it() {
    let directory = TestDirectory::new();
    let mut journals =
        crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now()).unwrap();
    let first = [5; 16];
    let journal = journals.create(first, 45, 2).unwrap();
    let room = directory.0.join("05".repeat(16));
    // A stray file keeps the directory, so its removal fails.
    std::fs::write(room.join("stray"), b"").unwrap();
    journals.cleanup(journal, false);
    assert!(room.is_dir());
    let held = journals.reserved(&first).unwrap();
    assert_eq!(journals.total(), held);
    // The stray file is gone, so the removal would succeed now.
    std::fs::remove_file(room.join("stray")).unwrap();

    let second = [6; 16];
    let journal = journals.create(second, 45, 2).unwrap();
    journals.cleanup(journal, false);
    assert!(!directory.0.join("06".repeat(16)).exists());
    // The other room's removal did not run: it waits for the timed retry.
    assert!(room.is_dir());
    assert_eq!(journals.reserved(&first), Some(held));
    assert_eq!(journals.total(), held);
    assert_eq!(journals.growth(), 0);

    journals.retry(0);
    assert!(!room.exists());
    assert_eq!(journals.reserved(&first), None);
    assert_eq!(journals.total(), 0);
}

#[test]
fn failed_archive_creation_and_removal_release_their_slot() {
    let directory = TestDirectory::new();
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
    let mut first = client(&key, 0);
    connect(&mut server, &mut first, 0);
    let cert = certificate(&key, 3, 2000);
    let room = directory.0.join("05".repeat(16));
    crate::journal::FAIL_NEXT_INDEX.set(true);
    assert!(matches!(
        configure(&mut server, &mut first, &cert, 0).as_slice(),
        [OrderEvent::Error {
            code: OrderError::StorageUnavailable,
            ..
        }]
    ));
    assert!(!room.exists());
    assert!(
        configure(&mut server, &mut first, &cert, 0)
            .iter()
            .any(|event| matches!(event, OrderEvent::Status(_)))
    );
    // A cancelled start reclaims its empty archive at once. Another file
    // in its directory makes that removal fail until the file is gone.
    std::fs::write(room.join("stray"), b"").unwrap();
    let mut bytes = Vec::new();
    first.leave_ordering(&mut bytes).unwrap();
    events(&mut server, &mut first, &bytes, 0);
    server.advance_clock(0, unix(0));
    assert!(room.exists());
    std::fs::remove_file(room.join("stray")).unwrap();
    server.advance_clock(1000, unix(1000));
    assert!(!room.exists());
}

mod pace_replay;
