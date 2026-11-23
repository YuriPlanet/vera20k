//! Ordinary-wait and roster tests: wait buckets, lowest-frame projection, invalid
//! roster context, retry-service failures and removing the last peer.

use super::*;

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
        None,
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
        StartupAgreement::new(
            [(0, 0)],
            [0],
            1,
            10,
            30,
            NonZeroU32::new(5).unwrap(),
            0,
            0,
            "",
            "",
        )
        .unwrap(),
        transport(),
        0,
        1,
        NonZeroU32::new(5).unwrap(),
        LiveCommandMode::Lan,
        None,
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
            LiveCommandMode::Lan,
            None
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
            LiveCommandMode::Lan,
            None
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
        );
        let r = right.poll(
            &mut b.network_command_queues(),
            context,
            10,
            1000,
            (1000_u32).wrapping_mul(16),
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
        assert!(start.elapsed() < STALL_LIMIT);
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
            )
            .unwrap();
        assert_eq!(left.take_service_report().unwrap().failed_peer, Some(1));
        if pass == NetworkQueuePass::Dispatch {
            break;
        }
        assert!(start.elapsed() < STALL_LIMIT);
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
