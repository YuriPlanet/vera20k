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
        LiveCommandSession::new(left, 0, seed, NonZeroU32::new(5).unwrap()).unwrap(),
        LiveCommandSession::new(right, 1, seed, NonZeroU32::new(5).unwrap()).unwrap(),
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
            )
            .unwrap();
        passes[1] = right
            .poll(
                &mut right_world.network_command_queues(),
                context,
                10,
                now,
                (now).wrapping_mul(16),
            )
            .unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "frame{frame} did not finish"
        );
        std::thread::yield_now();
    }
    (passes[0], passes[1])
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
            (1001_u32).wrapping_mul(16)
        )
        .unwrap(),
        NetworkQueuePass::Dispatch
    );
    assert_eq!(left.sent_commands(), 1);
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
                &connection_prefix(0xaa04, 2, sequence, 0),
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
    let mut live = LiveCommandSession::new(wire, 0, 73, NonZeroU32::new(5).unwrap()).unwrap();
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
                now * 16
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
        live.poll(&mut sim.network_command_queues(), context, 10, 1002, 16032)
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
            live.poll(&mut sim.network_command_queues(), next, 10, 1200, 19200)
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
    let mut live = LiveCommandSession::new(wire, 0, 73, NonZeroU32::new(5).unwrap()).unwrap();
    let mut sim = Simulation::new();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 0x12345678,
    };
    assert_eq!(
        live.poll(&mut sim.network_command_queues(), context, 5, 1000, 16000)
            .unwrap(),
        NetworkQueuePass::Waiting
    );
    let probe = live.exchange.frame_probe(1, 0, 5, 73, live.policy).unwrap();
    live.exchange
        .receive(1, &probe, 73, &mut sim.network_command_queues(), 1)
        .unwrap();
    assert_eq!(
        live.poll(&mut sim.network_command_queues(), context, 5, 1001, 16016)
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
        .receive(peer, &connection_prefix(0xaa04, 2, 0, 0), &mut |_, _| {})
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
                (1300 + frame) * 16
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
            20880
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(
        live.wait_info(),
        Some(LiveWaitInfo {
            kind: LiveWaitKind::Ordinary,
            entered_at: 1305,
            response_time_at_entry: 10,
        })
    );
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
    let mut live = LiveCommandSession::new(wire, 0, 91, NonZeroU32::new(5).unwrap()).unwrap();
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
            (1000_u32).wrapping_mul(16)
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(
        live.wait_info(),
        Some(LiveWaitInfo {
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
            (1059_u32).wrapping_mul(16)
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
            (1060_u32).wrapping_mul(16)
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
    )
    .unwrap();
    assert_eq!(live.take_service_report().unwrap().sent_datagrams, 0);
    live.poll(
        &mut sim.network_command_queues(),
        context,
        10,
        1160,
        (1160_u32).wrapping_mul(16),
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
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2000,
            (2000_u32).wrapping_mul(16)
        )
        .unwrap(),
        NetworkQueuePass::Waiting
    );
    assert_eq!(left.frame_wait().unwrap().missing_frame, Some(1));
    assert_eq!(
        left.wait_info(),
        Some(LiveWaitInfo {
            kind: LiveWaitKind::Ordinary,
            entered_at: 2000,
            response_time_at_entry: 0,
        })
    );
    assert_eq!(left.lowest_frame_peer().unwrap().house_id(), 1);
    assert_eq!(
        left.poll(
            &mut a.network_command_queues(),
            context,
            10,
            2179,
            (2179_u32).wrapping_mul(16)
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
            (2180_u32).wrapping_mul(16)
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
    )
    .unwrap();
    assert_eq!(left.take_service_report().unwrap().sent_datagrams, 0);
    left.poll(
        &mut a.network_command_queues(),
        context,
        10,
        2480,
        (2480_u32).wrapping_mul(16),
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
    let mut live = LiveCommandSession::new(wire, 0, 73, NonZeroU32::new(5).unwrap()).unwrap();
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
    // human, and removing the last remote retains the same behavior.
    assert!(LiveCommandSession::new(transport(), 0, 1, NonZeroU32::new(5).unwrap()).is_ok());
    let mut wire = transport();
    wire.connections_mut()
        .add_peer(200, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234))
        .unwrap();
    assert!(matches!(
        LiveCommandSession::new(wire, 0, 1, NonZeroU32::new(5).unwrap()),
        Err(LiveCommandError::InvalidRemoteHouse(200))
    ));
    let mut wire = transport();
    wire.connections_mut()
        .add_peer(1, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .unwrap();
    assert!(matches!(
        LiveCommandSession::new(wire, 0, 1, NonZeroU32::new(5).unwrap()),
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
            (1000_u32).wrapping_mul(16)
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
        );
        let r = right.poll(
            &mut b.network_command_queues(),
            context,
            10,
            1000,
            (1000_u32).wrapping_mul(16),
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
            (2003_u32).wrapping_mul(16)
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
            (2001_u32).wrapping_mul(16)
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
            (2002_u32).wrapping_mul(16)
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
                (2003_u32).wrapping_mul(16)
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
}

impl Boundary {
    fn new(live: LiveCommandSession) -> Self {
        Self {
            live: RefCell::new(live),
            checksums: RefCell::new(FrameChecksumState::default()),
            now: Cell::new(1000),
        }
    }
}

impl NetworkCommandBoundary for Boundary {
    fn exchange_commands(
        &self,
        context: NetworkQueueContext,
        queues: &mut NetworkCommandQueues<'_>,
    ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
        self.live
            .borrow_mut()
            .poll(
                queues,
                context,
                10,
                self.now.get(),
                (self.now.get()).wrapping_mul(16),
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
    if sim.pending_network_wait() {
        sim.resume_network_wait(
            rules,
            None,
            FrameEffects::empty().with_network_commands(boundary),
        )
        .unwrap()
    } else {
        sim.advance_app_frame(
            &[],
            rules,
            None,
            33,
            TickLane::Ordinary,
            FrameEffects::empty().with_network_commands(boundary),
        )
        .unwrap()
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
