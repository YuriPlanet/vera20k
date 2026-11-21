//! Start admission tests: start identity and agreement checks, the startup READY
//! barrier and its retries, initial response histories and the initial probe cadence.

use super::*;

/// Games that load the same world but would play it differently, another
/// build, other rules or picture and model files the simulation reads
/// differently, never certify one start; the start report names the part
/// that differs.
#[test]
fn start_identity_differs_by_build_and_by_rules() {
    let agreement = |version: &str, rules: &str, file_inputs: u64| {
        let rate = NonZeroU32::new(1).unwrap();
        StartupAgreement::new(
            [(0, 0), (1, 1)],
            [0, 1],
            73,
            10,
            30,
            rate,
            0,
            file_inputs,
            version,
            rules,
        )
        .unwrap()
    };
    let base = agreement("1.0:a:0000", "rules", 0);
    for (other, part) in [
        (agreement("1.0:b:0000", "rules", 0), "build"),
        (agreement("1.0:a:0000", "other", 0), "rules"),
        (agreement("1.0:a:0000", "rules", 7), "asset_files"),
    ] {
        assert_ne!(
            other.sequencing_identity().unwrap().0,
            base.sequencing_identity().unwrap().0
        );
        let differs: Vec<_> = base
            .report_parts()
            .into_iter()
            .zip(other.report_parts())
            .filter(|(ours, theirs)| ours != theirs)
            .map(|(ours, _)| ours.0)
            .collect();
        assert_eq!(differs, [part]);
    }
}

/// A real private-UDP cohort; the agreement is the install-time snapshot,
/// never inferred from subsequent probes or the mutable connection table.
fn startup_cohort(active: &[u8], hashes: &[u64]) -> Vec<LiveCommandSession> {
    assert_eq!(active.len(), hashes.len());
    let mut wires: Vec<_> = active.iter().map(|_| transport()).collect();
    let addresses: Vec<_> = wires
        .iter()
        .map(|wire| wire.primary_addr().unwrap())
        .collect();
    for (index, wire) in wires.iter_mut().enumerate() {
        for (peer, &source) in active.iter().enumerate().filter(|(peer, _)| *peer != index) {
            wire.connections_mut()
                .add_peer(i32::from(source), addresses[peer])
                .unwrap();
        }
    }
    wires
        .into_iter()
        .enumerate()
        .map(|(index, wire)| {
            let rate = NonZeroU32::new(5).unwrap();
            let rows = [2, 7, 11];
            let local = active[index];
            let original = std::iter::once(local).chain((0..3).filter(|&source| source != local));
            let agreement = StartupAgreement::new(
                original.map(|source| (rows[source as usize], source as i8)),
                active.iter().map(|&source| rows[source as usize]),
                73,
                10,
                30,
                rate,
                hashes[index],
                0,
                "",
                "",
            )
            .unwrap();
            LiveCommandSession::new(
                agreement,
                wire,
                active[index] as i8,
                73,
                rate,
                if active.len() == 3 {
                    LiveCommandMode::Lan
                } else {
                    LiveCommandMode::Wol
                },
                None,
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn full_and_majority_loading_rosters_require_the_same_frozen_world() {
    for active in [&[0, 1, 2][..], &[1, 2][..]] {
        let mut worlds: Vec<_> = active.iter().map(|_| Simulation::with_seed(73)).collect();
        let hash = worlds[0].state_hash();
        let mut clients = startup_cohort(active, &vec![hash; active.len()]);
        let start = Instant::now();
        let mut passes = vec![NetworkQueuePass::Waiting; active.len()];
        while passes.contains(&NetworkQueuePass::Waiting) {
            for (index, client) in clients.iter_mut().enumerate() {
                passes[index] = client
                    .poll(
                        &mut worlds[index].network_command_queues(),
                        NetworkQueueContext {
                            frame: 0,
                            checksum: 7,
                        },
                        10,
                        100 + (start.elapsed().as_millis() / 16) as u32,
                        1600,
                        0,
                    )
                    .unwrap();
            }
            assert!(start.elapsed() < STALL_LIMIT);
            std::thread::yield_now();
        }
        assert!(
            passes
                .iter()
                .all(|pass| *pass == NetworkQueuePass::SkipDispatch)
        );
        assert!(
            clients
                .iter()
                .all(|client| !client.startup_pending() && client.sent_commands() == 0)
        );
        assert!(worlds.iter().all(|world| {
            world.pending_command_snapshot().iter().all(|entry| {
                matches!(&entry.payload, crate::sim::command::Command::SynchronizedRecord { record }
                if record.opcode()==0x1c)
            })
        }));
    }
    assert_eq!(
        StartupAgreement::new(
            [(0, 0), (1, 1), (2, 2)],
            [2],
            73,
            10,
            30,
            NonZeroU32::new(5).unwrap(),
            0,
            0,
            "",
            ""
        ),
        Err(StartupError::NoMajority)
    );
    let mut worlds = [Simulation::with_seed(73), Simulation::with_seed(74)];
    let hashes = worlds.each_ref().map(|world| world.state_hash());
    assert_ne!(hashes[0], hashes[1]);
    let mut clients = startup_cohort(&[1, 2], &hashes);
    let start = Instant::now();
    let mut rejected = [false; 2];
    while rejected.contains(&false) {
        for index in 0..2 {
            if rejected[index] {
                continue;
            }
            match clients[index].poll(
                &mut worlds[index].network_command_queues(),
                NetworkQueueContext {
                    frame: 0,
                    checksum: 7,
                },
                10,
                100,
                1600,
                0,
            ) {
                Err(LiveCommandError::Startup(StartupError::Mismatch)) => rejected[index] = true,
                Ok(NetworkQueuePass::Waiting) => {}
                other => panic!("unexpected startup result {other:?}"),
            }
        }
        assert!(start.elapsed() < STALL_LIMIT);
        std::thread::yield_now();
    }
    assert!(clients.iter().all(|client| client.sent_commands() == 0));
    assert!(
        worlds
            .iter_mut()
            .all(|world| world.network_command_queues().synchronized_len() == 0)
    );
}

/// A byte-preserving two-socket test relay. It only loses one current READY
/// and replays three retired datagrams; all controls, checksums and ACKs come
/// from the production transport and reliability owners.
#[test]
fn startup_ready_retries_after_release_and_old_scope_cannot_certify_rematch() {
    use crate::net::lan_messages::SessionScope;

    struct Proxy {
        sockets: [UdpSocket; 2],
        endpoints: [SocketAddrV4; 2],
        hold_ready: bool,
        ready_attempts: usize,
        held: Option<Vec<u8>>,
        old_hello: Option<Vec<u8>>,
        old_ready: Option<Vec<u8>>,
        old_ack: Option<Vec<u8>>,
    }
    impl Proxy {
        fn pump(&mut self) {
            // The LAN scope header, the UDP checksum, then ConnectionClass.
            use crate::net::udp::{CHECKSUM_LEN, LAN_SCOPE_HEADER_LEN, LAN_SCOPE_MAGIC};
            const CONNECTION_OFFSET: usize = LAN_SCOPE_HEADER_LEN + CHECKSUM_LEN;
            for side in 0..2 {
                loop {
                    let mut packet = [0; crate::net::udp::UDP_DATAGRAM_CAPACITY];
                    let (len, source) = match self.sockets[side].recv_from(&mut packet) {
                        Ok(received) => received,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => panic!("proxy receive: {error}"),
                    };
                    assert_eq!(source, std::net::SocketAddr::V4(self.endpoints[side]));
                    let packet = &packet[..len];
                    assert!(packet.starts_with(LAN_SCOPE_MAGIC));
                    let frame = &packet[CONNECTION_OFFSET..];
                    let control = frame
                        .get(crate::net::reliable::CONNECTION_HEADER_LEN..)
                        .filter(|payload| payload.starts_with(b"VSA1"));
                    if side == 0
                        && let Some(control) = control
                    {
                        if control[4] == 1 {
                            self.old_hello.get_or_insert_with(|| packet.to_vec());
                        }
                        if control[4] == 2 {
                            self.old_ready.get_or_insert_with(|| packet.to_vec());
                            if self.hold_ready {
                                self.ready_attempts += 1;
                                self.held = Some(packet.to_vec());
                                continue;
                            }
                        }
                    }
                    if side == 1 && frame[2] == 2 && crate::net::reliable::sequence(frame) == 0 {
                        self.old_ack.get_or_insert_with(|| packet.to_vec());
                    }
                    self.sockets[1 - side]
                        .send_to(packet, self.endpoints[1 - side])
                        .unwrap();
                }
            }
        }
    }
    let mut wires = [transport(), transport()];
    let mut proxy = Proxy {
        sockets: std::array::from_fn(|_| {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            socket.set_nonblocking(true).unwrap();
            socket
        }),
        endpoints: wires.each_ref().map(|wire| wire.primary_addr().unwrap()),
        hold_ready: true,
        ready_attempts: 0,
        held: None,
        old_hello: None,
        old_ready: None,
        old_ack: None,
    };
    let proxy_addresses =
        proxy
            .sockets
            .each_ref()
            .map(|socket| match socket.local_addr().unwrap() {
                std::net::SocketAddr::V4(address) => address,
                _ => unreachable!(),
            });
    let scope = |epoch| SessionScope {
        epoch: [epoch; 16],
        revision: 1,
    };
    let rate = NonZeroU32::new(5).unwrap();
    for (side, wire) in wires.iter_mut().enumerate() {
        wire.set_lan_scope(scope(1)).unwrap();
        wire.connections_mut()
            .add_peer((1 - side) as i32, proxy_addresses[side])
            .unwrap();
        wire.connections_mut().set_timing(RetryTiming {
            retry_delta: 3,
            max_retries: u32::MAX,
            timeout: 600,
        });
    }
    let agreement = StartupAgreement::new(
        [(0, 0), (1, 1)],
        [0, 1],
        73,
        10,
        30,
        rate,
        Simulation::with_seed(73).state_hash(),
        0,
        "",
        "",
    )
    .unwrap();
    let mut clients = wires
        .into_iter()
        .enumerate()
        .map(|(side, wire)| {
            LiveCommandSession::new(
                agreement.clone(),
                wire,
                side as i8,
                73,
                rate,
                LiveCommandMode::Lan,
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut worlds = [Simulation::with_seed(73), Simulation::with_seed(73)];
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 7,
    };
    let mut passes = [NetworkQueuePass::Waiting; 2];
    let mut now = 100;
    let started = Instant::now();
    while passes[0] == NetworkQueuePass::Waiting {
        for side in 0..2 {
            passes[side] = clients[side]
                .poll(
                    &mut worlds[side].network_command_queues(),
                    context,
                    10,
                    now,
                    now * 16,
                    0,
                )
                .unwrap();
        }
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < STALL_LIMIT);
        std::thread::yield_now();
    }
    assert_eq!(
        passes,
        [NetworkQueuePass::SkipDispatch, NetworkQueuePass::Waiting]
    );
    assert!(!clients[0].startup_pending());
    assert!(clients[1].startup_pending());
    while clients[0].connections().queued_send_count(1) != Some(1) {
        clients[0].service_transport(now);
        passes[1] = clients[1]
            .poll(
                &mut worlds[1].network_command_queues(),
                context,
                10,
                now,
                now * 16,
                0,
            )
            .unwrap();
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < STALL_LIMIT);
        std::thread::yield_now();
    }
    // The private reliable owner reports timeout but keeps the SAME entry and
    // retry cadence. No new startup retry owner or timer extends native waits.
    now += 700;
    clients[0].service_transport(now);
    proxy.pump();
    assert_eq!(
        clients[0].take_service_report().unwrap().failed_peer,
        Some(1)
    );
    while proxy.ready_attempts < 2 {
        proxy.pump();
        assert!(started.elapsed() < STALL_LIMIT);
        std::thread::yield_now();
    }
    proxy.hold_ready = false;
    proxy.sockets[1]
        .send_to(proxy.held.take().unwrap().as_slice(), proxy.endpoints[1])
        .unwrap();
    while passes[1] == NetworkQueuePass::Waiting
        || clients.iter().enumerate().any(|(side, client)| {
            client.connections().queued_send_count((1 - side) as i32) != Some(0)
        })
    {
        for side in 0..2 {
            passes[side] = clients[side]
                .poll(
                    &mut worlds[side].network_command_queues(),
                    context,
                    10,
                    now,
                    now * 16,
                    0,
                )
                .unwrap();
            clients[side].service_transport(now);
        }
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < STALL_LIMIT);
        std::thread::yield_now();
    }
    assert_eq!(passes, [NetworkQueuePass::SkipDispatch; 2]);
    // Keep both physical sockets and endpoints; retire only the match scope.
    worlds = [Simulation::with_seed(73), Simulation::with_seed(73)];
    let mut clients = clients
        .into_iter()
        .enumerate()
        .map(|(side, client)| {
            let mut wire = client.into_transport();
            wire.clear_lan_scope();
            wire.set_lan_scope(scope(2)).unwrap();
            wire.connections_mut()
                .add_peer((1 - side) as i32, proxy_addresses[side])
                .unwrap();
            LiveCommandSession::new(
                agreement.clone(),
                wire,
                side as i8,
                73,
                rate,
                LiveCommandMode::Lan,
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        clients[0]
            .poll(
                &mut worlds[0].network_command_queues(),
                context,
                10,
                now,
                now * 16,
                0
            )
            .unwrap(),
        NetworkQueuePass::Waiting
    );
    // Sequence zero is reused by the real reliability owner. An old ACK must
    // not retire the fresh HELLO, even with identical seed/world/endpoints.
    clients[0].take_service_report();
    proxy.sockets[0]
        .send_to(proxy.old_ack.as_ref().unwrap(), proxy.endpoints[0])
        .unwrap();
    let deadline = Instant::now() + STALL_LIMIT;
    loop {
        clients[0].service_transport(now);
        if clients[0]
            .take_service_report()
            .is_some_and(|report| report.malformed_datagrams != 0)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(clients[0].connections().queued_send_count(1), Some(1));
    for old in [&proxy.old_hello, &proxy.old_ready] {
        proxy.sockets[1]
            .send_to(old.as_ref().unwrap(), proxy.endpoints[1])
            .unwrap();
    }
    let mut rejected = 0;
    let deadline = Instant::now() + STALL_LIMIT;
    while rejected < 2 {
        clients[1].service_transport(now);
        rejected += clients[1]
            .take_service_report()
            .unwrap()
            .malformed_datagrams;
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        clients[1]
            .poll(
                &mut worlds[1].network_command_queues(),
                context,
                10,
                now,
                now * 16,
                0
            )
            .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert!(clients[1].startup_pending());
    assert_eq!(clients[1].sent_commands(), 0);
    assert!(clients[1].peers().iter().all(|peer| peer.frame() == -1));
    passes = [NetworkQueuePass::Waiting; 2];
    while passes.contains(&NetworkQueuePass::Waiting) {
        for side in 0..2 {
            passes[side] = clients[side]
                .poll(
                    &mut worlds[side].network_command_queues(),
                    context,
                    10,
                    now,
                    now * 16,
                    0,
                )
                .unwrap();
        }
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < STALL_LIMIT);
        std::thread::yield_now();
    }
    assert_eq!(passes, [NetworkQueuePass::SkipDispatch; 2]);
}

#[test]
fn initial_barrier_off_cadence_and_positive_frame_use_one_native_queue() {
    let (mut left, mut right) = pair(73);
    let mut a = Simulation::new();
    let mut b = Simulation::new();
    assert_eq!(
        poll_pair(&mut left, &mut right, &mut a, &mut b, 0),
        (
            NetworkQueuePass::SkipDispatch,
            NetworkQueuePass::SkipDispatch
        )
    );
    let command = CommandRecord::encode(6, 0, 1, &[42, 0, 0, 0, 0x34]).unwrap();
    assert!(a.issue_local_command(CommandEnvelope::synchronized(command, 1)));
    for frame in 1..5 {
        assert_eq!(
            poll_pair(&mut left, &mut right, &mut a, &mut b, frame),
            (
                NetworkQueuePass::SkipDispatch,
                NetworkQueuePass::SkipDispatch
            )
        );
        assert_eq!(a.network_command_queues().local_len(), 1);
    }
    assert_eq!(
        poll_pair(&mut left, &mut right, &mut a, &mut b, 5),
        (NetworkQueuePass::Dispatch, NetworkQueuePass::Dispatch)
    );
    assert_eq!(left.sent_commands(), 1);
    assert_eq!(right.sent_commands(), 0);
    assert_eq!(a.network_command_queues().local_len(), 0);
    assert_eq!(b.network_command_queues().local_len(), 0);
    // The same frame result is idempotent and cannot create another send.
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            NetworkQueueContext {
                frame: 5,
                checksum: 0x12345678
            },
            10,
            1001,
            (1001_u32).wrapping_mul(16),
            0
        )
        .unwrap(),
        NetworkQueuePass::Dispatch
    );
    assert_eq!(left.sent_commands(), 1);
    // Frame5 still has credit from the frame0 packet, so it may dispatch
    // before the other socket's frame5 packet arrives. At frame10 the strict
    // minimum-frame gate6495F9 requires newer progress; its command count
    // must also be satisfied before the delayed event can execute at15.
    for frame in 6..=10 {
        poll_pair(&mut left, &mut right, &mut a, &mut b, frame);
    }
    for world in [&a, &b] {
        let records = world.pending_command_snapshot();
        assert_eq!(
            records
                .iter()
                .filter(|event| matches!(&event.payload,
            crate::sim::command::Command::SynchronizedRecord { record }
            if record.opcode() == 6 && record.frame_stamp() == 15 && record.house_id() == 0))
                .count(),
            1
        );
    }
}

#[test]
fn initial_response_histories_clear_only_on_successful_barrier_once() {
    use crate::net::global::GlobalDestination;
    use crate::net::reliable::connection_prefix;

    fn seed_history(
        live: &mut LiveCommandSession,
        endpoint: SocketAddrV4,
        sequence: u32,
        now: u32,
    ) {
        assert!(live.transport.connections_mut().send_to(
            1,
            b"private history",
            Delivery::Reliable
        ));
        live.transport
            .connections_mut()
            .service(now, &mut |_, _| {});
        live.transport
            .connections_mut()
            .receive(
                endpoint,
                &connection_prefix(0xaa04, 2, sequence + 2, 0),
                &mut |_, _| {},
            )
            .unwrap();
        live.transport
            .connections_mut()
            .service(now + 11, &mut |_, _| {});
        assert!(live.global_mut().send(
            b"global history",
            GlobalDestination::direct(endpoint),
            Delivery::Reliable,
            0
        ));
        live.global_mut().service(now, &mut |_, _| {});
        live.global_mut()
            .receive(
                endpoint,
                0,
                &connection_prefix(0x1236, 2, sequence, 2),
                &mut |_, _| {},
            )
            .unwrap();
        live.global_mut().service(now + 22, &mut |_, _| {});
    }

    let mut wire = transport();
    let peer_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let std::net::SocketAddr::V4(peer) = peer_socket.local_addr().unwrap() else {
        unreachable!()
    };
    wire.connections_mut().add_peer(1, peer).unwrap();
    let mut live = LiveCommandSession::new(
        fixture_agreement(&wire, 0, 73, NonZeroU32::new(5).unwrap()),
        wire,
        0,
        73,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Lan,
        None,
    )
    .unwrap();
    certify_native_probe_fixture(&mut live);
    seed_history(&mut live, peer, 0, 100);
    let mut sim = Simulation::new();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 0x12345678,
    };
    for now in [1000, 1001] {
        assert_eq!(
            live.poll(
                &mut sim.network_command_queues(),
                context,
                10,
                now,
                now * 16,
                0
            )
            .unwrap(),
            NetworkQueuePass::Waiting
        );
        assert_eq!(live.connections().mean_response_time(1), Some(11));
        assert_eq!(live.global().mean_response_time(), 22);
    }
    let probe = live
        .exchange
        .frame_probe(1, 0, 10, 73, live.policy)
        .unwrap();
    live.exchange
        .receive(1, &probe, 73, &mut sim.network_command_queues(), 1)
        .unwrap();
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            context,
            10,
            1002,
            16032,
            0
        )
        .unwrap(),
        NetworkQueuePass::SkipDispatch
    );
    assert_eq!(live.connections().mean_response_time(1), Some(0));
    assert_eq!(live.global().mean_response_time(), 0);
    assert_eq!(live.peers()[0].frame(), 0);

    seed_history(&mut live, peer, 1, 1100);
    for next in [
        context,
        NetworkQueueContext {
            frame: 1,
            ..context
        },
    ] {
        assert_eq!(
            live.poll(&mut sim.network_command_queues(), next, 10, 1200, 19200, 0)
                .unwrap(),
            NetworkQueuePass::SkipDispatch
        );
        assert_eq!(live.connections().mean_response_time(1), Some(11));
        assert_eq!(live.global().mean_response_time(), 22);
    }
}

#[test]
fn ordinary_wait_uses_private_response_history_with_larger_primary_global_mean() {
    use crate::net::global::GlobalDestination;
    use crate::net::reliable::connection_prefix;

    let mut wire = transport();
    let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let std::net::SocketAddr::V4(peer) = receiver.local_addr().unwrap() else {
        unreachable!()
    };
    wire.connections_mut().add_peer(1, peer).unwrap();
    let mut live = LiveCommandSession::new(
        fixture_agreement(&wire, 0, 73, NonZeroU32::new(5).unwrap()),
        wire,
        0,
        73,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Lan,
        None,
    )
    .unwrap();
    certify_native_probe_fixture(&mut live);
    let mut sim = Simulation::new();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 0x12345678,
    };
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            context,
            5,
            1000,
            16000,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    let probe = live.exchange.frame_probe(1, 0, 5, 73, live.policy).unwrap();
    live.exchange
        .receive(1, &probe, 73, &mut sim.network_command_queues(), 1)
        .unwrap();
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            context,
            5,
            1001,
            16016,
            0
        )
        .unwrap(),
        NetworkQueuePass::SkipDispatch
    );

    assert!(
        live.transport
            .connections_mut()
            .send_to(1, b"private sample", Delivery::Reliable)
    );
    live.transport
        .connections_mut()
        .service(1100, &mut |_, _| {});
    live.transport
        .connections_mut()
        .receive(peer, &connection_prefix(0xaa04, 2, 2, 0), &mut |_, _| {})
        .unwrap();
    live.transport
        .connections_mut()
        .service(1110, &mut |_, _| {});
    assert!(live.global_mut().send(
        b"global sample",
        GlobalDestination::direct(peer),
        Delivery::Reliable,
        0
    ));
    live.global_mut().service(1100, &mut |_, _| {});
    live.global_mut()
        .receive(peer, 0, &connection_prefix(0x1236, 2, 0, 2), &mut |_, _| {})
        .unwrap();
    live.global_mut().service(1300, &mut |_, _| {});
    assert_eq!(live.connections().mean_response_time(1), Some(10));
    assert_eq!(live.global().mean_response_time(), 200);
    for frame in 1..5 {
        assert_eq!(
            live.poll(
                &mut sim.network_command_queues(),
                NetworkQueueContext { frame, ..context },
                5,
                1300 + frame,
                (1300 + frame) * 16,
                0
            )
            .unwrap(),
            NetworkQueuePass::SkipDispatch
        );
    }
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            NetworkQueueContext {
                frame: 5,
                ..context
            },
            5,
            1305,
            20880,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(
        live.wait_info(),
        Some(LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Ordinary,
            entered_at: 1305,
            response_time_at_entry: 10,
        })
    );
}

#[test]
fn online_initial_probe_waits_for_the_online_table_boundary() {
    let mut wire = transport();
    let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let std::net::SocketAddr::V4(peer) = receiver.local_addr().unwrap() else {
        unreachable!()
    };
    wire.connections_mut().add_peer(1, peer).unwrap();
    let mut live = LiveCommandSession::new(
        fixture_agreement(&wire, 0, 73, NonZeroU32::new(5).unwrap()),
        wire,
        0,
        73,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Wol,
        None,
    )
    .unwrap();
    certify_native_probe_fixture(&mut live);
    let mut sim = Simulation::new();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 1,
    };
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1000,
        16000,
        0,
    )
    .unwrap();
    live.take_service_report();
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1119,
        17904,
        0,
    )
    .unwrap();
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 0);
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1120,
        17920,
        0,
    )
    .unwrap();
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 1);
    assert_eq!(live.wait_info().unwrap().mode, LiveCommandMode::Wol);
}

#[test]
fn initial_probe_repeats_only_at_the_native_wall_clock_boundary() {
    let mut wire = transport();
    let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    receiver.set_read_timeout(Some(STALL_LIMIT)).unwrap();
    let address = match receiver.local_addr().unwrap() {
        std::net::SocketAddr::V4(a) => a,
        _ => unreachable!(),
    };
    wire.connections_mut().add_peer(1, address).unwrap();
    let mut live = LiveCommandSession::new(
        fixture_agreement(&wire, 0, 91, NonZeroU32::new(5).unwrap()),
        wire,
        0,
        91,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Lan,
        None,
    )
    .unwrap();
    certify_native_probe_fixture(&mut live);
    let mut sim = Simulation::new();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 10,
    };
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            context,
            10,
            1000,
            (1000_u32).wrapping_mul(16),
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(
        live.wait_info(),
        Some(LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Initial,
            entered_at: 1000,
            response_time_at_entry: 0,
        })
    );
    let mut bytes = [0; 640];
    let (length, _) = receiver.recv_from(&mut bytes).unwrap();
    assert_eq!(
        crate::net::event_packet::decode_frame_sync(&bytes[18..length])
            .unwrap()
            .checksum,
        91
    );
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            context,
            10,
            1059,
            (1059_u32).wrapping_mul(16),
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 0);
    assert_eq!(
        live.poll(
            &mut sim.network_command_queues(),
            context,
            10,
            1060,
            (1060_u32).wrapping_mul(16),
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 1);
    let (length, _) = receiver.recv_from(&mut bytes).unwrap();
    assert!(crate::net::event_packet::decode_frame_sync(&bytes[18..length]).is_some());
    assert!(live.restart_wait_probe(1100));
    assert_eq!(live.wait_info().unwrap().entered_at, 1000);
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1159,
        (1159_u32).wrapping_mul(16),
        0,
    )
    .unwrap();
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 0);
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1160,
        (1160_u32).wrapping_mul(16),
        0,
    )
    .unwrap();
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 1);
}
