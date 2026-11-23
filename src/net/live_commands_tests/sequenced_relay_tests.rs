//! Sequenced relay tests: the loopback relay fixture and its paired games, push
//! and repair of lost ranges, pause, lease renewal, slow-client pacing and how
//! relay failures end or keep a match.

use super::*;

/// Real authenticated UDP transport and production frames, with the relay's
/// clock independent of either client's frame/UI service.
#[test]
fn sequenced_udp_paused_client_catches_up_without_stalling_healthy_commands() {
    use crate::sim::command::{Command, IDLE_OPCODE, MEGAMISSION_OPCODE};
    use std::collections::BTreeMap;
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
    let (mut worlds, rules, actor, boundaries, mut relay) =
        sequenced_pair_with(0, Some(&journal_dir));
    let mut beta = 0;
    for sim in &mut worlds {
        beta = sim.spawn_object("AMCV", "Beta", 30, 30, 0, &rules).unwrap();
    }
    let second_address = boundaries[1].live.borrow().primary_addr().unwrap();
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
        for side in 0..2 {
            boundaries[side].now.set((now_ms >> 4) as u32);
            boundaries[side]
                .live
                .borrow_mut()
                .service_transport((now_ms >> 4) as u32);
        }
        // A suspended client loses what it sends and what the relay sends
        // it, pushed frames included.
        let mut datagrams = relay.received();
        datagrams.retain(|(from, _)| !paused || *from != second_address);
        let mut overtaken = false;
        relay.answer(now_ms, datagrams, |reply| {
            if paused && reply.endpoint == second_address {
                return false;
            }
            // Hold one complete authenticated reply until a later one has
            // gone. Recovery uses the immutable cursor, never a
            // simulation-side prediction.
            if healthy_frame >= 8 && !reordered && held.is_none() {
                held = Some((reply.endpoint, reply.bytes.clone()));
                return false;
            }
            overtaken |= held.is_some();
            true
        });
        if overtaken && let Some((endpoint, bytes)) = held.take() {
            relay.socket.send_to(&bytes, endpoint).unwrap();
            reordered = true;
            settle_loopback();
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
    drop(boundaries);
    drop(relay);
    std::fs::remove_dir_all(journal_dir).unwrap();
}

/// A record a newer relay keeps sending, which this build cannot read, ends
/// the match through the session's service once repeated for five seconds;
/// a lone earlier one does not start that window.
#[test]
fn sequenced_service_ends_a_match_a_newer_relay_keeps_stalling() {
    use vera20k_relay_protocol::OrderEvent;

    let (_, _, _, boundaries, _) = sequenced_pair();
    let mut live = boundaries[0].live.borrow_mut();
    let live = &mut *live;
    let session = live.sequenced.as_mut().unwrap();
    // Before the start, no Status this build can read arrives.
    let mut unreadable_at = |now: u32| {
        live.transport
            .push_ordering_received_for_test(OrderEvent::Unrecognized);
        session.service(&mut live.transport, now);
        session.failed()
    };
    assert!(!unreadable_at(0));
    assert!(!unreadable_at(6_000));
    for now in (7_000..11_000).step_by(1_000) {
        assert!(!unreadable_at(now));
    }
    assert!(unreadable_at(11_000));
}

/// A started match whose relay goes silent waits for it, showing Reconnecting,
/// until the transport releases the route; then it ends as absent instead of
/// waiting for as long as the game stays open.
#[test]
fn sequenced_match_ends_as_absent_once_its_silent_relay_route_is_released() {
    let (mut worlds, rules, _, boundaries, mut relay) = sequenced_pair();
    let counters = |boundary: &Boundary| boundary.live.borrow().delivery_counters().unwrap();
    let frame = |worlds: &mut [Simulation; 2], ticks: u32| {
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            boundary.now.set(ticks);
            let mut network = boundary;
            let mut effects = FrameEffects::empty().with_network_commands(&mut network);
            if world.pending_network_wait() {
                let _ = world.resume_network_wait(&rules, &mut effects);
            } else {
                let _ = world.advance_app_frame(&[], &rules, 33, TickLane::Ordinary, &mut effects);
            }
        }
    };
    let mut now_ms = 16_000;
    for _ in 0..200 {
        now_ms += 16;
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |_| true);
        frame(&mut worlds, (now_ms >> 4) as u32);
        if boundaries.iter().all(|boundary| counters(boundary).started) {
            break;
        }
    }
    assert!(boundaries.iter().all(|boundary| counters(boundary).started));
    // From here the relay answers nothing. The last answer came within the
    // second before, so a second short of the limit both games still wait.
    let silent = (now_ms >> 4) as u32;
    let limit = ((vera20k_relay_protocol::ABSENT_MS + 15_000) / 16) as u32;
    frame(&mut worlds, silent + limit - 63);
    for boundary in &boundaries {
        assert_eq!(counters(boundary).failure, None);
        assert!(boundary.live.borrow().relay_expires().is_some());
    }
    frame(&mut worlds, silent + limit + 1);
    for boundary in &boundaries {
        assert_eq!(counters(boundary).failure, Some("absent"));
        assert!(boundary.live.borrow().relay_expires().is_none());
    }
}

#[test]
fn sequenced_unsent_batch_retries_when_the_relay_route_recovers() {
    use vera20k_relay_protocol::OrderEvent;

    let (_, _, _, boundaries, mut relay) = sequenced_pair();
    // The relay orders only matches of two or more sources. Both games
    // start, and from then on the second stays silent.
    let mut now = 16_000;
    for _ in 0..60 {
        for boundary in &boundaries {
            boundary.live.borrow_mut().service_transport(now >> 4);
        }
        relay.pass(u64::from(now), |_| true);
        if boundaries[0]
            .live
            .borrow()
            .sequenced
            .as_ref()
            .unwrap()
            .started()
        {
            break;
        }
        now += 16;
    }
    let mut live = boundaries[0].live.borrow_mut();
    let live = &mut *live;
    let (session, wire) = (live.sequenced.as_mut().unwrap(), &mut live.transport);
    assert!(session.started(), "authenticated ordering startup");
    let pump = |wire: &mut LanUdpTransport, relay: &mut LoopbackRelay, now: u32| {
        wire.service(now >> 4);
        relay.pass(u64::from(now), |_| true);
        wire.service(now >> 4);
    };
    let mut sim = Simulation::new();
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    assert_eq!(
        session
            .poll(
                wire,
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
            wire,
            &mut sim.network_command_queues(),
            NetworkQueueContext {
                frame: 1,
                checksum: 0,
            },
            now,
        )
        .unwrap();

    let recovered_at = now + 16;
    let deadline = Instant::now() + STALL_LIMIT;
    while !wire.relay_ready() && Instant::now() < deadline {
        pump(wire, &mut relay, recovered_at);
        std::thread::yield_now();
    }
    assert!(
        wire.relay_ready(),
        "same route completed its real challenge"
    );
    session.service(wire, recovered_at);
    pump(wire, &mut relay, recovered_at);
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
    let mut acknowledged = drain_ack(wire);
    let deadline = Instant::now() + STALL_LIMIT;
    while !acknowledged && Instant::now() < deadline {
        pump(wire, &mut relay, recovered_at);
        acknowledged = drain_ack(wire);
        std::thread::yield_now();
    }
    assert!(
        acknowledged,
        "an unsent batch must not wait for the 100ms retransmission deadline"
    );

    // The relay session owns the acceptance: it resends nothing more
    // (spec §6.4), and the batch leaves OutList although this game never
    // read the ACK, as when its full inbox drops one.
    session.service(wire, recovered_at + 112);
    pump(wire, &mut relay, recovered_at + 112);
    assert!(!drain_ack(wire), "no retry of an acknowledged batch");
    assert_eq!(
        sim.network_command_queues().local_records().unwrap(),
        vec![&stop]
    );
    session.service_pending_commands(wire, &mut sim.network_command_queues(), recovered_at + 112);
    assert_eq!(session.sent_commands(), 1);
    assert!(
        sim.network_command_queues()
            .local_records()
            .unwrap()
            .is_empty()
    );
}

type SequencedPair = ([Simulation; 2], RuleSet, u64, Vec<Boundary>, LoopbackRelay);

/// The relay both games of a sequenced test reach, on one loopback socket. Its
/// Unix clock reads 100 s at the tests' 16 000 ms start.
struct LoopbackRelay {
    server: crate::net::udp::test_support::RelayServer,
    socket: UdpSocket,
}

impl LoopbackRelay {
    /// What the games have sent the relay since it last read, with each sender.
    fn received(&self) -> Vec<(SocketAddrV4, Vec<u8>)> {
        settle_loopback();
        let mut bytes = [0; crate::net::udp::test_support::MAX_DATAGRAM + 1];
        std::iter::from_fn(|| match self.socket.recv_from(&mut bytes) {
            Ok((length, std::net::SocketAddr::V4(from))) => Some((from, bytes[..length].to_vec())),
            Ok(_) => unreachable!(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(error) => panic!("{error}"),
        })
        .collect()
    }

    /// Moves the relay's clock to `relay_ms`, has it read `datagrams`, and
    /// delivers each of its replies and pushes that `keep` lets through.
    fn answer(
        &mut self,
        relay_ms: u64,
        datagrams: Vec<(SocketAddrV4, Vec<u8>)>,
        mut keep: impl FnMut(&vera20k_relay_protocol::ForwardDatagram) -> bool,
    ) {
        let unix = 100 + (relay_ms - 16_000) / 1000;
        self.server.advance_clock(relay_ms, unix);
        let mut replies = Vec::new();
        for (from, bytes) in datagrams {
            replies.extend(
                self.server
                    .process_at(from, &bytes, unix, relay_ms)
                    .unwrap_or_default(),
            );
        }
        replies.extend(self.server.drain_ordering_push(relay_ms, unix));
        for reply in replies {
            if keep(&reply) {
                self.socket.send_to(&reply.bytes, reply.endpoint).unwrap();
            }
        }
        settle_loopback();
    }

    /// `answer` for everything the games have sent.
    fn pass(
        &mut self,
        relay_ms: u64,
        keep: impl FnMut(&vera20k_relay_protocol::ForwardDatagram) -> bool,
    ) {
        let datagrams = self.received();
        self.answer(relay_ms, datagrams, keep);
    }
}

/// Services both games' transports and pending commands at `now_ms`.
fn service_pair(worlds: &mut [Simulation; 2], boundaries: &[Boundary], now_ms: u64) {
    for (world, boundary) in worlds.iter_mut().zip(boundaries) {
        boundary.now.set((now_ms >> 4) as u32);
        boundary
            .live
            .borrow_mut()
            .service_pending_commands(&mut world.network_command_queues(), (now_ms >> 4) as u32);
    }
}

fn sequenced_pair() -> SequencedPair {
    sequenced_pair_with(0, None)
}

/// `offset` makes the second game's start agreement differ from the first's;
/// `journal` is the directory the relay keeps its journal in.
fn sequenced_pair_with(offset: u64, journal: Option<&std::path::Path>) -> SequencedPair {
    sequenced_pair_paced(offset, journal, Default::default())
}

/// [`sequenced_pair_with`] under a certificate with `pacing`.
fn sequenced_pair_paced(
    offset: u64,
    journal: Option<&std::path::Path>,
    pacing: crate::net::udp::CertificatePacing,
) -> SequencedPair {
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
    let server = match journal {
        Some(directory) => RelayServer::with_journal(key.clone(), 2, directory).unwrap(),
        None => RelayServer::new(key.clone(), 2),
    };
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
        pacing,
        ordered_alone: false,
    }
    .sign(&key)
    .unwrap();
    let world_hash = worlds[0].state_hash();
    let agreement = |source: u8| {
        StartupAgreement::new(
            [(0, 0), (1, 1)],
            [0, 1],
            73,
            10,
            30,
            NonZeroU32::new(1).unwrap(),
            world_hash.wrapping_add(u64::from(source) * offset),
            0,
            "",
            "",
        )
        .unwrap()
    };
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
            let clock = relay_clock(LiveCommandMode::Wol, &wire, 2).unwrap();
            Boundary::new(
                LiveCommandSession::new(
                    agreement(source),
                    wire,
                    source as i8,
                    73,
                    NonZeroU32::new(1).unwrap(),
                    LiveCommandMode::Wol,
                    clock,
                )
                .unwrap(),
            )
        })
        .collect();
    (
        worlds,
        rules,
        actor,
        boundaries,
        LoopbackRelay { server, socket },
    )
}

/// The relay answers a second game with a different start agreement by an
/// error it counts nowhere, so that game's report has to name it.
#[test]
fn sequenced_agreement_mismatch_names_itself_in_the_match_report() {
    let (mut worlds, rules, _, boundaries, mut relay) = sequenced_pair_with(1, None);
    let counters = |boundary: &Boundary| boundary.live.borrow().delivery_counters().unwrap();
    let mut now_ms = 16_000;
    for _ in 0..200 {
        now_ms += 16;
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |_| true);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            let mut network = boundary;
            let mut effects = FrameEffects::empty().with_network_commands(&mut network);
            // This match is meant to fail; its report is what is checked.
            if world.pending_network_wait() {
                let _ = world.resume_network_wait(&rules, &mut effects);
            } else {
                let _ = world.advance_app_frame(&[], &rules, 33, TickLane::Ordinary, &mut effects);
            }
        }
        if boundaries
            .iter()
            .any(|boundary| counters(boundary).failure.is_some())
        {
            break;
        }
    }
    let mut failures: Vec<_> = boundaries
        .iter()
        .map(|boundary| counters(boundary).failure)
        .collect();
    failures.sort_unstable();
    assert_eq!(failures, [None, Some("agreement_mismatch")]);
    assert!(
        boundaries
            .iter()
            .all(|boundary| !counters(boundary).started)
    );
}

/// Under continuous pacing the same slow client slows the match to a whole
/// rate under what it ran, not to the Game Speed row below it: both clients
/// read the marker and pace at it, and the slow one catches up.
///
/// A step lowers the rate by at most a quarter, so 30 reaches 18 in two
/// steps (30, 23, 18), and the slow client then drains what it fell behind by
/// in the first one at the three frames a second it has to spare.
#[test]
fn continuous_pacing_slows_both_clients_to_a_whole_rate() {
    use crate::net::live_commands::MatchSpeedChange;
    use crate::net::udp::CertificatePacing;

    let (mut worlds, rules, _, boundaries, mut relay) =
        sequenced_pair_paced(0, None, CertificatePacing::Continuous);
    let mut now_ms = 16_000;
    let mut changes = [Vec::new(), Vec::new()];
    for step in 0..2400 {
        now_ms += 16;
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |_| true);
        for (index, (world, boundary)) in worlds.iter_mut().zip(&boundaries).enumerate() {
            if index == 0 || step % 3 == 0 {
                production_frame(world, &rules, boundary);
            }
        }
        for (seen, boundary) in changes.iter_mut().zip(&boundaries) {
            seen.extend(boundary.live.borrow_mut().take_speed_changes());
        }
    }
    // 21 a second: nine tenths of it, where rows went to 15.
    for (seen, boundary) in changes.iter().zip(&boundaries) {
        let rate = boundary.live.borrow().relay_frames_per_second().unwrap();
        assert!((16..=20).contains(&rate), "paced at {rate}");
        assert!(
            (1..=2).contains(&seen.len())
                && seen.iter().all(|change| *change == MatchSpeedChange::SlowedBy(1)),
            "speed changes {seen:?}"
        );
    }
    let [fast, slow] = worlds
        .each_ref()
        .map(|world| world.session().binary_frame());
    assert!(fast - slow <= 2, "the slow client is {} frames behind", fast - slow);
}

/// A client that runs fewer frames than the relay seals slows the match to a
/// Game Speed rate it can drain its waiting frames at. Both clients pace at
/// it and get one line naming the slow player, the slow one catches up, and
/// the simulation keeps the certificate's rate.
#[test]
fn a_client_that_cannot_keep_up_slows_both_clients_until_it_catches_up() {
    use crate::net::live_commands::MatchSpeedChange;

    let (mut worlds, rules, _, boundaries, mut relay) = sequenced_pair();
    let mut now_ms = 16_000;
    let mut changes = [Vec::new(), Vec::new()];
    for step in 0..1200 {
        now_ms += 16;
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |_| true);
        // The second PC runs one frame in three 16 ms wakes: 21 a second
        // against the certificate's 30.
        for (index, (world, boundary)) in worlds.iter_mut().zip(&boundaries).enumerate() {
            if index == 0 || step % 3 == 0 {
                production_frame(world, &rules, boundary);
            }
        }
        for (seen, boundary) in changes.iter_mut().zip(&boundaries) {
            seen.extend(boundary.live.borrow_mut().take_speed_changes());
        }
    }
    // 21 a second leaves 15 the only rate with a tenth to spare.
    for (seen, boundary) in changes.iter().zip(&boundaries) {
        assert_eq!(boundary.live.borrow().relay_frames_per_second(), Some(15));
        assert_eq!(seen, &[MatchSpeedChange::SlowedBy(1)]);
    }
    let [fast, slow] = worlds
        .each_ref()
        .map(|world| world.session().binary_frame());
    assert!(
        fast - slow <= 2,
        "the slow client is {} frames behind",
        fast - slow
    );
    for world in &worlds {
        assert_eq!(
            world
                .session()
                .network_command_timing()
                .unwrap()
                .frames_per_second(),
            30
        );
    }
}

#[test]
fn sequenced_push_advances_without_requests_and_repairs_a_lost_range() {
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};

    let (mut worlds, rules, actor, boundaries, mut relay) = sequenced_pair();
    let second_address = boundaries[1].live.borrow().primary_addr().unwrap();
    let mut now_ms = 16_000;
    for _ in 0..250 {
        now_ms += 16;
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |_| true);
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
        service_pair(&mut worlds, &boundaries, now_ms);
        idle_requests += relay.received().len();
        relay.answer(now_ms, Vec::new(), |_| true);
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
        service_pair(&mut worlds, &boundaries, now_ms);
        // Lose a real range of pushed frames and replies, then allow the
        // existing Pull/history owner to repair the contiguous stream.
        relay.pass(now_ms, |reply| {
            step >= 15 || reply.endpoint != second_address
        });
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

fn pausable_sequenced_pair() -> (SequencedPair, u64) {
    let (mut worlds, rules, actor, boundaries, mut relay) = sequenced_pair();
    let mut now = 16_000;
    for _ in 0..250 {
        now += 16;
        service_pair(&mut worlds, &boundaries, now);
        relay.pass(now, |_| true);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            production_frame(world, &rules, boundary);
        }
        if worlds
            .iter()
            .all(|world| world.session().binary_frame() >= 12)
            && boundaries.iter().all(|boundary| {
                boundary
                    .live
                    .borrow()
                    .multiplayer_pause()
                    .is_some_and(|pause| pause.available)
            })
        {
            return ((worlds, rules, actor, boundaries, relay), now);
        }
    }
    panic!("players could not reach a pausable relay timeline");
}

#[test]
fn ended_narrative_hold_stops_pause_retries_but_resumes_a_late_pause() {
    use crate::net::live_commands::NarrativeHold;
    use vera20k_relay_protocol::envelope::{CONTROL, Direction, HEADER_LEN, HOP_TAG_LEN, peek};

    let ((mut worlds, rules, _, boundaries, mut relay), mut now) = pausable_sequenced_pair();
    let mut host = boundaries[0].live.borrow_mut();
    let host_endpoint = host.primary_addr().unwrap();
    host.set_narrative_hold(Some(NarrativeHold::Host));
    drop(host);

    now += 16;
    service_pair(&mut worlds, &boundaries, now);
    let is_host_pause = |(from, bytes): &(SocketAddrV4, Vec<u8>)| {
        *from == host_endpoint
            && peek(Direction::Up, bytes).is_ok_and(|header| header.kind == CONTROL)
            // PAUSE_REQUEST payload ends in the requested pause bit.
            && bytes.get(HEADER_LEN) == Some(&5)
            && bytes.get(bytes.len() - HOP_TAG_LEN - 1) == Some(&1)
    };
    let (delayed, other): (Vec<_>, Vec<_>) = relay.received().into_iter().partition(is_host_pause);
    assert_eq!(delayed.len(), 1);
    relay.answer(now, other, |_| true);
    boundaries[0].live.borrow_mut().set_narrative_hold(None);

    // More than two retry intervals pass before the original request reaches
    // the relay. Ending the hold leaves no pending request in the retry slot.
    for _ in 0..16 {
        now += 16;
        service_pair(&mut worlds, &boundaries, now);
        let datagrams = relay.received();
        assert!(!datagrams.iter().any(is_host_pause), "late pause retry");
        relay.answer(now, datagrams, |_| true);
        for (world, boundary) in worlds.iter_mut().zip(&boundaries) {
            production_frame(world, &rules, boundary);
        }
    }
    let pause = boundaries[0].live.borrow().multiplayer_pause().unwrap();
    assert_eq!(pause.epoch, 0);
    assert!(!pause.pending);

    relay.answer(now, delayed, |_| true);
    for _ in 0..30 {
        now += 16;
        service_pair(&mut worlds, &boundaries, now);
        relay.pass(now, |_| true);
    }
    for boundary in &boundaries {
        let pause = boundary.live.borrow().multiplayer_pause().unwrap();
        assert_eq!(
            pause.epoch, 2,
            "the sent pause and automatic resume both land"
        );
        assert!(!pause.paused && !pause.pending);
    }
}

#[test]
fn authored_narrative_hold_stops_the_ordered_relay_clock() {
    use crate::net::live_commands::NarrativeHold;

    let ((mut worlds, _, _, boundaries, mut relay), mut now) = pausable_sequenced_pair();
    for (boundary, hold) in boundaries
        .iter()
        .zip([NarrativeHold::Host, NarrativeHold::Player])
    {
        boundary.live.borrow_mut().set_narrative_hold(Some(hold));
    }
    let head_before_hold = boundaries[0].live.borrow().relay_head_frame().unwrap();
    let pump_held =
        |now: &mut u64, worlds: &mut [Simulation; 2], relay: &mut LoopbackRelay, passes: usize| {
            for _ in 0..passes {
                *now += 16;
                service_pair(worlds, &boundaries, *now);
                relay.pass(*now, |_| true);
            }
        };
    pump_held(&mut now, &mut worlds, &mut relay, 36);
    let pause = boundaries[0].live.borrow().multiplayer_pause().unwrap();
    assert!(pause.paused);
    assert!(pause.frame - head_before_hold < 15);
    assert!(
        !boundaries[1]
            .live
            .borrow_mut()
            .request_multiplayer_pause(false)
    );

    // No player consumes frames during the clip. An active relay clock
    // would seal another 120 frames during this interval.
    pump_held(&mut now, &mut worlds, &mut relay, 250);
    for boundary in &boundaries {
        let live = boundary.live.borrow();
        assert_eq!(live.relay_head_frame(), Some(pause.frame));
        assert_eq!(live.committed_frontier(), pause.frame);
    }

    for boundary in &boundaries {
        boundary.live.borrow_mut().set_narrative_hold(None);
    }
    pump_held(&mut now, &mut worlds, &mut relay, 24);
    for boundary in &boundaries {
        let live = boundary.live.borrow();
        let resumed = live.multiplayer_pause().unwrap();
        assert!(!resumed.paused);
        assert_eq!(resumed.epoch, pause.epoch + 1);
        let head = live.relay_head_frame().unwrap();
        assert!((pause.frame + 1..pause.frame + 15).contains(&head));
    }
}

#[test]
fn sequenced_pause_stops_at_one_boundary_and_recovers_a_lost_resume() {
    use crate::net::live_commands::MatchPauseChange;
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};
    use vera20k_relay_protocol::envelope::{CONTROL, Direction, HEADER_LEN, peek};

    let (mut worlds, rules, actor, boundaries, mut relay) = sequenced_pair();
    let second_address = boundaries[1].live.borrow().primary_addr().unwrap();
    let mut committed = [
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    ];
    let step = |worlds: &mut [Simulation; 2],
                relay: &mut LoopbackRelay,
                committed: &mut [std::collections::BTreeMap<u32, u64>; 2],
                now_ms: u64,
                lose_second: bool,
                lose_pause_status: bool| {
        service_pair(worlds, &boundaries, now_ms);
        relay.pass(now_ms, |reply| {
            // A PauseStatus reply: a CONTROL payload led by relay-protocol's
            // `PAUSE_STATUS_EVENT`. Lose those but keep canonical history.
            let pause_status = peek(Direction::Down, &reply.bytes)
                .is_ok_and(|header| header.kind == CONTROL)
                && reply.bytes.get(HEADER_LEN) == Some(&8);
            reply.endpoint != second_address
                || (!lose_second && !(lose_pause_status && pause_status))
        });
        for (side, (world, boundary)) in worlds.iter_mut().zip(&boundaries).enumerate() {
            boundary
                .live
                .borrow_mut()
                .service_transport((now_ms >> 4) as u32);
            for _ in 0..4 {
                // Use the production app's canonical admission rule. A
                // retained post-Logic transaction must finish, not be replayed.
                // Local EXIT4C7917 skips MainTick55DE7E's frame commit; like
                // the app, never run an exited world's Logic on that frame again.
                let frame = world.session().binary_frame();
                if world.quit_requested()
                    || (!world.pending_network_wait()
                        && frame != 0
                        && frame > boundary.live.borrow().committed_frontier())
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
    // Only the other player is told who paused, once, though it lost the
    // marker's first delivery.
    let named: Vec<_> = boundaries
        .iter()
        .map(|boundary| boundary.live.borrow_mut().take_pause_changes())
        .collect();
    assert_eq!(
        named,
        [
            vec![],
            vec![MatchPauseChange {
                house: 0,
                paused: true
            }]
        ]
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
    let named: Vec<_> = boundaries
        .iter()
        .map(|boundary| boundary.live.borrow_mut().take_pause_changes())
        .collect();
    assert_eq!(
        named,
        [
            vec![MatchPauseChange {
                house: 1,
                paused: false
            }],
            vec![]
        ]
    );
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
    // Neither world has a local House, so each runs Alpha's EXIT as its own,
    // possibly a step apart. Later relay frames must leave both on their
    // uncommitted EXIT frame.
    let ended = worlds
        .each_ref()
        .map(|world| world.session().binary_frame());
    for _ in 0..5 {
        now_ms += 16;
        step(
            &mut worlds,
            &mut relay,
            &mut committed,
            now_ms,
            false,
            false,
        );
    }
    assert_eq!(
        worlds
            .each_ref()
            .map(|world| world.session().binary_frame()),
        ended
    );
}

/// A room whose players are on several relays answers a pause probe without
/// this game's own source (relay `control_pause`): it never pauses, so the
/// game stops asking. One still waiting for another player keeps asking.
#[test]
fn a_relay_room_that_cannot_pause_is_not_asked_again() {
    use vera20k_relay_protocol::OrderEvent;

    let (_, _, _, boundaries, mut relay) = sequenced_pair();
    let mut live = boundaries[0].live.borrow_mut();
    let live = &mut *live;
    // Only the relay's handshake reaches this game: HELLO, CHALLENGE, READY.
    for now in 1..=3 {
        live.transport.service(now);
        relay.pass(16_000, |_| true);
    }
    assert!(live.transport.relay_ready());
    let session = live.sequenced.as_mut().unwrap();
    let mut service = |now: u32, event: Option<OrderEvent>| {
        if let Some(event) = event {
            live.transport.push_ordering_received_for_test(event);
        }
        session.service(&mut live.transport, now);
        relay.received().len()
    };
    let started = OrderEvent::Status(sequenced::fixtures::started_status());
    let pause_status = |supported_sources| OrderEvent::PauseStatus {
        supported_sources,
        active_sources: 3,
        request_id: 0,
        epoch: 0,
        paused: false,
        frame: 0,
    };
    assert_eq!(service(0, None), 1, "its Ready");
    assert_eq!(service(1, Some(started)), 1, "the first probe");
    assert_eq!(
        service(501, Some(pause_status(0b01))),
        1,
        "the other player has not yet"
    );
    assert_eq!(service(1_001, Some(pause_status(0))), 0);
    for now in (1_501..=6_001).step_by(500) {
        assert_eq!(service(now, None), 0, "at {now} ms");
    }
    assert!(!session.pause_status().available);
}

#[test]
fn sequenced_push_resubscribes_as_soon_as_a_renewed_lease_is_confirmed() {
    use crate::net::udp::{
        RelayTicket,
        test_support::{SecretKey, derive_packet_key},
    };
    let (mut worlds, rules, _, boundaries, mut relay) = sequenced_pair();
    let std::net::SocketAddr::V4(address) = relay.socket.local_addr().unwrap() else {
        unreachable!()
    };
    let mut pass = |relay_ms: u64, client_ms: u64| {
        for boundary in &boundaries {
            boundary.now.set((client_ms >> 4) as u32);
            boundary
                .live
                .borrow_mut()
                .service_transport((client_ms >> 4) as u32);
        }
        relay.pass(relay_ms, |_| true);
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
    // Hold the client clock so no watermark falls due, and wait on the confirmed
    // lease itself.
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
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};
    let (mut worlds, rules, actor, boundaries, mut relay) = sequenced_pair();
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
        for boundary in &boundaries {
            boundary.now.set((now_ms >> 4) as u32);
            boundary
                .live
                .borrow_mut()
                .service_transport((now_ms >> 4) as u32);
        }
        relay.pass(now_ms, |reply| {
            !hold_receipts || reply.endpoint != first_address
        });
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
fn sequenced_fresh_batch_survives_one_lost_datagram_within_an_rto() {
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};
    let (mut worlds, rules, actor, boundaries, mut relay) = sequenced_pair();
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
            .next()
            .is_some()
    };
    for step in 0..250 {
        let now_ms = 16_000 + step * 16;
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
        service_pair(&mut worlds, &boundaries, now_ms);
        if sent.is_none() && traced_send(&boundaries[0]) {
            sent = Some(step);
        }
        let mut packets = relay.received();
        if sent.is_some() && !lost {
            // Lose the first datagram of the fresh command batch, the
            // sender's largest; small control datagrams still arrive.
            let largest = packets
                .iter()
                .filter(|(from, _)| *from == first_address)
                .map(|(_, bytes)| bytes.len())
                .max();
            // The batch may leave on the next service: wait for it.
            if let Some(index) = packets
                .iter()
                .position(|(from, bytes)| *from == first_address && Some(bytes.len()) == largest)
            {
                packets.remove(index);
                lost = true;
            }
        }
        relay.answer(now_ms, packets, |_| true);
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
    // A good link sends no copy: the next UPLINK repairs it within
    // max(100 ms, RTO), seven 16 ms buckets (spec §13.2), plus the turn.
    assert!(
        observed - sent <= 11,
        "Move waited {} steps after one lost datagram",
        observed - sent
    );
    let live = boundaries[0].live.borrow();
    let trace = live
        .command_delivery_traces()
        .last()
        .expect("command batch traced");
    assert!(trace.assigned.is_some());
}

#[test]
fn sequenced_transport_sends_input_while_the_canonical_frontier_is_closed() {
    use crate::sim::command::{Command, MEGAMISSION_OPCODE};

    let (mut worlds, rules, actor, boundaries, mut relay) = sequenced_pair();
    let first_address = boundaries[0].live.borrow().primary_addr().unwrap();
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
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |reply| {
            !hold_sender || reply.endpoint != first_address
        });
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
        service_pair(&mut worlds, &boundaries, now_ms);
        relay.pass(now_ms, |_| true);
        let mut peer = boundaries[1].live.borrow_mut();
        // An explicit diagnostic read is independent of normal pushed
        // delivery. Idle clients no longer request a status every frame.
        peer.transport.pull_ordering(status_cursor).unwrap();
        let deadline = Instant::now() + STALL_LIMIT;
        let mut saw_status = false;
        while !saw_status && Instant::now() < deadline {
            relay.pass(now_ms, |_| true);
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

/// VERA: a player whose connection this game removed, as when it ran their
/// EXIT, is not named when the relay, which may never have heard them leave,
/// stops hearing them.
#[test]
fn a_removed_player_the_relay_stops_hearing_is_not_named() {
    let (_, _, _, boundaries, _) = sequenced_pair();
    let mut live = boundaries[0].live.borrow_mut();
    let silent = |live: &mut LiveCommandSession, sources| {
        live.sequenced.as_mut().unwrap().note_silent(sources);
        live.take_connection_changes()
    };
    let named = |responding| {
        [MatchConnectionChange {
            house: 1,
            responding,
        }]
    };
    assert_eq!(silent(&mut live, 0b10), named(false));
    assert_eq!(silent(&mut live, 0), named(true));
    assert!(live.remove_peer(1));
    assert!(silent(&mut live, 0b10).is_empty());
}

/// A relay that lost a running match's configuration (NotConfigured) stops its
/// clock until it is configured again, but the match did start: a player lost
/// meanwhile leaves a running match, not a start.
#[test]
fn a_running_match_its_relay_lost_is_not_startup_pending() {
    use vera20k_relay_protocol::{OrderError, OrderEvent};

    let (_, _, _, boundaries, _) = sequenced_pair();
    let mut live = boundaries[0].live.borrow_mut();
    assert!(live.startup_pending());
    live.transport
        .push_ordering_received_for_test(OrderEvent::Status(sequenced::fixtures::started_status()));
    live.service_transport(1);
    assert!(!live.startup_pending());
    live.transport
        .push_ordering_received_for_test(OrderEvent::Error {
            code: OrderError::NotConfigured,
            expected: 0,
        });
    live.service_transport(2);
    assert!(live.awaiting_initial_barrier(), "the relay's clock stopped");
    assert!(!live.startup_pending());
}

/// The session that ends a match keeps it ended: poll reports the error once,
/// and the match still reads as failed after it.
#[test]
fn a_reported_relay_failure_stays_with_its_session() {
    use vera20k_relay_protocol::{OrderError, OrderEvent};

    let (mut worlds, _, _, boundaries, _) = sequenced_pair();
    let mut live = boundaries[0].live.borrow_mut();
    live.transport
        .push_ordering_received_for_test(OrderEvent::Error {
            code: OrderError::HistoryGap,
            expected: 0,
        });
    let mut queues = worlds[0].network_command_queues();
    let context = NetworkQueueContext {
        frame: 0,
        checksum: 0,
    };
    let reported = live.poll(&mut queues, context, 10, 1, 16, 0);
    assert!(matches!(
        reported,
        Err(LiveCommandError::Sequenced(SequencedError::HistoryExpired))
    ));
    assert!(live.has_terminal_network_error());
    let again = live.poll(&mut queues, context, 10, 2, 32, 0);
    assert!(matches!(again, Err(LiveCommandError::Failed)));
    assert_eq!(
        live.delivery_counters().unwrap().failure,
        Some("history_expired")
    );
}
