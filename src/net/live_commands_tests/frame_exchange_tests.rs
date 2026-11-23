//! LAN and peer-to-peer UDP frame-exchange tests through the production frame
//! boundary: delayed commands, WOL timing and adaptive delay, shared houses and
//! mass orders under backpressure.

use super::*;

#[test]
fn retail_infantry_plan_finishes_both_moves_over_lan() {
    use crate::sim::command::Command;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let scene = || {
        let mut sim = Simulation::with_seed(73);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        for (index, name) in ["Alpha", "Beta"].iter().enumerate() {
            let owner = sim.interner.intern(name);
            sim.houses
                .register(HouseState::new(owner, index as u8, None, true, 0, 10));
        }
        let owner = sim.interner.get("Alpha").unwrap();
        sim.session.set_current_house_for_test(Some(owner));
        sim.session.set_game_mode(ScenarioGameMode::Lan);
        sim.session.install_network_timing(10);
        sim.session
            .install_network_command_timing(0, std::num::NonZeroU8::new(5).unwrap(), [0, 1]);
        sim.session.game_options_mut_for_test().short_game = false;
        let actor = sim.spawn_object("E1", "Alpha", 12, 12, 0, &rules).unwrap();
        sim.spawn_object("E1", "Beta", 28, 28, 0, &rules).unwrap();
        (sim, owner, actor)
    };
    let (mut a, owner, actor) = scene();
    let (mut b, _, _) = scene();
    let (a_live, b_live) = pair(73);
    let (a_boundary, b_boundary) = (Boundary::new(a_live), Boundary::new(b_live));
    for target_rx in [13, 14] {
        let record = a
            .encode_planning_order_record(
                owner,
                &Command::Move {
                    entity_id: actor,
                    target_rx,
                    target_ry: 12,
                },
                None,
            )
            .unwrap();
        assert!(a.issue_local_command(CommandEnvelope::synchronized(record, a.session.tick())));
    }
    for frame in 0..=180 {
        if frame == 15 {
            // The ordinary orders must reach both peers before the player
            // commits. The generated flag2 events then use DoList directly.
            for sim in [&a, &b] {
                assert_eq!(sim.planning_paths(owner)[0].cells, [(13, 12), (14, 12)]);
            }
            let record = a.encode_plan_commit_record(owner).unwrap();
            assert!(a.issue_local_command(CommandEnvelope::synchronized(record, a.session.tick())));
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
                    .advance_clock(1000 + frame * 10 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(sim, &rules, boundary);
                assert!(output.network_failure.is_none());
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                completed[index] = output.tick.frame_committed;
            }
            assert!(start.elapsed() < STALL_LIMIT, "plan frame {frame} stalled");
            std::thread::yield_now();
        }
        assert_eq!(checksums[0], checksums[1], "plan frame {frame} CRC");
    }
    for (sim, boundary) in [(&a, &a_boundary), (&b, &b_boundary)] {
        let applied = boundary.applied.borrow();
        let generated: Vec<_> = applied
            .iter()
            .filter(|(_, record, _)| record.opcode() == 4 && record.payload()[22] == 2)
            .collect();
        assert_eq!(
            generated.len(),
            2,
            "both planned moves must survive dispatch"
        );
        assert!(generated.iter().all(|(_, _, ok)| *ok));
        let entity = sim.entities().get(actor).unwrap();
        assert_eq!((entity.position.rx(), entity.position.ry()), (14, 12));
        assert!(sim.planning_paths(owner).is_empty());
    }
}

/// One person watching AI play runs the match through the relay when the
/// lobby marked its certificate, so that others can watch it; any other
/// one-human match keeps local input. A LAN match never runs on the relay's
/// clock, and a certificate naming another number of players refuses the match.
#[test]
fn a_one_human_match_orders_through_the_relay_only_when_the_lobby_marked_it() {
    use crate::net::udp::{
        CertificatePacing, RelayTicket,
        test_support::{SecretKey, derive_packet_key},
    };
    let key = SecretKey::new([31; 32]);
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
    let relay_wire = |alone| {
        let mut wire = transport();
        wire.enable_relay(
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9),
            &ticket,
            *derive_packet_key(&key, &ticket).expose_for_https(),
            [29; 32],
        )
        .unwrap();
        let certificate = OrderingCertificate {
            room: [27; 16],
            sources_mask: 1,
            tick_hz: 30,
            expires: 86_500,
            pacing: CertificatePacing::On,
            ordered_alone: alone,
        };
        wire.set_ordering_certificate(certificate.sign(&key).unwrap())
            .unwrap();
        wire
    };
    let rate = NonZeroU32::new(1).unwrap();
    for alone in [false, true] {
        let wire = relay_wire(alone);
        assert_eq!(relay_clock(LiveCommandMode::Lan, &wire, 1), Ok(None));
        assert_eq!(
            relay_clock(LiveCommandMode::Wol, &wire, 2),
            Err(StartupError::ClockRoster),
            "marked: {alone}"
        );
        let clock = relay_clock(LiveCommandMode::Wol, &wire, 1).unwrap();
        let agreement =
            StartupAgreement::new([(0, 0)], [0], 73, 10, 30, rate, 0, 0, "", "").unwrap();
        let live =
            LiveCommandSession::new(agreement, wire, 0, 73, rate, LiveCommandMode::Wol, clock)
                .unwrap();
        assert_eq!(live.uses_sequenced_lockstep(), alone, "marked: {alone}");
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
        StartupAgreement::new([(0, 0)], [0], 73, 40, 30, rate, sim.state_hash(), 0, "", "")
            .unwrap();
    let mut live = LiveCommandSession::new(
        agreement,
        transport(),
        0,
        73,
        rate,
        LiveCommandMode::Wol,
        None,
    )
    .unwrap();
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
        None,
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
                    .advance_clock(1000 + frame * 10 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(sim, &rules, boundary);
                assert!(output.network_failure.is_none());
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                completed[index] = output.tick.frame_committed;
            }
            assert!(
                start.elapsed() < STALL_LIMIT,
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
                None,
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
                    .advance_clock(1000 + frame * 10 + (start.elapsed().as_millis() / 16) as u32);
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
                start.elapsed() < STALL_LIMIT,
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
            // The UDP checksum, no LAN scope header.
            let frame = &packet[crate::net::udp::CHECKSUM_LEN..];
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
                    let frame = &packet[crate::net::udp::CHECKSUM_LEN..];
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
                None,
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
            started.elapsed() < STALL_LIMIT,
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
                .entities()
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
                sim.entity_mut_for_test(actor)
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
                    .advance_clock(1000 + frame * 100 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(sim, &rules, boundary);
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                if output.tick.frame_committed {
                    completed[index] = true;
                    executed[index] += output.tick.executed_commands;
                }
            }
            assert!(start.elapsed() < STALL_LIMIT, "frame{frame} stalled");
            std::thread::yield_now();
        }
        assert_eq!(checksums[0], checksums[1], "frame{frame} CRC");
        assert_eq!(executed, if frame < 10 { [0, 0] } else { [1, 1] });
    }
    for sim in [&a, &b] {
        assert!(
            sim.entities()
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
fn two_udp_clients_sharing_a_house_both_command_it() {
    use crate::sim::command::StopRecord;
    // VERA shared control: senders 0 and 1 both control Alpha.
    let (a_live, b_live) = pair(73);
    let boundaries = [Boundary::new(a_live), Boundary::new(b_live)];
    let (a, rules, actor) = stop_scene();
    let (b, _, _) = stop_scene();
    let mut worlds = [a, b];
    for (source, sim) in (0..).zip(&mut worlds) {
        let alpha = sim.interner.get("Alpha").unwrap();
        sim.session
            .install_shared_house_sources(vec![0, 0], Some(source));
        sim.session.set_current_house_for_test(Some(alpha));
        let stop = sim.encode_stop_record(alpha, actor).unwrap();
        assert_eq!(stop.house_id(), source);
        assert!(sim.issue_local_command(CommandEnvelope::synchronized(stop, 0)));
    }
    advance_shared_house_pair(&mut worlds, &rules, &boundaries, 0..=10);
    // Each client ran both players' orders for Alpha on the same frame.
    for boundary in &boundaries {
        let mut stops: Vec<_> = boundary
            .applied
            .borrow()
            .iter()
            .filter_map(|(frame, record, ok)| {
                StopRecord::decode(record).map(|stop| (*frame, stop.house_id, *ok))
            })
            .collect();
        stops.sort();
        assert_eq!(stops, [(10, 0, true), (10, 1, true)]);
    }
}

fn advance_shared_house_pair(
    worlds: &mut [Simulation; 2],
    rules: &RuleSet,
    boundaries: &[Boundary; 2],
    frames: std::ops::RangeInclusive<u32>,
) {
    for frame in frames {
        let start = Instant::now();
        let mut completed = [false; 2];
        let mut checksums = [None; 2];
        while !completed.iter().all(|done| *done) {
            for index in 0..2 {
                if completed[index] {
                    continue;
                }
                boundaries[index]
                    .advance_clock(1000 + frame * 100 + (start.elapsed().as_millis() / 16) as u32);
                let output = production_frame(&mut worlds[index], rules, &boundaries[index]);
                if let Some(sync) = output.multiplayer_checksum {
                    checksums[index] = Some(sync.checksum.unwrap().value);
                }
                completed[index] = output.tick.frame_committed;
            }
            assert!(start.elapsed() < STALL_LIMIT, "frame{frame} stalled");
            std::thread::yield_now();
        }
        assert_eq!(checksums[0], checksums[1], "frame{frame} CRC");
    }
}

#[test]
fn shared_house_udp_keeps_each_sources_third_party_scope() {
    use crate::sim::command::{
        ARCHIVE_OPCODE, ArchiveRecord, Command, IDLE_OPCODE, MEGAMISSION_OPCODE, MegaMissionOrder,
        MegaMissionRecord, MegaMissionTarget, ObjectAction, ObjectActionRecord, ProductionEvent,
        ProductionRecord, REPAIR_OPCODE, RepairRecord, StopRecord, TARGET_HOUSE_OPCODE,
        TargetHouseRecord,
    };
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel, ControlScope};
    use crate::sim::mission::MissionType;
    use crate::sim::production::ProductionCategory;

    let (left, right) = pair(73);
    let boundaries = [Boundary::new(left), Boundary::new(right)];
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(STOP_SCENE_RULES);
    ini.merge(&IniFile::from_str(
        "[Countries]\n0=Americans\n[Sides]\nGDI=Americans\n\
         [VehicleTypes]\n1=MTNK\n\
         [MTNK]\nStrength=300\nSpeed=4\nCost=500\nTechLevel=1\nOwner=Americans\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\
         [BuildingTypes]\n0=GAWEAP\n1=GAPOWR\n\
         [GAWEAP]\nStrength=400\nOwner=Americans\nFactory=UnitType\nWeaponsFactory=yes\n\
         [GAPOWR]\nStrength=400\n",
    ));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.set_buildup_control_for_test("GAPOWR", [0, 17, 3]);
    let (a, [_, beta_unit]) = stop_scene_with_rules(&rules);
    let (b, [_, other_beta_unit]) = stop_scene_with_rules(&rules);
    assert_eq!(other_beta_unit, beta_unit);
    let mut worlds = [a, b];
    let grants = vec![
        ControlGrant {
            source: 0,
            house: 1,
            level: ControlGrantLevel::FullControl,
        },
        ControlGrant {
            source: 1,
            house: 1,
            level: ControlGrantLevel::UnitsOnly,
        },
        ControlGrant {
            source: 0,
            house: 2,
            level: ControlGrantLevel::UnitsOnly,
        },
    ];
    let add_targets = |sim: &mut Simulation| {
        let americans = sim.interner.intern("Americans");
        for name in ["Alpha", "Beta"] {
            let house = sim.interner.get(name).unwrap();
            sim.houses
                .get_mut(&house)
                .unwrap()
                .set_country_for_test(Some(americans));
        }
        let gamma = sim.interner.intern("Gamma");
        sim.houses
            .register(HouseState::new(gamma, 2, None, false, 0, 10));
        let unit = sim
            .spawn_object("AMCV", "Gamma", 40, 40, 0, &rules)
            .unwrap();
        let factory = sim
            .spawn_object("GAWEAP", "Beta", 30, 30, 0, &rules)
            .expect("buildable ground admits the Beta factory");
        let power_plant = sim
            .spawn_object("GAPOWR", "Beta", 30, 40, 0, &rules)
            .expect("buildable ground admits the Beta power plant");
        // DoList runs staged MEGAMISSIONs after ARCHIVE for the same
        // source. A Foot move clears the ArchiveTarget it just set.
        let beta_archiver = sim.spawn_object("AMCV", "Beta", 26, 25, 0, &rules).unwrap();
        let gamma_archiver = sim
            .spawn_object("AMCV", "Gamma", 41, 40, 0, &rules)
            .unwrap();
        (unit, factory, power_plant, beta_archiver, gamma_archiver)
    };
    let targets = add_targets(&mut worlds[0]);
    assert_eq!(add_targets(&mut worlds[1]), targets);
    let (gamma_unit, factory, power_plant, beta_archiver, gamma_archiver) = targets;
    for (source, sim) in (0..).zip(&mut worlds) {
        let alpha = sim.interner.get("Alpha").unwrap();
        sim.session
            .install_shared_house_sources(vec![0, 0], Some(source));
        sim.session.set_current_house_for_test(Some(alpha));
        sim.install_control_grants(grants.clone()).unwrap();
        assert!(!sim.apply_command(
            "Alpha",
            &Command::ToggleRepair { entity_id: factory },
            Some(&rules),
        ));
        let beta_house = sim.interner.get("Beta").unwrap();
        assert_eq!(
            sim.control_scope_for_source(source, beta_house),
            if source == 0 {
                ControlScope::FullControl
            } else {
                ControlScope::UnitsOnly
            },
        );
        assert_eq!(
            crate::sim::production::find_factory(
                sim,
                &rules,
                beta_house,
                rules.object("MTNK").unwrap(),
                false,
                true,
                true,
            ),
            Some(factory),
            "Beta's factory can build the targeted vehicle"
        );
        // The UDP path also carries commands that a remote source must refuse.
        for record in [
            sim.encode_stop_record(alpha, beta_unit).unwrap(),
            sim.encode_repair_record(alpha, factory).unwrap(),
            sim.encode_stop_record(alpha, gamma_unit).unwrap(),
            sim.encode_object_action_record(
                alpha,
                &Command::SellBuilding {
                    entity_id: power_plant,
                },
            )
            .unwrap(),
        ] {
            assert_eq!(record.house_id(), source);
            assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, 0)));
        }
        for entity_id in [beta_unit, gamma_unit] {
            let record = sim
                .encode_megamission_record(
                    alpha,
                    &Command::Move {
                        entity_id,
                        target_rx: 45,
                        target_ry: 44,
                    },
                )
                .unwrap();
            assert_eq!(record.house_id(), source);
            assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, 0)));
        }
        let production = sim
            .encode_production_record(
                alpha,
                ProductionEvent::Produce,
                sim.interner.get("MTNK").unwrap(),
                &rules,
            )
            .unwrap();
        let targeted = TargetHouseRecord::encode(1, &production).unwrap();
        assert_eq!(targeted.house_id(), source);
        assert!(sim.issue_local_command(CommandEnvelope::synchronized(targeted, 0)));
        if source == 1 {
            assert!(sim.issue_local_command(CommandEnvelope::new(
                alpha,
                0,
                Command::SetRally {
                    rx: 35,
                    ry: 35,
                    producer_ids: vec![factory],
                },
            )));
            for entity in [beta_archiver, gamma_archiver] {
                let mut record =
                    CommandRecord::encode(ARCHIVE_OPCODE, i32::from(source), 0, &[]).unwrap();
                ArchiveRecord {
                    house_id: source,
                    frame: 0,
                    entity_id: i32::try_from(entity).unwrap(),
                    target: MegaMissionTarget::Object {
                        id: i32::try_from(factory).unwrap(),
                    },
                }
                .write_into(&mut record)
                .unwrap();
                assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, 0)));
            }
        }
        sim.publish_scenario_started();
    }
    assert_eq!(worlds[0].state_hash(), worlds[1].state_hash());
    advance_shared_house_pair(&mut worlds, &rules, &boundaries, 0..=4);
    let add_delta = |sim: &mut Simulation| {
        assert!(sim.session.started());
        let house = sim.interner.intern("Delta");
        sim.houses
            .register(HouseState::new(house, 3, None, false, 0, 10));
        sim.spawn_object("AMCV", "Delta", 48, 48, 0, &rules)
            .expect("the new House's unit must be a live command target")
    };
    let late_unit = add_delta(&mut worlds[0]);
    assert_eq!(add_delta(&mut worlds[1]), late_unit);
    for (source, sim) in (0..).zip(&mut worlds) {
        let alpha = sim.interner.get("Alpha").unwrap();
        let stop = sim.encode_stop_record(alpha, late_unit).unwrap();
        assert_eq!(stop.house_id(), source);
        assert!(sim.issue_local_command(CommandEnvelope::synchronized(stop, sim.session.tick())));
        if source == 1 {
            for entity_id in [beta_unit, gamma_unit] {
                let plan = sim
                    .encode_planning_order_record(
                        alpha,
                        &Command::Move {
                            entity_id,
                            target_rx: 47,
                            target_ry: 46,
                        },
                        None,
                    )
                    .unwrap();
                assert_eq!(plan.house_id(), source);
                assert!(
                    sim.issue_local_command(CommandEnvelope::synchronized(
                        plan,
                        sim.session.tick()
                    ))
                );
            }
        }
    }
    advance_shared_house_pair(&mut worlds, &rules, &boundaries, 5..=10);
    for sim in &worlds {
        use crate::sim::combat::TargetKind;

        assert_eq!(
            sim.entities().get(factory).unwrap().archive_target(),
            Some(TargetKind::Cell(35, 35))
        );
        assert_eq!(
            sim.entities().get(beta_archiver).unwrap().archive_target(),
            Some(TargetKind::Entity(factory))
        );
        assert_eq!(
            sim.entities().get(gamma_archiver).unwrap().archive_target(),
            None
        );
        assert_eq!(
            sim.entities()
                .get(power_plant)
                .unwrap()
                .mission
                .effective()
                .known(),
            Some(MissionType::Selling)
        );
    }
    advance_shared_house_pair(&mut worlds, &rules, &boundaries, 11..=15);
    for boundary in &boundaries {
        let applied = boundary.applied.borrow();
        let outcome = |source, opcode, subject| {
            applied.iter().find_map(|(frame, record, ok)| {
                if record.house_id() != source || record.opcode() != opcode {
                    return None;
                }
                let actor = StopRecord::decode(record)
                    .map(|event| event.entity_id)
                    .or_else(|| RepairRecord::decode(record).map(|event| event.entity_id))
                    .or_else(|| ObjectActionRecord::decode(record).map(|event| event.entity_id))
                    .or_else(|| MegaMissionRecord::decode(record).map(|event| event.source_id))
                    .or_else(|| ArchiveRecord::decode(record).map(|event| event.entity_id))?;
                (u64::try_from(actor).ok() == Some(subject)).then_some((*frame, *ok))
            })
        };
        assert_eq!(outcome(0, IDLE_OPCODE, beta_unit), Some((10, true)));
        assert_eq!(outcome(1, IDLE_OPCODE, beta_unit), Some((10, true)));
        assert_eq!(outcome(0, REPAIR_OPCODE, factory), Some((10, true)));
        assert_eq!(outcome(1, REPAIR_OPCODE, factory), Some((10, false)));
        assert_eq!(outcome(0, IDLE_OPCODE, gamma_unit), Some((10, true)));
        assert_eq!(outcome(1, IDLE_OPCODE, gamma_unit), Some((10, false)));
        assert_eq!(outcome(0, MEGAMISSION_OPCODE, beta_unit), Some((10, true)));
        assert_eq!(outcome(1, MEGAMISSION_OPCODE, beta_unit), Some((10, true)));
        assert_eq!(outcome(0, MEGAMISSION_OPCODE, gamma_unit), Some((10, true)));
        assert_eq!(
            outcome(1, MEGAMISSION_OPCODE, gamma_unit),
            Some((10, false))
        );
        let movement = |source, actor| {
            applied
                .iter()
                .filter(|(frame, record, _)| *frame == 10 && record.house_id() == source)
                .filter_map(|(_, record, _)| MegaMissionRecord::decode(record))
                .find(|event| u64::try_from(event.source_id).ok() == Some(actor))
                .unwrap()
                .order
        };
        assert_ne!(
            movement(0, beta_unit),
            movement(0, gamma_unit),
            "both of the permitted units participate in formation spread"
        );
        assert_eq!(
            movement(1, beta_unit),
            MegaMissionOrder::Move {
                target_x: 45,
                target_y: 44
            },
            "the denied formation member cannot shift the permitted move"
        );
        // Sell also reaches one of the inner actor-ownership checks.
        let sell = ObjectAction::Sell.opcode();
        assert_eq!(outcome(0, sell, power_plant), Some((10, true)));
        assert_eq!(outcome(1, sell, power_plant), Some((10, false)));
        assert_eq!(outcome(1, ARCHIVE_OPCODE, factory), Some((10, true)));
        assert_eq!(outcome(1, ARCHIVE_OPCODE, beta_archiver), Some((10, true)));
        assert_eq!(
            outcome(1, ARCHIVE_OPCODE, gamma_archiver),
            Some((10, false))
        );
        assert_eq!(outcome(0, IDLE_OPCODE, late_unit), Some((15, false)));
        assert_eq!(outcome(1, IDLE_OPCODE, late_unit), Some((15, false)));

        let plans: Vec<_> = applied
            .iter()
            .filter(|(frame, record, _)| *frame == 15 && record.house_id() == 1)
            .filter_map(|(_, record, accepted)| {
                let (event, flag) = MegaMissionRecord::decode_planned(record)?;
                (flag == 1).then_some((u64::try_from(event.source_id).ok()?, *accepted))
            })
            .collect();
        assert_eq!(plans, [(beta_unit, true), (gamma_unit, false)]);

        let mut targeted: Vec<_> = applied
            .iter()
            .filter(|(_, record, _)| record.opcode() == TARGET_HOUSE_OPCODE)
            .map(|(frame, record, accepted)| {
                let event = TargetHouseRecord::decode(record).unwrap();
                assert_eq!(event.target_house, 1);
                assert_eq!(
                    ProductionRecord::decode(&event.command).unwrap().event,
                    ProductionEvent::Produce
                );
                (*frame, record.house_id(), *accepted)
            })
            .collect();
        targeted.sort_unstable();
        assert_eq!(targeted, [(10, 0, true), (10, 1, false)]);
    }
    assert_eq!(worlds[0].state_hash(), worlds[1].state_hash());
    for sim in &worlds {
        let alpha = sim.interner.get("Alpha").unwrap();
        let beta = sim.interner.get("Beta").unwrap();
        let gamma = sim.interner.get("Gamma").unwrap();
        let factories = sim.production().factories();
        assert!(factories.view(alpha, ProductionCategory::Vehicle).is_none());
        let production = factories.view(beta, ProductionCategory::Vehicle).unwrap();
        assert_eq!(
            production.object.map(|product| product.type_id),
            sim.interner.get("MTNK")
        );
        assert!(production.queue.is_empty());
        let paths = sim.planning_paths(beta);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].actor_id, beta_unit);
        assert_eq!(paths[0].cells, [(47, 46)]);
        assert!(sim.planning_paths(gamma).is_empty());
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
                started.elapsed() < STALL_LIMIT,
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
            sim.entities()
                .get(*id)
                .unwrap()
                .navigation
                .nav_list()
                .is_empty()
        }));
    }
}
