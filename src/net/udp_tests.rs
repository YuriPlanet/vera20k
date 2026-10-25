use super::*;
use crate::net::global::GlobalDestination;
use crate::net::reliable::{Delivery, LanConnectionConfig, RetryTiming};
use std::time::{Duration, Instant};

fn loopback_config() -> UdpBindConfig {
    UdpBindConfig {
        primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
    }
}

fn connections() -> LanConnections {
    LanConnections::new(
        LanConnectionConfig::default(),
        RetryTiming {
            retry_delta: 0,
            max_retries: u32::MAX,
            timeout: u32::MAX,
        },
    )
}

fn transport() -> LanUdpTransport {
    LanUdpTransport::bind(loopback_config(), connections()).unwrap()
}

fn received(transport: &mut LanUdpTransport) -> ReceivedMessage {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let report = transport.service(1);
        assert!(report.first_io_error.is_none(), "{report:?}");
        if let Some(message) = transport.pop_received() {
            return message;
        }
        assert!(Instant::now() < deadline, "UDP message did not arrive");
        std::thread::yield_now();
    }
}

fn raw_peer() -> UdpSocket {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
}

fn connection_frame(kind: u8, sequence: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x04, 0xaa, kind, 0];
    bytes.extend(sequence.to_le_bytes());
    bytes.extend([0; 6]);
    bytes.extend(payload);
    bytes
}

fn datagram(frame: &[u8]) -> Vec<u8> {
    let mut bytes = udp_checksum(frame).to_le_bytes().to_vec();
    bytes.extend(frame);
    bytes
}

#[test]
fn relay_uses_existing_reliable_owner_across_a_lost_and_duplicated_datagram() {
    use vera20k_relay_protocol::{MAX_DATAGRAM, RelayServer, SecretKey, Ticket, derive_packet_key};

    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.set_nonblocking(true).unwrap();
    let SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let key = SecretKey::new([42; 32]);
    let mut server = RelayServer::new(key.clone(), 1);
    let mut first = transport();
    let mut second = transport();
    for (client, source) in [(&mut first, 0), (&mut second, 29)] {
        let ticket = Ticket {
            room: [11; 16],
            source,
            expires: now + 120,
            nonce: [source + 1; 16],
            player: [source + 1; 16],
            generation: 1,
        }
        .sign(&key)
        .unwrap();
        let packet_key = derive_packet_key(&key, &ticket);
        client
            .enable_relay(address, &ticket, *packet_key.expose_for_https(), [17; 32])
            .unwrap();
    }
    first
        .connections_mut()
        .add_peer(29, relay_peer_endpoint(29).unwrap())
        .unwrap();
    second
        .connections_mut()
        .add_peer(0, relay_peer_endpoint(0).unwrap())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut tick = 0;
    let mut relay = |server: &mut RelayServer| {
        let mut buffer = [0; MAX_DATAGRAM + 1];
        loop {
            let (length, source) = match socket.recv_from(&mut buffer) {
                Ok(packet) => packet,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("relay read failed: {error}"),
            };
            let SocketAddr::V4(source) = source else {
                unreachable!()
            };
            for output in server.process(source, &buffer[..length], now).unwrap() {
                socket.send_to(&output.bytes, output.endpoint).unwrap();
            }
        }
    };
    while !first.relay_ready() || !second.relay_ready() {
        first.service(tick);
        second.service(tick);
        relay(&mut server);
        tick += 1;
        assert!(
            Instant::now() < deadline,
            "authenticated relay probe did not complete"
        );
    }
    first.begin_relay_peer_probes(&[29]).unwrap();
    second.begin_relay_peer_probes(&[0]).unwrap();
    while first.relay_peer_round_trips().is_none() || second.relay_peer_round_trips().is_none() {
        first.service(tick);
        second.service(tick);
        relay(&mut server);
        tick += 1;
        assert!(
            Instant::now() < deadline,
            "peer round trips did not complete"
        );
    }
    let first_sample = first.relay_peer_round_trips().unwrap();
    let second_sample = second.relay_peer_round_trips().unwrap();
    assert_eq!(first_sample[0].0, 29);
    assert_eq!(second_sample[0].0, 0);
    assert!(first_sample[0].1 > 0 && second_sample[0].1 > 0);
    // Release closure borrows so the impairment applies only after admission.
    drop(relay);
    let mut drop_next = true;
    assert!(
        first
            .connections_mut()
            .broadcast(b"real player command", Delivery::Reliable)
    );
    loop {
        first.service(tick);
        second.service(tick);
        let mut buffer = [0; MAX_DATAGRAM + 1];
        while let Ok((length, SocketAddr::V4(source))) = socket.recv_from(&mut buffer) {
            for output in server.process(source, &buffer[..length], now).unwrap() {
                if drop_next {
                    drop_next = false;
                    continue;
                }
                socket.send_to(&output.bytes, output.endpoint).unwrap();
                socket.send_to(&output.bytes, output.endpoint).unwrap();
            }
        }
        if let Some(message) = second.pop_received() {
            assert_eq!(message.peer_id, 0);
            assert_eq!(message.payload, b"real player command");
            break;
        }
        tick += 1;
        assert!(
            Instant::now() < deadline,
            "reliable command did not survive relay loss"
        );
    }
    second.service(tick + 1);
    assert!(
        second.pop_received().is_none(),
        "duplicate relay packet dispatched twice"
    );
    let renewed = Ticket {
        room: [11; 16],
        source: 0,
        expires: now + 240,
        nonce: [99; 16],
        player: [1; 16],
        generation: 1,
    }
    .sign(&key)
    .unwrap();
    let renewed_key = derive_packet_key(&key, &renewed);
    assert!(
        first
            .connections_mut()
            .broadcast(b"command through renewal", Delivery::Reliable)
    );
    assert!(first.global_mut().send(
        b"global through renewal",
        GlobalDestination::direct(relay_peer_endpoint(29).unwrap()),
        Delivery::Reliable,
        0
    ));
    let counts = {
        let io = first.lock();
        (
            io.connections.queued_send_count(29),
            io.global.queued_send_count(),
        )
    };
    let endpoint = first.primary_addr().unwrap();
    first
        .renew_relay(address, &renewed, *renewed_key.expose_for_https(), [17; 32])
        .unwrap();
    assert_eq!(
        first.relay_expires(),
        Some(now + 120),
        "candidate must not replace an unconfirmed route"
    );
    assert_eq!(
        {
            let io = first.lock();
            (
                io.connections.queued_send_count(29),
                io.global.queued_send_count(),
            )
        },
        counts
    );
    assert_eq!(first.relay_peer_round_trips(), Some(first_sample.clone()));
    let mut lost_ready = false;
    let mut got_private = false;
    let mut got_global = false;
    while first.relay_expires() != Some(now + 240) || !got_private || !got_global {
        tick += 1;
        first.service(tick);
        second.service(tick);
        let mut buffer = [0; MAX_DATAGRAM + 1];
        while let Ok((length, SocketAddr::V4(source))) = socket.recv_from(&mut buffer) {
            let Ok(outputs) = server.process(source, &buffer[..length], now) else {
                continue;
            };
            for output in outputs {
                if !lost_ready
                    && output.endpoint == endpoint
                    && output.bytes[4] == 1
                    && output.bytes[21..37] == [99; 16]
                {
                    lost_ready = true;
                    continue;
                }
                socket.send_to(&output.bytes, output.endpoint).unwrap();
            }
        }
        while let Some(message) = second.pop_received() {
            got_private |= message.payload == b"command through renewal";
        }
        while let Some(message) = second.pop_global_received() {
            got_global |= message.payload == b"global through renewal";
        }
        assert!(
            Instant::now() < deadline,
            "renewal must survive a lost READY without resetting reliable traffic"
        );
    }
    assert!(lost_ready);
    assert_eq!(first.primary_addr().unwrap(), endpoint);
    assert_eq!(first.relay_peer_round_trips(), Some(first_sample));
    assert!(
        first
            .renew_relay(address, &renewed, *renewed_key.expose_for_https(), [17; 32])
            .is_ok(),
        "duplicate grant is idempotent"
    );
    let stale = Ticket {
        room: [11; 16],
        source: 0,
        expires: now + 120,
        nonce: [1; 16],
        player: [1; 16],
        generation: 1,
    }
    .sign(&key)
    .unwrap();
    assert!(
        first
            .renew_relay(
                address,
                &stale,
                *derive_packet_key(&key, &stale).expose_for_https(),
                [17; 32]
            )
            .is_err()
    );
    let foreign = Ticket {
        room: [11; 16],
        source: 1,
        expires: now + 300,
        nonce: [100; 16],
        player: [2; 16],
        generation: 1,
    }
    .sign(&key)
    .unwrap();
    assert!(
        first
            .renew_relay(
                address,
                &foreign,
                *derive_packet_key(&key, &foreign).expose_for_https(),
                [17; 32]
            )
            .is_err()
    );
    first.retire_relay();
    assert_eq!(first.primary_addr().unwrap(), endpoint);
    assert_eq!(first.relay_expires(), None);
    assert_eq!(first.connections().queued_send_count(29), None);
    assert_eq!(first.global().queued_send_count(), 0);
}

#[test]
fn relay_retirement_keeps_the_original_route_until_a_retried_leave_is_acknowledged() {
    use vera20k_relay_protocol::{MAX_DATAGRAM, RelayServer, SecretKey, Ticket, derive_packet_key};

    let socket = raw_peer();
    socket.set_nonblocking(true).unwrap();
    let address = ipv4_addr(&socket).unwrap();
    let key = SecretKey::new([42; 32]);
    let mut server = RelayServer::new(key.clone(), 1);
    let mut wire = transport();
    let primary = wire.primary_addr().unwrap();
    let ticket = Ticket {
        room: [11; 16],
        source: 0,
        expires: 1200,
        nonce: [1; 16],
        player: [1; 16],
        generation: 1,
    }
    .sign(&key)
    .unwrap();
    let packet_key = *derive_packet_key(&key, &ticket).expose_for_https();
    wire.enable_relay(address, &ticket, packet_key, [17; 32])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = [0; MAX_DATAGRAM + 1];
    while !wire.relay_ready() {
        wire.service(0);
        while let Ok((length, SocketAddr::V4(source))) = socket.recv_from(&mut bytes) {
            for output in server
                .process_at(source, &bytes[..length], 1000, 0)
                .unwrap()
            {
                socket.send_to(&output.bytes, output.endpoint).unwrap();
            }
        }
        assert!(Instant::now() < deadline, "relay handshake did not finish");
        std::thread::yield_now();
    }
    socket.set_nonblocking(false).unwrap();
    let cert = OrderingCertificate {
        room: [11; 16],
        sources_mask: 1,
        tick_hz: 45,
        expires: 2000,
    }
    .sign(&key)
    .unwrap();
    wire.set_ordering_certificate(cert.to_vec()).unwrap();
    wire.configure_ordering([7; 32]).unwrap();
    let (length, SocketAddr::V4(source)) = socket.recv_from(&mut bytes).unwrap() else {
        panic!("IPv4");
    };
    for output in server
        .process_at(source, &bytes[..length], 1000, 0)
        .unwrap()
    {
        socket.send_to(&output.bytes, output.endpoint).unwrap();
    }
    wire.retire_relay();
    assert!(wire.relay_retirement_pending());
    assert_eq!(
        wire.enable_relay(address, &ticket, packet_key, [17; 32])
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    wire.service(0);
    let (length, _) = socket.recv_from(&mut bytes).unwrap();
    let lost_leave = bytes[..length].to_vec();
    wire.service(16);
    let (length, SocketAddr::V4(source)) = socket.recv_from(&mut bytes).unwrap() else {
        panic!("IPv4");
    };
    assert_eq!(&bytes[..length], lost_leave.as_slice());
    // The first request was lost; lose this retry's ACK after hop expiry too.
    let lost_ack = server
        .process_at(source, &bytes[..length], 1350, 350_000)
        .unwrap();
    assert_eq!(lost_ack.len(), 1);
    assert!(wire.relay_retirement_pending());
    wire.service(32);
    let (length, SocketAddr::V4(source)) = socket.recv_from(&mut bytes).unwrap() else {
        panic!("IPv4");
    };
    assert_eq!(&bytes[..length], lost_leave.as_slice());
    let ack = server
        .process_at(source, &bytes[..length], 1351, 351_000)
        .unwrap();
    let mut forged = ack[0].bytes.clone();
    *forged.last_mut().unwrap() ^= 1;
    socket.send_to(&forged, ack[0].endpoint).unwrap();
    wire.service(32);
    assert!(
        wire.relay_retirement_pending(),
        "forged ACK cannot retire the route"
    );
    socket.send_to(&ack[0].bytes, ack[0].endpoint).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while wire.relay_retirement_pending() {
        wire.service(32);
        assert!(
            Instant::now() < deadline,
            "terminal ACK did not retire the route"
        );
        std::thread::yield_now();
    }
    assert_eq!(wire.relay_source(), None);
    assert_eq!(wire.primary_addr().unwrap(), primary);
}

#[test]
fn raw_send_errors_preserve_owned_bytes_destination_and_fifo_order() {
    let mut queue = UdpSendQueue::default();
    let mut report = UdpServiceReport::default();
    let first = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let second = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4321);
    let mut frame = connection_frame(1, 4, b"unreliable");
    let original = datagram(&frame);
    let ack = connection_frame(2, 8, &[]);
    let mut attempts = Vec::new();
    queue.send(
        first,
        &frame,
        None,
        UdpChannel::Primary,
        &mut report,
        &mut |bytes, endpoint| {
            attempts.push((endpoint, bytes.to_vec()));
            Err(io::ErrorKind::PermissionDenied.into())
        },
    );
    frame.fill(0xff);
    queue.send(
        second,
        &ack,
        None,
        UdpChannel::Primary,
        &mut report,
        &mut |bytes, endpoint| {
            attempts.push((endpoint, bytes.to_vec()));
            Err(io::ErrorKind::Interrupted.into())
        },
    );
    assert_eq!(queue.pending.len(), 2);
    assert_eq!(report.sent_datagrams, 0);
    assert_eq!(report.send_would_block, 0);
    assert_eq!(
        report.first_io_error.as_ref().unwrap().error.kind(),
        io::ErrorKind::PermissionDenied
    );
    queue.service(UdpChannel::Primary, &mut report, &mut |bytes, endpoint| {
        attempts.push((endpoint, bytes.to_vec()));
        Ok(0) // Native retires every non--1 return, including a short result.
    });
    assert_eq!(queue.pending.len(), 1);
    queue.service(UdpChannel::Primary, &mut report, &mut |bytes, endpoint| {
        attempts.push((endpoint, bytes.to_vec()));
        Ok(bytes.len())
    });
    assert_eq!(report.sent_datagrams, 2);
    assert!(queue.pending.is_empty());
    let normalized = SocketAddrV4::new(Ipv4Addr::LOCALHOST, DEFAULT_LAN_PORT);
    assert_eq!(attempts[..3], vec![(normalized, original); 3]);
    assert_eq!(attempts[3], (second, datagram(&ack)));
}

#[test]
fn would_block_retires_raw_attempt_while_reliable_owner_can_retry() {
    let mut connections = connections();
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4321);
    connections.add_peer(1, endpoint).unwrap();
    assert!(connections.send_to(1, b"retry", Delivery::Reliable));
    let mut queue = UdpSendQueue::default();
    let mut report = UdpServiceReport::default();
    let mut attempted = Vec::new();
    connections.service(1, &mut |endpoint, frame| {
        queue.send(
            endpoint,
            frame,
            None,
            UdpChannel::Primary,
            &mut report,
            &mut |bytes, _| {
                attempted.push(bytes.to_vec());
                Err(io::ErrorKind::WouldBlock.into())
            },
        );
    });
    assert_eq!(report.send_would_block, 1);
    assert!(report.first_io_error.is_none());
    assert!(queue.pending.is_empty());
    assert_eq!(connections.queued_send_count(1), Some(1));
    queue.service(UdpChannel::Primary, &mut report, &mut |_, _| {
        panic!("a retired raw attempt cannot be serviced again")
    });
    connections.service(2, &mut |endpoint, frame| {
        queue.send(
            endpoint,
            frame,
            None,
            UdpChannel::Primary,
            &mut report,
            &mut |bytes, _| {
                attempted.push(bytes.to_vec());
                Ok(bytes.len())
            },
        );
    });
    assert_eq!(attempted.len(), 2);
    assert_eq!(attempted[0], attempted[1]);
    assert!(queue.pending.is_empty());
    assert_eq!(connections.queued_send_count(1), Some(1));
}

#[test]
fn raw_fifos_grow_beyond_native_pool_and_clear_independently_of_reliability() {
    let mut transport = transport();
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4321);
    transport.connections_mut().add_peer(1, endpoint).unwrap();
    assert!(
        transport
            .connections_mut()
            .send_to(1, b"retained", Delivery::Reliable)
    );
    let mut report = UdpServiceReport::default();
    for sequence in 0..130 {
        transport.lock().primary_send.send(
            endpoint,
            &connection_frame(1, sequence, b"pooled or heap"),
            None,
            UdpChannel::Primary,
            &mut report,
            &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
        );
    }
    transport.lock().alternate_send.send(
        endpoint,
        b"alternate",
        None,
        UdpChannel::Alternate,
        &mut report,
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );
    assert_eq!(transport.lock().primary_send.pending.len(), 130);
    assert_eq!(transport.lock().alternate_send.pending.len(), 1);
    let mut alternate = Vec::new();
    transport
        .lock()
        .alternate_send
        .service(UdpChannel::Alternate, &mut report, &mut |bytes, _| {
            alternate.extend_from_slice(bytes);
            Ok(bytes.len())
        });
    assert_eq!(alternate, datagram(b"alternate"));
    assert_eq!(transport.lock().primary_send.pending.len(), 130);
    transport.lock().alternate_send.send(
        endpoint,
        b"discard",
        None,
        UdpChannel::Alternate,
        &mut report,
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );
    transport.clear_pending_sends();
    assert!(transport.lock().primary_send.pending.is_empty());
    assert!(transport.lock().alternate_send.pending.is_empty());
    assert_eq!(transport.connections().queued_send_count(1), Some(1));
}

#[test]
fn portable_service_retries_one_retained_head_without_new_protocol_send() {
    let mut transport = transport();
    let sink = raw_peer();
    let endpoint = ipv4_addr(&sink).unwrap();
    let mut report = UdpServiceReport::default();
    for frame in [b"first".as_slice(), b"second".as_slice()] {
        transport.lock().primary_send.send(
            endpoint,
            frame,
            None,
            UdpChannel::Primary,
            &mut report,
            &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
        );
    }
    let mut bytes = [0; 64];
    for (remaining, frame) in [(1, b"first".as_slice()), (0, b"second".as_slice())] {
        let report = transport.service(1);
        assert_eq!(report.sent_datagrams, 1);
        assert!(report.first_io_error.is_none());
        assert_eq!(transport.lock().primary_send.pending.len(), remaining);
        let (length, source) = sink.recv_from(&mut bytes).unwrap();
        assert_eq!(source, SocketAddr::V4(transport.primary_addr().unwrap()));
        assert_eq!(&bytes[..length], datagram(frame));
    }
    assert_eq!(transport.service(1).sent_datagrams, 0);
}

#[test]
fn generated_ack_joins_the_same_fifo_behind_retained_raw_datagrams() {
    let mut receiver = transport();
    let sink = raw_peer();
    let sender = raw_peer();
    receiver
        .connections_mut()
        .add_peer(7, ipv4_addr(&sender).unwrap())
        .unwrap();
    let mut report = UdpServiceReport::default();
    for frame in [b"first".as_slice(), b"second".as_slice()] {
        receiver.lock().primary_send.send(
            ipv4_addr(&sink).unwrap(),
            frame,
            None,
            UdpChannel::Primary,
            &mut report,
            &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
        );
    }
    sender
        .send_to(
            &datagram(&connection_frame(0, 0, b"command")),
            receiver.alternate_addr().unwrap(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = [0; 64];
    loop {
        match receiver.lock().alternate.peek_from(&mut bytes) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            Err(error) => panic!("UDP peek failed: {error}"),
        }
    }
    let report = receiver.service(1);
    assert_eq!(report.received_datagrams, 1);
    assert_eq!(report.sent_datagrams, 2);
    for frame in [b"first".as_slice(), b"second".as_slice()] {
        let (length, _) = sink.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..length], datagram(frame));
    }
    // The received command queues its ACK behind the second old datagram.
    assert_eq!(receiver.pop_received().unwrap().payload, b"command");
    assert_eq!(receiver.lock().primary_send.pending.len(), 1);
    assert_eq!(receiver.service(1).sent_datagrams, 1);
    let (length, source) = sender.recv_from(&mut bytes).unwrap();
    assert_eq!(source, SocketAddr::V4(receiver.primary_addr().unwrap()));
    assert_eq!(&bytes[..length], datagram(&connection_frame(2, 0, &[])));
}

#[test]
fn independent_udp_owners_deliver_reliable_data_and_retire_the_acknowledged_send() {
    let mut left = transport();
    let mut right = transport();
    left.connections_mut()
        .add_peer(42, right.primary_addr().unwrap())
        .unwrap();
    right
        .connections_mut()
        .add_peer(7, left.primary_addr().unwrap())
        .unwrap();
    let payload = b"opaque Event packet bytes";
    assert!(
        left.connections_mut()
            .broadcast(payload, Delivery::Reliable)
    );
    let sent = left.service(1);
    assert_eq!(sent.sent_datagrams, 1);
    assert!(sent.first_io_error.is_none());
    let message = received(&mut right);
    assert_eq!(message.peer_id, 7);
    assert_eq!(message.payload, payload);
    let deadline = Instant::now() + Duration::from_secs(2);
    while left.connections().queued_send_count(42) != Some(0) {
        let report = left.service(2);
        assert!(report.first_io_error.is_none(), "{report:?}");
        assert!(Instant::now() < deadline, "UDP ACK did not arrive");
        std::thread::yield_now();
    }
    assert_eq!(left.service(3).sent_datagrams, 0);
}

#[test]
fn unknown_source_global_admission_and_private_gameplay_share_the_same_udp_owner() {
    let mut left = transport();
    let mut right = transport();
    let left_address = left.primary_addr().unwrap();
    let right_address = right.primary_addr().unwrap();
    assert!(left.global_mut().send(
        b"join",
        GlobalDestination::direct(right_address),
        Delivery::Reliable,
        0
    ));
    assert_eq!(left.service(1).sent_datagrams, 1);
    let deadline = Instant::now() + Duration::from_secs(2);
    let joined = loop {
        let report = right.service(1);
        assert!(report.first_io_error.is_none(), "{report:?}");
        assert_eq!(report.rejected_connections, 0);
        if let Some(message) = right.pop_global_received() {
            break message;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert_eq!(joined.source, left_address);
    assert_eq!(joined.product_id, 0xaa04);
    assert_eq!(joined.payload, b"join");
    while left.global().queued_send_count() != 0 {
        left.service(2);
        assert!(Instant::now() < deadline, "global ACK did not arrive");
        std::thread::yield_now();
    }
    assert_eq!(left.global().mean_response_time(), 1);
    assert_eq!(left.response_time(), 0);
    // Admission hands the same physical endpoint to the private channel;
    // neither side rebinds or opens a second independent socket transport.
    left.connections_mut().add_peer(1, right_address).unwrap();
    right.connections_mut().add_peer(0, joined.source).unwrap();
    assert!(
        left.connections_mut()
            .broadcast(b"command", Delivery::Reliable)
    );
    left.service(3);
    let message = received(&mut right);
    assert_eq!(message.peer_id, 0);
    assert_eq!(message.payload, b"command");
    assert_eq!(left.primary_addr().unwrap(), left_address);
    assert_eq!(right.primary_addr().unwrap(), right_address);
}

#[test]
fn manager_response_reset_keeps_sockets_raw_queues_and_pending_channel_traffic() {
    let mut wire = transport();
    let peer = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4321);
    wire.connections_mut().add_peer(1, peer).unwrap();
    assert!(
        wire.connections_mut()
            .send_to(1, b"private history", Delivery::Reliable)
    );
    wire.connections_mut().service(100, &mut |_, _| {});
    wire.connections_mut()
        .receive(peer, &connection_frame(2, 0, &[]), &mut |_, _| {})
        .unwrap();
    wire.connections_mut().service(111, &mut |_, _| {});
    assert!(wire.global_mut().send(
        b"global history",
        GlobalDestination::direct(peer),
        Delivery::Reliable,
        0
    ));
    let mut global_frame = Vec::new();
    wire.global_mut()
        .service(100, &mut |_, frame| global_frame = frame.to_vec());
    let mut global_ack = global_frame.clone();
    global_ack[2] = 2;
    wire.global_mut()
        .receive(peer, 0, &global_ack, 122, &mut |_, _| {})
        .unwrap();
    wire.global_mut().service(122, &mut |_, _| {});
    assert_eq!(wire.connections().mean_response_time(1), Some(11));
    assert_eq!(wire.global().mean_response_time(), 22);
    assert!(
        wire.connections_mut()
            .send_to(1, b"private pending", Delivery::Reliable)
    );
    assert!(wire.global_mut().send(
        b"global pending",
        GlobalDestination::direct(peer),
        Delivery::Reliable,
        0
    ));
    wire.connections_mut()
        .receive(
            peer,
            &connection_frame(0, 0, b"private receive"),
            &mut |_, _| {},
        )
        .unwrap();
    wire.global_mut()
        .receive(peer, 0, &global_frame, 123, &mut |_, _| {})
        .unwrap();
    wire.lock().primary_send.send(
        peer,
        &connection_frame(2, 9, &[]),
        None,
        UdpChannel::Primary,
        &mut UdpServiceReport::default(),
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );
    wire.lock().alternate_send.send(
        peer,
        &connection_frame(2, 10, &[]),
        None,
        UdpChannel::Alternate,
        &mut UdpServiceReport::default(),
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );
    wire.set_broadcast_targets(&[peer]).unwrap();
    let sockets = (wire.primary_addr().unwrap(), wire.alternate_addr().unwrap());
    let raw = {
        let io = wire.lock();
        (
            format!("{:?}", io.primary_send),
            format!("{:?}", io.alternate_send),
        )
    };
    wire.reset_response_time(false);
    assert_eq!(wire.connections().mean_response_time(1), Some(11));
    assert_eq!(
        wire.global().mean_response_time(),
        0,
        "manager always clears primary-global"
    );
    wire.reset_response_time(true);
    assert_eq!(wire.response_time(), 0);
    assert_eq!(wire.connections().queued_send_count(1), Some(1));
    assert_eq!(wire.global().queued_send_count(), 1);
    assert_eq!(
        (wire.primary_addr().unwrap(), wire.alternate_addr().unwrap()),
        sockets
    );
    assert_eq!(
        {
            let io = wire.lock();
            (
                format!("{:?}", io.primary_send),
                format!("{:?}", io.alternate_send),
            )
        },
        raw
    );
    assert_eq!(wire.lock().broadcast_targets, [peer]);
    assert_eq!(wire.pop_received().unwrap().payload, b"private receive");
    assert_eq!(
        wire.pop_global_received().unwrap().payload,
        b"global history"
    );
}

#[test]
fn response_time_matches_native_private_maximum_excluding_primary_global() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_manager_timing.json",
    ))
    .unwrap();
    for case in corpus["response_maximum"].as_array().unwrap() {
        let mut wire = transport();
        for (index, mean) in case["private"].as_array().unwrap().iter().enumerate() {
            let id = index as i32;
            let peer = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 10000 + index as u16);
            let mean = mean.as_u64().unwrap() as u32;
            wire.connections_mut().add_peer(id, peer).unwrap();
            assert!(
                wire.connections_mut()
                    .send_to(id, b"sample", Delivery::Reliable)
            );
            wire.connections_mut().service(1, &mut |_, _| {});
            wire.connections_mut()
                .receive(peer, &connection_frame(2, 0, &[]), &mut |_, _| {})
                .unwrap();
            wire.connections_mut()
                .service(1_u32.wrapping_add(mean), &mut |_, _| {});
            assert_eq!(wire.connections().mean_response_time(id), Some(mean));
        }
        let primary = case["primary"].as_u64().unwrap() as u32;
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 20000);
        assert!(wire.global_mut().send(
            b"primary sample",
            GlobalDestination::direct(endpoint),
            Delivery::Reliable,
            0
        ));
        let mut ack = Vec::new();
        wire.global_mut()
            .service(1, &mut |_, frame| ack = frame.to_vec());
        ack[2] = 2;
        wire.global_mut()
            .receive(endpoint, 0, &ack, 1, &mut |_, _| {})
            .unwrap();
        wire.global_mut()
            .service(1_u32.wrapping_add(primary), &mut |_, _| {});
        assert_eq!(wire.global().mean_response_time(), primary);
        assert_eq!(
            u64::from(wire.response_time()),
            case["result"].as_u64().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn explicit_global_broadcast_targets_are_deduplicated_and_need_no_private_connections() {
    let mut sender = transport();
    let mut first = transport();
    let mut second = transport();
    let first_address = first.primary_addr().unwrap();
    let second_address = second.primary_addr().unwrap();
    sender
        .set_broadcast_targets(&[first_address, second_address, first_address])
        .unwrap();
    assert!(sender.global_mut().send(
        b"discover",
        GlobalDestination::BROADCAST,
        Delivery::Reliable,
        0
    ));
    assert_eq!(sender.service(1).sent_datagrams, 2);
    for receiver in [&mut first, &mut second] {
        let deadline = Instant::now() + Duration::from_secs(2);
        let message = loop {
            let report = receiver.service(1);
            assert_eq!(report.sent_datagrams, 0, "broadcast must not request ACK");
            if let Some(message) = receiver.pop_global_received() {
                break message;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(message.payload, b"discover");
    }
    sender.service(2);
    assert_eq!(sender.global().queued_send_count(), 0);
}

#[test]
fn alternate_socket_receives_but_ordinary_ack_uses_primary_without_redelivery() {
    let mut receiver = transport();
    let sender = raw_peer();
    receiver
        .connections_mut()
        .add_peer(9, ipv4_addr(&sender).unwrap())
        .unwrap();
    let bytes = datagram(&connection_frame(0, 0, b"alternate"));
    sender
        .send_to(&bytes, receiver.alternate_addr().unwrap())
        .unwrap();
    assert_eq!(received(&mut receiver).payload, b"alternate");
    let mut reply = [0; 64];
    let (length, source) = sender.recv_from(&mut reply).unwrap();
    assert_eq!(source, SocketAddr::V4(receiver.primary_addr().unwrap()));
    assert_eq!(&reply[..length], datagram(&connection_frame(2, 0, &[])));
    sender
        .send_to(&bytes, receiver.alternate_addr().unwrap())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let report = receiver.service(1);
        if report.received_datagrams != 0 {
            assert_eq!(report.sent_datagrams, 1);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(receiver.pop_received().is_none());
}

#[test]
fn alternate_global_receive_also_sends_ack_from_primary() {
    let mut receiver = transport();
    let sender = raw_peer();
    let mut frame = vec![0x36, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0xaa];
    frame.extend(b"global");
    sender
        .send_to(&datagram(&frame), receiver.alternate_addr().unwrap())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        receiver.service(1);
        if let Some(message) = receiver.pop_global_received() {
            assert_eq!(message.payload, b"global");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let mut reply = [0; 64];
    let (length, source) = sender.recv_from(&mut reply).unwrap();
    assert_eq!(source, SocketAddr::V4(receiver.primary_addr().unwrap()));
    frame.truncate(16);
    frame[2] = 2;
    assert_eq!(&reply[..length], datagram(&frame));
}

#[test]
fn malformed_checksum_unknown_source_and_oversize_never_enter_reliable_delivery() {
    let mut receiver = transport();
    let sender = raw_peer();
    receiver
        .connections_mut()
        .add_peer(3, ipv4_addr(&sender).unwrap())
        .unwrap();
    let destination = receiver.primary_addr().unwrap();
    sender.send_to(&[1, 2, 3], destination).unwrap();
    let mut corrupt = datagram(&connection_frame(0, 0, b"corrupted"));
    *corrupt.last_mut().unwrap() ^= 1;
    sender.send_to(&corrupt, destination).unwrap();
    sender
        .send_to(&vec![0; UDP_DATAGRAM_CAPACITY + 5], destination)
        .unwrap();
    sender.send_to(&datagram(&[0xaa]), destination).unwrap();
    let stranger = raw_peer();
    stranger
        .send_to(&datagram(&connection_frame(0, 0, b"unknown")), destination)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut count = 0;
    let mut malformed = 0;
    let mut rejected = 0;
    while count < 5 {
        let report = receiver.service(1);
        assert!(report.first_io_error.is_none(), "{report:?}");
        count += report.received_datagrams;
        malformed += report.malformed_datagrams;
        rejected += report.rejected_connections;
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(malformed, 3);
    assert_eq!(rejected, 2);
    assert!(receiver.pop_received().is_none());
}

#[test]
fn connection_failure_retains_the_native_peer_identity() {
    let mut transport = LanUdpTransport::bind(
        loopback_config(),
        LanConnections::new(
            LanConnectionConfig::default(),
            RetryTiming {
                retry_delta: 0,
                max_retries: 1,
                timeout: u32::MAX,
            },
        ),
    )
    .unwrap();
    let sink = raw_peer();
    transport
        .connections_mut()
        .add_peer(71, ipv4_addr(&sink).unwrap())
        .unwrap();
    assert!(
        transport
            .connections_mut()
            .send_to(71, b"no ACK", Delivery::Reliable)
    );
    assert_eq!(transport.service(1).failed_peer, None);
    assert_eq!(transport.service(2).failed_peer, Some(71));
}

#[test]
fn dropping_transport_closes_both_bound_sockets_and_all_connection_state() {
    let mut first = transport();
    let config = UdpBindConfig {
        primary: first.primary_addr().unwrap(),
        alternate: first.alternate_addr().unwrap(),
    };
    first
        .connections_mut()
        .add_peer(10, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234))
        .unwrap();
    assert!(
        first
            .connections_mut()
            .send_to(10, b"queued", Delivery::Reliable)
    );
    let mut report = UdpServiceReport::default();
    let mut io = first.lock();
    let UdpIo {
        primary_send,
        alternate_send,
        ..
    } = &mut *io;
    for (queue, channel) in [
        (primary_send, UdpChannel::Primary),
        (alternate_send, UdpChannel::Alternate),
    ] {
        queue.send(
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234),
            b"old raw attempt",
            None,
            channel,
            &mut report,
            &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
        );
        assert_eq!(queue.pending.len(), 1);
    }
    drop(io);
    drop(first);
    let fresh = LanUdpTransport::bind(config, connections()).unwrap();
    assert_eq!(fresh.connections().peers().len(), 0);
    assert!(fresh.lock().primary_send.pending.is_empty());
    assert!(fresh.lock().alternate_send.pending.is_empty());
    assert_eq!(fresh.primary_addr().unwrap(), config.primary);
    assert_eq!(fresh.alternate_addr().unwrap(), config.alternate);
}

#[test]
fn fresh_session_clears_raw_private_and_global_traffic_without_rebinding() {
    let mut wire = transport();
    let primary = wire.primary_addr().unwrap();
    let alternate = wire.alternate_addr().unwrap();
    let sink = raw_peer();
    let destination = ipv4_addr(&sink).unwrap();
    wire.connections_mut().add_peer(4, destination).unwrap();
    assert!(
        wire.connections_mut()
            .broadcast(b"old private", Delivery::Reliable)
    );
    assert!(wire.global_mut().send(
        b"old global",
        GlobalDestination::direct(destination),
        Delivery::Reliable,
        0
    ));
    wire.lock().primary_send.send(
        destination,
        b"old raw",
        None,
        UdpChannel::Primary,
        &mut UdpServiceReport::default(),
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );

    wire.reset_session();
    assert_eq!(wire.primary_addr().unwrap(), primary);
    assert_eq!(wire.alternate_addr().unwrap(), alternate);
    assert_eq!(wire.global().queued_send_count(), 0);
    assert!(wire.connections().peers().next().is_none());
    assert_eq!(wire.service(10).sent_datagrams, 0);

    assert!(wire.global_mut().send(
        b"new visit",
        GlobalDestination::direct(destination),
        Delivery::Reliable,
        0
    ));
    assert_eq!(wire.service(11).sent_datagrams, 1);
    let mut bytes = [0; UDP_DATAGRAM_CAPACITY];
    let (length, source) = sink.recv_from(&mut bytes).unwrap();
    assert_eq!(source, SocketAddr::V4(primary));
    assert_eq!(
        &bytes[CHECKSUM_LEN + 4..CHECKSUM_LEN + 8],
        &0_u32.to_le_bytes()
    );
    assert_eq!(
        &bytes[CHECKSUM_LEN + super::super::global::GLOBAL_HEADER_LEN..length],
        b"new visit"
    );
}

fn take_wire_datagrams(wire: &LanUdpTransport, count: usize) -> Vec<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut packets = Vec::new();
    let mut bytes = [0; UDP_DATAGRAM_CAPACITY + 1];
    while packets.len() < count {
        match wire.lock().primary.recv_from(&mut bytes) {
            Ok((length, _)) => packets.push(bytes[..length].to_vec()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "scoped UDP datagram did not arrive"
                );
                std::thread::yield_now();
            }
            Err(error) => panic!("scoped UDP receive failed: {error}"),
        }
    }
    packets
}

// A successful send_to does not guarantee that a following nonblocking
// recv_from can already see the loopback datagram on every platform. Hold the
// existing clock bucket so reliable retries cannot manufacture extra input.
fn malformed_after_receipt(wire: &mut LanUdpTransport, now: u32, expected: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut received = 0;
    let mut malformed = 0;
    while received < expected {
        let report = wire.service(now);
        assert!(report.first_io_error.is_none(), "{report:?}");
        received += report.received_datagrams;
        malformed += report.malformed_datagrams;
        assert!(
            Instant::now() < deadline,
            "expected {expected} datagrams, received {received}; last report {report:?}"
        );
        std::thread::yield_now();
    }
    assert_eq!(received, expected);
    malformed
}

#[test]
fn scoped_lan_keeps_full_private_payload_and_retires_to_plain_discovery() {
    let mut left = transport();
    let mut right = transport();
    let left_address = left.primary_addr().unwrap();
    let right_address = right.primary_addr().unwrap();
    let scope = SessionScope {
        epoch: [23; 16],
        revision: (1_u64 << 32) + 7,
    };
    for (wire, peer, address) in [(&mut left, 1, right_address), (&mut right, 0, left_address)] {
        wire.set_lan_scope(scope).unwrap();
        wire.set_lan_scope(scope).unwrap();
        assert_eq!(wire.lan_scope(), Some(scope));
        wire.connections_mut().add_peer(peer, address).unwrap();
    }
    assert_eq!(
        left.enable_relay(right_address, &[], [0; 32], [0; 32])
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    let payload = vec![71; LanConnectionConfig::default().max_payload_bytes];
    assert!(
        left.connections_mut()
            .send_to(1, &payload, Delivery::Reliable)
    );
    assert!(left.global_mut().send(
        b"match side channel",
        GlobalDestination::direct(right_address),
        Delivery::Reliable,
        0
    ));
    assert_eq!(left.service(1).sent_datagrams, 2);
    let packets = take_wire_datagrams(&right, 2);
    assert!(
        packets
            .iter()
            .all(|packet| packet.len() <= UDP_DATAGRAM_CAPACITY
                && packet.starts_with(LAN_SCOPE_MAGIC))
    );
    for packet in &packets {
        left.lock().primary.send_to(packet, right_address).unwrap();
    }
    assert_eq!(received(&mut right).payload, payload);
    assert_eq!(
        right.pop_global_received().unwrap().payload,
        b"match side channel"
    );
    // send_to can finish before the loopback ACK is visible to recv_from.
    // Keep the original send bucket while waiting for both real receipts;
    // advancing it would admit a retry before a delayed loopback ACK arrives.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let report = left.service(1);
        assert!(report.first_io_error.is_none(), "{report:?}");
        let private_retired = left.connections().queued_send_count(1) == Some(0);
        let global_retired = left.global().queued_send_count() == 0;
        if private_retired && global_retired {
            break;
        }
        assert!(Instant::now() < deadline, "scoped UDP ACKs did not arrive");
        std::thread::yield_now();
    }
    assert_eq!(left.connections().queued_send_count(1), Some(0));
    assert_eq!(left.global().queued_send_count(), 0);

    // The same physical sockets return to ordinary unscoped LAN discovery.
    // In-flight packets from the retired match cannot enter either owner.
    left.clear_lan_scope();
    right.clear_lan_scope();
    assert_eq!(left.lan_scope(), None);
    assert_eq!(right.lan_scope(), None);
    assert!(left.connections().peers().next().is_none());
    for packet in &packets {
        left.lock().primary.send_to(packet, right_address).unwrap();
    }
    assert_eq!(malformed_after_receipt(&mut right, 3, 2), 2);
    assert!(right.pop_received().is_none());
    assert!(right.pop_global_received().is_none());
    assert!(left.global_mut().send(
        b"new lobby",
        GlobalDestination::direct(right_address),
        Delivery::Reliable,
        0
    ));
    left.service(4);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        right.service(4);
        if let Some(message) = right.pop_global_received() {
            assert_eq!(message.payload, b"new lobby");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        (left.primary_addr().unwrap(), right.primary_addr().unwrap()),
        (left_address, right_address)
    );
}

#[test]
fn loading_scope_bridge_survives_lost_progress_before_both_peers_activate() {
    let mut host = transport();
    let mut guest = transport();
    let host_address = host.primary_addr().unwrap();
    let guest_address = guest.primary_addr().unwrap();
    let scope = SessionScope {
        epoch: [91; 16],
        revision: 7,
    };
    for (wire, peer, address) in [(&mut host, 1, guest_address), (&mut guest, 0, host_address)] {
        wire.prepare_lan_scope(scope).unwrap();
        wire.connections_mut().add_peer(peer, address).unwrap();
    }
    let host_progress = LoadingProgressMessage::new("Host", 100)
        .unwrap()
        .scoped_bytes(scope);
    let guest_progress = LoadingProgressMessage::new("Guest", 100)
        .unwrap()
        .scoped_bytes(scope);
    // Host has completed first, while guest missed its last ordinary loading
    // publication. The same reliable entry retries under host's active route.
    host.set_lan_scope(scope).unwrap();
    assert!(host.global_mut().send(
        &host_progress,
        GlobalDestination::direct(guest_address),
        Delivery::Reliable,
        0,
    ));
    assert!(
        host.connections_mut()
            .send_to(1, b"first order", Delivery::Reliable)
    );
    host.service(1);
    let first = take_wire_datagrams(&guest, 2);
    let private = first
        .iter()
        .find(|packet| packet.ends_with(b"first order"))
        .unwrap();
    host.lock().primary.send_to(private, guest_address).unwrap();
    assert_eq!(malformed_after_receipt(&mut guest, 1, 1), 1);
    assert!(
        guest.pop_received().is_none(),
        "loading cannot admit private gameplay"
    );
    assert!(guest.global_mut().send(
        &guest_progress,
        GlobalDestination::direct(host_address),
        Delivery::Reliable,
        0,
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut tick = 2;
    let mut got_host = false;
    let mut got_guest = false;
    loop {
        guest.service(tick);
        host.service(tick);
        if let Some(message) = host.pop_global_received() {
            assert_eq!(message.payload, guest_progress);
            got_guest = true;
        }
        if let Some(message) = guest.pop_global_received() {
            assert_eq!(message.payload, host_progress);
            got_host = true;
        }
        assert!(guest.pop_received().is_none());
        if got_host
            && got_guest
            && host.global().queued_send_count() == 0
            && guest.global().queued_send_count() == 0
        {
            break;
        }
        tick += 1;
        assert!(
            Instant::now() < deadline,
            "mixed loading scopes did not drain"
        );
        std::thread::yield_now();
    }
    guest.set_lan_scope(scope).unwrap();
    host.service(tick + 1);
    assert_eq!(received(&mut guest).payload, b"first order");
    host.service(tick + 2);
    assert_eq!(host.connections().queued_send_count(1), Some(0));
}

#[test]
fn rematch_rejects_old_ack_data_and_frame_sync_before_reliable_state() {
    use crate::net::event_packet::encode_live_frame_sync;
    use crate::sim::command_queue::FrameInfo;

    let mut left = transport();
    let mut right = transport();
    let left_address = left.primary_addr().unwrap();
    let right_address = right.primary_addr().unwrap();
    let former = SessionScope {
        epoch: [37; 16],
        revision: (1_u64 << 32) + 9,
    };
    for (wire, peer, address) in [(&mut left, 1, right_address), (&mut right, 0, left_address)] {
        wire.set_lan_scope(former).unwrap();
        wire.connections_mut().add_peer(peer, address).unwrap();
    }
    let old_sync = encode_live_frame_sync(
        FrameInfo {
            house_id: 0,
            event_frame: 10,
            checksum: 19,
            timing_word: 0,
            delay: 10,
        },
        0,
    )
    .unwrap();
    assert!(
        left.connections_mut()
            .send_to(1, b"former orders", Delivery::Reliable)
    );
    assert!(
        left.connections_mut()
            .send_to(1, &old_sync, Delivery::Unreliable)
    );
    assert!(left.global_mut().send(
        b"former match side channel",
        GlobalDestination::direct(right_address),
        Delivery::Reliable,
        0
    ));
    assert_eq!(left.service(1).sent_datagrams, 3);
    let old_packets = take_wire_datagrams(&right, 3);
    for packet in &old_packets {
        left.lock().primary.send_to(packet, right_address).unwrap();
    }
    assert_eq!(malformed_after_receipt(&mut right, 1, 3), 0);
    let old_acks = take_wire_datagrams(&left, 2);

    // Same host epoch, different final configuration. The low32 revision is
    // deliberately equal, so truncating the full agreed u64 is observable.
    let current = SessionScope {
        revision: 9,
        ..former
    };
    for (wire, peer, address) in [(&mut left, 1, right_address), (&mut right, 0, left_address)] {
        wire.reset_session();
        wire.set_lan_scope(current).unwrap();
        wire.connections_mut().add_peer(peer, address).unwrap();
    }
    assert!(
        left.connections_mut()
            .send_to(1, b"current orders", Delivery::Reliable)
    );
    assert!(left.global_mut().send(
        b"current match side channel",
        GlobalDestination::direct(right_address),
        Delivery::Reliable,
        0
    ));
    assert_eq!(left.service(1).sent_datagrams, 2);
    let current_packets = take_wire_datagrams(&right, 2);
    for ack in &old_acks {
        right.lock().primary.send_to(ack, left_address).unwrap();
    }
    assert_eq!(malformed_after_receipt(&mut left, 1, 2), 2);
    assert_eq!(
        left.connections().queued_send_count(1),
        Some(1),
        "old ACK retired the new reliable sequence0"
    );
    assert_eq!(
        left.global().queued_send_count(),
        1,
        "old ACK retired the new global sequence0"
    );
    for packet in &old_packets {
        left.lock().primary.send_to(packet, right_address).unwrap();
    }
    assert_eq!(malformed_after_receipt(&mut right, 1, 3), 3);
    assert!(right.pop_received().is_none());
    assert!(right.pop_global_received().is_none());
    for packet in &current_packets {
        left.lock().primary.send_to(packet, right_address).unwrap();
    }
    assert_eq!(received(&mut right).payload, b"current orders");
    assert_eq!(
        right.pop_global_received().unwrap().payload,
        b"current match side channel"
    );
    assert_eq!(malformed_after_receipt(&mut left, 1, 2), 0);
    assert_eq!(left.connections().queued_send_count(1), Some(0));
    assert_eq!(left.global().queued_send_count(), 0);

    // A fresh host visit can reuse the old revision; all128 epoch bits still
    // separate its FRAMESYNC from the former match's seed/progress path.
    right.reset_session();
    right
        .set_lan_scope(SessionScope {
            epoch: [41; 16],
            ..former
        })
        .unwrap();
    right.connections_mut().add_peer(0, left_address).unwrap();
    let packet = old_packets
        .iter()
        .find(|packet| packet.ends_with(&old_sync))
        .unwrap();
    left.lock().primary.send_to(packet, right_address).unwrap();
    assert_eq!(malformed_after_receipt(&mut right, 4, 1), 1);
    assert!(right.pop_received().is_none());
}
