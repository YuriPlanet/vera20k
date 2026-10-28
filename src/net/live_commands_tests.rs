use super::*;
use crate::net::reliable::LanConnectionConfig;
use crate::net::udp::UdpBindConfig;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::command::{CommandEnvelope, CommandRecord};
use crate::sim::command_queue::{FrameChecksumState, FrameInfo};
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{
    FrameEffects, NetworkCommandBoundary, NetworkCommandError, SimFrameOutput, Simulation, TickLane,
};
use std::cell::{Cell, RefCell};
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

fn transport() -> LanUdpTransport {
    LanUdpTransport::bind(
        UdpBindConfig {
            primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        },
        LanConnections::new(
            LanConnectionConfig::default(),
            RetryTiming {
                retry_delta: 0,
                max_retries: u32::MAX,
                timeout: u32::MAX,
            },
        ),
    )
    .unwrap()
}

#[test]
fn wol_catchup_pacing_uses_admitted_peer_progress_and_existing_rtt() {
    let (mut live, _) = pair(73);
    live.mode = LiveCommandMode::Wol;
    let mut sim = Simulation::new();
    let packet = crate::net::event_packet::EventPacketCodec::default()
        .encode_packet(
            &[],
            crate::net::event_packet::EventPacketHeader::Vera {
                frame_info: FrameInfo {
                    house_id: 1,
                    event_frame: 0,
                    checksum: 0,
                    timing_word: 0,
                    delay: 0,
                },
                command_frontier: 0,
            },
            0,
            crate::net::event_packet::NATIVE_EVENT_PACKET_CAPACITY,
        )
        .unwrap()
        .bytes;
    assert!(
        live.exchange
            .receive(1, &packet, 73, &mut sim.network_command_queues(), 0)
            .unwrap()
    );
    live.exchange.record_peer_lag(1, 40, 60, 40);
    assert_eq!(live.peers()[0].lag_frames(), 20);
    assert_eq!(live.pacing_delay_millis(40), 20);
    assert_eq!(live.pacing_delay_millis(30), 0);
    assert!(
        !live
            .exchange
            .receive(1, &packet, 73, &mut sim.network_command_queues(), 0)
            .unwrap()
    );
    assert_eq!(live.pacing_delay_millis(40), 20);
    live.remove_peer(1);
    assert_eq!(live.pacing_delay_millis(40), 0);
}

// Pure native queue/probe fixtures declare their complete protocol snapshot.
// The new certificate regressions below exercise real sockets and real hashes.
fn fixture_agreement(
    wire: &LanUdpTransport,
    local: i8,
    seed: u32,
    rate: NonZeroU32,
) -> StartupAgreement {
    let mut houses: Vec<_> = wire
        .connections()
        .peers()
        .filter_map(|(house, _)| i8::try_from(house).ok())
        .filter(|house| (0..32).contains(house))
        .collect();
    houses.push(local);
    houses.sort_unstable();
    houses.dedup();
    StartupAgreement::new(
        houses
            .iter()
            .copied()
            .enumerate()
            .map(|(row, house)| (row as u8, house)),
        0..houses.len() as u8,
        seed,
        10,
        30,
        rate,
        0,
    )
    .unwrap()
}

/// The following four tests isolate native probe/history timing after the
/// modern reliable certificate is a satisfied precondition. Use the same
/// control codec, admission and reliable ACK/retirement owners; no simulation
/// frame or native probe is synthesized by this fixture preparation.
fn certify_native_probe_fixture(live: &mut LiveCommandSession) {
    let rate = NonZeroU32::new(5).unwrap();
    let agreement = fixture_agreement(&live.transport, live.local_house, live.initial_seed, rate);
    let mut houses: Vec<_> = live.peers().iter().map(|peer| peer.house_id()).collect();
    houses.push(live.local_house);
    let controls: Vec<_> = houses
        .iter()
        .copied()
        .filter(|&house| house != live.local_house)
        .map(|house| {
            let peer = startup::StartupHandshake::new(
                agreement.clone(),
                house,
                houses.iter().copied().filter(|&other| other != house),
                live.initial_seed,
                rate,
            )
            .unwrap();
            (house, peer.control(1), peer.control(2))
        })
        .collect();
    live.flush_startup();
    for (house, hello, _) in &controls {
        assert!(
            live.startup
                .receive(*house, hello, Delivery::Reliable)
                .unwrap()
        );
    }
    live.flush_startup();
    for (house, _, ready) in &controls {
        assert!(
            live.startup
                .receive(*house, ready, Delivery::Reliable)
                .unwrap()
        );
    }
    let mut sent = Vec::new();
    live.transport
        .connections_mut()
        .service(1, &mut |address, bytes| {
            sent.push((address, bytes.to_vec()))
        });
    for (address, bytes) in sent {
        assert!(bytes[crate::net::reliable::CONNECTION_HEADER_LEN..].starts_with(b"VSA1"));
        let ack = crate::net::reliable::connection_prefix(
            0xaa04,
            2,
            crate::net::reliable::sequence(&bytes),
            0,
        );
        live.transport
            .connections_mut()
            .receive(address, &ack, &mut |_, _| {})
            .unwrap();
    }
    live.transport.connections_mut().service(2, &mut |_, _| {});
    live.transport.reset_response_time(true);
    assert!(live.startup.complete());
}

fn pair(seed: u32) -> (LiveCommandSession, LiveCommandSession) {
    let mut left = transport();
    let mut right = transport();
    left.connections_mut()
        .add_peer(1, right.primary_addr().unwrap())
        .unwrap();
    right
        .connections_mut()
        .add_peer(0, left.primary_addr().unwrap())
        .unwrap();
    (
        LiveCommandSession::new(
            fixture_agreement(&left, 0, seed, NonZeroU32::new(5).unwrap()),
            left,
            0,
            seed,
            NonZeroU32::new(5).unwrap(),
            LiveCommandMode::Lan,
        )
        .unwrap(),
        LiveCommandSession::new(
            fixture_agreement(&right, 1, seed, NonZeroU32::new(5).unwrap()),
            right,
            1,
            seed,
            NonZeroU32::new(5).unwrap(),
            LiveCommandMode::Lan,
        )
        .unwrap(),
    )
}

fn poll_pair(
    left: &mut LiveCommandSession,
    right: &mut LiveCommandSession,
    left_world: &mut Simulation,
    right_world: &mut Simulation,
    frame: u32,
) -> (NetworkQueuePass, NetworkQueuePass) {
    let start = Instant::now();
    let context = NetworkQueueContext {
        frame,
        checksum: 0x12345678,
    };
    let mut passes = [NetworkQueuePass::Waiting; 2];
    while passes.contains(&NetworkQueuePass::Waiting) {
        let now = 1000 + (start.elapsed().as_millis() / 16) as u32;
        passes[0] = left
            .poll(
                &mut left_world.network_command_queues(),
                context,
                10,
                now,
                (now).wrapping_mul(16),
                0,
                0,
            )
            .unwrap();
        passes[1] = right
            .poll(
                &mut right_world.network_command_queues(),
                context,
                10,
                now,
                (now).wrapping_mul(16),
                0,
                0,
            )
            .unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "frame{frame} did not finish"
        );
        std::thread::yield_now();
    }
    while [&*left, &*right].into_iter().any(|live| {
        let connections = live.connections();
        let pending = connections
            .peers()
            .any(|(house, _)| connections.queued_send_count(house) != Some(0));
        pending
    }) {
        let now = 1000 + (start.elapsed().as_millis() / 16) as u32;
        left.service_transport(now);
        right.service_transport(now);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "certificate ACK did not retire"
        );
        std::thread::yield_now();
    }
    (passes[0], passes[1])
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
                        0,
                    )
                    .unwrap();
            }
            assert!(start.elapsed() < Duration::from_secs(2));
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
            0
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
                0,
            ) {
                Err(LiveCommandError::Startup(StartupError::Mismatch)) => rejected[index] = true,
                Ok(NetworkQueuePass::Waiting) => {}
                other => panic!("unexpected startup result {other:?}"),
            }
        }
        assert!(start.elapsed() < Duration::from_secs(2));
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
            // Existing VLS1 outer scope, UDP checksum, then ConnectionClass.
            const CONNECTION_OFFSET: usize = 4 + 16 + 8 + 4;
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
                    assert!(packet.starts_with(b"VLS\x01"));
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
                    0,
                )
                .unwrap();
        }
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < Duration::from_secs(2));
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
                0,
            )
            .unwrap();
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < Duration::from_secs(2));
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
        assert!(started.elapsed() < Duration::from_secs(2));
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
                    0,
                )
                .unwrap();
            clients[side].service_transport(now);
        }
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < Duration::from_secs(2));
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
                0,
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
    let deadline = Instant::now() + Duration::from_secs(2);
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
    let deadline = Instant::now() + Duration::from_secs(2);
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
                0,
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
                    0,
                )
                .unwrap();
        }
        proxy.pump();
        now += 1;
        assert!(started.elapsed() < Duration::from_secs(2));
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
            0,
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
                now + 22,
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
                0,
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
            0,
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
            live.poll(
                &mut sim.network_command_queues(),
                next,
                10,
                1200,
                19200,
                0,
                0
            )
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
            0,
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
            0,
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
        .receive(
            peer,
            0,
            &connection_prefix(0x1236, 2, 0, 2),
            1300,
            &mut |_, _| {},
        )
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
                0,
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
            0,
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
    receiver
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
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
            0,
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
        super::super::event_packet::decode_frame_sync(&bytes[18..length])
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
            0,
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
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 1);
    let (length, _) = receiver.recv_from(&mut bytes).unwrap();
    assert!(super::super::event_packet::decode_frame_sync(&bytes[18..length]).is_some());
    assert!(live.restart_wait_probe(1100));
    assert_eq!(live.wait_info().unwrap().entered_at, 1000);
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1159,
        (1159_u32).wrapping_mul(16),
        0,
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
        0,
    )
    .unwrap();
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 1);
}

#[test]
fn ordinary_wait_uses_180_buckets_and_keeps_the_frame_send_complete() {
    let (mut left, mut right) = pair(73);
    let mut a = Simulation::new();
    let mut b = Simulation::new();
    poll_pair(&mut left, &mut right, &mut a, &mut b, 0);
    for frame in 1..10 {
        let pass = left
            .poll(
                &mut a.network_command_queues(),
                NetworkQueueContext {
                    frame,
                    checksum: 0x12345678,
                },
                10,
                2000,
                (2000_u32).wrapping_mul(16),
                0,
                0,
            )
            .unwrap();
        assert_eq!(
            pass,
            if frame == 5 {
                NetworkQueuePass::Dispatch
            } else {
                NetworkQueuePass::SkipDispatch
            }
        );
    }
    let context = NetworkQueueContext {
        frame: 10,
        checksum: 0x12345678,
    };
    // The real UDP startup may span a 16ms wall-clock bucket. The wait must
    // retain the measured entry RTT, which need not be zero on a loaded host.
    let response_time_at_entry = left.response_time();
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2000,
            (2000_u32).wrapping_mul(16),
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(left.frame_wait().unwrap().missing_frame, Some(1));
    assert_eq!(
        left.wait_info(),
        Some(LiveWaitInfo {
            mode: LiveCommandMode::Lan,
            kind: LiveWaitKind::Ordinary,
            entered_at: 2000,
            response_time_at_entry,
        })
    );
    assert_eq!(left.lowest_frame_peer().unwrap().house_id(), 1);
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2179,
            (2179_u32).wrapping_mul(16),
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(left.take_service_report().unwrap().sent_datagrams, 0);
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2180,
            (2180_u32).wrapping_mul(16),
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(left.take_service_report().unwrap().sent_datagrams, 1);
    assert_eq!(left.sent_commands(), 0);
    assert!(left.restart_wait_probe(2300));
    assert_eq!(left.wait_info().unwrap().entered_at, 2000);
    left.poll(
        &mut a.network_command_queues(),
        context,
        10,
        2479,
        (2479_u32).wrapping_mul(16),
        0,
        0,
    )
    .unwrap();
    assert_eq!(left.take_service_report().unwrap().sent_datagrams, 0);
    left.poll(
        &mut a.network_command_queues(),
        context,
        10,
        2480,
        (2480_u32).wrapping_mul(16),
        0,
        0,
    )
    .unwrap();
    assert_eq!(left.take_service_report().unwrap().sent_datagrams, 1);
}

#[test]
fn lowest_frame_projection_uses_existing_signed_progress_and_first_tie() {
    let mut wire = transport();
    for house in [3, 1, 2] {
        wire.connections_mut()
            .add_peer(
                house,
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, 20000 + house as u16),
            )
            .unwrap();
    }
    let mut live = LiveCommandSession::new(
        fixture_agreement(&wire, 0, 73, NonZeroU32::new(5).unwrap()),
        wire,
        0,
        73,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Lan,
    )
    .unwrap();
    let mut sim = Simulation::new();
    assert_eq!(live.lowest_frame_peer().unwrap().house_id(), 3);
    assert_eq!(live.lowest_frame_peer().unwrap().frame(), -1);
    for (house, expected) in [(3, 1), (1, 2), (2, 3)] {
        let packet = live
            .exchange
            .frame_probe(house, 20, 0, 73, live.policy)
            .unwrap();
        live.exchange
            .receive(house, &packet, 73, &mut sim.network_command_queues(), 0)
            .unwrap();
        assert_eq!(live.lowest_frame_peer().unwrap().house_id(), expected);
    }
    assert_eq!(live.lowest_frame_peer().unwrap().frame(), 20);
}

#[test]
fn invalid_roster_context_and_initial_seed_are_explicit_failures() {
    // Native zero-remote loops finish immediately; LAN can start with one
    // human. Once startup is admitted, losing the last peer also retains
    // the normal local-only transfer path (covered below).
    let mut one = LiveCommandSession::new(
        StartupAgreement::new([(0, 0)], [0], 1, 10, 30, NonZeroU32::new(5).unwrap(), 0).unwrap(),
        transport(),
        0,
        1,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Lan,
    )
    .unwrap();
    assert_eq!(
        one.poll(
            &mut Simulation::new().network_command_queues(),
            NetworkQueueContext {
                frame: 0,
                checksum: 0
            },
            10,
            100,
            1600,
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::SkipDispatch
    );
    let mut wire = transport();
    wire.connections_mut()
        .add_peer(200, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234))
        .unwrap();
    assert!(matches!(
        LiveCommandSession::new(
            fixture_agreement(&wire, 0, 1, NonZeroU32::new(5).unwrap()),
            wire,
            0,
            1,
            NonZeroU32::new(5).unwrap(),
            LiveCommandMode::Lan
        ),
        Err(LiveCommandError::InvalidRemoteHouse(200))
    ));
    let mut wire = transport();
    wire.connections_mut()
        .add_peer(1, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .unwrap();
    assert!(matches!(
        LiveCommandSession::new(
            fixture_agreement(&wire, 0, 1, NonZeroU32::new(5).unwrap()),
            wire,
            0,
            1,
            NonZeroU32::new(5).unwrap(),
            LiveCommandMode::Lan
        ),
        Err(LiveCommandError::UnboundPeer { .. })
    ));
    let (mut left, mut right) = pair(73);
    right.initial_seed = 74;
    let mut a = Simulation::new();
    let mut b = Simulation::new();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 10,
    };
    assert!(matches!(
        left.poll(
            &mut a.network_command_queues(),
            NetworkQueueContext {
                frame: 1,
                checksum: 10
            },
            10,
            1000,
            (1000_u32).wrapping_mul(16),
            0,
            0
        ),
        Err(LiveCommandError::FrameSequence {
            expected: 0,
            actual: 1
        })
    ));
    let start = Instant::now();
    loop {
        let l = left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            1000,
            (1000_u32).wrapping_mul(16),
            0,
            0,
        );
        let r = right.poll(
            &mut b.network_command_queues(),
            context,
            10,
            1000,
            (1000_u32).wrapping_mul(16),
            0,
            0,
        );
        if matches!(
            l,
            Err(LiveCommandError::Exchange(
                CommandExchangeError::InitialChecksum { .. }
            ))
        ) || matches!(
            r,
            Err(LiveCommandError::Exchange(
                CommandExchangeError::InitialChecksum { .. }
            ))
        ) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
}

#[test]
fn failed_retry_service_keeps_receiving_and_retains_its_diagnostic() {
    let (mut left, mut right) = pair(73);
    let mut a = Simulation::new();
    let mut b = Simulation::new();
    poll_pair(&mut left, &mut right, &mut a, &mut b, 0);
    for frame in 1..10 {
        left.poll(
            &mut a.network_command_queues(),
            NetworkQueueContext {
                frame,
                checksum: 0x12345678,
            },
            10,
            2000,
            (2000_u32).wrapping_mul(16),
            0,
            0,
        )
        .unwrap();
    }
    // A valid reliable message without an ACK fails this service attempt.
    // Native648B94/649518 still admit the peer's progress and test readiness.
    left.set_retry_timing(RetryTiming {
        retry_delta: 0,
        max_retries: 0,
        timeout: u32::MAX,
    });
    let pending = left
        .exchange
        .frame_probe(0, 10, 10, 73, left.policy)
        .unwrap();
    assert!(
        left.transport
            .connections_mut()
            .broadcast(&pending, Delivery::Reliable)
    );
    let progress = right
        .exchange
        .frame_probe(1, 20, 10, 73, right.policy)
        .unwrap();
    assert!(
        right
            .transport
            .connections_mut()
            .broadcast(&progress, Delivery::Unreliable)
    );
    right.transport.service(2001);
    let context = NetworkQueueContext {
        frame: 10,
        checksum: 0x12345678,
    };
    let start = Instant::now();
    let mut now = 2002;
    loop {
        let pass = left
            .poll(
                &mut a.network_command_queues(),
                context,
                10,
                now,
                (now).wrapping_mul(16),
                0,
                0,
            )
            .unwrap();
        assert_eq!(left.take_service_report().unwrap().failed_peer, Some(1));
        if pass == NetworkQueuePass::Dispatch {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        now = now.wrapping_add(1);
        std::thread::yield_now();
    }
    assert_eq!(left.peers()[0].frame(), 20);
    assert!(left.connections().queued_send_count(1).unwrap() > 0);
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2003,
            (2003_u32).wrapping_mul(16),
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Dispatch
    );
}

#[test]
fn removing_the_last_peer_resumes_wait_and_keeps_local_transfer_running() {
    let (mut left, mut right) = pair(73);
    let mut a = Simulation::new();
    let mut b = Simulation::new();
    poll_pair(&mut left, &mut right, &mut a, &mut b, 0);
    for frame in 1..10 {
        left.poll(
            &mut a.network_command_queues(),
            NetworkQueueContext {
                frame,
                checksum: 0x12345678,
            },
            10,
            2000,
            (2000_u32).wrapping_mul(16),
            0,
            0,
        )
        .unwrap();
    }
    let command = CommandRecord::encode(6, 0, 10, &[42, 0, 0, 0, 0x34]).unwrap();
    assert!(a.issue_local_command(CommandEnvelope::synchronized(command, 10)));
    let context = NetworkQueueContext {
        frame: 10,
        checksum: 0x12345678,
    };
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2001,
            (2001_u32).wrapping_mul(16),
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert!(left.remove_peer(1));
    assert!(left.peers().is_empty());
    assert_eq!(left.connections().peers().len(), 0);
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2002,
            (2002_u32).wrapping_mul(16),
            0,
            0
        )
        .unwrap(),
        NetworkQueuePass::Dispatch
    );
    let command = CommandRecord::encode(6, 0, 11, &[43, 0, 0, 0, 0x34]).unwrap();
    assert!(a.issue_local_command(CommandEnvelope::synchronized(command, 11)));
    for frame in 11..=15 {
        assert_eq!(
            left.poll(
                &mut a.network_command_queues(),
                NetworkQueueContext {
                    frame,
                    checksum: 0x12345678,
                },
                10,
                2003,
                (2003_u32).wrapping_mul(16),
                0,
                0
            )
            .unwrap(),
            if frame == 15 {
                NetworkQueuePass::Dispatch
            } else {
                NetworkQueuePass::SkipDispatch
            }
        );
    }
    assert_eq!(left.sent_commands(), 2);
    assert_eq!(a.network_command_queues().local_len(), 0);
    for frame in [20, 25] {
        assert!(a.pending_command_snapshot().iter().any(|command| {
            matches!(&command.payload,
                crate::sim::command::Command::SynchronizedRecord { record }
                if record.opcode() == 6 && record.frame_stamp() == frame)
        }));
    }
}

struct Boundary {
    live: RefCell<LiveCommandSession>,
    checksums: RefCell<FrameChecksumState>,
    now: Cell<u32>,
    process_millis: Option<u32>,
    sampled_frame: Cell<Option<u32>>,
    applied: RefCell<Vec<(u32, CommandRecord, bool)>>,
}

impl Boundary {
    fn new(live: LiveCommandSession) -> Self {
        Self {
            live: RefCell::new(live),
            checksums: RefCell::new(FrameChecksumState::default()),
            now: Cell::new(1000),
            process_millis: None,
            sampled_frame: Cell::new(None),
            applied: RefCell::new(Vec::new()),
        }
    }
}

impl NetworkCommandBoundary for Boundary {
    fn exchange_commands(
        &self,
        context: NetworkQueueContext,
        queues: &mut NetworkCommandQueues<'_>,
    ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
        let mut live = self.live.borrow_mut();
        if self.sampled_frame.replace(Some(context.frame)) != Some(context.frame)
            && let Some(milliseconds) = self.process_millis
        {
            live.record_processing_time(milliseconds);
        }
        let ahead = queues.max_ahead_frames().unwrap_or(10);
        live.poll(
            queues,
            context,
            ahead,
            self.now.get(),
            (self.now.get()).wrapping_mul(16),
            0,
            0,
        )
        .map(Some)
        .map_err(|error| NetworkCommandError::Unavailable(error.to_string()))
    }

    fn record_frame_checksum(&self, frame: u32, checksum: u32) {
        self.checksums
            .borrow_mut()
            .record_frame_checksum(frame, checksum);
    }

    fn compare_frame_info(
        &self,
        record: &CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        self.checksums
            .borrow()
            .compare_frame_info(FrameInfo::decode(record).unwrap(), current_frame)
            .map_err(NetworkCommandError::Mismatch)
    }
}

fn stop_scene() -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=AMCV\n[AircraftTypes]\n[BuildingTypes]\n\
         [AMCV]\nStrength=1000\nSpeed=4\nSight=0\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\
         [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(73);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (index, name) in ["Alpha", "Beta"].iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, index as u8, None, true, 0, 10));
    }
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.install_network_timing(10);
    sim.session.game_options_mut_for_test().short_game = false;
    let costs = rules
        .terrain_rules
        .semantics_for_land_type(crate::rules::terrain_rules::LandType::Clear.as_index())
        .unwrap()
        .speed_costs;
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
                cell.speed_costs = costs;
                cell.base_speed_costs = costs;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(64, 64, cells),
    );
    assert!(sim.rebuild_dynamic_navigation(&rules));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let actor = sim
        .spawn_object("AMCV", "Alpha", 20, 20, 0, &rules)
        .unwrap();
    sim.spawn_object("AMCV", "Beta", 25, 25, 0, &rules).unwrap();
    (sim, rules, actor)
}

fn production_frame(sim: &mut Simulation, rules: &RuleSet, boundary: &Boundary) -> SimFrameOutput {
    let applied = |sim: &Simulation, record: &CommandRecord, applied: bool| {
        boundary
            .applied
            .borrow_mut()
            .push((sim.session().binary_frame(), record.clone(), applied));
    };
    let effects = FrameEffects::empty()
        .with_network_commands(boundary)
        .with_command_applied(&applied);
    if sim.pending_network_wait() {
        sim.resume_network_wait(rules, None, effects).unwrap()
    } else {
        sim.advance_app_frame(&[], rules, None, 33, TickLane::Ordinary, effects)
            .unwrap()
    }
}

#[test]
fn solo_wol_commands_apply_without_ahead_or_batching_across_timing_updates() {
    let (mut sim, rules, actor) = stop_scene();
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    sim.session.install_network_timing(40);
    sim.session.game_options_mut_for_test().game_speed = 1;
    sim.session.game_options_mut_for_test().fog_of_war = true;
    sim.session
        .install_network_command_timing(0, std::num::NonZeroU8::new(10).unwrap(), [0]);
    let rate = NonZeroU32::new(10).unwrap();
    let agreement =
        StartupAgreement::new([(0, 0)], [0], 73, 40, 30, rate, sim.state_hash()).unwrap();
    let mut live =
        LiveCommandSession::new(agreement, transport(), 0, 73, rate, LiveCommandMode::Wol).unwrap();
    live.configure_precalculated_timing(40, 45);
    let mut boundary = Boundary::new(live);
    boundary.process_millis = Some(60);
    let owner = sim.interner.get("Alpha").unwrap();
    assert!(sim.issue_local_command(CommandEnvelope::synchronized(
        sim.encode_stop_record(owner, actor).unwrap(),
        0,
    )));
    for _ in 0..2 {
        production_frame(&mut sim, &rules, &boundary);
    }
    let stops = || {
        boundary
            .applied
            .borrow()
            .iter()
            .filter_map(|(frame, record, ok)| {
                crate::sim::command::StopRecord::decode(record).map(|_| (*frame, *ok))
            })
            .collect::<Vec<_>>()
    };
    // Frame0 primes the network queue; the first dispatchable frame is1.
    assert_eq!(stops(), [(1, true)]);
    assert_eq!(sim.session().network_max_ahead_frames(), Some(40));
    assert_eq!(
        sim.session()
            .network_command_timing()
            .unwrap()
            .frames_per_second(),
        45
    );
    // Reach the real adaptive-timing gate without injecting TIMING records.
    for _ in 0..5 {
        assert!(
            sim.issue_local_command(CommandEnvelope::synchronized(
                crate::sim::command::GameSpeedRecord {
                    house_id: 0,
                    frame: 2,
                    speed: 1
                }
                .encode()
                .unwrap(),
                sim.session().tick(),
            ))
        );
    }
    for frame in 2..=512 {
        if frame == 257 {
            boundary.process_millis = Some(1);
        }
        production_frame(&mut sim, &rules, &boundary);
        assert_eq!(sim.session().binary_frame(), frame + 1);
        if frame == 256 {
            assert_eq!(
                sim.session()
                    .network_command_timing()
                    .unwrap()
                    .frames_per_second(),
                16
            );
        }
    }
    assert_eq!(
        sim.session()
            .network_command_timing()
            .unwrap()
            .frames_per_second(),
        45
    );
    let issued = sim.session().binary_frame();
    assert!(sim.issue_local_command(CommandEnvelope::synchronized(
        sim.encode_stop_record(owner, actor).unwrap(),
        sim.session().tick(),
    )));
    production_frame(&mut sim, &rules, &boundary);
    assert_eq!(stops(), [(1, true), (issued, true)]);
    assert_eq!(sim.session().network_max_ahead_frames(), Some(40));
    assert_eq!(
        sim.session()
            .network_command_timing()
            .unwrap()
            .frame_send_rate()
            .get(),
        10
    );
}

#[test]
fn originally_multiplayer_wol_keeps_its_delay_after_the_last_peer_leaves() {
    let (mut sim, rules, actor) = stop_scene();
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    sim.session
        .install_network_command_timing(0, std::num::NonZeroU8::new(5).unwrap(), [0, 1]);
    let mut wire = transport();
    let peer = transport();
    wire.connections_mut()
        .add_peer(1, peer.primary_addr().unwrap())
        .unwrap();
    let rate = NonZeroU32::new(5).unwrap();
    let mut live = LiveCommandSession::new(
        fixture_agreement(&wire, 0, 73, rate),
        wire,
        0,
        73,
        rate,
        LiveCommandMode::Wol,
    )
    .unwrap();
    certify_native_probe_fixture(&mut live);
    let boundary = Boundary::new(live);
    // The first Queue_AI visit clears peer progress; supply its native
    // response after entering that real initial wait, before peer departure.
    production_frame(&mut sim, &rules, &boundary);
    {
        let mut live = boundary.live.borrow_mut();
        let probe = live
            .exchange
            .frame_probe(1, 0, 10, 73, live.policy)
            .unwrap();
        live.exchange
            .receive(1, &probe, 73, &mut sim.network_command_queues(), 0)
            .unwrap();
    }
    production_frame(&mut sim, &rules, &boundary);
    assert!(!boundary.live.borrow().startup_pending());
    assert!(boundary.live.borrow_mut().remove_peer(1));
    let owner = sim.interner.get("Alpha").unwrap();
    assert!(sim.issue_local_command(CommandEnvelope::synchronized(
        sim.encode_stop_record(owner, actor).unwrap(),
        sim.session().tick(),
    )));
    for _ in 1..=15 {
        production_frame(&mut sim, &rules, &boundary);
    }
    let stops = boundary
        .applied
        .borrow()
        .iter()
        .filter_map(|(frame, record, ok)| {
            crate::sim::command::StopRecord::decode(record).map(|_| (*frame, *ok))
        })
        .collect::<Vec<_>>();
    assert_eq!(stops, [(15, true)]);
}

#[test]
fn real_udp_timing_commands_apply_precalc_then_adapt_to_the_slowest_peer() {
    let (mut a_live, mut b_live) = pair(73);
    a_live.mode = LiveCommandMode::Wol;
    b_live.mode = LiveCommandMode::Wol;
    // The host is House1, not an assumed zero; no second event may follow
    // the one-shot startup value until periodic processing feedback is ready.
    b_live.configure_precalculated_timing(20, 45);
    let mut a_boundary = Boundary::new(a_live);
    let mut b_boundary = Boundary::new(b_live);
    a_boundary.process_millis = Some(20);
    b_boundary.process_millis = Some(50);
    let (mut a, rules, _) = stop_scene();
    let (mut b, _, _) = stop_scene();
    for sim in [&mut a, &mut b] {
        sim.session.set_game_mode(ScenarioGameMode::Wol);
        sim.session
            .install_network_command_timing(1, std::num::NonZeroU8::new(5).unwrap(), [0, 1]);
    }
    for _ in 0..5 {
        b.issue_local_command(CommandEnvelope::synchronized(
            crate::sim::command::GameSpeedRecord {
                house_id: 1,
                frame: 0,
                speed: 3,
            }
            .encode()
            .unwrap(),
            0,
        ));
    }
    for frame in 0..=300 {
        let start = Instant::now();
        let mut completed = [false; 2];
        let mut checksums = [None; 2];
        while !completed.iter().all(|done| *done) {
            for (index, (sim, boundary)) in [(&mut a, &a_boundary), (&mut b, &b_boundary)]
                .into_iter()
                .enumerate()
            {
                if completed[index] {
                    continue;
                }
                boundary
                    .now
                    .set(1000 + frame * 10 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(sim, &rules, boundary);
                assert!(output.network_failure.is_none());
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                completed[index] = output.tick.frame_committed;
            }
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "timing frame {frame} stalled"
            );
            std::thread::yield_now();
        }
        assert_eq!(checksums[0], checksums[1], "timing frame {frame} CRC");
        if frame == 10 {
            assert_eq!(
                a.session
                    .network_command_timing()
                    .unwrap()
                    .frames_per_second(),
                45
            );
            assert_eq!(
                b.session.network_command_timing(),
                a.session.network_command_timing()
            );
        }
    }
    for sim in [&a, &b] {
        let timing = sim.session.network_command_timing().unwrap();
        assert_eq!(timing.maximum_processing_millis([0, 1]), Some(50));
        assert_eq!(timing.frames_per_second(), 20);
        assert_eq!(timing.frame_send_rate().get(), 5);
    }
    assert_eq!(
        a.session.network_command_timing(),
        b.session.network_command_timing()
    );
}

#[test]
fn three_udp_clients_keep_adaptive_timing_after_the_original_publisher_exits() {
    let mut transports: [LanUdpTransport; 3] = std::array::from_fn(|_| transport());
    let addresses: [SocketAddrV4; 3] =
        std::array::from_fn(|index| transports[index].primary_addr().unwrap());
    for (index, transport) in transports.iter_mut().enumerate() {
        for (peer, address) in addresses.iter().enumerate() {
            if peer != index {
                transport
                    .connections_mut()
                    .add_peer(peer as i32, *address)
                    .unwrap();
            }
        }
    }
    let mut boundaries: Vec<_> = transports
        .into_iter()
        .enumerate()
        .map(|(index, transport)| {
            let live = LiveCommandSession::new(
                fixture_agreement(&transport, index as i8, 73, NonZeroU32::new(5).unwrap()),
                transport,
                index as i8,
                73,
                NonZeroU32::new(5).unwrap(),
                LiveCommandMode::Lan,
            )
            .unwrap();
            let mut boundary = Boundary::new(live);
            boundary.process_millis = Some(if index == 2 { 50 } else { 20 });
            boundary
        })
        .collect();
    let (first, rules, _) = stop_scene();
    let mut worlds = [first, stop_scene().0, stop_scene().0];
    for (index, sim) in worlds.iter_mut().enumerate() {
        let third = sim.interner.intern("Gamma");
        sim.houses
            .register(HouseState::new(third, 2, None, true, 0, 10));
        sim.spawn_object("AMCV", "Gamma", 30, 30, 0, &rules)
            .unwrap();
        sim.session
            .set_current_house_for_test(Some(sim.houses.order()[index]));
        sim.session.install_network_command_timing(
            0,
            std::num::NonZeroU8::new(5).unwrap(),
            [0, 1, 2],
        );
    }
    // Admit enough ordinary synchronized records to reach native's adaptive
    // gate. EXIT itself then crosses all three real reliable UDP connections.
    for _ in 0..5 {
        assert!(
            worlds[1].issue_local_command(CommandEnvelope::synchronized(
                crate::sim::command::GameSpeedRecord {
                    house_id: 1,
                    frame: 0,
                    speed: 3
                }
                .encode()
                .unwrap(),
                0,
            ))
        );
    }
    assert!(
        worlds[0].issue_local_command(CommandEnvelope::synchronized(
            crate::sim::command::ExitRecord {
                house_id: 0,
                frame: 0
            }
            .encode()
            .unwrap(),
            0,
        ))
    );
    let mut retired = false;
    for frame in 0..=300 {
        let start = Instant::now();
        let mut completed = [retired, false, false];
        let mut checksums = [None; 3];
        while !completed.iter().all(|done| *done) {
            for index in 0..3 {
                if completed[index] {
                    continue;
                }
                boundaries[index]
                    .now
                    .set(1000 + frame * 10 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(&mut worlds[index], &rules, &boundaries[index]);
                assert!(output.network_failure.is_none());
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                // Local EXIT4C7917 skips MainTick55DE7E's frame commit.
                // Consume the same terminal edge as the app instead of running
                // that exited world's Logic again on its unchanged frame10.
                if let Some(owner) = worlds[index].take_executed_exit_owner() {
                    assert_eq!(index, 0, "only the original publisher leaves");
                    assert_eq!(owner, worlds[index].houses.order()[0]);
                    assert_eq!(frame, 10, "EXIT keeps its admitted execution frame");
                    assert!(worlds[index].quit_requested());
                    assert!(!output.tick.frame_committed);
                    completed[index] = true;
                } else {
                    completed[index] = output.tick.frame_committed;
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "host-loss timing frame{frame} stalled"
            );
            std::thread::yield_now();
        }
        assert_eq!(checksums[1], checksums[2], "surviving frame{frame} CRC");
        if !retired
            && worlds
                .iter()
                .all(|sim| sim.houses[&sim.houses.order()[0]].left_game())
        {
            for boundary in &mut boundaries[1..] {
                assert!(boundary.live.borrow_mut().remove_peer(0));
                assert_eq!(boundary.live.borrow().peers().len(), 1);
            }
            retired = true;
        }
    }
    assert!(
        retired,
        "the transmitted EXIT never executed on every client"
    );
    for sim in &mut worlds[1..] {
        assert_eq!(sim.network_command_queues().timing_host_house(), Some(1));
        let timing = sim.session.network_command_timing().unwrap();
        assert_eq!(timing.maximum_processing_millis([1, 2]), Some(50));
        assert_eq!(timing.frames_per_second(), 20);
    }
    assert_eq!(
        worlds[1].session.network_command_timing(),
        worlds[2].session.network_command_timing()
    );
    assert_eq!(worlds[1].state_hash(), worlds[2].state_hash());
}

#[test]
fn three_udp_clients_recover_a_retained_frontier_across_a_timing_shrink() {
    use crate::net::event_packet::decode_frame_info;
    use crate::sim::command::{Command, MegaMissionRecord, StopRecord};
    use std::collections::BTreeMap;

    // Byte-preserving fault on only House0 <-> House2. Startup, probes and
    // ACKs pass normally. The first frame325 command attempt is lost; the
    // remaining real reliable packets wait until the sender hits its FIFO.
    struct Link {
        sockets: [UdpSocket; 2],
        endpoints: [SocketAddrV4; 2],
        hold: bool,
        first_lost: Option<u32>,
        retried: bool,
        delivered_retry: bool,
        reordered: bool,
        held: BTreeMap<u32, Vec<u8>>,
    }
    impl Link {
        fn forward(&mut self, side: usize, packet: &[u8]) {
            let frame = &packet[4..]; // Existing UDP checksum, no outer scope.
            if side == 0
                && let Some(first) = self.first_lost
            {
                let sequence = crate::net::reliable::sequence(frame);
                if frame[2] == 0 {
                    self.reordered |= sequence > first && !self.delivered_retry;
                    self.delivered_retry |= sequence == first;
                }
            }
            self.sockets[1 - side]
                .send_to(packet, self.endpoints[1 - side])
                .unwrap();
        }

        fn pump(&mut self) {
            for side in 0..2 {
                loop {
                    let mut packet = [0; crate::net::udp::UDP_DATAGRAM_CAPACITY];
                    let (len, source) = match self.sockets[side].recv_from(&mut packet) {
                        Ok(received) => received,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(error) => panic!("timing link receive: {error}"),
                    };
                    assert_eq!(source, std::net::SocketAddr::V4(self.endpoints[side]));
                    let packet = &packet[..len];
                    let frame = &packet[4..];
                    let body = &frame[crate::net::reliable::CONNECTION_HEADER_LEN..];
                    if side == 0
                        && frame[2] == 0
                        && decode_frame_info(body).is_some_and(|info| info.event_frame == 325)
                    {
                        let sequence = crate::net::reliable::sequence(frame);
                        if self.first_lost.is_none() {
                            self.first_lost = Some(sequence);
                            continue;
                        }
                        self.retried |= self.first_lost == Some(sequence);
                        if self.hold {
                            self.held.entry(sequence).or_insert_with(|| packet.to_vec());
                            assert!(
                                self.held.len() <= 32,
                                "the actual reliable FIFO bounds the hold"
                            );
                            continue;
                        }
                    }
                    self.forward(side, packet);
                }
            }
        }

        fn release(&mut self) {
            self.hold = false;
            // Deliver later reliable sequences before the lost one's retry.
            for (_, packet) in std::mem::take(&mut self.held).into_iter().rev() {
                self.forward(0, &packet);
            }
        }
    }

    let mut transports: [LanUdpTransport; 3] = std::array::from_fn(|_| transport());
    let addresses = std::array::from_fn::<_, 3, _>(|i| transports[i].primary_addr().unwrap());
    let sockets: [UdpSocket; 2] = std::array::from_fn(|_| {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.set_nonblocking(true).unwrap();
        socket
    });
    let proxy_addresses: [SocketAddrV4; 2] = std::array::from_fn(|i| {
        let std::net::SocketAddr::V4(address) = sockets[i].local_addr().unwrap() else {
            unreachable!()
        };
        address
    });
    for (index, wire) in transports.iter_mut().enumerate() {
        for (peer, &address) in addresses
            .iter()
            .enumerate()
            .filter(|(peer, _)| *peer != index)
        {
            let endpoint = match (index, peer) {
                (0, 2) => proxy_addresses[0],
                (2, 0) => proxy_addresses[1],
                _ => address,
            };
            wire.connections_mut()
                .add_peer(peer as i32, endpoint)
                .unwrap();
        }
    }
    let mut link = Link {
        sockets,
        endpoints: [addresses[0], addresses[2]],
        hold: true,
        first_lost: None,
        retried: false,
        delivered_retry: false,
        reordered: false,
        held: BTreeMap::new(),
    };
    let boundaries: Vec<_> = transports
        .into_iter()
        .enumerate()
        .map(|(index, wire)| {
            let mut live = LiveCommandSession::new(
                fixture_agreement(&wire, index as i8, 73, NonZeroU32::new(5).unwrap()),
                wire,
                index as i8,
                73,
                NonZeroU32::new(5).unwrap(),
                LiveCommandMode::Wol,
            )
            .unwrap();
            if index == 0 {
                live.configure_precalculated_timing(40, 45);
            }
            let mut boundary = Boundary::new(live);
            boundary.process_millis = Some(if index == 2 { 500 } else { 20 });
            boundary
        })
        .collect();
    let (first, rules, actor) = stop_scene();
    let mut worlds = [first, stop_scene().0, stop_scene().0];
    for (index, sim) in worlds.iter_mut().enumerate() {
        let third = sim.interner.intern("Gamma");
        sim.houses
            .register(HouseState::new(third, 2, None, true, 0, 10));
        sim.spawn_object("AMCV", "Gamma", 30, 30, 0, &rules)
            .unwrap();
        sim.session.set_game_mode(ScenarioGameMode::Wol);
        sim.session
            .set_current_house_for_test(Some(sim.houses.order()[index]));
        sim.session.install_network_command_timing(
            0,
            std::num::NonZeroU8::new(5).unwrap(),
            [0, 1, 2],
        );
    }
    // Reach ProcessNetwork647BF4's ordinary-command gate without injecting a
    // TIMING/PROCESS_TIME record. LiveTiming produces those from real samples.
    for _ in 0..5 {
        assert!(
            worlds[0].issue_local_command(CommandEnvelope::synchronized(
                crate::sim::command::GameSpeedRecord {
                    house_id: 0,
                    frame: 0,
                    speed: 1
                }
                .encode()
                .unwrap(),
                0,
            ))
        );
    }
    let owner = worlds[0].interner.get("Alpha").unwrap();
    let mut crcs: [BTreeMap<u32, u32>; 3] = std::array::from_fn(|_| BTreeMap::new());
    let mut hashes: [BTreeMap<u32, u64>; 3] = std::array::from_fn(|_| BTreeMap::new());
    let mut released_slow = false;
    let mut queued_move = false;
    let mut queued_stop = false;
    let mut saw_backpressure = false;
    let mut saw_incomplete_frontier = false;
    let started = Instant::now();
    let mut turn = 0;
    while worlds.iter().any(|sim| sim.session().binary_frame() < 400) {
        turn += 1;
        link.pump();
        // Keep peer2 at285 while the fast clients apply the real frame300
        // decrease. At301 they now lack frame credit under MaxAhead15.
        if !released_slow
            && worlds[2].session().binary_frame() == 285
            && worlds[0].session().binary_frame() >= 301
        {
            assert_eq!(worlds[2].session().binary_frame(), 285);
            assert!(worlds[0].session().binary_frame() - worlds[2].session().binary_frame() > 15);
            released_slow = true;
        }
        for index in 0..3 {
            let boundary = &boundaries[index];
            boundary
                .now
                .set(1000 + (started.elapsed().as_millis() / 16) as u32);
            // Same Network_ServiceLoop48D080 boundary as the app: even a
            // deliberately slow or finished simulation still services ACKs.
            boundary
                .live
                .borrow_mut()
                .service_transport(boundary.now.get());
            let sim = &mut worlds[index];
            let frame = sim.session().binary_frame();
            if frame >= 400
                || (index == 2
                    && ((!released_slow && frame >= 285) || (released_slow && turn % 4 != 0)))
            {
                continue;
            }
            if index == 0 && frame == 310 && !queued_move {
                let tick = sim.session().tick();
                // Distinct targets prevent MegaMission run compression from
                // hiding the production32-packet queue boundary.
                let commands = (0..1024)
                    .map(|n| {
                        CommandEnvelope::synchronized(
                            sim.encode_megamission_record(
                                owner,
                                &Command::Move {
                                    entity_id: actor,
                                    target_rx: 4 + n % 32,
                                    target_ry: 4 + n / 32,
                                },
                            )
                            .unwrap(),
                            tick,
                        )
                    })
                    .collect::<Vec<_>>();
                assert!(sim.issue_local_commands(commands));
                queued_move = true;
            }
            if index == 0 && frame == 350 && !queued_stop {
                assert!(sim.issue_local_command(CommandEnvelope::synchronized(
                    sim.encode_stop_record(owner, actor).unwrap(),
                    sim.session().tick(),
                )));
                queued_stop = true;
            }
            let output = production_frame(sim, &rules, boundary);
            assert!(output.network_failure.is_none());
            if let Some(sync) = output.multiplayer_checksum {
                crcs[index].insert(frame, sync.checksum.unwrap().value);
            }
            if output.tick.frame_committed {
                hashes[index].insert(frame, sim.state_hash());
                if frame == 10 || frame == 300 {
                    let timing = sim.session.network_command_timing().unwrap();
                    let expected = if frame == 10 {
                        (40, 45, 10)
                    } else {
                        (15, 2, 5)
                    };
                    assert_eq!(
                        (
                            sim.session.network_max_ahead_frames().unwrap(),
                            timing.frames_per_second(),
                            timing.frame_send_rate().get()
                        ),
                        expected
                    );
                }
            }
            let live = boundary.live.borrow();
            saw_backpressure |= index == 0
                && live.exchange.sending()
                && sim.pending_local_command_count() != 0
                && live.connections().queued_send_count(2) == Some(32);
            saw_incomplete_frontier |= index != 0
                && live
                    .frame_wait()
                    .is_some_and(|wait| wait.missing_commands == Some(0));
        }
        link.pump();
        if link.hold && saw_backpressure && saw_incomplete_frontier && !link.held.is_empty() {
            link.release();
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "heterogeneous exchange stalled: frames={:?}, local={:?}, waits={:?}, sending={:?}, peers={:?}, held={}, lost={:?}, retry={}, pressure={}, frontier={}",
            worlds.each_ref().map(|sim| sim.session().binary_frame()),
            worlds
                .each_ref()
                .map(|sim| sim.pending_local_command_count()),
            boundaries
                .iter()
                .map(|b| b.live.borrow().frame_wait())
                .collect::<Vec<_>>(),
            boundaries
                .iter()
                .map(|b| b.live.borrow().exchange.sending())
                .collect::<Vec<_>>(),
            boundaries
                .iter()
                .map(|b| b.live.borrow().peers().to_vec())
                .collect::<Vec<_>>(),
            link.held.len(),
            link.first_lost,
            link.retried,
            saw_backpressure,
            saw_incomplete_frontier
        );
        std::thread::yield_now();
    }
    assert!(
        released_slow && queued_move && queued_stop && saw_backpressure && saw_incomplete_frontier
    );
    assert!(link.first_lost.is_some() && link.retried && link.delivered_retry && link.reordered);
    let sent: Vec<_> = boundaries
        .iter()
        .map(|b| b.live.borrow().sent_commands())
        .collect();
    for index in 0..3 {
        assert!(crcs[index].keys().copied().eq(0..400));
        assert!(hashes[index].keys().copied().eq(0..400));
        assert_eq!(crcs[index], crcs[0], "client{index} native CRC");
        assert_eq!(
            hashes[index], hashes[0],
            "client{index} shared world hashes"
        );
        let live = boundaries[index].live.borrow();
        for peer in live.peers() {
            assert_eq!(peer.advertised_commands(), sent[peer.house_id() as usize]);
            assert_eq!(peer.received_commands(), sent[peer.house_id() as usize]);
        }
        let applied = boundaries[index].applied.borrow();
        let moves = applied
            .iter()
            .filter_map(|(frame, record, ok)| {
                MegaMissionRecord::decode(record).map(|move_| (*frame, move_, *ok))
            })
            .collect::<Vec<_>>();
        assert_eq!(moves.len(), 1024);
        assert!(
            moves
                .iter()
                .all(|(frame, move_, ok)| *frame == 325 && move_.source_id as u64 == actor && *ok)
        );
        let stops = applied
            .iter()
            .filter_map(|(frame, record, ok)| {
                StopRecord::decode(record).map(|stop| (*frame, stop, *ok))
            })
            .collect::<Vec<_>>();
        assert_eq!(stops.len(), 1);
        assert_eq!(
            (stops[0].0, stops[0].1.entity_id as u64, stops[0].2),
            (365, actor, true)
        );
        assert!(
            worlds[index]
                .substrate
                .entities
                .get(actor)
                .unwrap()
                .navigation
                .nav_list()
                .is_empty()
        );
        let timing = worlds[index].session.network_command_timing().unwrap();
        assert_eq!(timing.maximum_processing_millis([0, 1, 2]), Some(500));
    }
}

#[test]
fn two_udp_clients_execute_the_delayed_stop_through_the_production_frame_boundary() {
    let (a_live, b_live) = pair(73);
    let a_boundary = Boundary::new(a_live);
    let b_boundary = Boundary::new(b_live);
    let (mut a, rules, actor) = stop_scene();
    let (mut b, _, second_actor) = stop_scene();
    assert_eq!(actor, second_actor);
    let owner = a.interner.get("Alpha").unwrap();
    let command = a.encode_stop_record(owner, actor).unwrap();
    assert!(a.issue_local_command(CommandEnvelope::synchronized(command, 0)));
    let mut executed = [0; 2];
    for frame in 0..=10 {
        if frame == 10 {
            for sim in [&mut a, &mut b] {
                sim.substrate
                    .entities
                    .get_mut(actor)
                    .unwrap()
                    .navigation
                    .set_nav_list_for_test(
                        vec![crate::sim::components::NavTargetRef::cell(21, 20)],
                        false,
                    );
            }
        }
        let start = Instant::now();
        let mut completed = [false; 2];
        let mut checksums = [None; 2];
        while !completed.iter().all(|done| *done) {
            for (index, (sim, boundary)) in [(&mut a, &a_boundary), (&mut b, &b_boundary)]
                .into_iter()
                .enumerate()
            {
                if completed[index] {
                    continue;
                }
                boundary
                    .now
                    .set(1000 + frame * 100 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(sim, &rules, boundary);
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                if output.tick.frame_committed {
                    completed[index] = true;
                    executed[index] += output.tick.executed_commands;
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "frame{frame} stalled"
            );
            std::thread::yield_now();
        }
        assert_eq!(checksums[0], checksums[1], "frame{frame} CRC");
        assert_eq!(executed, if frame < 10 { [0, 0] } else { [1, 1] });
    }
    for sim in [&a, &b] {
        assert!(
            sim.substrate
                .entities
                .get(actor)
                .unwrap()
                .navigation
                .nav_list()
                .is_empty()
        );
        assert_eq!(sim.session.binary_frame(), 11);
    }
}

#[test]
fn real_udp_mass_order_survives_backpressure_and_executes_one_shared_frame() {
    use crate::sim::command::{Command, MegaMissionRecord, StopRecord};
    use std::collections::{BTreeMap, BTreeSet};

    let (a_live, b_live) = pair(73);
    let a_boundary = Boundary::new(a_live);
    let b_boundary = Boundary::new(b_live);
    let (mut a, rules, first) = stop_scene();
    let (mut b, _, _) = stop_scene();
    let mut actors = vec![first];
    // The existing Alpha actor already occupies (20,20).
    let cells = (4..24)
        .flat_map(|y| (4..34).map(move |x| (x, y)))
        .filter(|cell| *cell != (20, 20));
    for (x, y) in cells {
        let left = a.spawn_object("AMCV", "Alpha", x, y, 0, &rules).unwrap();
        assert_eq!(
            left,
            b.spawn_object("AMCV", "Alpha", x, y, 0, &rules).unwrap()
        );
        actors.push(left);
    }
    assert_eq!(actors.len(), 600);
    let owner = a.interner.get("Alpha").unwrap();
    a.session.set_current_house_for_test(Some(owner));
    b.session.set_current_house_for_test(b.interner.get("Beta"));
    let moves = actors
        .iter()
        .map(|&entity_id| {
            a.encode_megamission_record(
                owner,
                &Command::Move {
                    entity_id,
                    target_rx: 45,
                    target_ry: 45,
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        a.issue_local_commands(
            (0..20_000).map(|index| {
                CommandEnvelope::synchronized(moves[index % moves.len()].clone(), 0)
            })
        )
    );
    assert_eq!(a.pending_local_command_count(), 20_000);
    assert!(
        !a.issue_local_commands(
            (0..20_000).map(|index| {
                CommandEnvelope::synchronized(moves[index % moves.len()].clone(), 0)
            })
        )
    );
    assert_eq!(
        a.pending_local_command_count(),
        20_000,
        "rejected batch is atomic"
    );

    let started = Instant::now();
    let mut checksums = [BTreeMap::new(), BTreeMap::new()];
    let mut saw_backpressure = false;
    // Advance peers independently: the peer that already committed frame zero
    // must still service its socket while the sender drains more than 32 packets.
    let mut until = 11;
    for phase in 0..2 {
        while a.session().binary_frame() < until || b.session().binary_frame() < until {
            for (index, (sim, boundary)) in [(&mut a, &a_boundary), (&mut b, &b_boundary)]
                .into_iter()
                .enumerate()
            {
                boundary
                    .now
                    .set(1000 + (started.elapsed().as_millis() / 16) as u32);
                // The app's Network_ServiceLoop48D080 pump continues even
                // when this client's simulation is ahead of its peer. Keep
                // its admitted packets retrying while the other catches up.
                boundary
                    .live
                    .borrow_mut()
                    .service_transport(boundary.now.get());
                let frame = sim.session().binary_frame();
                let output = production_frame(sim, &rules, boundary);
                assert!(output.network_failure.is_none());
                if let Some(sync) = output.multiplayer_checksum
                    && (phase == 0 || frame < until)
                {
                    checksums[index].insert(frame, sync.checksum.unwrap().value);
                }
                if index == 0
                    && sim.pending_local_command_count() != 0
                    && sim.pending_network_wait()
                {
                    saw_backpressure = true;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "mass UDP exchange stalled: frames={:?}, local={:?}, waits={:?}, peers={:?}, queued={:?}",
                [a.session().binary_frame(), b.session().binary_frame()],
                [
                    a.pending_local_command_count(),
                    b.pending_local_command_count()
                ],
                [
                    a_boundary.live.borrow().wait_info(),
                    b_boundary.live.borrow().wait_info()
                ],
                [
                    a_boundary.live.borrow().peers(),
                    b_boundary.live.borrow().peers()
                ],
                [
                    a_boundary.live.borrow().connections().queued_send_count(1),
                    b_boundary.live.borrow().connections().queued_send_count(0)
                ],
            );
            std::thread::yield_now();
        }
        if phase == 0 {
            for boundary in [&a_boundary, &b_boundary] {
                let applied = boundary.applied.borrow();
                let moves = applied
                    .iter()
                    .filter_map(|(frame, record, ok)| {
                        MegaMissionRecord::decode(record).map(|command| (*frame, command, *ok))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(moves.len(), 20_000);
                assert!(moves.iter().all(|(frame, _, ok)| *frame == 10 && *ok));
                assert_eq!(
                    moves
                        .iter()
                        .map(|(_, command, _)| command.source_id as u64)
                        .collect::<BTreeSet<_>>(),
                    actors.iter().copied().collect()
                );
            }
            let tick = a.session().tick();
            let stops = actors
                .iter()
                .map(|&actor| {
                    CommandEnvelope::synchronized(a.encode_stop_record(owner, actor).unwrap(), tick)
                })
                .collect::<Vec<_>>();
            assert!(a.issue_local_commands(stops));
            // The current cadence may already have sent before entering a
            // held wait. Rate5/MaxAhead10 then admits Stop five frames later
            // and executes it ten frames after that. Include that frame in
            // the declared prefix while both clients continue normal probes.
            until = a.session().binary_frame().max(b.session().binary_frame()) + 16;
        }
    }
    assert!(
        saw_backpressure,
        "fixture must cross the live 32-packet send queue"
    );
    assert!(
        checksums
            .iter()
            .all(|history| history.keys().copied().eq(0..until)),
        "both clients must record the entire declared shared prefix"
    );
    assert_eq!(checksums[0], checksums[1]);
    for (sim, boundary) in [(&a, &a_boundary), (&b, &b_boundary)] {
        let applied = boundary.applied.borrow();
        let stops = applied
            .iter()
            .filter_map(|(_, record, ok)| {
                StopRecord::decode(record).map(|command| (command.entity_id as u64, *ok))
            })
            .collect::<Vec<_>>();
        assert_eq!(stops.len(), actors.len());
        assert!(stops.iter().all(|(_, ok)| *ok));
        assert_eq!(
            stops.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>(),
            actors.iter().copied().collect()
        );
        assert!(actors.iter().all(|id| {
            sim.substrate
                .entities
                .get(*id)
                .unwrap()
                .navigation
                .nav_list()
                .is_empty()
        }));
    }
}

/// Real authenticated UDP transport and production frames, with the relay's
/// clock independent of either client's frame/UI service.
#[test]
fn sequenced_udp_paused_client_catches_up_without_stalling_healthy_commands() {
    use crate::net::udp::{
        RelayTicket, relay_peer_endpoint,
        test_support::{MAX_DATAGRAM, RelayServer, SecretKey, derive_packet_key},
    };
    use crate::sim::command::{Command, IDLE_OPCODE, MEGAMISSION_OPCODE};
    use std::collections::BTreeMap;
    let key = SecretKey::new([11; 32]);
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let address = match socket.local_addr().unwrap() {
        std::net::SocketAddr::V4(a) => a,
        _ => unreachable!(),
    };
    let journal_dir = std::env::temp_dir().join(format!(
        "vera20k-catchup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut directory = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(&journal_dir).unwrap();
    let mut relay = RelayServer::with_journal(key.clone(), 4, &journal_dir).unwrap();
    let (a, rules, actor) = stop_scene();
    let (b, _, _) = stop_scene();
    let mut worlds = [a, b];
    let mut beta = 0;
    for sim in &mut worlds {
        sim.session.set_game_mode(ScenarioGameMode::Wol);
        sim.network_command_queues().install_timing(
            0,
            std::num::NonZeroU8::new(1).unwrap(),
            [0, 1],
        );
        sim.network_command_queues().configure_sequenced_clock(30);
        beta = sim.spawn_object("AMCV", "Beta", 30, 30, 0, &rules).unwrap();
    }
    let certificate = crate::net::udp::OrderingCertificate {
        room: [7; 16],
        sources_mask: 3,
        tick_hz: 30,
        expires: 86_500,
    }
    .sign(&key)
    .unwrap();
    let agreement = StartupAgreement::new(
        [(0, 0), (1, 1)],
        [0, 1],
        73,
        10,
        30,
        NonZeroU32::new(1).unwrap(),
        worlds[0].state_hash(),
    )
    .unwrap();
    let boundaries: Vec<_> = (0..2u8)
        .map(|source| {
            let ticket = RelayTicket {
                room: [7; 16],
                player: [source + 1; 16],
                source,
                generation: 1,
                expires: 300,
                nonce: [source + 3; 16],
            }
            .sign(&key)
            .unwrap();
            let mut wire = transport();
            wire.enable_relay(
                address,
                &ticket,
                *derive_packet_key(&key, &ticket).expose_for_https(),
                [9; 32],
            )
            .unwrap();
            wire.set_ordering_certificate(certificate.to_vec()).unwrap();
            wire.connections_mut()
                .add_peer(
                    i32::from(1 - source),
                    relay_peer_endpoint(1 - source).unwrap(),
                )
                .unwrap();
            Boundary::new(
                LiveCommandSession::new(
                    agreement.clone(),
                    wire,
                    source as i8,
                    73,
                    NonZeroU32::new(1).unwrap(),
                    LiveCommandMode::Wol,
                )
                .unwrap(),
            )
        })
        .collect();
    let mut now_ms = 16_000u64;
    let mut move_issued = false;
    let mut stop_issued = false;
    let mut late_issued = false;
    let mut paused_at = None;
    let mut hashes = [BTreeMap::new(), BTreeMap::new()];
    let mut held = None;
    let mut reordered = false;
    let mut saw_reconnecting = false;
    for _ in 0..4000 {
        let healthy_frame = worlds[0].session().binary_frame();
        if healthy_frame >= 5 && !move_issued {
            let owner = worlds[0].interner.get("Alpha").unwrap();
            let record = worlds[0]
                .encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: actor,
                        target_rx: 42,
                        target_ry: 42,
                    },
                )
                .unwrap();
            assert!(
                worlds[0].issue_local_command(CommandEnvelope::synchronized(
                    record,
                    healthy_frame as u64
                ))
            );
            move_issued = true;
        }
        if healthy_frame >= 20 && !stop_issued {
            let owner = worlds[0].interner.get("Alpha").unwrap();
            let record = worlds[0].encode_stop_record(owner, actor).unwrap();
            assert!(
                worlds[0].issue_local_command(CommandEnvelope::synchronized(
                    record,
                    healthy_frame as u64
                ))
            );
            stop_issued = true;
        }
        let paused = (12..1150).contains(&healthy_frame);
        if paused {
            paused_at.get_or_insert(worlds[1].session().binary_frame());
        }
        if healthy_frame >= 1150 && !late_issued {
            assert!(
                worlds[1].session().binary_frame() < 20,
                "the peer was actually suspended"
            );
            let owner = worlds[1].interner.get("Beta").unwrap();
            let record = worlds[1].encode_stop_record(owner, beta).unwrap();
            let tick = worlds[1].session().tick();
            assert!(worlds[1].issue_local_command(CommandEnvelope::synchronized(record, tick)));
            late_issued = true;
        }
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        for forwarded in relay.drain_ordering_push(now_ms, 100 + (now_ms - 16_000) / 1000) {
            // A disconnected client loses unsolicited delivery too; dropping
            // only its requests no longer pauses a server-pushed stream.
            if !paused || forwarded.endpoint != boundaries[1].live.borrow().primary_addr().unwrap()
            {
                socket
                    .send_to(&forwarded.bytes, forwarded.endpoint)
                    .unwrap();
            }
        }
        for side in 0..2 {
            boundaries[side].now.set((now_ms >> 4) as u32);
            boundaries[side]
                .live
                .borrow_mut()
                .service_transport((now_ms >> 4) as u32);
        }
        loop {
            let mut packet = [0; MAX_DATAGRAM + 1];
            let (length, from) = match socket.recv_from(&mut packet) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            };
            let std::net::SocketAddr::V4(from) = from else {
                unreachable!()
            };
            if paused && from == boundaries[1].live.borrow().primary_addr().unwrap() {
                continue;
            }
            for forwarded in relay
                .process_at(
                    from,
                    &packet[..length],
                    100 + (now_ms - 16_000) / 1000,
                    now_ms,
                )
                .unwrap_or_default()
            {
                // Lose/reorder one complete authenticated reply. Recovery uses
                // the immutable cursor, never a simulation-side prediction.
                if healthy_frame >= 8 && !reordered {
                    if held.is_none() {
                        held = Some(forwarded);
                        continue;
                    }
                    socket
                        .send_to(&forwarded.bytes, forwarded.endpoint)
                        .unwrap();
                    let previous = held.take().unwrap();
                    socket.send_to(&previous.bytes, previous.endpoint).unwrap();
                    reordered = true;
                } else {
                    socket
                        .send_to(&forwarded.bytes, forwarded.endpoint)
                        .unwrap();
                }
            }
        }
        for side in 0..2 {
            for _ in 0..8 {
                if worlds[side].session().binary_frame() >= 1200 {
                    break;
                }
                let output = production_frame(&mut worlds[side], &rules, &boundaries[side]);
                assert!(
                    worlds[side].pending_network_interruption().is_none(),
                    "the canonical stream must remain valid"
                );
                if output.tick.frame_committed {
                    hashes[side].insert(
                        worlds[side].session().binary_frame(),
                        worlds[side].state_hash(),
                    );
                } else {
                    break;
                }
            }
        }
        saw_reconnecting |= boundaries[1]
            .live
            .borrow()
            .recovery_status(now_ms as u32, worlds[1].session().binary_frame())
            == Some(NetworkRecoveryStatus::Reconnecting);
        if worlds
            .iter()
            .all(|world| world.session().binary_frame() == 1200)
        {
            break;
        }
        now_ms += 16;
    }
    assert!(reordered && late_issued && stop_issued && paused_at.is_some() && saw_reconnecting);
    assert_eq!(worlds[0].session().binary_frame(), 1200);
    assert_eq!(worlds[1].session().binary_frame(), 1200);
    assert_eq!(
        hashes[0], hashes[1],
        "every committed simulation frame, including catch-up"
    );
    assert_eq!(worlds[0].state_hash(), worlds[1].state_hash());
    let commands: Vec<_> = boundaries
        .iter()
        .map(|b| {
            b.applied
                .borrow()
                .iter()
                .filter(|(_, r, _)| matches!(r.opcode(), IDLE_OPCODE | MEGAMISSION_OPCODE))
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(commands[0], commands[1]);
    assert!(commands[0].iter().any(|(frame, r, applied)| *frame < 50
        && r.opcode() == IDLE_OPCODE
        && r.house_id() == 0
        && *applied));
    assert!(
        commands[0].iter().any(|(frame, r, applied)| *frame >= 1152
            && r.opcode() == IDLE_OPCODE
            && r.house_id() == 1
            && *applied),
        "a late order executes only at a future canonical frame"
    );
    assert_eq!(boundaries[0].live.borrow().pacing_delay_millis(40), 0);
    drop(boundaries);
    drop(relay);
    std::fs::remove_dir_all(journal_dir).unwrap();
}

#[test]
fn sequenced_unsent_batch_retries_when_the_relay_route_recovers() {
    use crate::net::udp::{
        OrderingCertificate, RelayTicket,
        test_support::{MAX_DATAGRAM, RelayServer, SecretKey, derive_packet_key},
    };
    use vera20k_relay_protocol::OrderEvent;

    let key = SecretKey::new([31; 32]);
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let std::net::SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!()
    };
    let ticket = RelayTicket {
        room: [27; 16],
        player: [1; 16],
        source: 0,
        generation: 1,
        expires: 300,
        nonce: [3; 16],
    }
    .sign(&key)
    .unwrap();
    let certificate = OrderingCertificate {
        room: [27; 16],
        sources_mask: 1,
        tick_hz: 30,
        expires: 86_500,
    }
    .sign(&key)
    .unwrap();
    let mut wire = transport();
    wire.enable_relay(
        address,
        &ticket,
        *derive_packet_key(&key, &ticket).expose_for_https(),
        [29; 32],
    )
    .unwrap();
    wire.set_ordering_certificate(certificate.to_vec()).unwrap();
    let agreement =
        StartupAgreement::new([(0, 0)], [0], 73, 10, 30, NonZeroU32::new(1).unwrap(), 0).unwrap();
    let mut session = sequenced::SequencedSession::new(&agreement, &wire, 0).unwrap();
    let mut relay = RelayServer::new(key, 1);
    let pump = |wire: &mut LanUdpTransport, relay: &mut RelayServer, now: u32| {
        wire.service(now >> 4);
        loop {
            let mut packet = [0; MAX_DATAGRAM + 1];
            let (length, std::net::SocketAddr::V4(from)) = (match socket.recv_from(&mut packet) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }) else {
                unreachable!()
            };
            for reply in relay
                .process_at(
                    from,
                    &packet[..length],
                    100 + u64::from(now) / 1000,
                    u64::from(now),
                )
                .unwrap()
            {
                socket.send_to(&reply.bytes, reply.endpoint).unwrap();
            }
        }
        wire.service(now >> 4);
    };
    let mut now = 16_000;
    for _ in 0..60 {
        session.service(&mut wire, now);
        pump(&mut wire, &mut relay, now);
        if session.started() {
            break;
        }
        now += 16;
    }
    assert!(session.started(), "authenticated ordering startup");
    let mut sim = Simulation::new();
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    assert_eq!(
        session
            .poll(
                &mut wire,
                &mut sim.network_command_queues(),
                NetworkQueueContext {
                    frame: 0,
                    checksum: 0
                },
                now
            )
            .unwrap(),
        NetworkQueuePass::SkipDispatch
    );

    // The production silence timer withdraws readiness. Retain the same
    // ordering session and source; its next first-send attempt cannot go out.
    now += 10_016;
    wire.service(now >> 4);
    assert!(!wire.relay_ready());
    let mut stop = CommandRecord::encode(crate::sim::command::IDLE_OPCODE, 0, 0, &[]).unwrap();
    crate::sim::command::StopRecord {
        house_id: 0,
        frame: 0,
        entity_id: 7,
    }
    .write_into(&mut stop);
    assert!(sim.issue_local_command(CommandEnvelope::synchronized(stop.clone(), 0)));
    session
        .poll(
            &mut wire,
            &mut sim.network_command_queues(),
            NetworkQueueContext {
                frame: 1,
                checksum: 0,
            },
            now,
        )
        .unwrap();

    let recovered_at = now + 16;
    let deadline = Instant::now() + Duration::from_secs(2);
    while !wire.relay_ready() && Instant::now() < deadline {
        pump(&mut wire, &mut relay, recovered_at);
        std::thread::yield_now();
    }
    assert!(
        wire.relay_ready(),
        "same route completed its real challenge"
    );
    session.service(&mut wire, recovered_at);
    pump(&mut wire, &mut relay, recovered_at);
    let drain_ack = |wire: &mut LanUdpTransport| {
        let mut acknowledged = false;
        while let Some(event) = wire.pop_ordering_received() {
            match event {
                OrderEvent::Ack { batch_id: 1, .. } => acknowledged = true,
                OrderEvent::Error { code, .. } => {
                    panic!("relay rejected immutable batch: {code:?}")
                }
                _ => {}
            }
        }
        acknowledged
    };
    let mut acknowledged = drain_ack(&mut wire);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !acknowledged && Instant::now() < deadline {
        pump(&mut wire, &mut relay, recovered_at);
        acknowledged = drain_ack(&mut wire);
        std::thread::yield_now();
    }
    assert!(
        acknowledged,
        "an unsent batch must not wait for the 100ms retransmission deadline"
    );

    // Withhold the acceptance from the app owner. The ordinary later retry
    // must receive another ACK, rather than a ModifiedRetry rejection.
    session.service(&mut wire, recovered_at + 112);
    let mut acknowledged = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while !acknowledged && Instant::now() < deadline {
        pump(&mut wire, &mut relay, recovered_at + 112);
        acknowledged = drain_ack(&mut wire);
        std::thread::yield_now();
    }
    assert!(
        acknowledged,
        "sealed retry retained the accepted ciphertext"
    );
    assert_eq!(session.sent_commands(), 0);
    assert_eq!(
        sim.network_command_queues().local_records().unwrap(),
        vec![&stop]
    );
}

fn sequenced_pair() -> (
    [Simulation; 2],
    RuleSet,
    u64,
    Vec<Boundary>,
    UdpSocket,
    crate::net::udp::test_support::RelayServer,
) {
    use crate::net::udp::{
        OrderingCertificate, RelayTicket, relay_peer_endpoint,
        test_support::{RelayServer, SecretKey, derive_packet_key},
    };
    use std::num::NonZeroU8;
    let key = SecretKey::new([21; 32]);
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let std::net::SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!()
    };
    let relay = RelayServer::new(key.clone(), 2);
    let (first, rules, actor) = stop_scene();
    let (second, _, _) = stop_scene();
    let mut worlds = [first, second];
    for sim in &mut worlds {
        sim.session.set_game_mode(ScenarioGameMode::Wol);
        sim.network_command_queues()
            .install_timing(0, NonZeroU8::new(1).unwrap(), [0, 1]);
        sim.network_command_queues().configure_sequenced_clock(30);
    }
    let certificate = OrderingCertificate {
        room: [17; 16],
        sources_mask: 3,
        tick_hz: 30,
        expires: 86_500,
    }
    .sign(&key)
    .unwrap();
    let agreement = StartupAgreement::new(
        [(0, 0), (1, 1)],
        [0, 1],
        73,
        10,
        30,
        NonZeroU32::new(1).unwrap(),
        worlds[0].state_hash(),
    )
    .unwrap();
    let boundaries: Vec<_> = (0..2u8)
        .map(|source| {
            let ticket = RelayTicket {
                room: [17; 16],
                player: [source + 1; 16],
                source,
                generation: 1,
                expires: 300,
                nonce: [source + 3; 16],
            }
            .sign(&key)
            .unwrap();
            let mut wire = transport();
            wire.enable_relay(
                address,
                &ticket,
                *derive_packet_key(&key, &ticket).expose_for_https(),
                [19; 32],
            )
            .unwrap();
            wire.set_ordering_certificate(certificate.to_vec()).unwrap();
            wire.connections_mut()
                .add_peer(
                    i32::from(1 - source),
                    relay_peer_endpoint(1 - source).unwrap(),
                )
                .unwrap();
            Boundary::new(
                LiveCommandSession::new(
                    agreement.clone(),
                    wire,
                    source as i8,
                    73,
                    NonZeroU32::new(1).unwrap(),
                    LiveCommandMode::Wol,
                )
                .unwrap(),
            )
        })
        .collect();
    (worlds, rules, actor, boundaries, socket, relay)
}

#[test]
fn sequenced_push_advances_without_requests_and_repairs_a_lost_range() {
    use crate::net::udp::test_support::MAX_DATAGRAM;
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};

    let (mut worlds, rules, actor, boundaries, socket, mut relay) = sequenced_pair();
    let second_address = boundaries[1].live.borrow().primary_addr().unwrap();
    let pump = |relay: &mut crate::net::udp::test_support::RelayServer,
                now_ms: u64,
                accept_requests: bool,
                lose_second: bool| {
        let deliver = |packet: vera20k_relay_protocol::ForwardDatagram| {
            if !lose_second || packet.endpoint != second_address {
                socket.send_to(&packet.bytes, packet.endpoint).unwrap();
            }
        };
        let mut requests = 0;
        {
            loop {
                let mut bytes = [0; MAX_DATAGRAM + 1];
                let (length, std::net::SocketAddr::V4(source)) = (match socket.recv_from(&mut bytes)
                {
                    Ok(packet) => packet,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("{error}"),
                }) else {
                    unreachable!()
                };
                requests += 1;
                if !accept_requests {
                    continue;
                }
                for packet in relay
                    .process_at(
                        source,
                        &bytes[..length],
                        100 + (now_ms - 16_000) / 1000,
                        now_ms,
                    )
                    .unwrap_or_default()
                {
                    deliver(packet);
                }
            }
        }
        for packet in relay.drain_ordering_push(now_ms, 100 + (now_ms - 16_000) / 1000) {
            deliver(packet);
        }
        requests
    };
    let service = |worlds: &mut [Simulation; 2], now_ms: u64| {
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            boundary.now.set((now_ms >> 4) as u32);
            boundary.live.borrow_mut().service_pending_commands(
                &mut world.network_command_queues(),
                (now_ms >> 4) as u32,
            );
        }
    };
    let mut now_ms = 16_000;
    for _ in 0..250 {
        now_ms += 16;
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        service(&mut worlds, now_ms);
        pump(&mut relay, now_ms, true, false);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            production_frame(world, &rules, boundary);
        }
        if boundaries
            .iter()
            .all(|boundary| boundary.live.borrow().sequenced_timing().is_some())
            && worlds
                .iter()
                .all(|world| world.session().binary_frame() >= 20)
        {
            break;
        }
    }
    assert!(
        boundaries
            .iter()
            .all(|boundary| boundary.live.borrow().sequenced_timing().is_some())
    );
    let before = worlds.each_ref().map(|world| world.session().tick());
    // For less than one credit window, discard every client request before
    // server processing. Only independently clocked push supplies these frames.
    let mut idle_requests = 0;
    for _ in 0..24 {
        now_ms += 16;
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        service(&mut worlds, now_ms);
        idle_requests += pump(&mut relay, now_ms, false, false);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            production_frame(world, &rules, boundary);
        }
    }
    for (world, before) in worlds.iter().zip(before) {
        assert!(
            world.session().tick() >= before + 8,
            "push cannot depend on frame requests"
        );
    }
    assert!(
        idle_requests < 48,
        "healthy clients must not request frames on every service pass"
    );
    let owner = worlds[0].interner.get("Alpha").unwrap();
    let record = worlds[0]
        .encode_megamission_record(
            owner,
            &Command::Move {
                entity_id: actor,
                target_rx: 42,
                target_ry: 42,
            },
        )
        .unwrap();
    let tick = worlds[0].session().tick();
    assert!(worlds[0].issue_local_command(CommandEnvelope::synchronized(record, tick)));
    let mut recovered = false;
    let mut committed = [
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    ];
    for step in 0..250 {
        now_ms += 16;
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        service(&mut worlds, now_ms);
        // Lose a real range of pushed frames and replies, then allow the
        // existing Pull/history owner to repair the contiguous stream.
        pump(&mut relay, now_ms, true, step < 15);
        for (side, (world, boundary)) in worlds.iter_mut().zip(&boundaries).enumerate() {
            for _ in 0..4 {
                if production_frame(world, &rules, boundary)
                    .tick
                    .frame_committed
                {
                    committed[side].insert(world.session().binary_frame(), world.state_hash());
                } else {
                    break;
                }
            }
            assert!(world.pending_network_interruption().is_none());
        }
        let applied = |boundary: &Boundary| {
            boundary
                .applied
                .borrow()
                .iter()
                .filter(|(_, record, ok)| {
                    record.opcode() == MEGAMISSION_OPCODE && record.house_id() == 0 && *ok
                })
                .count()
        };
        if step >= 15
            && applied(&boundaries[0]) == 1
            && applied(&boundaries[1]) == 1
            && worlds[0].session().binary_frame() == worlds[1].session().binary_frame()
        {
            let frame = worlds[0].session().binary_frame();
            // Current state may already include the following frame's Logic
            // while its network boundary waits. Compare only completed ticks.
            assert_eq!(committed[0].get(&frame), committed[1].get(&frame));
            assert!(committed[0].contains_key(&frame));
            for (frame, hash) in &committed[0] {
                if let Some(other) = committed[1].get(frame) {
                    assert_eq!(hash, other, "replayed frame {frame}");
                }
            }
            recovered = true;
            break;
        }
    }
    assert!(
        recovered,
        "the lost push range must replay exactly once to the same world"
    );
}

#[test]
fn sequenced_pause_stops_at_one_boundary_and_recovers_a_lost_resume() {
    use crate::net::udp::test_support::MAX_DATAGRAM;
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};

    let (mut worlds, rules, actor, boundaries, socket, mut relay) = sequenced_pair();
    let second_address = boundaries[1].live.borrow().primary_addr().unwrap();
    let mut committed = [
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    ];
    let step = |worlds: &mut [Simulation; 2],
                relay: &mut crate::net::udp::test_support::RelayServer,
                committed: &mut [std::collections::BTreeMap<u32, u64>; 2],
                now_ms: u64,
                lose_second: bool,
                lose_pause_status: bool| {
        let now = 100 + (now_ms - 16_000) / 1000;
        relay.advance_clock(now_ms, now);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            boundary.now.set((now_ms >> 4) as u32);
            boundary.live.borrow_mut().service_pending_commands(
                &mut world.network_command_queues(),
                (now_ms >> 4) as u32,
            );
        }
        let deliver = |packet: vera20k_relay_protocol::ForwardDatagram| {
            // Protocol v2 has a 48-byte server header; payload kind8 is
            // PauseStatus. Lose those replies but retain canonical history.
            let lost_status = lose_pause_status && packet.bytes.get(48) == Some(&8);
            if packet.endpoint != second_address || (!lose_second && !lost_status) {
                socket.send_to(&packet.bytes, packet.endpoint).unwrap();
            }
        };
        loop {
            let mut bytes = [0; MAX_DATAGRAM + 1];
            let (length, std::net::SocketAddr::V4(source)) = (match socket.recv_from(&mut bytes) {
                Ok(packet) => packet,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }) else {
                unreachable!()
            };
            for packet in relay
                .process_at(source, &bytes[..length], now, now_ms)
                .unwrap()
            {
                deliver(packet);
            }
        }
        for packet in relay.drain_ordering_push(now_ms, now) {
            deliver(packet);
        }
        for (side, (world, boundary)) in worlds.iter_mut().zip(&boundaries).enumerate() {
            boundary
                .live
                .borrow_mut()
                .service_transport((now_ms >> 4) as u32);
            for _ in 0..4 {
                // Use the production app's canonical admission rule. A
                // retained post-Logic transaction must finish, not be replayed.
                let frame = world.session().binary_frame();
                if !world.pending_network_wait()
                    && frame != 0
                    && frame > boundary.live.borrow().committed_frontier()
                {
                    break;
                }
                if production_frame(world, &rules, boundary)
                    .tick
                    .frame_committed
                {
                    committed[side].insert(world.session().binary_frame(), world.state_hash());
                } else {
                    break;
                }
            }
            assert!(world.pending_network_interruption().is_none());
        }
        std::thread::yield_now();
    };
    let mut now_ms = 16_000;
    for _ in 0..250 {
        now_ms += 16;
        step(&mut worlds, &mut relay, &mut committed, now_ms, false, true);
        if boundaries[0]
            .live
            .borrow()
            .multiplayer_pause()
            .unwrap()
            .available
            && now_ms >= 18_000
            && worlds
                .iter()
                .all(|world| world.session().binary_frame() >= 12)
        {
            break;
        }
    }
    assert!(
        boundaries[0]
            .live
            .borrow_mut()
            .request_multiplayer_pause(true)
    );
    assert!(
        !boundaries[1]
            .live
            .borrow()
            .multiplayer_pause()
            .unwrap()
            .available,
        "all capability replies for the second client were intentionally lost"
    );
    // The second player misses the pause announcement and part of its final
    // frame range. Recovery must still bring it to exactly the same boundary.
    for index in 0..160 {
        now_ms += 16;
        step(
            &mut worlds,
            &mut relay,
            &mut committed,
            now_ms,
            index < 20,
            true,
        );
    }
    let pause = boundaries[0].live.borrow().multiplayer_pause().unwrap();
    assert!(pause.paused && !pause.pending);
    assert_eq!(pause.epoch, 1);
    for (world, boundary) in worlds.iter().zip(&boundaries) {
        let state = boundary.live.borrow().multiplayer_pause().unwrap();
        assert!(state.paused && !state.pending);
        assert_eq!(state.frame, pause.frame);
        assert_eq!(world.session().binary_frame(), pause.frame + 1);
        assert!(!world.pending_network_wait());
    }
    assert_eq!(worlds[0].state_hash(), worlds[1].state_hash());

    let owner = worlds[0].interner.get("Alpha").unwrap();
    let record = worlds[0]
        .encode_megamission_record(
            owner,
            &Command::Move {
                entity_id: actor,
                target_rx: 42,
                target_ry: 42,
            },
        )
        .unwrap();
    let tick = worlds[0].session().tick();
    assert!(worlds[0].issue_local_command(CommandEnvelope::synchronized(record, tick)));
    for _ in 0..20 {
        now_ms += 16;
        step(&mut worlds, &mut relay, &mut committed, now_ms, false, true);
    }
    assert!(boundaries.iter().all(|boundary| {
        !boundary
            .applied
            .borrow()
            .iter()
            .any(|(_, record, _)| record.opcode() == MEGAMISSION_OPCODE)
    }));
    assert!(
        worlds
            .iter()
            .all(|world| world.session().binary_frame() == pause.frame + 1)
    );

    // A different player resumes; its receipt and pushed resume are lost.
    // Retrying the same control cannot create another pause or clock jump.
    assert!(
        boundaries[1]
            .live
            .borrow_mut()
            .request_multiplayer_pause(false)
    );
    for index in 0..250 {
        now_ms += 16;
        step(
            &mut worlds,
            &mut relay,
            &mut committed,
            now_ms,
            index < 20,
            false,
        );
        if index > 20
            && worlds
                .iter()
                .all(|world| world.session().binary_frame() > pause.frame + 12)
        {
            break;
        }
    }
    for (world, boundary) in worlds.iter().zip(&boundaries) {
        let state = boundary.live.borrow().multiplayer_pause().unwrap();
        assert!(!state.paused && !state.pending);
        assert_eq!(state.epoch, 2);
        assert!(world.session().binary_frame() > pause.frame + 12);
        assert_eq!(
            boundary
                .applied
                .borrow()
                .iter()
                .filter(|(_, record, ok)| { record.opcode() == MEGAMISSION_OPCODE && *ok })
                .count(),
            1
        );
    }
    let mut compared_after_resume = 0;
    for (frame, hash) in &committed[0] {
        if let Some(other) = committed[1].get(frame) {
            assert_eq!(hash, other, "committed frame {frame}");
            compared_after_resume += usize::from(*frame > pause.frame + 1);
        }
    }
    assert!(compared_after_resume >= 10);

    // Confirming Leave while a Pause request is still pending must preserve
    // the native EXIT and keep resuming until that event can be dispatched.
    assert!(
        boundaries[0]
            .live
            .borrow_mut()
            .request_multiplayer_pause(true)
    );
    let exit = crate::sim::command::ExitRecord {
        house_id: 0,
        frame: 0,
    }
    .encode()
    .unwrap();
    let tick = worlds[0].session().tick();
    assert!(worlds[0].issue_local_command(CommandEnvelope::synchronized(exit, tick)));
    boundaries[0].live.borrow_mut().keep_running_for_exit();
    for _ in 0..200 {
        now_ms += 16;
        step(
            &mut worlds,
            &mut relay,
            &mut committed,
            now_ms,
            false,
            false,
        );
        if boundaries.iter().all(|boundary| {
            boundary
                .applied
                .borrow()
                .iter()
                .any(|(_, record, ok)| record.opcode() == crate::sim::command::EXIT_OPCODE && *ok)
        }) {
            break;
        }
    }
    for boundary in &boundaries {
        assert_eq!(
            boundary
                .applied
                .borrow()
                .iter()
                .filter(|(_, record, ok)| {
                    record.opcode() == crate::sim::command::EXIT_OPCODE && *ok
                })
                .count(),
            1,
            "queued EXIT must dispatch exactly once after automatic resume"
        );
        assert!(!boundary.live.borrow().multiplayer_pause().unwrap().paused);
    }
}

#[test]
fn sequenced_push_resubscribes_as_soon_as_a_renewed_lease_is_confirmed() {
    use crate::net::udp::{
        RelayTicket,
        test_support::{MAX_DATAGRAM, SecretKey, derive_packet_key},
    };
    let (mut worlds, rules, _, boundaries, socket, mut relay) = sequenced_pair();
    let std::net::SocketAddr::V4(address) = socket.local_addr().unwrap() else {
        unreachable!()
    };
    let mut pass = |relay_ms: u64, client_ms: u64| {
        let now = 100 + (relay_ms - 16_000) / 1000;
        relay.advance_clock(relay_ms, now);
        for packet in relay.drain_ordering_push(relay_ms, now) {
            socket.send_to(&packet.bytes, packet.endpoint).unwrap();
        }
        for boundary in &boundaries {
            boundary.now.set((client_ms >> 4) as u32);
            boundary
                .live
                .borrow_mut()
                .service_transport((client_ms >> 4) as u32);
        }
        let mut bytes = [0; MAX_DATAGRAM + 1];
        while let Ok((length, std::net::SocketAddr::V4(source))) = socket.recv_from(&mut bytes) {
            for packet in relay
                .process_at(source, &bytes[..length], now, relay_ms)
                .unwrap_or_default()
            {
                socket.send_to(&packet.bytes, packet.endpoint).unwrap();
            }
        }
    };
    let mut now_ms = 16_000;
    for _ in 0..250 {
        if boundaries
            .iter()
            .all(|boundary| boundary.live.borrow().sequenced_timing().is_some())
            && worlds
                .iter()
                .all(|world| world.session().binary_frame() >= 20)
        {
            break;
        }
        now_ms += 16;
        pass(now_ms, now_ms);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            production_frame(world, &rules, boundary);
        }
    }
    assert!(boundaries[0].live.borrow().sequenced_timing().is_some());
    let key = SecretKey::new([21; 32]);
    let renewed = RelayTicket {
        room: [17; 16],
        player: [1; 16],
        source: 0,
        generation: 1,
        // Outlives the first lease and stays within the relay's ticket lifetime.
        expires: 350,
        nonce: [42; 16],
    }
    .sign(&key)
    .unwrap();
    boundaries[0]
        .live
        .borrow_mut()
        .renew_relay(
            address,
            &renewed,
            *derive_packet_key(&key, &renewed).expose_for_https(),
            [19; 32],
        )
        .unwrap();
    // Hold the client clock so no watermark falls due. Loopback datagrams can
    // land passes late, so wait on the confirmed lease itself.
    for _ in 0..64 {
        if boundaries[0].live.borrow().relay_expires() == Some(350) {
            break;
        }
        pass(now_ms, now_ms);
    }
    assert_eq!(boundaries[0].live.borrow().relay_expires(), Some(350));
    let frame = || {
        boundaries[0]
            .live
            .borrow()
            .sequenced_timing()
            .unwrap()
            .frame
    };
    let before = frame();
    // The relay keeps closing frames, but this client's watermark never falls
    // due. The confirmed lease starts unsubscribed, so only the READY can
    // restore push.
    let mut relay_ms = now_ms;
    while frame() == before && relay_ms < now_ms + 1_000 {
        relay_ms += 16;
        pass(relay_ms, now_ms);
    }
    assert!(
        frame() > before,
        "a renewed lease must resume push without waiting for a watermark"
    );
}

#[test]
fn sequenced_player_input_passes_an_unacknowledged_checksum_batch() {
    use crate::net::udp::test_support::MAX_DATAGRAM;
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};
    let (mut worlds, rules, actor, boundaries, socket, mut relay) = sequenced_pair();
    let first_address = boundaries[0].live.borrow().primary_addr().unwrap();
    let mut hold_receipts = false;
    let mut issued = false;
    let mut observed = false;
    for step in 0..250 {
        let now_ms = 16_000 + step * 16;
        // Frame15 emits the normal checksum-only batch. Withhold every
        // response to this sender, including its canonical acceptance entry.
        let checksum_already_held = hold_receipts;
        hold_receipts |= worlds[0].session().binary_frame() >= 15;
        if checksum_already_held && !issued {
            let owner = worlds[0].interner.get("Alpha").unwrap();
            let record = worlds[0]
                .encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: actor,
                        target_rx: 42,
                        target_ry: 42,
                    },
                )
                .unwrap();
            let tick = worlds[0].session().tick();
            assert!(worlds[0].issue_local_command(CommandEnvelope::synchronized(record, tick)));
            issued = true;
        }
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        for forwarded in relay.drain_ordering_push(now_ms, 100 + (now_ms - 16_000) / 1000) {
            if !hold_receipts || forwarded.endpoint != first_address {
                socket
                    .send_to(&forwarded.bytes, forwarded.endpoint)
                    .unwrap();
            }
        }
        for boundary in &boundaries {
            boundary.now.set((now_ms >> 4) as u32);
            boundary
                .live
                .borrow_mut()
                .service_transport((now_ms >> 4) as u32);
        }
        loop {
            let mut packet = [0; MAX_DATAGRAM + 1];
            let (length, std::net::SocketAddr::V4(from)) = (match socket.recv_from(&mut packet) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }) else {
                unreachable!()
            };
            for forwarded in relay
                .process_at(
                    from,
                    &packet[..length],
                    100 + (now_ms - 16_000) / 1000,
                    now_ms,
                )
                .unwrap_or_default()
            {
                if !hold_receipts || forwarded.endpoint != first_address {
                    socket
                        .send_to(&forwarded.bytes, forwarded.endpoint)
                        .unwrap();
                }
            }
        }
        for side in 0..2 {
            production_frame(&mut worlds[side], &rules, &boundaries[side]);
        }
        observed = boundaries[1]
            .applied
            .borrow()
            .iter()
            .any(|(_, record, applied)| {
                record.opcode() == MEGAMISSION_OPCODE && record.house_id() == 0 && *applied
            });
        if observed {
            break;
        }
    }
    assert!(
        hold_receipts && issued,
        "setup did not reach the held checksum: frames={:?}",
        worlds
            .iter()
            .map(|sim| sim.session().binary_frame())
            .collect::<Vec<_>>()
    );
    assert!(
        observed,
        "another client must execute the Move while its sender's checksum ACK is withheld"
    );
    assert_eq!(
        boundaries[0].live.borrow().sent_commands(),
        0,
        "the sender received no acceptance receipt for either batch"
    );
    assert_eq!(
        worlds[0].network_command_queues().local_len(),
        1,
        "unacknowledged input stays in the authoritative OutList"
    );
    assert_eq!(
        boundaries[1]
            .applied
            .borrow()
            .iter()
            .filter(|(_, record, applied)| record.opcode() == MEGAMISSION_OPCODE
                && record.house_id() == 0
                && *applied)
            .count(),
        1
    );
}

#[test]
fn sequenced_fresh_batch_survives_one_lost_datagram_without_a_retry_wait() {
    use crate::net::udp::test_support::MAX_DATAGRAM;
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};
    let (mut worlds, rules, actor, boundaries, socket, mut relay) = sequenced_pair();
    let first_address = boundaries[0].live.borrow().primary_addr().unwrap();
    let mut issued = false;
    let mut sent = None;
    let mut lost = false;
    let mut observed = None;
    let traced_send = |boundary: &Boundary| {
        boundary
            .live
            .borrow()
            .command_delivery_traces()
            .last()
            .is_some_and(|trace| trace.sent_at.is_some())
    };
    for step in 0..250 {
        let now_ms = 16_000 + step * 16;
        let unix = 100 + (now_ms - 16_000) / 1000;
        if !issued && worlds[0].session().binary_frame() >= 20 {
            let owner = worlds[0].interner.get("Alpha").unwrap();
            let record = worlds[0]
                .encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: actor,
                        target_rx: 42,
                        target_ry: 42,
                    },
                )
                .unwrap();
            let tick = worlds[0].session().tick();
            assert!(worlds[0].issue_local_command(CommandEnvelope::synchronized(record, tick)));
            issued = true;
        }
        relay.advance_clock(now_ms, unix);
        for forwarded in relay.drain_ordering_push(now_ms, unix) {
            socket
                .send_to(&forwarded.bytes, forwarded.endpoint)
                .unwrap();
        }
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            boundary.now.set((now_ms >> 4) as u32);
            boundary.live.borrow_mut().service_pending_commands(
                &mut world.network_command_queues(),
                (now_ms >> 4) as u32,
            );
        }
        if sent.is_none() && traced_send(&boundaries[0]) {
            sent = Some(step);
        }
        let mut packets = Vec::new();
        loop {
            let mut packet = [0; MAX_DATAGRAM + 1];
            let (length, std::net::SocketAddr::V4(from)) = (match socket.recv_from(&mut packet) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }) else {
                unreachable!()
            };
            packets.push((from, packet[..length].to_vec()));
        }
        if sent.is_some() && !lost {
            // Lose the first datagram of the fresh command batch, the
            // sender's largest; small control datagrams still arrive.
            let largest = packets
                .iter()
                .filter(|(from, _)| *from == first_address)
                .map(|(_, bytes)| bytes.len())
                .max();
            let index = packets
                .iter()
                .position(|(from, bytes)| *from == first_address && Some(bytes.len()) == largest)
                .unwrap();
            packets.remove(index);
            lost = true;
        }
        for (from, bytes) in packets {
            for forwarded in relay
                .process_at(from, &bytes, unix, now_ms)
                .unwrap_or_default()
            {
                socket
                    .send_to(&forwarded.bytes, forwarded.endpoint)
                    .unwrap();
            }
        }
        for side in 0..2 {
            production_frame(&mut worlds[side], &rules, &boundaries[side]);
        }
        if sent.is_none() && traced_send(&boundaries[0]) {
            sent = Some(step);
        }
        if boundaries[1]
            .applied
            .borrow()
            .iter()
            .any(|(_, record, applied)| {
                record.opcode() == MEGAMISSION_OPCODE && record.house_id() == 0 && *applied
            })
        {
            observed = Some(step);
            break;
        }
    }
    let (sent, observed) = (sent.unwrap(), observed.expect("Move executed"));
    assert!(lost);
    // The 100ms retry is seven 16ms buckets away; the duplicate needs none.
    assert!(
        observed - sent <= 4,
        "Move waited {} steps after one lost datagram",
        observed - sent
    );
    let live = boundaries[0].live.borrow();
    let trace = live
        .command_delivery_traces()
        .last()
        .expect("command batch traced");
    assert!(trace.sent_at.is_some() && trace.assigned.is_some());
}

#[test]
fn sequenced_transport_sends_input_while_the_canonical_frontier_is_closed() {
    use crate::net::udp::test_support::MAX_DATAGRAM;
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};

    let (mut worlds, rules, actor, boundaries, socket, mut relay) = sequenced_pair();
    let first_address = boundaries[0].live.borrow().primary_addr().unwrap();
    let pump = |relay: &mut crate::net::udp::test_support::RelayServer,
                now_ms: u64,
                hold_sender: bool| {
        loop {
            let mut packet = [0; MAX_DATAGRAM + 1];
            let (length, std::net::SocketAddr::V4(from)) = (match socket.recv_from(&mut packet) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("{error}"),
            }) else {
                unreachable!()
            };
            for forwarded in relay
                .process_at(
                    from,
                    &packet[..length],
                    100 + (now_ms - 16_000) / 1000,
                    now_ms,
                )
                .unwrap_or_default()
            {
                if !hold_sender || forwarded.endpoint != first_address {
                    socket
                        .send_to(&forwarded.bytes, forwarded.endpoint)
                        .unwrap();
                }
            }
        }
        for forwarded in relay.drain_ordering_push(now_ms, 100 + (now_ms - 16_000) / 1000) {
            if !hold_sender || forwarded.endpoint != first_address {
                socket
                    .send_to(&forwarded.bytes, forwarded.endpoint)
                    .unwrap();
            }
        }
    };
    let mut hold_sender = false;
    let mut held_tick = None;
    let mut observed = false;
    for step in 0..250 {
        let now_ms = 16_000 + step * 16;
        // Let the real pair establish a checksum context, then withhold new
        // canonical frames. Input arrives after the sender's frontier closes.
        hold_sender |= worlds[0].session().binary_frame() >= 15;
        if hold_sender
            && held_tick.is_none()
            && worlds[0].session().binary_frame() > boundaries[0].live.borrow().committed_frontier()
        {
            let owner = worlds[0].interner.get("Alpha").unwrap();
            let record = worlds[0]
                .encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: actor,
                        target_rx: 42,
                        target_ry: 42,
                    },
                )
                .unwrap();
            let tick = worlds[0].session().tick();
            assert!(worlds[0].issue_local_command(CommandEnvelope::synchronized(record, tick)));
            held_tick = Some(tick);
        }
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            boundary.now.set((now_ms >> 4) as u32);
            boundary.live.borrow_mut().service_pending_commands(
                &mut world.network_command_queues(),
                (now_ms >> 4) as u32,
            );
        }
        pump(&mut relay, now_ms, hold_sender);
        if held_tick.is_none() {
            production_frame(&mut worlds[0], &rules, &boundaries[0]);
        }
        production_frame(&mut worlds[1], &rules, &boundaries[1]);
        observed = boundaries[1]
            .applied
            .borrow()
            .iter()
            .any(|(_, record, applied)| {
                record.opcode() == MEGAMISSION_OPCODE && record.house_id() == 0 && *applied
            });
        if observed {
            break;
        }
    }
    let held_tick = held_tick.expect("the sender reached a closed canonical frontier");
    assert_eq!(worlds[0].session().tick(), held_tick);
    assert!(
        observed,
        "transport service must send fresh input without another canonical frame"
    );
    assert_eq!(
        boundaries[1]
            .applied
            .borrow()
            .iter()
            .filter(|(_, record, applied)| record.opcode() == MEGAMISSION_OPCODE
                && record.house_id() == 0
                && *applied)
            .count(),
        1
    );
    assert_eq!(worlds[0].network_command_queues().local_len(), 1);

    // Release receipts but keep both simulations fixed. Once prior work has
    // committed, each relay frame adds only its FrameEnd record. Repeated
    // empty services must not manufacture checksum-only submissions.
    let mut committed_batches = None;
    let mut status_cursor = 1;
    let quiescent_start = u64::from(boundaries[0].now.get()) * 16;
    for step in 1..=32 {
        let now_ms = quiescent_start + step * 16;
        relay.advance_clock(now_ms, 100 + (now_ms - 16_000) / 1000);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            boundary.live.borrow_mut().service_pending_commands(
                &mut world.network_command_queues(),
                (now_ms >> 4) as u32,
            );
        }
        pump(&mut relay, now_ms, false);
        let mut peer = boundaries[1].live.borrow_mut();
        // An explicit diagnostic read is independent of normal pushed
        // delivery. Idle clients no longer request a status every frame.
        peer.transport.pull_ordering(status_cursor).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut saw_status = false;
        while !saw_status && Instant::now() < deadline {
            pump(&mut relay, now_ms, false);
            peer.transport.service((now_ms >> 4) as u32);
            while let Some(event) = peer.transport.pop_ordering_received() {
                match event {
                    vera20k_relay_protocol::OrderEvent::Status(status) => {
                        saw_status = true;
                        status_cursor = status.next_seq;
                        if step >= 12 {
                            let count = status.next_seq - u64::from(status.head_frame);
                            assert_eq!(
                                *committed_batches.get_or_insert(count),
                                count,
                                "empty transport services created extra canonical batches"
                            );
                        }
                    }
                    vera20k_relay_protocol::OrderEvent::Error { code, .. } => {
                        panic!("relay rejected the retained stream: {code:?}");
                    }
                    _ => {}
                }
            }
            std::thread::yield_now();
        }
        assert!(saw_status, "relay returned this service's status");
    }
    assert!(
        committed_batches.is_some(),
        "observed the quiescent relay timeline"
    );
    assert_eq!(worlds[0].session().tick(), held_tick);
    assert_eq!(worlds[0].network_command_queues().local_len(), 0);
}
