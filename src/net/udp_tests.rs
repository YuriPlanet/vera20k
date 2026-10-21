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
        transport.primary_send.send(
            endpoint,
            &connection_frame(1, sequence, b"pooled or heap"),
            UdpChannel::Primary,
            &mut report,
            &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
        );
    }
    transport.alternate_send.send(
        endpoint,
        b"alternate",
        UdpChannel::Alternate,
        &mut report,
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );
    assert_eq!(transport.primary_send.pending.len(), 130);
    assert_eq!(transport.alternate_send.pending.len(), 1);
    let mut alternate = Vec::new();
    transport
        .alternate_send
        .service(UdpChannel::Alternate, &mut report, &mut |bytes, _| {
            alternate.extend_from_slice(bytes);
            Ok(bytes.len())
        });
    assert_eq!(alternate, datagram(b"alternate"));
    assert_eq!(transport.primary_send.pending.len(), 130);
    transport.alternate_send.send(
        endpoint,
        b"discard",
        UdpChannel::Alternate,
        &mut report,
        &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
    );
    transport.clear_pending_sends();
    assert!(transport.primary_send.pending.is_empty());
    assert!(transport.alternate_send.pending.is_empty());
    assert_eq!(transport.connections().queued_send_count(1), Some(1));
}

#[test]
fn portable_service_retries_one_retained_head_without_new_protocol_send() {
    let mut transport = transport();
    let sink = raw_peer();
    let endpoint = ipv4_addr(&sink).unwrap();
    let mut report = UdpServiceReport::default();
    for frame in [b"first".as_slice(), b"second".as_slice()] {
        transport.primary_send.send(
            endpoint,
            frame,
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
        assert_eq!(transport.primary_send.pending.len(), remaining);
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
        receiver.primary_send.send(
            ipv4_addr(&sink).unwrap(),
            frame,
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
        match receiver.alternate.peek_from(&mut bytes) {
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
    assert_eq!(receiver.primary_send.pending.len(), 1);
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
    assert_eq!(left.response_time(), 1);
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
    for (queue, channel) in [
        (&mut first.primary_send, UdpChannel::Primary),
        (&mut first.alternate_send, UdpChannel::Alternate),
    ] {
        queue.send(
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234),
            b"old raw attempt",
            channel,
            &mut report,
            &mut |_, _| Err(io::ErrorKind::PermissionDenied.into()),
        );
        assert_eq!(queue.pending.len(), 1);
    }
    drop(first);
    let fresh = LanUdpTransport::bind(config, connections()).unwrap();
    assert_eq!(fresh.connections().peers().len(), 0);
    assert!(fresh.primary_send.pending.is_empty());
    assert!(fresh.alternate_send.pending.is_empty());
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
    wire.primary_send.send(
        destination,
        b"old raw",
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
