use super::*;
use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
use crate::sim::{house_state::HouseState, world::Simulation};

pub(super) fn service() -> LanService {
    let mut service = LanService::open_on(
        UdpBindConfig {
            primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        },
        &[],
    )
    .unwrap();
    service.setup_scope = Some(SessionScope {
        epoch: [7; 16],
        revision: 7,
    });
    service
}

#[test]
fn same_named_games_keep_each_host_endpoint() {
    let mut service = service();
    let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
    let other = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1236);
    service.record_advertised_game(host, "Host".into(), false, 10);
    service.record_advertised_game(other, "Host".into(), true, 11);
    let original = service
        .game(endpoint_id(host))
        .expect("original host's advertisement");
    assert!(!original.open);
    assert_eq!(original.last_seen, 10);

    service.record_advertised_game(host, "Host".into(), true, 12);
    let reopened = service.game(endpoint_id(host)).unwrap();
    assert!(reopened.open);
    assert_eq!(reopened.last_seen, 12);
    assert_eq!(service.game(endpoint_id(other)).unwrap().last_seen, 11);
    assert_eq!(service.games.len(), 2);
}

#[test]
fn full_discovery_tables_ignore_new_senders_and_refresh_listed_ones() {
    // One port past the address cap on one address, then one port per address
    // until one sender past the table cap: the last of each run is refused.
    let senders = |cap: usize, per_address: usize| -> Vec<SocketAddrV4> {
        let address = |index: usize| Ipv4Addr::from(0x0A00_0000 + u32::try_from(index).unwrap());
        let ports = (0..=per_address)
            .map(|port| SocketAddrV4::new(address(0), 1234 + u16::try_from(port).unwrap()));
        let others =
            (1..=cap - per_address + 1).map(|index| SocketAddrV4::new(address(index), 1234));
        ports.chain(others).collect()
    };
    let mut service = service();
    let games = senders(MAX_LAN_GAMES, MAX_LAN_GAMES_PER_ADDRESS);
    for &sender in &games {
        service.record_advertised_game(sender, "Host".into(), false, 10);
    }
    let players = senders(MAX_LAN_PARTICIPANTS, MAX_LAN_PARTICIPANTS_PER_ADDRESS);
    let presence = LanMessage::presence("Guest", 0, 0).unwrap();
    for &sender in &players {
        service.record_observed_participant(sender, presence.clone(), 10);
    }
    assert_eq!(service.games.len(), MAX_LAN_GAMES);
    assert_eq!(service.observed_participants.len(), MAX_LAN_PARTICIPANTS);
    let refused_games = [games[MAX_LAN_GAMES_PER_ADDRESS], *games.last().unwrap()];
    assert!(
        refused_games
            .iter()
            .all(|&sender| service.game(endpoint_id(sender)).is_none())
    );
    let refused_players = [
        players[MAX_LAN_PARTICIPANTS_PER_ADDRESS],
        *players.last().unwrap(),
    ];
    assert!(
        service
            .observed_participants
            .iter()
            .all(|peer| !refused_players.contains(&peer.endpoint))
    );

    service.record_advertised_game(games[0], "Host".into(), true, 12);
    let refreshed = service.game(endpoint_id(games[0])).unwrap();
    assert!(refreshed.open);
    assert_eq!(refreshed.last_seen, 12);
    service.record_observed_participant(players[0], presence, 12);
    assert_eq!(service.observed_participants[0].last_seen, 12);
}

/// The `0x549` handler broadcasts only for another row (`0x005DDCFF`);
/// the discovery rebuild's clamp sends the game query alone (`0x005DB9FF`).
#[test]
fn a_selection_forces_only_what_its_native_caller_forces() {
    let mut service = service();
    let all = DueBroadcasts {
        discovery: true,
        query: true,
        presence: true,
    };
    let none = DueBroadcasts {
        discovery: false,
        query: false,
        presence: false,
    };
    assert_eq!(service.due_broadcasts(10, false), all);
    service.select_game(Some(1), LanSelectionChange::Unchanged);
    assert_eq!(service.due_broadcasts(11, false), none);
    service.select_game(Some(2), LanSelectionChange::Clamped);
    let query = DueBroadcasts {
        query: true,
        ..none
    };
    assert_eq!(service.due_broadcasts(12, false), query);
    service.select_game(Some(3), LanSelectionChange::Changed);
    assert_eq!(service.due_broadcasts(13, false), all);
}

fn launch() -> crate::skirmish_launch::SkirmishLaunchSession {
    let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.command-bar.example.json",
    ))
    .unwrap();
    let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    launch.opponents.clear();
    launch.ai_player_count = 0;
    launch.humans[0].name = "Host".into();
    let mut guest = launch.local_player().clone();
    guest.name = "Guest".into();
    guest.raw_slot_index = Some(1);
    guest.slot.color_index = 2;
    guest.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Position(2);
    launch.humans.push(guest);
    launch
}

pub(super) fn session(host: bool, remote: SocketAddrV4) -> MatchSession {
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::LanLobby);
    let mut launch = launch();
    launch.local_player_index = usize::from(!host);
    session
        .accept_network_setup(launch, host, None, false)
        .unwrap();
    session
        .bind_network_endpoint(if host { "Guest" } else { "Host" }, remote)
        .unwrap();
    session
}

fn next_message(service: &mut LanService, session: &MatchSession) -> GlobalMessage {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        service.transport.as_mut().unwrap().service(32);
        if let Some(message) = service.pop_lobby_received(session) {
            return message;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "LAN packet did not arrive"
        );
        std::thread::yield_now();
    }
}

fn global_frames(service: &mut LanService, now: u32) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    service
        .transport
        .as_mut()
        .unwrap()
        .global_mut()
        .service(now, &mut |_, bytes| frames.push(bytes.to_vec()));
    frames
}

fn global_receive(service: &mut LanService, source: SocketAddrV4, frame: &[u8]) -> Vec<Vec<u8>> {
    let mut replies = Vec::new();
    service
        .transport
        .as_mut()
        .unwrap()
        .global_mut()
        .receive(source, 0, frame, &mut |_, bytes| {
            replies.push(bytes.to_vec())
        })
        .unwrap();
    replies
}

fn global_capacity(service: &mut LanService, slots: usize) {
    *service.transport.as_mut().unwrap().global_mut() = crate::net::global::GlobalChannel::new(
        crate::net::global::GlobalConfig {
            send_capacity: slots,
            ..Default::default()
        },
        LOBBY_RETRY_TIMING,
    );
}

/// Another player sees a room's line as gamemd draws it, `[name] text` in
/// the fixed `0x0000F0F0` whatever colour its sender plays, and the
/// sender's own broadcast copy shows nothing, a name the wire narrows too.
#[test]
fn chat_reaches_others_in_the_fixed_colour_and_skips_its_own_copy() {
    let text: Vec<u16> = "hi".encode_utf16().collect();
    let message = chat_packet("Guest", "Host's game", &text).unwrap();
    let chat = message.in_game_chat_fields().unwrap();
    assert_eq!(
        chat_line("Host", &message, &chat),
        Some(crate::ui::lan_shell::LanChatLine {
            text: "[Guest] hi".encode_utf16().collect(),
            color: 0x0000_F0F0,
        })
    );
    assert_eq!(chat_line("guest", &message, &chat), None);
    let own = chat_packet("玩家", "", &text).unwrap();
    assert_eq!(
        chat_line("玩家", &own, &own.in_game_chat_fields().unwrap()),
        None
    );
}

#[test]
fn go_drain_observes_worker_failure_with_the_empty_queue() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    static BUCKET: AtomicU32 = AtomicU32::new(0);
    fn bucket() -> u32 {
        BUCKET.load(Ordering::Acquire)
    }
    let sink = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    sink.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let std::net::SocketAddr::V4(destination) = sink.local_addr().unwrap() else {
        unreachable!()
    };
    let mut service = service();
    let transport = service.transport.as_mut().unwrap();
    transport.global_mut().set_timing(RetryTiming {
        retry_delta: 0,
        max_retries: u32::MAX,
        timeout: 0,
    });
    transport.start_io_pump(bucket).unwrap();
    assert!(
        service
            .queue_go(&LanMessage::ready_to_go(), &[direct(destination)])
            .unwrap()
    );

    // The first real send succeeds, but this socket deliberately never
    // sends an ACK. Hold the fixture clock while the main poll reads OK.
    BUCKET.store(1, Ordering::Release);
    let mut bytes = [0; crate::net::udp::UDP_DATAGRAM_CAPACITY];
    let (length, _) = sink.recv_from(&mut bytes).unwrap();
    assert!(length > 20);
    assert_eq!(&bytes[20..24], &0x13_u32.to_le_bytes());
    let earlier = service.transport.as_mut().unwrap().service(1);
    let earlier_ok = earlier.discarded_global_sends == 0 && earlier.failed_peer.is_none();
    assert!(earlier_ok);
    assert_eq!(service.queued_setup_count(), 1);

    // Interleave the existing retry/timeout/discard body AFTER that poll
    // and BEFORE the GO admission observation. No injected queue/report.
    BUCKET.store(2, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(2);
    while service.queued_setup_count() != 0 {
        assert!(Instant::now() < deadline, "GO reply did not time out");
        std::thread::yield_now();
    }
    let observation = service.go_transport_observation(earlier_ok).unwrap();
    assert_eq!(observation.queued, 0);
    assert!(
        !observation.service_ok,
        "a timed-out GO reply must not become a successful empty drain"
    );
    // The admission snapshot must leave diagnostics for their ordinary
    // consumer instead of silently eating the worker's failure report.
    assert_eq!(
        service
            .transport
            .as_mut()
            .unwrap()
            .service(2)
            .discarded_global_sends,
        1
    );
}

#[test]
fn admission_proofs_bind_current_attempt_scope_full_build_and_selected_rules() {
    let mut service = service();
    let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    service.pending_host = Some(host);
    service.join_nonce = Some([11; 16]);
    let agreement = super::super::shell_network_setup::agreement::MultiplayerAgreement {
        version: super::super::shell_network_setup::agreement::version(),
        rules_digest: "a".repeat(64),
    };
    service.setup_agreement = Some(agreement.clone());
    let proof = LanSessionProof {
        scope: service.setup_scope().unwrap(),
        join_nonce: [11; 16],
        admission_nonce: [0; 16],
        version: agreement.version,
        rules_digest: agreement.rules_digest,
    };
    let packet = LanMessage::join_accepted(
        "Host",
        crate::net::lan_messages::LanAcceptance {
            country: 0,
            color: 1,
            assigned_row: 1,
            observer_row: 7,
        },
    )
    .unwrap()
    .with_session_proof(&proof)
    .unwrap();
    let decoded = LanMessage::decode(packet.as_bytes()).unwrap();
    let mut received = decoded.session_proof().unwrap().unwrap();
    assert_eq!(
        received, proof,
        "full build/rules identities must survive the native packet tail"
    );
    assert!(service.accepts_join_reply(host, &received));
    assert!(service.validate_peer_agreement(&received));
    received.join_nonce = [12; 16];
    assert!(
        !service.accepts_join_reply(host, &received),
        "old attempt cannot complete the new join"
    );
    received = proof.clone();
    received.scope.epoch = [8; 16];
    assert!(
        !service.validate_peer_agreement(&received),
        "old room cannot ready/GO a new room"
    );
    received = proof.clone();
    received.scope.revision -= 1;
    assert!(
        !service.validate_scope(&received),
        "delayed A0/K0 cannot retract a newer revision"
    );
    received = proof.clone();
    received.version.push('x');
    assert!(
        !service.validate_peer_agreement(&received),
        "same package version with another build is incompatible"
    );
    received = proof;
    received.rules_digest = "b".repeat(64);
    assert!(
        !service.validate_peer_agreement(&received),
        "a different selected mode/rules closure is incompatible"
    );
}

/// Source `local`'s direct spawn.ini for `count` humans, all on loopback
/// with port 1300 + source (docs/spawn.md).
fn direct_spawn(
    count: usize,
    local: usize,
    seed: u32,
) -> crate::app::frontend::cncnet_spawn::NetworkSpawn {
    let mut text = format!(
        "[Settings]\nName=P{local}\nSide={}\nColor={local}\nPlayerCount={count}\n\
         SourceIndex={local}\nSeed={seed}\nSpecialHouseIsAlly=No\nPort={}\n",
        local % 2,
        1300 + local
    );
    for (other, source) in (0..count).filter(|&source| source != local).enumerate() {
        text.push_str(&format!(
            "[Other{}]\nName=P{source}\nSide={}\nColor={source}\nSourceIndex={source}\n\
             Ip=127.0.0.1\nPort={}\n",
            other + 1,
            source % 2,
            1300 + source
        ));
    }
    match crate::app::frontend::cncnet_spawn::SpawnRequest::parse(&text)
        .unwrap()
        .game
    {
        crate::app::frontend::cncnet_spawn::SpawnGame::Network(network) => *network,
        other => panic!("expected a network spawn, got {other:?}"),
    }
}

fn direct_admission(count: usize, local: usize, seed: u32) -> (DirectAdmission, u16) {
    let network = direct_spawn(count, local, seed);
    let crate::app::frontend::cncnet_spawn::SpawnTransport::Direct(direct) = network.transport
    else {
        panic!("direct transport");
    };
    (
        DirectAdmission::derive(&network.session, seed, &direct.peers, b"map").unwrap(),
        direct.local_port,
    )
}

#[test]
fn direct_spawn_participants_derive_one_admission() {
    let endpoint = |source: usize| SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1300 + source as u16);
    let all: Vec<_> = (0..3).map(|local| direct_admission(3, local, 77)).collect();
    let (host, host_port) = &all[0];
    assert_eq!(*host_port, 1300);
    assert_ne!(host.scope.epoch, [0; 16]);
    assert_eq!(host.scope.revision, 1, "a scope the match transport admits");
    assert_eq!(host.host, None, "source 0 is the host");
    for (local, (admission, port)) in all.iter().enumerate() {
        assert_eq!(*port, 1300 + local as u16);
        assert_eq!(admission.scope, host.scope, "one shared setup scope");
        assert_ne!(admission.local.join_nonce, [0; 16]);
        assert_ne!(admission.local.admission_nonce, [0; 16]);
        let remote: Vec<_> = admission.peers.iter().map(|peer| peer.0).collect();
        let expected: Vec<_> = (0..3).filter(|&s| s != local).map(endpoint).collect();
        assert_eq!(remote, expected);
        for (peer, name, identity) in &admission.peers {
            let source = usize::from(peer.port() - 1300);
            assert_eq!(*name, format!("P{source}"));
            // Each record names the pair that source's own proofs carry.
            assert_eq!(*identity, all[source].0.local);
        }
        if local != 0 {
            assert_eq!(admission.host, Some(("P0".to_owned(), endpoint(0))));
        }
    }
    assert_ne!(
        all[1].0.local, all[2].0.local,
        "sources have distinct pairs"
    );
    assert_ne!(
        direct_admission(3, 1, 78).0.scope,
        all[1].0.scope,
        "another launch never shares a scope"
    );
    let network = direct_spawn(2, 0, 77);
    assert!(
        DirectAdmission::derive(&network.session, 77, &[None, None], b"map").is_err(),
        "every remote source needs an endpoint"
    );
}

#[test]
fn direct_spawn_admission_satisfies_the_lan_go_owners_over_udp() {
    let mut host = service();
    let mut guest = service();
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let network = |local: usize| {
        let mut launch = launch();
        launch.local_player_index = local;
        launch
    };
    let peers = [None, Some(guest_address)];
    let host_admission = DirectAdmission::derive(&network(0), 5, &peers, b"map").unwrap();
    let guest_admission =
        DirectAdmission::derive(&network(1), 5, &[Some(host_address), None], b"map").unwrap();
    let host_session = session(true, guest_address);
    let guest_session = session(false, host_address);
    let ordinary = service();
    assert!(!ordinary.setup_roster_complete(&host_session));
    assert!(!ordinary.setup_maps_verified(None));
    host.adopt_direct_admission(&host_admission);
    guest.adopt_direct_admission(&guest_admission);
    let agreement = super::super::shell_network_setup::agreement::MultiplayerAgreement {
        version: super::super::shell_network_setup::agreement::version(),
        rules_digest: "a".repeat(64),
    };
    host.setup_agreement = Some(agreement.clone());
    guest.setup_agreement = Some(agreement);
    assert!(host.setup_roster_complete(&host_session));
    assert!(guest.setup_roster_complete(&guest_session));
    assert!(host.setup_maps_verified(None) && guest.setup_maps_verified(None));
    assert_eq!(guest.admitted_host, Some(("Host".to_owned(), host_address)));
    assert!(!host.direct_peers_heard(), "GO waits for every guest");

    guest.send_setup_keepalives(&guest_session).unwrap();
    guest.transport.as_mut().unwrap().service(31);
    let received = next_message(&mut host, &host_session);
    let message = LanMessage::decode(&received.payload).unwrap();
    assert_eq!(
        host.accept_setup_keepalive(&host_session, &received, &message, 1)
            .unwrap(),
        SetupKeepalive::Renewed,
        "the keepalive carries the rules both play"
    );
    assert!(host.direct_peers_heard());

    host.send_setup_keepalives(&host_session).unwrap();
    host.transport.as_mut().unwrap().service(32);
    let received = next_message(&mut guest, &guest_session);
    let message = LanMessage::decode(&received.payload).unwrap();
    assert_eq!(
        guest
            .accept_setup_keepalive(&guest_session, &received, &message, 2)
            .unwrap(),
        SetupKeepalive::Renewed,
        "the host echoes the guest's own pair"
    );
    // GO and its Type13 reply carry proofs the other side admits.
    let go = host.control_proof().unwrap();
    assert!(guest.validate_scope(&go) && guest.peer_admission_matches(host_address, &go));
    let reply = guest.control_proof().unwrap();
    assert!(host.validate_scope(&reply) && host.peer_admission_matches(guest_address, &reply));
}

/// A direct spawn's participant of another build or other rules files
/// derives the same scope and nonces from the launch; its keepalive names
/// it instead of being dropped until its lease runs out.
#[test]
fn direct_spawn_keepalive_of_another_build_or_rules_names_its_participant() {
    use super::super::shell_network_setup::agreement;
    let guest = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30125);
    let session = session(true, guest);
    let admission = DirectAdmission::derive(
        session.network_setup().unwrap(),
        5,
        &[None, Some(guest)],
        b"map",
    )
    .unwrap();
    let mut host = service();
    host.adopt_direct_admission(&admission);
    host.setup_agreement = Some(agreement::MultiplayerAgreement {
        version: agreement::version(),
        rules_digest: "a".repeat(64),
    });
    let pair = admission.peers[0].2;
    let mut keepalive = |sequence, version: String, rules: char| {
        let proof = LanSessionProof {
            scope: admission.scope,
            join_nonce: pair.join_nonce,
            admission_nonce: pair.admission_nonce,
            version,
            rules_digest: rules.to_string().repeat(64),
        };
        let message = LanMessage::named(LanNamedMessage::Keepalive, "Guest")
            .unwrap()
            .with_session_proof(&proof)
            .unwrap();
        let incoming = GlobalMessage {
            source: guest,
            product_id: 0xaa04,
            sequence,
            payload: message.as_bytes().to_vec(),
        };
        host.accept_setup_keepalive(&session, &incoming, &message, 1)
            .unwrap()
    };
    assert_eq!(
        keepalive(1, "0.1.0:released:0000".into(), 'a'),
        SetupKeepalive::OtherBuild("Guest".into())
    );
    assert_eq!(
        keepalive(2, agreement::version(), 'b'),
        SetupKeepalive::OtherRules("Guest".into())
    );
    assert_eq!(
        keepalive(3, agreement::version(), 'a'),
        SetupKeepalive::Renewed
    );
}

#[test]
fn setup_keepalive_uses_the_confirmed_pair_and_ignores_reordered_replays() {
    let mut host = service();
    let mut guest = service();
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let host_session = session(true, guest_address);
    let guest_session = session(false, host_address);
    guest.join_nonce = Some([11; 16]);
    guest.admission_nonce = Some([12; 16]);
    guest.admitted_host = Some(("Host".into(), host_address));
    let proof = guest.control_proof().unwrap();
    host.commit_peer_admission(guest_address, "Guest", &proof);
    guest.replace_peer_admissions([(
        host_address,
        "Host".into(),
        PeerAdmission {
            join_nonce: [0; 16],
            admission_nonce: [0; 16],
        },
    )]);
    assert_eq!(host.expired_setup_peer(&host_session, 0), None);
    assert_eq!(guest.expired_setup_peer(&guest_session, 0), None);
    // One whole lease minus one bucket may pass without a probe. The real
    // UDP/global path supplies the sender sequence and physical endpoint.
    guest.send_setup_keepalives(&guest_session).unwrap();
    guest.transport.as_mut().unwrap().service(31);
    let received = next_message(&mut host, &host_session);
    let message = LanMessage::decode(&received.payload).unwrap();
    host.setup_scope.as_mut().unwrap().revision += 1;
    assert!(
        !host.validate_scope(&proof),
        "old readiness remains invalid"
    );
    assert_eq!(
        host.accept_setup_keepalive(&host_session, &received, &message, SETUP_PEER_LEASE - 1)
            .unwrap(),
        SetupKeepalive::Renewed
    );
    assert_eq!(
        host.accept_setup_keepalive(&host_session, &received, &message, 2 * SETUP_PEER_LEASE - 2)
            .unwrap(),
        SetupKeepalive::Ignored
    );
    assert_eq!(
        host.expired_setup_peer(&host_session, 2 * SETUP_PEER_LEASE - 2),
        None
    );
    assert_eq!(
        host.expired_setup_peer(&host_session, 2 * SETUP_PEER_LEASE - 1),
        Some((1, guest_address))
    );
    // The host echoes this recipient's pair, rather than a public zero
    // proof, so a fresh same-endpoint guest rejects the former heartbeat.
    host.send_setup_keepalives(&host_session).unwrap();
    host.transport.as_mut().unwrap().service(31);
    let received = next_message(&mut guest, &guest_session);
    let message = LanMessage::decode(&received.payload).unwrap();
    assert_eq!(
        guest
            .accept_setup_keepalive(&guest_session, &received, &message, 10)
            .unwrap(),
        SetupKeepalive::Renewed
    );
    guest.admission_nonce = Some([13; 16]);
    let mut newer = received;
    newer.sequence = newer.sequence.wrapping_add(1);
    assert_eq!(
        guest
            .accept_setup_keepalive(&guest_session, &newer, &message, 11)
            .unwrap(),
        SetupKeepalive::Ignored
    );
}

#[test]
fn setup_peer_expiry_cancels_only_that_endpoint_before_republishing_roster() {
    let mut host = service();
    global_capacity(&mut host, 2);
    let expired = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    let survivor = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
    let mut session = session(true, expired);
    let mut other = session.network_setup().unwrap().humans[1].clone();
    other.name = "Other".into();
    other.raw_slot_index = Some(2);
    session.add_network_participant(other).unwrap();
    session.bind_network_endpoint("Other", survivor).unwrap();
    let mut proof = host.control_proof().unwrap();
    proof.join_nonce = [11; 16];
    proof.admission_nonce = [12; 16];
    host.commit_peer_admission(expired, "Guest", &proof);
    proof.admission_nonce = [13; 16];
    host.commit_peer_admission(survivor, "Other", &proof);
    assert_eq!(host.expired_setup_peer(&session, 0), None);
    let message = LanMessage::named(LanNamedMessage::Keepalive, "Other")
        .unwrap()
        .with_session_proof(&proof)
        .unwrap();
    let incoming = GlobalMessage {
        source: survivor,
        product_id: 0xaa04,
        sequence: 8,
        payload: message.as_bytes().to_vec(),
    };
    assert_eq!(
        host.accept_setup_keepalive(&session, &incoming, &message, SETUP_PEER_LEASE - 1)
            .unwrap(),
        SetupKeepalive::Renewed
    );
    assert!(host.send(b"expired previous", direct(expired), true));
    assert!(host.send(b"survivor previous", direct(survivor), true));
    host.pending_setup.extend([
        (b"expired map".to_vec(), direct(expired)),
        (b"survivor map".to_vec(), direct(survivor)),
    ]);
    let (index, endpoint) = host.expired_setup_peer(&session, SETUP_PEER_LEASE).unwrap();
    assert_eq!((index, endpoint), (1, expired));
    assert!(!host.retire_setup_peer(endpoint));
    // The production app emits ParticipantLeft through the same existing
    // Session mutation before republishing the canonical setup snapshot.
    session.remove_network_participant(index).unwrap();
    host.flush_setup();
    assert_eq!(session.network_setup().unwrap().humans.len(), 2);
    assert_eq!(session.network_endpoint_source(survivor), Some(1));
    assert!(host.setup_roster_complete(&session));
    assert_eq!(host.queued_setup_count(), 2);
    let frames = global_frames(&mut host, 31);
    assert_eq!(frames.len(), 2);
    assert!(
        frames
            .iter()
            .all(|frame| !frame.ends_with(b"expired previous") && !frame.ends_with(b"expired map"))
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.ends_with(b"survivor previous"))
    );
    assert!(frames.iter().any(|frame| frame.ends_with(b"survivor map")));
}

#[test]
fn identical_host_snapshots_do_not_renew_the_lease_and_host_loss_retires_setup() {
    let mut guest = service();
    let host_address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    let remote_address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
    let mut session = session(false, host_address);
    let mut other = session.network_setup().unwrap().humans[0].clone();
    other.name = "Other".into();
    other.raw_slot_index = Some(2);
    session.add_network_participant(other).unwrap();
    session
        .bind_network_endpoint("Other", remote_address)
        .unwrap();
    guest.admitted_host = Some(("Host".into(), host_address));
    guest.join_nonce = Some([11; 16]);
    guest.admission_nonce = Some([12; 16]);
    let identity = PeerAdmission {
        join_nonce: [0; 16],
        admission_nonce: [0; 16],
    };
    guest.replace_peer_admissions([
        (host_address, "Host".into(), identity),
        (remote_address, "Other".into(), identity),
    ]);
    let now = u32::MAX - 20;
    assert_eq!(guest.expired_setup_peer(&session, now), None);
    // A full current options snapshot may repeat during map download; it
    // cannot pretend the host sent a fresh authenticated presence probe.
    guest.replace_peer_admissions([
        (host_address, "Host".into(), identity),
        (remote_address, "Other".into(), identity),
    ]);
    assert_eq!(
        guest.expired_setup_peer(&session, now.wrapping_add(SETUP_PEER_LEASE - 1)),
        None
    );
    assert_eq!(
        guest.expired_setup_peer(&session, now.wrapping_add(SETUP_PEER_LEASE)),
        Some((0, host_address))
    );
    guest
        .pending_setup
        .push_back((b"old map request".to_vec(), direct(host_address)));
    assert!(guest.send(b"old options", direct(host_address), true));
    assert!(guest.retire_setup_peer(host_address));
    assert_eq!(guest.setup_scope(), None);
    assert!(guest.received_options.is_none());
    assert!(guest.admitted_host.is_none());
    assert!(guest.peer_admissions.is_empty());
    assert_eq!(guest.queued_setup_count(), 0);
    assert!(guest.queue_go(&LanMessage::go(2), &[]).is_err());
    assert_eq!(
        session.return_to_network_lobby(),
        Some(NetworkReturnContext::LanLobby)
    );
    assert!(session.network_setup().is_none());
    // A normal retry starts with no prior deadline/sequence ownership.
    guest.setup_scope = Some(SessionScope {
        epoch: [8; 16],
        revision: 1,
    });
    guest.admitted_host = Some(("Host".into(), host_address));
    guest.replace_peer_admissions([(host_address, "Host".into(), identity)]);
    let session = self::session(false, host_address);
    assert_eq!(
        guest.expired_setup_peer(&session, now.wrapping_add(SETUP_PEER_LEASE)),
        None
    );
}

#[test]
fn newer_join_challenge_survives_old_request_confirm_and_cancel() {
    use crate::net::lan_messages::{LanJoinRequest, LanVersionInterval};
    let mut host = service();
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    let fields = LanJoinRequest {
        country: -2,
        color: -2,
        version: LanVersionInterval::RETAIL,
        compatibility: LanCompatibility {
            field_0x43: 1,
            field_0x47: 2,
            field_0x4b: 3,
            field_0x4f: 4,
        },
        observer_request: 0,
    };
    let request = |nonce| {
        let mut proof = host.control_proof().unwrap();
        proof.scope = SessionScope {
            epoch: [0; 16],
            revision: 0,
        };
        proof.join_nonce = [nonce; 16];
        LanMessage::join_request("Guest", &[], fields)
            .unwrap()
            .with_session_proof(&proof)
            .unwrap()
    };
    let old = request(1);
    let current = request(2);
    let old_identity = host.join_offer(endpoint, &old, 31).unwrap();
    let identity = host.join_offer(endpoint, &current, 32).unwrap();
    assert_ne!(old_identity.admission_nonce, identity.admission_nonce);
    assert_eq!(host.join_offer(endpoint, &old, 33).unwrap(), old_identity);
    assert!(
        host.peer_admissions.is_empty(),
        "an offer cannot admit a Session peer"
    );
    let mut proof = host.control_proof().unwrap();
    proof.join_nonce = identity.join_nonce;
    proof.admission_nonce = identity.admission_nonce;
    // Concurrent peers can advance options while this challenge is in flight.
    host.setup_scope.as_mut().unwrap().revision += 1;
    assert_eq!(
        host.confirmed_request(endpoint, &proof, 34),
        Some((current.name(), fields))
    );
    host.commit_peer_admission(endpoint, "Host", &proof);
    assert!(
        !host.accepts_participant(true, endpoint, "Host", &proof),
        "host Type3 must never bypass Confirm"
    );
    assert!(host.accepts_participant(false, endpoint, "Host", &proof));
    assert!(
        !host.accepts_participant(false, endpoint, "Other", &proof),
        "a guest assembles only the current host-authored binding"
    );
    let mut stale = proof.clone();
    stale.join_nonce = old_identity.join_nonce;
    stale.admission_nonce = old_identity.admission_nonce;
    assert!(host.confirmed_request(endpoint, &stale, 35).is_none());
    assert!(!host.cancel_peer_admission(endpoint, &stale));
    assert!(host.peer_admission_matches(endpoint, &proof));
    let replayed_offer = host.join_offer(endpoint, &old, 36).unwrap();
    assert_ne!(replayed_offer.admission_nonce, old_identity.admission_nonce);
    assert!(host.confirmed_request(endpoint, &stale, 37).is_none());
    stale.admission_nonce = [0; 16];
    assert!(!host.cancel_peer_admission(endpoint, &stale));
    assert!(host.peer_admission_matches(endpoint, &proof));
    assert!(host.pending_admissions.is_empty());
    // StartPrompt, Departure, the kick and cancel admit a proof through
    // in_room: any options revision, but only this build.
    assert!(host.in_room(&proof) && !host.validate_scope(&proof));
    let mut other_build = proof.clone();
    other_build.version.push('x');
    assert!(
        !host.in_room(&other_build),
        "another build's proof is not in this room"
    );
    assert!(host.cancel_peer_admission(endpoint, &proof));
    assert!(host.peer_admissions.is_empty());
    assert!(
        !host.accepts_participant(false, endpoint, "Host", &proof),
        "late Type3 cannot resurrect a canceled source"
    );
}

#[test]
fn departure_fifo_survives_full_global_and_waits_for_every_ack() {
    let mut host = service();
    let mut guest = service();
    global_capacity(&mut host, 1);
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let session = session(true, guest_address);
    assert!(host.send(b"previous setup", direct(guest_address), true));
    host.pending_setup
        .push_back((b"superseded map chunk".to_vec(), direct(guest_address)));
    host.begin_departure(&session, DepartureReason::Back, 1)
        .unwrap();
    assert_eq!(host.queued_setup_count(), 3);
    assert!(host.departure_ready(31, true).is_none());
    let previous = global_frames(&mut host, 31).pop().unwrap();
    let ack = global_receive(&mut guest, host_address, &previous)
        .pop()
        .unwrap();
    host.flush_setup();
    assert_eq!(
        host.pending_setup.len(),
        2,
        "full global retains both departures"
    );
    global_receive(&mut host, guest_address, &ack);
    global_frames(&mut host, 33);
    host.flush_setup();
    let departure = global_frames(&mut host, 64).pop().unwrap();
    let ack = global_receive(&mut guest, host_address, &departure)
        .pop()
        .unwrap();
    let mut global = guest.transport.as_mut().unwrap().global_mut();
    assert_eq!(global.pop_received().unwrap().payload, b"previous setup");
    global.retire_consumed();
    assert_eq!(
        LanMessage::decode(&global.pop_received().unwrap().payload)
            .unwrap()
            .kind(),
        LanMessageKind::Departure
    );
    drop(global);
    assert!(
        host.departure_ready(64, true).is_none(),
        "direct receipt is still unacknowledged"
    );
    global_receive(&mut host, guest_address, &ack);
    global_frames(&mut host, 65);
    host.flush_setup();
    assert!(
        host.departure_ready(65, true).is_none(),
        "native broadcast still waits in the same queue"
    );
    global_frames(&mut host, 96);
    global_frames(&mut host, 97);
    assert_eq!(host.departure_ready(97, true), Some(DepartureReason::Back));
    assert!(host.departure_ready(97, true).is_none());
}

#[test]
fn canceled_join_requires_its_receipt_and_timeout_preserves_global_sequence() {
    let mut guest = service();
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    let session = MatchSession::default();
    guest.pending_host = Some(endpoint);
    guest.join_nonce = Some([1; 16]);
    guest.admission_nonce = Some([2; 16]);
    guest
        .begin_departure(&session, DepartureReason::CancelJoin { game_id: 7 }, 100)
        .unwrap();
    let proof = guest.control_proof().unwrap();
    assert!(
        !guest.accepts_join_reply(endpoint, &proof),
        "a final reply already queued when BB times out cannot commit after cancellation intent"
    );
    assert_eq!(
        guest.pending_host,
        Some(endpoint),
        "cleanup retains its endpoint"
    );
    let mut stale = proof.clone();
    stale.admission_nonce = [3; 16];
    assert!(!guest.accept_cancel_receipt(endpoint, &stale));
    assert!(guest.departure_ready(132, true).is_none());
    assert!(guest.accept_cancel_receipt(endpoint, &proof));
    assert!(
        guest.departure_ready(132, true).is_none(),
        "receipt does not bypass queued ACK drain"
    );
    let sequence = guest
        .transport
        .as_ref()
        .unwrap()
        .global()
        .next_send_sequence();
    assert_eq!(
        guest.departure_ready(700, true),
        Some(DepartureReason::CancelJoin { game_id: 7 })
    );
    guest.clear_setup();
    assert!(guest.send(b"new request", direct(endpoint), true));
    assert_eq!(
        guest
            .transport
            .as_ref()
            .unwrap()
            .global()
            .next_send_sequence(),
        sequence + 1,
        "same endpoint retry must not collide with the host's retained sequence ring"
    );
}

#[test]
fn setup_fifo_retains_backpressure_and_following_control_packets() {
    let mut host = service();
    host.setup_scope = Some(SessionScope {
        epoch: [7; 16],
        revision: 7,
    });
    let mut guest = service();
    global_capacity(&mut host, 2);
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    host.queue_setup(
        [
            b"roster".to_vec(),
            b"options one".to_vec(),
            b"options two".to_vec(),
        ]
        .into_iter()
        .map(|bytes| (bytes, direct(guest_address))),
    )
    .unwrap();
    host.queue_setup(std::iter::once((b"ready".to_vec(), direct(guest_address))))
        .unwrap();
    assert_eq!(host.pending_setup.len(), 2);
    assert_eq!(host.queued_setup_count(), 4);
    let frames = global_frames(&mut host, 31);
    assert_eq!(frames.len(), 2);
    for frame in frames {
        for ack in global_receive(&mut guest, host_address, &frame) {
            global_receive(&mut host, guest_address, &ack);
        }
    }
    global_frames(&mut host, 33);
    host.flush_setup();
    assert!(host.pending_setup.is_empty());
    for frame in global_frames(&mut host, 64) {
        global_receive(&mut guest, host_address, &frame);
    }
    let mut global = guest.transport.as_mut().unwrap().global_mut();
    for expected in [
        b"roster".as_slice(),
        b"options one",
        b"options two",
        b"ready",
    ] {
        assert_eq!(global.pop_received().unwrap().payload, expected);
        global.retire_consumed();
    }
    assert!(global.pop_received().is_none());
}

#[test]
fn go_waits_for_the_last_final_fragment_ack_and_its_own_drain() {
    use crate::net::lan_messages::extended_setup::{Reassembly, SetupKind, encode};
    let mut host = service();
    host.setup_scope = Some(SessionScope {
        epoch: [7; 16],
        revision: 7,
    });
    let mut guest = service();
    global_capacity(&mut host, 2);
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let payload = vec![73; 650];
    let parts = encode(
        SetupKind::Options,
        SessionScope {
            epoch: [7; 16],
            revision: 7,
        },
        &payload,
    )
    .unwrap();
    host.queue_setup(parts.into_iter().map(|part| (part, direct(guest_address))))
        .unwrap();
    assert!(host.pending_setup.is_empty());
    let go = LanMessage::go(10);
    assert!(!host.queue_go(&go, &[direct(guest_address)]).unwrap());
    let mut frames = global_frames(&mut host, 31);
    let last = frames.pop().unwrap();
    let delayed_ack = global_receive(&mut guest, host_address, &last)
        .pop()
        .unwrap();
    for frame in frames {
        for ack in global_receive(&mut guest, host_address, &frame) {
            global_receive(&mut host, guest_address, &ack);
        }
    }
    global_frames(&mut host, 33);
    assert_eq!(host.queued_setup_count(), 1);
    assert!(!host.queue_go(&go, &[direct(guest_address)]).unwrap());
    let mut global = guest.transport.as_mut().unwrap().global_mut();
    let mut reassembly = Reassembly::default();
    assert!(
        reassembly
            .receive(&global.pop_received().unwrap().payload, 32)
            .unwrap()
            .is_none()
    );
    global.retire_consumed();
    assert_eq!(
        reassembly
            .receive(&global.pop_received().unwrap().payload, 32)
            .unwrap(),
        Some((SetupKind::Options, payload))
    );
    global.retire_consumed();
    drop(global);
    global_receive(&mut host, guest_address, &delayed_ack);
    global_frames(&mut host, 34);
    assert!(host.queue_go(&go, &[direct(guest_address)]).unwrap());
    assert_eq!(
        host.queued_setup_count(),
        1,
        "GO handoff still waits for its ACK"
    );
    for frame in global_frames(&mut host, 65) {
        for ack in global_receive(&mut guest, host_address, &frame) {
            global_receive(&mut host, guest_address, &ack);
        }
    }
    let received_go = LanMessage::decode(
        &guest
            .transport
            .as_mut()
            .unwrap()
            .global_mut()
            .pop_received()
            .unwrap()
            .payload,
    )
    .unwrap();
    assert_eq!(received_go.kind(), LanMessageKind::Go);
    assert_eq!(received_go.payload(), go.payload());
    assert_eq!(
        received_go.session_proof().unwrap().unwrap().scope,
        host.setup_scope().unwrap()
    );
    global_frames(&mut host, 66);
    assert_eq!(host.queued_setup_count(), 0);
}

#[test]
fn go_recipient_admission_is_atomic_when_global_capacity_is_insufficient() {
    let mut host = service();
    host.setup_scope = Some(SessionScope {
        epoch: [7; 16],
        revision: 7,
    });
    global_capacity(&mut host, 1);
    let destinations = [
        direct(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123)),
        direct(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30125)),
    ];
    assert!(!host.queue_go(&LanMessage::go(10), &destinations).unwrap());
    assert_eq!(host.queued_setup_count(), 0);
    assert_eq!(
        host.transport
            .as_ref()
            .unwrap()
            .global()
            .next_send_sequence(),
        0
    );
    global_capacity(&mut host, 2);
    assert!(host.queue_go(&LanMessage::go(10), &destinations).unwrap());
    assert!(host.pending_setup.is_empty());
    assert_eq!(host.queued_setup_count(), 2);
}

#[test]
fn admitted_join_keeps_all_early_setup_packets_in_the_transport_until_setup_exists() {
    let mut sender = service();
    let mut receiver = service();
    let host = sender.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest = receiver.transport.as_ref().unwrap().primary_addr().unwrap();
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::LanLobby);
    receiver.admitted_host = Some(("Host".into(), host));
    let messages = [
        LanMessage::options("Host", b"K1", 0, 0).unwrap(),
        LanMessage::player_choices("Host", b"R-2,6,-2,-2").unwrap(),
        LanMessage::start_prompt("Host").unwrap(),
        LanMessage::go(10),
    ];
    for message in &messages {
        assert!(sender.send(message.as_bytes(), direct(guest), true));
    }
    sender.transport.as_mut().unwrap().service(31);
    // ACKs can retire every sender entry while the guest's shell still
    // transitions. Both sockets are nonblocking: wait for actual ACKs,
    // keeping protocol time fixed and every early packet in the queue.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        receiver.transport.as_mut().unwrap().service(32);
        assert!(receiver.pop_lobby_received(&session).is_none());
        sender.transport.as_mut().unwrap().service(33);
        if sender
            .transport
            .as_ref()
            .unwrap()
            .global()
            .queued_send_count()
            == 0
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "early setup packets were not acknowledged"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        receiver
            .transport
            .as_ref()
            .unwrap()
            .global()
            .queued_receive_count(),
        messages.len()
    );
    assert_eq!(
        sender
            .transport
            .as_ref()
            .unwrap()
            .global()
            .queued_send_count(),
        0
    );
    let mut accepted = launch();
    accepted.local_player_index = 1;
    session
        .accept_network_setup(accepted, false, None, false)
        .unwrap();
    session.bind_network_endpoint("Host", host).unwrap();
    for expected in messages {
        assert_eq!(
            next_message(&mut receiver, &session).payload,
            expected.as_bytes()
        );
    }

    session.prepare_network_timing(2, || 0).unwrap();
    session.freeze_network_start(19, None).unwrap();
    let progress =
        crate::net::lan_messages::ingame::LoadingProgressMessage::new("Host", 100).unwrap();
    assert!(sender.send(progress.as_bytes(), direct(guest), true));
    sender.transport.as_mut().unwrap().service(34);
    assert_eq!(
        next_message(&mut receiver, &session).payload,
        progress.as_bytes()
    );
}

#[test]
fn a_closing_departure_reaches_each_player_with_the_room_proof() {
    let mut host = service();
    let mut guest = service();
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let host_session = session(true, guest_address);
    let guest_session = session(false, host_address);
    let proof = guest.control_proof().unwrap();
    let packets = departure_packets(&guest_session, Some(&proof)).unwrap();
    assert_eq!(packets[0].1, direct(host_address));
    for (bytes, destination) in packets {
        assert!(guest.send(&bytes, destination, false));
    }
    guest.transport.as_mut().unwrap().service(31);
    let departure = LanMessage::decode(&next_message(&mut host, &host_session).payload).unwrap();
    assert_eq!(departure.kind(), LanMessageKind::Departure);
    assert_eq!(departure.session_proof().unwrap(), Some(proof));
}

#[test]
fn row_replacement_sends_kick_and_departure_cleans_membership_after_ack() {
    let mut host = service();
    let mut guest = service();
    let host_address = host.transport.as_ref().unwrap().primary_addr().unwrap();
    let guest_address = guest.transport.as_ref().unwrap().primary_addr().unwrap();
    let mut host_session = session(true, guest_address);
    let mut guest_session = session(false, host_address);
    guest.admitted_host = Some(("Host".into(), host_address));
    let removed = host_session
        .set_network_row_kind(
            1,
            super::super::match_runtime::session::NetworkRowKind::Open,
        )
        .unwrap()
        .unwrap();
    assert!(
        host_session.network_setup().unwrap().humans[1]
            .raw_slot_index
            .is_none()
    );
    host.kick_peer(&host_session, &removed.name, true, 30)
        .unwrap();
    let rejoin = |name: &str, now| host.keeps_out(guest_address, name, now);
    assert!(rejoin("Renamed", 30) && !rejoin("Renamed", 30 + KICK_KEEP_OUT_BUCKETS));
    host.transport.as_mut().unwrap().service(31);
    let denied = next_message(&mut guest, &guest_session);
    assert_eq!(
        LanMessage::decode(&denied.payload).unwrap().payload(),
        crate::net::lan_messages::LanPayload::JoinDenied { reason: 4 }
    );
    assert!(
        guest
            .begin_kicked_exit(&guest_session, guest_address, 32)
            .is_err()
    );
    guest
        .begin_kicked_exit(&guest_session, denied.source, 32)
        .unwrap();
    assert!(guest.departure_ready(32, true).is_none());
    guest.transport.as_mut().unwrap().service(33);
    let departed = next_message(&mut host, &host_session);
    let departed = LanMessage::decode(&departed.payload).unwrap();
    assert_eq!(departed.kind(), LanMessageKind::Departure);
    assert_eq!(departed.name(), "Guest");
    let source = host_session.network_endpoint_source(guest_address).unwrap();
    host_session.remove_network_participant(source).unwrap();
    assert_eq!(host_session.network_setup().unwrap().humans.len(), 1);
    assert!(
        host_session
            .network_endpoint_source(guest_address)
            .is_none()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while guest.departure_ready(33, true).is_none() {
        guest.transport.as_mut().unwrap().service(33);
        assert!(
            std::time::Instant::now() < deadline,
            "departure did not drain"
        );
        std::thread::yield_now();
    }
    assert!(
        guest.departure_ready(33, true).is_none(),
        "the shell exit occurs once"
    );
    assert!(guest.pop_lobby_received(&guest_session).is_none());
    guest.clear_setup();
    guest_session.return_to_network_lobby();
    assert!(guest.admitted_host.is_none());
    assert!(guest_session.network_setup().is_none());
    host.clear_setup();
    assert!(!host.keeps_out(guest_address, "Renamed", 31));
}

#[test]
fn lan_start_timing_survives_setup_back_and_resets_on_fresh_visit() {
    let mut service = service();
    service.apply_start_timing(false, true);
    let timing = service.transport.as_ref().unwrap().retry_timing();
    assert_eq!((timing.retry_delta, timing.timeout), (20, 160));
    let destination = direct(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123));
    assert!(service.send(b"pending after GO", destination, true));
    // A fresh BB visit explicitly restores lobby settings for both
    // channels; returning through Setup Back does not reset them.
    service.clear_setup();
    assert_eq!(service.transport.as_ref().unwrap().retry_timing(), timing);
    let mut global = service.transport.as_mut().unwrap().global_mut();
    // Guest5DD9FD leaves primary-global at60/-1/600, separately from
    // the private20/-1/160 above. Back must preserve that timing too.
    global.service(21, &mut |_, _| panic!("private timing leaked into global"));
    let mut sends = 0;
    global.service(61, &mut |_, _| sends += 1);
    assert_eq!(sends, 1);
    drop(global);
    service.restart_visit();
    assert_eq!(
        service.transport.as_ref().unwrap().retry_timing(),
        LOBBY_RETRY_TIMING
    );
    assert!(service.send(b"fresh visit", destination, true));
    let mut sends = 0;
    service
        .transport
        .as_mut()
        .unwrap()
        .global_mut()
        .service(LOBBY_RETRY_TIMING.retry_delta + 1, &mut |_, _| sends += 1);
    assert_eq!(sends, 1);
}

#[test]
fn lan_start_timing_reads_primary_global_history_not_the_private_maximum() {
    use crate::net::reliable::Delivery;

    let mut service = service();
    let transport = service.transport.as_mut().unwrap();
    let local = transport.primary_addr().unwrap();
    let remote = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    let mut connections = transport.connections_mut();
    connections.add_peer(7, remote).unwrap();
    assert!(connections.send_to(7, b"prior private traffic", Delivery::Reliable));
    let mut sent = Vec::new();
    connections.service(31, &mut |_, bytes| sent.push(bytes.to_vec()));
    assert_eq!(sent.len(), 1);

    let mut peer = LanConnections::new(LanConnectionConfig::default(), LOBBY_RETRY_TIMING);
    peer.add_peer(0, local).unwrap();
    let mut acks = Vec::new();
    peer.receive(local, &sent[0], &mut |_, bytes| acks.push(bytes.to_vec()))
        .unwrap();
    connections
        .receive(remote, &acks[0], &mut |_, _| {})
        .unwrap();
    connections.service(131, &mut |_, _| {});
    assert_eq!(connections.mean_response_time(7), Some(100));
    drop(connections);
    assert_eq!(transport.global().mean_response_time(), 0);
    assert_eq!(transport.response_time(), 100);

    // The native GO callers use5424C0, not the aggregate542450 getter
    // used by Wait_For_Players. A slower private channel is irrelevant.
    service.apply_start_timing(true, false);
    assert_eq!(
        service.transport.as_ref().unwrap().retry_timing(),
        RetryTiming {
            retry_delta: 30,
            max_retries: u32::MAX,
            timeout: 600,
        }
    );
    service.apply_start_timing(false, true);
    assert_eq!(
        service.transport.as_ref().unwrap().retry_timing(),
        RetryTiming {
            retry_delta: 20,
            max_retries: u32::MAX,
            timeout: 160,
        }
    );
}

#[test]
fn setup_back_ends_admission_but_only_a_fresh_visit_resets_transport_history() {
    let mut service = LanService::open_on(
        UdpBindConfig {
            primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        },
        &[],
    )
    .unwrap();
    let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32100);
    let address = service.transport.as_ref().unwrap().primary_addr().unwrap();
    service.admitted_host = Some(("old host".into(), host));
    service.pending_host = Some(host);
    service.selected_game = Some(endpoint_id(host));
    service
        .deferred_setup
        .push_back(super::super::shell_network_setup::NetworkSetupRequest::RequestAcceptance);
    assert!(service.send(b"old options", direct(host), true));

    service.clear_setup();
    assert!(service.admitted_host.is_none());
    assert!(service.pending_host.is_none());
    assert!(service.selected_game.is_none());
    assert!(service.deferred_setup.is_empty());
    assert_eq!(
        service
            .transport
            .as_ref()
            .unwrap()
            .global()
            .queued_send_count(),
        1
    );

    service.restart_visit();
    let transport = service.transport.as_ref().unwrap();
    assert_eq!(transport.global().queued_send_count(), 0);
    assert_eq!(transport.primary_addr().unwrap(), address);
    assert!(transport.connections().peers().next().is_none());
}

#[test]
fn gameplay_binding_uses_source_order_and_constructed_house_ids() {
    let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.command-bar.example.json",
    ))
    .unwrap();
    let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    launch.opponents.clear();
    launch.ai_player_count = 0;
    let mut guest = launch.local_player().clone();
    guest.name = "Guest".to_owned();
    guest.raw_slot_index = Some(1);
    guest.slot.color_index = 2;
    guest.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Auto;
    launch.humans.push(guest);
    launch.local_player_index = 1;
    let host_name = launch.humans[0].name.clone();
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(launch, false, None, false)
        .unwrap();
    session.set_network_start_seed(19).unwrap();
    let mut sim = Simulation::new();
    for (house, source) in [1, 0].into_iter().enumerate() {
        let owner = sim.intern(&session.network_source_owner(source).unwrap());
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, house as u8, None, true, 0, 10));
    }
    // Gameplay requires a frozen start and admitted endpoints. Rejection
    // does not take or mutate any sockets.
    assert!(gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).is_err());
    let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    session.bind_network_endpoint(&host_name, host).unwrap();
    session.prepare_network_timing(2, || 0).unwrap();
    session.freeze_network_start(19, None).unwrap();
    let (local_house, seed, connections) =
        gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).unwrap();
    assert_eq!((local_house, seed), (0, 19));
    assert_eq!(connections.peers().collect::<Vec<_>>(), vec![(1, host)]);
    assert_eq!(session.network_endpoint(0), Some(host));
}

#[test]
fn loading_survivor_connections_handoff_the_original_majority_certificate() {
    use crate::net::live_commands::{LiveCommandMode, LiveCommandSession, StartupAgreement};
    use std::num::NonZeroU32;

    let mut launch = launch();
    launch.humans[0].raw_slot_index = Some(0);
    let mut third = launch.humans[1].clone();
    third.name = "Third".into();
    third.raw_slot_index = Some(2);
    third.slot.color_index = 3;
    third.slot.start_position = crate::skirmish_launch::LaunchStartPosition::Position(3);
    launch.humans.push(third);
    launch.local_player_index = 1;
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch, false, None, false)
        .unwrap();
    let host = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30123);
    let third = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30124);
    session.bind_network_endpoint("Host", host).unwrap();
    session.bind_network_endpoint("Third", third).unwrap();
    session.prepare_wol_timing([0; 3], false).unwrap();
    session.freeze_network_start(19, None).unwrap();
    let mut sim = Simulation::with_seed(19);
    for (house, source) in [1, 2, 0].into_iter().enumerate() {
        let owner = sim.intern(&session.network_source_owner(source).unwrap());
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, house as u8, None, true, 0, 10));
    }
    super::super::match_runtime::network_match::destroy_connection(
        &mut session,
        &mut sim,
        0,
        super::super::match_runtime::network_match::ConnectionError::Lost,
        None,
    )
    .unwrap();
    assert_eq!(session.network_connected_source_indices(), &[1, 2]);
    assert_eq!(session.network_endpoint(0), Some(host));
    assert_eq!(session.network_setup().unwrap().humans.len(), 3);
    let (local, seed, connections) =
        gameplay_connections(&session, &sim, LOBBY_RETRY_TIMING).unwrap();
    let rate = NonZeroU32::new(5).unwrap();
    let agreement = StartupAgreement::new(
        [(0, 2), (1, 0), (2, 1)],
        [1, 2],
        seed,
        10,
        30,
        rate,
        sim.state_hash(),
        0,
        "",
        "",
    )
    .unwrap();
    let mut service = service();
    let mut transport = service.transport.take().unwrap();
    let address = transport.primary_addr().unwrap();
    *transport.connections_mut() = connections;
    let live = LiveCommandSession::new(
        agreement,
        transport,
        local,
        seed,
        rate,
        LiveCommandMode::Wol,
        None,
    )
    .expect("the production connected roster must install its two-of-three certificate");
    assert_eq!(live.primary_addr().unwrap(), address);
    assert_eq!(live.connections().peers().collect::<Vec<_>>(), [(1, third)]);
    assert_eq!(live.into_transport().primary_addr().unwrap(), address);
}
