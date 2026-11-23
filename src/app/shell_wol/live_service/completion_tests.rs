use base64::Engine;
use serde_json::json;

use super::match_start::{bind_relay_roster, followed_user};
use super::requests::{
    accepted_human_matches, check_map_capacity, create_payload, local_choices_match, request_match,
    send_chat,
};
use super::*;
use crate::app::persistence::account;
use crate::app::shell_wol::{WolHighScoreRecord, WolLadderRecord};
use crate::ui::wol_chat::WolChatLine;

/// The reason must not land in a transcript the player is about to lose.
#[test]
fn a_failed_matchs_reason_waits_for_the_chat_the_player_lands_in() {
    use crate::app::shell_route::ShellRoute;
    assert!(problem_chat_open(
        ShellRoute::WolLobby,
        false,
        false,
        true,
        false
    ));
    assert!(!problem_chat_open(
        ShellRoute::WolLobby,
        false,
        false,
        true,
        true
    ));
    assert!(problem_chat_open(
        ShellRoute::NetworkSetup,
        true,
        false,
        true,
        true
    ));
    assert!(problem_chat_open(
        ShellRoute::Wol,
        false,
        true,
        false,
        false
    ));
    assert!(!problem_chat_open(
        ShellRoute::Wol,
        false,
        false,
        false,
        false
    ));
}

#[test]
fn relay_grant_requires_match_player_and_encryption_key_agreement() {
    use crate::net::udp::{
        RelayTicket as Ticket,
        test_support::{SecretKey, derive_packet_key},
    };
    let match_id = "11111111-1111-1111-1111-111111111111";
    let player_id = "22222222-2222-2222-2222-222222222222";
    let signing_key = SecretKey::new([9; 32]);
    let ticket = Ticket {
        room: [0x11; 16],
        player: [0x22; 16],
        source: 1,
        generation: 1,
        expires: 300,
        nonce: [3; 16],
    }
    .sign(&signing_key)
    .unwrap();
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let mut grant = wire::RelayGrant {
        address: "127.0.0.1:5000".parse().unwrap(),
        ticket: encode(&ticket),
        packet_key: encode(derive_packet_key(&signing_key, &ticket).expose_for_https()),
        payload_key: encode(&[7; 32]),
        source_index: 1,
        ordering_certificate: encode(
            &crate::net::udp::OrderingCertificate {
                room: [0x11; 16],
                sources_mask: 3,
                tick_hz: 45,
                expires: 86400,
                pacing: Default::default(),
                ordered_alone: false,
            }
            .sign(&signing_key)
            .unwrap(),
        ),
        federation: None,
    };
    assert_eq!(
        decode_relay_grant(&grant, match_id, player_id)
            .unwrap()
            .payload_key,
        [7; 32]
    );
    assert!(decode_relay_grant(&grant, player_id, player_id).is_err());
    assert!(decode_relay_grant(&grant, match_id, match_id).is_err());
    // A VO2C certificate carries the host's pacing switch.
    let vo2c = crate::net::udp::OrderingCertificate {
        room: [0x11; 16],
        sources_mask: 3,
        tick_hz: 45,
        expires: 86400,
        pacing: crate::net::udp::CertificatePacing::Off,
        ordered_alone: false,
    }
    .sign(&signing_key)
    .unwrap();
    grant.ordering_certificate = encode(&vo2c);
    assert_eq!(
        decode_relay_grant(&grant, match_id, player_id)
            .unwrap()
            .ordering_certificate,
        vo2c
    );
    grant.payload_key.clear();
    assert!(decode_relay_grant(&grant, match_id, player_id).is_err());
}

/// A watcher stands in the first player's seat, and where only AI play and
/// everyone watches, in the first observer's.
#[test]
fn a_watcher_follows_the_first_player_or_where_only_ai_play_an_observer() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.command-bar.example.json",
    ))
    .unwrap();
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(fixture["launch"].clone()).unwrap();
    let human = |name: &str, observer| {
        let mut human = launch.humans[0].clone();
        human.name = name.into();
        human.set_observer(observer);
        human
    };
    let users = ["Host", "Guest"].map(|name| wire::WatchedUser {
        name: name.into(),
        flags: 0,
        source_index: 0,
    });
    let followed = |host_plays: bool, guest_plays: bool| {
        followed_user(
            &users,
            &[human("Host", !host_plays), human("Guest", !guest_plays)],
        )
    };
    assert_eq!(followed(false, true), Some("Guest"));
    assert_eq!(followed(false, false), Some("Host"));
}

/// A watch grant admits to the watched match's room only.
#[test]
fn a_watch_grant_is_for_its_match_only() {
    use crate::net::udp::{
        RelayWatchTicket,
        test_support::{SecretKey, derive_packet_key},
    };
    let signing_key = SecretKey::new([9; 32]);
    let ticket = RelayWatchTicket {
        room: [0x11; 16],
        expires: 300,
        nonce: [3; 16],
        watcher: [4; 16],
        delay_seconds: 120,
    }
    .sign(&signing_key)
    .unwrap();
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let grant = wire::WatchGrant {
        address: "127.0.0.1:5000".parse().unwrap(),
        ticket: encode(&ticket),
        packet_key: encode(derive_packet_key(&signing_key, &ticket).expose_for_https()),
        payload_key: encode(&[7; 32]),
        ordering_certificate: encode(
            &crate::net::udp::OrderingCertificate {
                room: [0x11; 16],
                sources_mask: 3,
                tick_hz: 45,
                expires: 86400,
                pacing: Default::default(),
                ordered_alone: false,
            }
            .sign(&signing_key)
            .unwrap(),
        ),
    };
    let watched = "11111111-1111-1111-1111-111111111111";
    assert_eq!(
        decode_watch_grant(&grant, watched).unwrap().payload_key,
        [7; 32]
    );
    assert!(decode_watch_grant(&grant, "33333333-3333-3333-3333-333333333333").is_err());
    // A watcher spawn.ini's grant (docs/spawn.md "Watching"): the ticket
    // alone says it watches, and the same decoder admits it.
    assert!(relay_ticket_is_watch(&grant.ticket));
    let wire::WatchGrant {
        address,
        ticket,
        packet_key,
        payload_key,
        ordering_certificate,
    } = grant;
    let external = |match_id| {
        decode_external_watch_grant(
            address,
            ticket.clone(),
            packet_key.clone(),
            payload_key.clone(),
            ordering_certificate.clone(),
            match_id,
        )
    };
    assert_eq!(external(watched).unwrap().payload_key, [7; 32]);
    assert!(external("33333333-3333-3333-3333-333333333333").is_err());
    assert!(!relay_ticket_is_watch(&packet_key) && !relay_ticket_is_watch("*"));
}

/// A launcher's watcher renews by polling its events at a player's
/// cadence and sends the match service no command.
#[test]
fn a_spawned_watcher_only_polls_for_its_renewal() {
    let (mut service, input, _output) = control_service(None);
    service.spawned = true;
    service.spawned_watch = Some("watched-match".into());
    let received = 1_000;
    service.relay_granted(received);
    service.poll_renewal(received + 149_999);
    service.command("leave", json!({}));
    service.begin_finish(Instant::now());
    assert!(input.try_recv().is_err());
    let due = received + 150_000;
    service.poll_renewal(due);
    assert!(matches!(input.try_recv(), Ok(http::Request::PollEvents)));
    // An unanswered poll goes again at the renewal retry.
    service.poll_renewal(due + RELAY_RENEWAL_RETRY_MS - 1);
    assert!(input.try_recv().is_err());
    service.poll_renewal(due + RELAY_RENEWAL_RETRY_MS);
    assert!(matches!(input.try_recv(), Ok(http::Request::PollEvents)));
}

fn control_service(
    match_id: Option<&str>,
) -> (LiveService, Receiver<http::Request>, Sender<http::Response>) {
    let (requests, input) = mpsc::channel();
    let (output, responses) = mpsc::channel();
    let mut service = LiveService::with_worker(
        "https://lobby.test".into(),
        Arc::new(AtomicBool::new(false)),
        requests,
        responses,
    );
    service.next_request = 5;
    service.match_id = match_id.map(str::to_owned);
    (service, input, output)
}

#[test]
fn a_refused_watch_releases_its_grant_without_finishing_the_players_match() {
    let (mut service, input, _output) = control_service(None);
    let id = "11111111-1111-1111-1111-111111111111";
    // A grant with no start that this game can read.
    let error = service
        .receive_watch_start(id, true, || {
            let _: wire::WatchStarted = decode(json!({"match_id": id}))?;
            unreachable!()
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "invalid lobby message");
    assert!(
        matches!(input.try_recv(), Ok(http::Request::Command { operation: "unwatch", payload, .. })
        if payload == json!({"match_id": id}))
    );
    assert!(service.watching.is_none());
    assert!(!service.match_unfinished());
    assert!(input.try_recv().is_err());
}

#[test]
fn a_stale_or_repeated_watch_grant_preserves_the_current_watch() {
    let (mut service, input, _output) = control_service(None);
    let current = "11111111-1111-1111-1111-111111111111";
    let old = "22222222-2222-2222-2222-222222222222";
    service.watching = Some(Watching {
        match_id: current.into(),
        start: None,
    });
    service
        .receive_watch_start(old, false, || panic!("stale grant"))
        .unwrap();
    assert!(
        matches!(input.try_recv(), Ok(http::Request::Command { operation: "unwatch", payload, .. })
        if payload == json!({"match_id": old}))
    );
    service
        .receive_watch_start(current, false, || panic!("duplicate"))
        .unwrap();
    assert!(input.try_recv().is_err());
    assert_eq!(service.watching.as_ref().unwrap().match_id, current);
}

#[test]
fn a_locally_refused_lobby_start_finishes_only_the_announced_match() {
    let (mut service, input, _output) = control_service(None);
    let id = "11111111-1111-1111-1111-111111111111";
    // The lobby has committed a room revision that this guest cannot
    // adopt. It never opens the relay or reports relay_ready.
    let start = json!({"match_id": id, "revision": 9, "version": "test", "rules_digest": "",
        "seed": 1, "users": [], "relay": {"address":"127.0.0.1:4000", "source_index":0,
        "packet_key":"", "payload_key":"", "ticket":"", "ordering_certificate":""}});
    let error = service
        .receive_lobby_start(start, |_, _| panic!("stale revision"))
        .unwrap_err();
    assert_eq!(error.to_string(), "stale options revision");
    assert!(matches!(input.try_recv(), Ok(http::Request::HoldMatch(Some(held))) if held == id));
    assert!(
        matches!(input.try_recv(), Ok(http::Request::Command { operation: "finish", payload, .. }) if payload == json!({"match_id": id}))
    );
    assert!(service.locally_refused_start);
    // A later callback cannot replace the identity under its finish.
    service
        .receive_lobby_start(
            json!({"match_id": "22222222-2222-2222-2222-222222222222"}),
            |_, _| panic!("already held"),
        )
        .unwrap();
    assert!(input.try_recv().is_err());
    assert_eq!(service.match_id.as_deref(), Some(id));
}

#[test]
fn a_game_started_without_a_match_identity_finishes_nothing() {
    let (mut service, input, _output) = control_service(None);
    assert!(
        service
            .receive_lobby_start(json!({"match_id": "other"}), |_, _| panic!("bad identity"))
            .is_err()
    );
    assert!(service.match_id.is_none());
    assert!(!service.locally_refused_start);
    assert!(input.try_recv().is_err());
}

/// The remembered sign-in signs its name in, survives another name's
/// sign-in, and is forgotten here and at the lobby by its name's password
/// sign-in or by another account's remembered sign-in.
#[test]
fn a_remembered_sign_in_lasts_until_its_name_or_another_account_replaces_it() {
    let directory = crate::map::source::test_support::TestDirectory::new("live-account");
    let settings = Some(directory.path());
    let (service, input, _output) = control_service(None);
    let token = |c: char| c.to_string().repeat(43);
    let forgotten = |input: &Receiver<http::Request>| match input.try_recv() {
        Ok(http::Request::ForgetSignIn { sign_in_token, .. }) => Some(sign_in_token),
        _ => None,
    };
    remember_sign_in(settings, &service, "Tanya", token('A'));
    // Guest entry, even with this account's name, leaves its saved sign-in alone.
    assert!(matches!(
        sign_in_credential(settings, &service, "tanya", String::new(), false),
        Some(http::Credential::Guest)
    ));
    assert!(matches!(
        sign_in_credential(settings, &service, "tanya", String::new(), true),
        Some(http::Credential::Remembered(sent)) if sent == token('A')
    ));
    assert!(sign_in_credential(settings, &service, "Boris", String::new(), true).is_none());
    remember_sign_in(settings, &service, "Boris", token('B'));
    assert_eq!(forgotten(&input), Some(token('A')));
    assert!(matches!(
        sign_in_credential(settings, &service, "Boris", "s3cret-pw".into(), true),
        Some(http::Credential::Password { remember: true, .. })
    ));
    assert_eq!(forgotten(&input), Some(token('B')));
    assert_eq!(account::load(directory.path(), &service.lobby), None);
}

#[test]
fn a_completed_deletion_survives_closing_its_online_visit() {
    let (service, _input, output) = control_service(None);
    let lobby = service.lobby.clone();
    let mut cleanup = OnlineCleanup::default();
    cleanup.retire(service, Instant::now());
    output
        .send(http::Response::AccountAnswered {
            attempt: 1,
            answer: Ok(AccountSuccess::Deleted("Tanya".into())),
        })
        .unwrap();
    cleanup.poll(Instant::now());
    assert_eq!(cleanup.deleted_accounts, [(lobby, "Tanya".to_owned())]);
}

/// A lobby from before the Ranked game choice refuses the field, so a
/// ticked box sends none.
#[test]
fn only_an_unranked_room_sends_ranked() {
    let channel = crate::ui::wol_lobby::WolChannel::default();
    assert_eq!(
        create_payload(channel.clone(), "k".into(), true).get("ranked"),
        None
    );
    assert_eq!(
        create_payload(channel, "k".into(), false)["ranked"],
        json!(false)
    );
}

#[test]
fn a_match_request_is_private_chat_to_the_matchbot() {
    let (mut service, input, _output) = control_service(None);
    let request = super::super::quick_match::MatchRequest {
        country: -2,
        color: 3,
        match_by_resolution: false,
        locale: None,
        width: 1024,
    };
    request_match(&mut service, "matchbot".into(), &request);
    let http::Request::Command {
        operation, payload, ..
    } = input.try_recv().unwrap()
    else {
        panic!("chat command");
    };
    assert_eq!(
        (operation, payload),
        (
            "chat",
            json!({
                "text": "Match COU=-2, COL=3, MBR=0, RAT=0, RES=1024, ADR=0, NAT=0, PRT=0",
                "to": ["matchbot"],
                "action": false,
            })
        )
    );
}

/// The lobby drops private chat to anyone outside the sender's room: a
/// page with no listed target fails unsent, and a reply never goes to
/// the whole room.
#[test]
fn a_page_or_reply_without_a_listed_target_fails_unsent() {
    let (mut service, input, _output) = control_service(None);
    let visit = super::super::WolVisit::open(false);
    let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
    let page = WolChatLine::Page {
        name: units("Bob"),
        text: units("hi"),
    };
    let reply = WolChatLine::Reply { text: units("hi") };
    for line in [page, reply] {
        let result = send_chat(&mut service, Some(&visit), line, Vec::new(), false);
        let sent = input.try_recv().ok().map(|request| match request {
            http::Request::Command { payload, .. } => payload,
            _ => Value::Null,
        });
        assert_eq!((result, sent), (Some(Err(0x8000_4005)), None));
    }
}

/// Only the matchbot's ANSI `Start` gives its host the pairing.
#[test]
fn a_chat_start_gives_its_host_the_pairing() {
    let chat = |ansi: bool| -> Chat {
        decode(json!({
            "from": {"player_id": "m", "name": "matchbot", "flags": 1},
            "text": "Start 7:Alpha,0,0,0,0,0,0,0,Beta,0,0,0,0,0,0,0",
            "private": true,
            "ansi": ansi,
            "quick_match": {
                "seed": 7,
                "host": {"name": "Alpha", "country": -2, "color": 3},
                "guest": {"name": "Beta", "country": 5, "color": -2},
            },
        }))
        .unwrap()
    };
    let pairing = chat(true).hosted_pairing("Alpha").unwrap();
    assert_eq!((pairing.seed, pairing.guest.country), (7, 5));
    assert_eq!(chat(true).hosted_pairing("Beta"), None);
    assert_eq!(chat(false).hosted_pairing("Alpha"), None);
}

/// The lobby's new-release line asks the updater, even with a value this
/// game cannot show; a player's line with the same words, or another
/// lobby line, does not.
#[test]
fn the_lobbys_new_release_line_announces_a_release() {
    let line = |name: &str, phrases: Value| -> Chat {
        decode(json!({
            "from": {"player_id": "00000000-0000-0000-0000-000000000000", "name": name, "flags": 1},
            "text": "VERA20k 0.3.7 is out, and you have 0.3.6.",
            "private": false,
            "action": false,
            "sent_at": 1,
            "phrases": phrases,
        }))
        .unwrap()
    };
    let release = json!([{"key": "NewRelease", "values": {"newest": "0.3.7", "later": true}}]);
    assert!(line("VERA20k", release).announces_release());
    assert!(!line("Tanya", Value::Null).announces_release());
    assert!(!line("VERA20k", json!([{"key": "OnlyPlayerOnline"}])).announces_release());
}

/// The lobby's lines that a player's game differs from the host's are
/// warnings the room shows red, a newer lobby's values included; its
/// other lines and a player's are not.
#[test]
fn the_lobbys_other_game_lines_are_warnings() {
    let line = |phrases: Value| -> Chat {
        decode(json!({
            "from": {"player_id": "p", "name": "VERA20k", "flags": 1},
            "text": "the lobby's text",
            "phrases": phrases,
        }))
        .unwrap()
    };
    let rules = json!([{"key": "RulesMismatch", "values": {"name": "Boris", "later": 1}}]);
    assert!(line(rules).warns_of_other_game());
    let build = json!([{"key": "BuildMismatch", "values": {"name": "Boris", "version": "0.3.8"}}]);
    assert!(line(build).warns_of_other_game());
    // The line naming what only one game has opens with its own sentence.
    let changes = json!([{"key": "RulesDifferFromHost", "values": {"name": "Boris"}},
        {"key": "SameRulesToStart"}]);
    assert!(line(changes).warns_of_other_game());
    assert!(!line(json!([{"key": "OnlyPlayerOnline"}])).warns_of_other_game());
    assert!(!line(Value::Null).warns_of_other_game());
}

/// Only a line shown from the lobby's phrases counts as localized, which
/// is what keeps it from translation into the interface language; a
/// player's line, or phrases this game cannot show, keep the text.
#[test]
fn only_a_line_shown_from_the_lobbys_phrases_is_localized() {
    let line = |phrases: Value| -> (String, bool) {
        let chat: Chat = decode(json!({
            "from": {"player_id": "p", "name": "VERA20k", "flags": 1},
            "text": "the lobby's text",
            "phrases": phrases,
        }))
        .unwrap();
        let (text, localized) = chat.shown_text();
        (String::from_utf16(&text).unwrap(), localized)
    };
    let hidden = "1 game is hidden because it runs another VERA20k version.";
    assert_eq!(
        line(json!([{"key": "GamesHidden", "values": {"count": 1}}])),
        (hidden.to_owned(), true)
    );
    let text = ("the lobby's text".to_owned(), false);
    assert_eq!(line(Value::Null), text);
    assert_eq!(line(json!([{"key": "NotYetKnown"}])), text);
}

#[test]
fn lobby_upgrade_notice_names_the_download_once_per_sign_in() {
    let (mut service, _input, _output) = control_service(None);
    let reason = "Online play needs VERA20k 0.3.0 or newer.";
    assert_eq!(
        service.outdated_notice(Some(reason)).as_deref(),
        Some(format!("{reason} https://vera20k.org/").as_str())
    );
    assert_eq!(service.outdated_notice(Some(reason)), None);
    service.auth_attempt += 1;
    assert_eq!(
        service.outdated_notice(None).as_deref(),
        Some("Update VERA20k to use secure online multiplayer. https://vera20k.org/")
    );
}

fn finish_payload_sent(service: &mut LiveService, input: &Receiver<http::Request>) -> Value {
    service.begin_finish(Instant::now());
    let http::Request::Command {
        operation, payload, ..
    } = input.try_recv().unwrap()
    else {
        panic!("finish command");
    };
    assert_eq!(operation, "finish");
    payload
}

#[test]
fn a_match_finishes_with_its_result_and_a_lobby_match_with_its_winners() {
    // A lobby match reports for the ranked ladder. A lobby from before
    // team ladders refuses `winners`: the finish goes again without them.
    // Its score screen held the finish back for a minute and a half: the
    // lobby times its player's leaving by `ended_ago`.
    let (mut lobby, input, _output) = control_service(Some("lobby-match"));
    lobby.finish_result = Some("won");
    lobby.finish_winners = Some(vec!["Tanya".into(), "Boris".into()]);
    lobby.finish_decided = Instant::now().checked_sub(Duration::from_secs(90));
    let first = finish_payload_sent(&mut lobby, &input);
    assert_eq!(
        first,
        json!({"match_id":"lobby-match","result":"won","winners":["Tanya","Boris"],"ended_ago":90})
    );
    let mut retried = |failure, at: u64| {
        assert!(!lobby.finish_failed(5, failure, Instant::now()));
        lobby.poll_cleanup(Instant::now() + Duration::from_secs(at));
        let http::Request::Command { payload, .. } = input.try_recv().unwrap() else {
            panic!("finish retry");
        };
        payload
    };
    // The lobby replays a committed request ID only with the payload it
    // committed: a finish whose answer was lost goes again as first sent.
    assert_eq!(retried(http::CommandFailure::Retryable, 3), first);
    // A lobby from before `ended_ago` refuses it, and one from before team
    // ladders `winners`: the finish goes again without the newest.
    let rejected = http::CommandFailure::Rejected(0x8000_4005);
    assert_eq!(
        retried(rejected, 5),
        json!({"match_id":"lobby-match","result":"won","winners":["Tanya","Boris"]})
    );
    assert_eq!(
        retried(rejected, 10),
        json!({"match_id":"lobby-match","result":"won"})
    );

    let (mut spawned, input, _output) = control_service(Some("spawned-match"));
    spawned.spawned = true;
    spawned.finish_result = Some("lost");
    let payload = finish_payload_sent(&mut spawned, &input);
    assert_eq!(
        payload,
        json!({"match_id":"spawned-match","result":"lost","opponents_left":false})
    );

    // Retries resend the same decided result.
    spawned.finish_failed(5, http::CommandFailure::Retryable, Instant::now());
    spawned.poll_cleanup(Instant::now() + Duration::from_secs(3));
    let http::Request::Command { payload: retry, .. } = input.try_recv().unwrap() else {
        panic!("finish retry");
    };
    assert_eq!(retry, payload);
    spawned.finish_opponents_left = true;
    assert_eq!(
        spawned.finish_payload("m", Instant::now())["opponents_left"],
        json!(true)
    );
}

#[test]
fn one_ladder_command_answers_the_ladder_and_high_score_tables() {
    let (mut service, input, _output) = control_service(None);
    let names = |input: &Receiver<http::Request>| {
        let mut sent = Vec::new();
        while let Ok(http::Request::Command {
            operation, payload, ..
        }) = input.try_recv()
        {
            assert_eq!(operation, "ladder");
            sent.push(payload["names"].clone());
        }
        sent
    };
    service.look_up_ladder(vec!["tanya".into()], false);
    service.look_up_ladder(vec!["tanya".into()], true);
    service.look_up_ladder(vec!["boris".into()], true);
    assert_eq!(names(&input), [json!(["tanya"]), json!(["boris"])]);

    // Boris played in an earlier season only: no record, but a badge.
    let ladder: wire::Ladder = serde_json::from_value(json!({"season":2,"records":[
        {"name":"Tanya","points":49,"os":48.51,"rank":7,"wins":40,"losses":12,"disconnects":1}],
        "high_scores":[{"name":"Tanya","points":1501,"rank":7},{"name":"Boris","points":81,"rank":0}]}))
    .unwrap();
    let high_score = |name: &str, points, rank| WolHighScoreRecord {
        name: name.into(),
        points,
        rank,
        coop: 0,
    };
    let tanya = WolLadderRecord {
        name: "Tanya".into(),
        wins: 40,
        losses: 12,
        points: 49,
        os_hundredths: 4851,
        rank: 7,
        disconnects: 1,
    };
    assert_eq!(
        ladder.tables(),
        (
            vec![tanya.clone()],
            vec![high_score("Tanya", 1501, 7), high_score("Boris", 81, 0)],
        )
    );
    assert_eq!(ladder.tables().0[0].os_text(), "48.51");
    // A lobby from before `os` gives the rounded OS.
    let older: wire::Ladder = serde_json::from_value(json!({"season":2,"records":[
        {"name":"Tanya","points":49,"rank":7,"wins":40,"losses":12,"disconnects":1}]}))
    .unwrap();
    let rounded = WolLadderRecord {
        os_hundredths: 4900,
        ..tanya
    };
    assert_eq!(older.tables(), (vec![rounded], vec![]));
    let unseasoned: wire::Ladder =
        serde_json::from_value(json!({"season":null,"records":[]})).unwrap();
    assert_eq!(unseasoned.tables(), (vec![], vec![]));
}

#[test]
fn a_played_lobby_match_sends_its_network_report_until_a_lobby_refuses_it() {
    let report = json!({"played_seconds": 600.0});
    let (mut lobby, input, _output) = control_service(Some("played-match"));
    lobby.keep_network_report(report.clone());
    assert_eq!(
        finish_payload_sent(&mut lobby, &input),
        json!({"match_id":"played-match","network_quality":report})
    );
    // A lobby from before match reports refuses the field: the finish
    // goes again without it.
    assert!(!lobby.finish_failed(
        5,
        http::CommandFailure::Rejected(0x8000_4005),
        Instant::now()
    ));
    lobby.poll_cleanup(Instant::now() + Duration::from_secs(3));
    let http::Request::Command { payload: retry, .. } = input.try_recv().unwrap() else {
        panic!("finish retry");
    };
    assert_eq!(retry, json!({"match_id":"played-match"}));

    // A launcher's match reports in spawn_result.json, not to its service.
    let (mut spawned, input, _output) = control_service(Some("spawned-match"));
    spawned.spawned = true;
    spawned.keep_network_report(report);
    assert!(
        finish_payload_sent(&mut spawned, &input)
            .get("network_quality")
            .is_none()
    );
}

#[test]
fn a_finished_matchs_report_never_reaches_a_later_matchs_finish() {
    let (mut service, input, _output) = control_service(Some("first-match"));
    service.keep_network_report(json!({"played_seconds": 600.0}));
    assert!(
        finish_payload_sent(&mut service, &input)
            .get("network_quality")
            .is_some()
    );
    service.finish_completed(5);
    // The next start is cancelled before it plays.
    service.hold_match(Some("cancelled-match".into()));
    assert!(matches!(
        input.try_recv(),
        Ok(http::Request::HoldMatch(None))
    ));
    assert!(matches!(
        input.try_recv(),
        Ok(http::Request::HoldMatch(Some(_)))
    ));
    assert_eq!(
        finish_payload_sent(&mut service, &input),
        json!({"match_id":"cancelled-match"})
    );
}

#[test]
fn relay_renewal_is_timed_from_grant_receipt_at_half_life() {
    // A slow local clock made the ticket's lobby-time expiry look distant,
    // so renewal was requested only after the relay rejected the ticket.
    // Half-life leaves a lobby outage 150 s before the ticket expires; the
    // receipt clock's 32-bit word wraps in between.
    let (mut service, input, _output) = control_service(Some("renew-match"));
    let received = u32::MAX - 100_000;
    service.relay_granted(received);
    service.poll_renewal(received + 1_000);
    service.poll_renewal(received.wrapping_add(149_999));
    assert!(input.try_recv().is_err());
    let due = received.wrapping_add(150_000);
    service.poll_renewal(due);
    let http::Request::Command {
        operation, payload, ..
    } = input.try_recv().unwrap()
    else {
        panic!("renewal command");
    };
    assert_eq!(
        (operation, payload),
        ("renew_relay", json!({"match_id":"renew-match"}))
    );
    // The reconnect notice reads the pass's lapse on the same receipt clock.
    assert!(!service.relay_pass_lapsed(received));
    assert!(!service.relay_pass_lapsed(received.wrapping_add(299_999)));
    assert!(service.relay_pass_lapsed(received.wrapping_add(300_000)));
    // The reply's fresh grant moves the deadline half a lifetime on.
    service.relay_granted(due + 1_000);
    service.poll_renewal(due + 11_000);
    assert!(input.try_recv().is_err());
    assert!(!service.relay_pass_lapsed(received.wrapping_add(300_000)));
    // A lobby session that ends mid-match cannot renew the pass any more.
    service.clear_cleanup();
    assert!(service.relay_pass_lapsed(due + 301_000));
}

#[test]
fn admission_reports_resend_within_the_start_window_after_a_busy_lobby() {
    // One 429 or 503 to relay_ready used to fail the start at its deadline.
    let (mut service, input, _output) = control_service(None);
    let now = Instant::now();
    service.pending = Some(start_waiting_until(now + RELAY_TIMEOUT, true));
    let sent = || match input.try_recv() {
        Ok(http::Request::Command {
            id,
            operation,
            payload,
        }) => Some((id, operation, payload)),
        _ => None,
    };
    service.send_report("relay_ready", json!({"match_id":"m"}), now);
    let first = sent().unwrap();
    assert!(service.report_failed(first.0, http::CommandFailure::Retryable, now));
    service.poll_report(now + Duration::from_millis(1999));
    assert_eq!(sent(), None);
    service.poll_report(now + Duration::from_secs(2));
    assert_eq!(sent(), Some(first.clone()), "same request ID and payload");
    service.report_completed(first.0);
    service.poll_report(now + RELAY_TIMEOUT);
    assert_eq!(sent(), None);

    // A refusal for any other reason stands.
    service.send_report("latency_ready", json!({"match_id":"m","pings":[]}), now);
    let latency = sent().unwrap();
    let rejected = http::CommandFailure::Rejected(0x8000_4005);
    assert!(!service.report_failed(latency.0, rejected, now));
    service.poll_report(now + RELAY_TIMEOUT);
    assert_eq!(sent(), None);
}

fn start_waiting_until(deadline: Instant, relay_acknowledged: bool) -> PendingStart {
    PendingStart {
        match_id: "m".into(),
        host_name: "Host".into(),
        transport: None,
        deadline,
        relay_acknowledged,
        relay_reported: false,
        probes_started: false,
        latencies_reported: false,
        resume_pings: None,
        report: None,
    }
}

/// A start that runs out says whose game it waited on: one whose own
/// relay answered used to read that it could not connect.
#[test]
fn an_expired_start_says_whose_game_could_not_reach_the_relay() {
    let (mut service, input, _output) = control_service(None);
    let now = Instant::now();
    let expired = now + RELAY_TIMEOUT;
    service.pending = Some(start_waiting_until(expired, false));
    assert_eq!(service.expired_admission(now), None);
    assert_eq!(
        service.expired_admission(expired),
        Some("VERA:StartRelayNotReached")
    );
    service.pending = Some(start_waiting_until(expired, true));
    service.send_report("relay_ready", json!({"match_id":"m"}), now);
    assert_eq!(
        service.expired_admission(expired),
        Some("VERA:StartLobbyBusy"),
        "the lobby never took its relay_ready"
    );
    let Ok(http::Request::Command { id, .. }) = input.try_recv() else {
        panic!("relay_ready");
    };
    service.report_completed(id);
    assert_eq!(
        service.expired_admission(expired),
        Some("VERA:StartRelayNotReachedByOther")
    );
    service.pending.as_mut().unwrap().probes_started = true;
    assert_eq!(
        service.expired_admission(expired),
        Some("VERA:StartLatencyCheckFailed")
    );
}

/// The lobby's reasons for refusing a start show as their own lines; one
/// this game does not know keeps the general line. The fixture lists the
/// reasons the lobby sends.
#[test]
fn a_refused_start_shows_the_lobbys_reason() {
    let (mut service, _input, _output) = control_service(None);
    let relay = "A player could not connect to the gameplay relay.";
    assert_eq!(
        service.start_rejected_line("A player left before the game started."),
        "VERA:StartPlayerLeft"
    );
    assert_eq!(
        service.start_rejected_line(relay),
        "VERA:StartRefusedConnection",
        "no start pending here"
    );
    service.pending = Some(start_waiting_until(Instant::now(), true));
    let messages =
        &crate::test_fixture::json("tests/fixtures/lobby/start_rejected.json")["messages"];
    for message in messages.as_array().unwrap() {
        let message = message.as_str().unwrap();
        assert_ne!(
            service.start_rejected_line(message),
            "VERA:StartRefusedConnection",
            "{message}"
        );
    }
    assert_eq!(
        service.start_rejected_line("A player cancelled the game start."),
        "VERA:StartPlayerCancelled"
    );
    assert_eq!(
        service.start_rejected_line("A player could not complete the peer latency check."),
        "VERA:StartLatencyCheckFailed"
    );
    assert_eq!(
        service.start_rejected_line(relay),
        "VERA:StartRelayNotReached",
        "the lobby never took its relay_ready"
    );
    service.pending.as_mut().unwrap().relay_reported = true;
    assert_eq!(
        service.start_rejected_line(relay),
        "VERA:StartRelayNotReachedByOther"
    );
    assert_eq!(
        service.start_rejected_line("A newer reason."),
        "VERA:StartRefusedConnection"
    );
    // The names a timed-out start's `start_rejected` carries, as
    // `service.js` sends them; a released lobby sends none.
    let names = serde_json::json!({"message": relay, "phrases": [{"key": "StartNotConnected",
        "values": {"names": {"and": ["Alex", "Sam"]}, "count": 2}}]});
    assert_eq!(
        lobby_names(&names).as_deref(),
        Some("Alex and Sam did not connect to the game server in time, so the game did not start.")
    );
    assert_eq!(lobby_names(&serde_json::json!({"message": relay})), None);
    for key in [
        "VERA:StartRelayNotReached",
        "VERA:StartRelayNotReachedByOther",
        "VERA:StartLatencyCheckFailed",
        "VERA:StartPlayerLeft",
        "VERA:StartPlayerCancelled",
    ] {
        assert!(crate::i18n::find(key).is_some(), "{key}");
    }
}

#[test]
fn spawned_session_binds_the_same_relay_roster_as_the_lobby_start() {
    // A guest at source 1: the lobby admits its own node first, then
    // canonicalizes the room roster before game_started binds it.
    let text = "[Settings]\nName=P1\nSide=1\nColor=1\nPlayerCount=2\nSourceIndex=1\n\
                Seed=77\nSpecialHouseIsAlly=No\n[Other1]\nName=P0\nSide=0\nColor=0\n\
                SourceIndex=0\nIp=10.0.0.1\n";
    let crate::app::frontend::cncnet_spawn::SpawnGame::Network(network) =
        crate::app::frontend::cncnet_spawn::SpawnRequest::parse(text)
            .unwrap()
            .game
    else {
        panic!("network spawn");
    };
    let canonical = network.session;
    let names = ["P0".to_owned(), "P1".to_owned()];

    let mut lobby = crate::app::match_runtime::session::MatchSession::default();
    lobby.enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
    let mut local_first = canonical.clone();
    local_first.humans.reverse();
    local_first.local_player_index = 0;
    lobby
        .accept_network_setup(local_first, false, None, false)
        .unwrap();
    lobby.canonicalize_online_roster(&names).unwrap();
    bind_relay_roster(&mut lobby, &names, 77).unwrap();

    let mut spawned = crate::app::match_runtime::session::MatchSession::default();
    crate::app::shell_network_setup::accept_external_session(
        &mut spawned,
        canonical,
        crate::app::match_runtime::session::NetworkReturnContext::Wol,
    )
    .unwrap();
    bind_relay_roster(&mut spawned, &names, 77).unwrap();

    assert_eq!(spawned.network_setup(), lobby.network_setup());
    assert_eq!(spawned.network_setup().unwrap().local_player_index, 1);
    assert_eq!(spawned.host(), lobby.host());
    assert_eq!(spawned.network_start_seed(), Some(77));
    assert_eq!(spawned.network_start_seed(), lobby.network_start_seed());
    for source in 0..2 {
        assert_eq!(
            spawned.network_endpoint(source),
            Some(crate::net::udp::relay_peer_endpoint(source as u8).unwrap())
        );
        assert_eq!(
            spawned.network_endpoint(source),
            lobby.network_endpoint(source)
        );
    }
}

#[test]
fn options_agreement_keeps_map_status_out_of_host_ready() {
    let (mut service, input, _output) = control_service(None);
    let agreement = json!({
        "revision": 7, "version": "0.2.2", "rules_digest": "rules",
        "map_digest": "physical", "effective_map_digest": "effective",
        "staging_digest": "staging"
    });
    service.publish_options_agreement(agreement.clone(), 1, true);
    let next = || match input.try_recv().expect("queued agreement") {
        http::Request::Command {
            operation, payload, ..
        } => (operation, payload),
        _ => panic!("expected a protocol command"),
    };
    let mut expected_map = agreement.clone();
    expected_map["status"] = json!(1);
    let mut expected_ready = agreement.clone();
    expected_ready["ready"] = json!(true);
    assert_eq!(next(), ("map_status", expected_map));
    assert_eq!(next(), ("ready", expected_ready));
    assert!(input.try_recv().is_err());

    service.publish_options_agreement(agreement.clone(), 0, false);
    let mut expected_map = agreement;
    expected_map["status"] = json!(0);
    assert_eq!(next(), ("map_status", expected_map));
    assert!(
        input.try_recv().is_err(),
        "a guest's readiness comes from its setup's Accept"
    );
}

#[test]
fn translations_stay_bounded_and_pause_after_an_unavailable_translator() {
    use super::super::translation::TranslatedLine;
    let (requests, input) = mpsc::channel();
    let mut translations = Translations::default();
    let now = Instant::now();
    let line = |name: &str| TranslatedLine::Match {
        name: name.into(),
        color: 0,
    };
    let mut ask = |name, at| {
        let text: Vec<u16> = "rush".encode_utf16().collect();
        translations.request(
            &requests,
            &text,
            ChatLanguage::SimplifiedChinese,
            line(name),
            at,
        );
    };
    for name in ["A", "B", "C", "D", "E"] {
        ask(name, now);
    }
    let sent: Vec<_> = input
        .try_iter()
        .map(|request| match request {
            http::Request::Translate { id, target, .. } => (id, target),
            _ => panic!("only translations were asked"),
        })
        .collect();
    assert_eq!(sent, [0, 1, 2, 3].map(|id| (id, "zh-Hans")));
    let shown = translations.reply(
        0,
        Ok(ChatReply {
            text: "冲".into(),
            source: None,
        }),
        now,
    );
    assert!(shown.is_some_and(|(shown, target, reply)| {
        shown == line("A") && target == ChatLanguage::SimplifiedChinese && reply.text == "冲"
    }));
    let failed = translations.reply(1, Err(http::TranslateFailure::Unavailable), now);
    assert!(failed.is_none());
    let mut ask = |name, at| {
        let text: Vec<u16> = "rush".encode_utf16().collect();
        translations.request(
            &requests,
            &text,
            ChatLanguage::SimplifiedChinese,
            line(name),
            at,
        );
    };
    ask("F", now + TRANSLATION_PAUSE - Duration::from_secs(1));
    assert_eq!(input.try_iter().count(), 0);
    ask("G", now + TRANSLATION_PAUSE);
    assert_eq!(input.try_iter().count(), 1);
}

#[test]
fn a_reply_that_only_repeats_its_line_shows_nothing() {
    use super::super::translation::TranslatedLine;
    let (requests, _input) = mpsc::channel();
    let mut translations = Translations::default();
    let now = Instant::now();
    for (id, (original, reply, shown)) in [
        ("gg", "gg", false),
        ("GG!", "gg", false),
        ("2v2 go", "2V2, go.", false),
        ("gg wp", "gg 打得好", true),
        ("gg", "[从英语翻译] gg", false),
    ]
    .into_iter()
    .enumerate()
    {
        let text: Vec<u16> = original.encode_utf16().collect();
        let line = TranslatedLine::Match {
            name: "A".into(),
            color: 0,
        };
        translations.request(&requests, &text, ChatLanguage::SimplifiedChinese, line, now);
        let reply = ChatReply {
            text: reply.into(),
            source: None,
        };
        let reply = translations.reply(id as u64, Ok(reply), now);
        assert_eq!(reply.is_some(), shown, "{original}");
    }
}

fn setup_session() -> crate::app::match_runtime::session::MatchSession {
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext};
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.command-bar.example.json",
    ))
    .unwrap();
    let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(fixture["launch"].clone()).unwrap();
    let mut guest = launch.humans[0].clone();
    launch.humans[0].name = "Host".into();
    launch.humans[0].raw_slot_index = Some(0);
    guest.name = "Guest".into();
    guest.raw_slot_index = Some(1);
    launch.humans = vec![launch.humans[0].clone(), guest];
    launch.local_player_index = 0;
    launch.opponents.clear();
    launch.ai_player_count = 0;
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch, true, None, false)
        .unwrap();
    session
}

#[test]
fn a_legacy_reply_can_only_retry_its_unchanged_publication_once() {
    let (mut service, input, _) = control_service(None);
    let options: wire::Options = serde_json::from_value(
        crate::test_fixture::json("tests/fixtures/lobby/options.json")["rooms"]["cooperative"]
            .clone(),
    )
    .unwrap();
    let publish = |service: &mut LiveService| {
        service.publish_room_options(options.clone());
        let http::Request::Command { id, payload, .. } = input.try_recv().unwrap() else {
            panic!("expected the room options");
        };
        (id, payload["options"].clone())
    };
    let (old, _) = publish(&mut service);
    let (latest, sent) = publish(&mut service);
    assert!(!service.take_legacy_refusal(old, false, || Some(sent.clone())));
    assert!(!service.take_legacy_refusal(latest, true, || Some(sent.clone())));
    let (edited, mut other) = publish(&mut service);
    other["launch"]["options"]["short_game"] = json!(!options.launch.options.short_game);
    assert!(!service.take_legacy_refusal(edited, false, || Some(other)));
    let (unchanged, sent) = publish(&mut service);
    assert!(service.take_legacy_refusal(unchanged, false, || Some(sent.clone())));
    assert!(!service.take_legacy_refusal(unchanged, false, || Some(sent)));
}

#[test]
fn host_options_need_one_map_start_per_playing_team() {
    let cases = crate::test_fixture::json("tests/fixtures/lobby/options.json");
    let mut room: wire::Options =
        serde_json::from_value(cases["rooms"]["fifteen_teams_of_thirty"].clone()).unwrap();
    assert!(check_map_capacity(&room).is_ok());
    room.map_metadata.player_capacity = 14;
    assert_eq!(check_map_capacity(&room).unwrap_err().0, 14);
}

#[test]
fn a_cooperative_campaign_reaches_the_guest_ahead_of_the_options_it_rides_in() {
    let launch = setup_session().network_setup().unwrap().clone();
    let mut value = json!({
        "launch": launch,
        "map_metadata": {"display_name": "Map", "player_capacity": 2, "official": true,
            "byte_length": 1, "digest": "d", "source_sha256": "11".repeat(32),
            "effective_digest": "aa".repeat(32)},
        "vacancies": vec!["open"; 30],
        "observer_row": null,
        "cooperative_difficulty": 1,
        "seed": null,
    });
    let plain: wire::Options = serde_json::from_value(value.clone()).unwrap();
    // Lobbies from before the field refuse it, so other modes leave it out.
    let sent = serde_json::to_value(&plain).unwrap();
    assert!(sent.get("cooperative_campaign").is_none());
    let events = plain.into_setup_events("Guest").unwrap();
    assert!(matches!(
        events.as_slice(),
        [NetworkSetupEvent::HostSetup(_)]
    ));
    // The native host sends its C0 ahead of the options (+0x94 at the
    // join accept 0x007A6E5A), so the guest applies it first.
    let text = "C0,0,0,0,0,0,0,0,0,0,0,0,1,2,-1,Host,Guest";
    value["cooperative_campaign"] = json!(text);
    let cooperative: wire::Options = serde_json::from_value(value).unwrap();
    let events = cooperative.into_setup_events("Guest").unwrap();
    assert!(matches!(
        events.as_slice(),
        [
            NetworkSetupEvent::CooperativeCampaign(campaign),
            NetworkSetupEvent::HostSetup(options),
        ] if campaign == text && options.launch.local_player_index == 1
    ));
}

#[test]
fn final_start_publication_requires_fresh_revision_readiness_and_map_acknowledgements() {
    let mut session = setup_session();
    for index in 0..2 {
        session.set_network_player_ready(index, true).unwrap();
        session.set_network_map_status(index, 1).unwrap();
    }
    let options = json!({"launch":session.network_setup()});
    let mut start = StartPreparation {
        users: vec!["Host".into(), "Guest".into()],
        options: options.clone(),
        revision: None,
        publication_request: Some(9),
        submitted_request: None,
        deadline: Instant::now() + RELAY_TIMEOUT,
    };
    assert!(!start.ready(&session, 4));
    assert!(start.accept_options(5, &options));
    session.invalidate_network_agreement().unwrap();
    assert!(!start.ready(&session, 4));
    assert!(!start.ready(&session, 5));
    session.set_network_player_ready(1, true).unwrap();
    assert!(
        !start.ready(&session, 5),
        "old map status cannot satisfy a new revision"
    );
    session.set_network_map_status(1, 1).unwrap();
    assert!(start.ready(&session, 5));
    assert!(!start.accept_options(6, &json!({"changed":true})));
}

#[test]
fn a_quick_match_host_starts_its_reverse_channel_order_unaccepted() {
    let mut session = setup_session();
    for index in 0..2 {
        session.set_network_map_status(index, 1).unwrap();
    }
    // The matchbot's options withdrew the host's own acceptance.
    session.set_network_player_ready(0, false).unwrap();
    session.set_network_player_ready(1, true).unwrap();
    let start = |users: [&str; 2]| StartPreparation {
        users: StartPreparation::users_in_human_order(&session, users.map(String::from).to_vec()),
        options: Value::Null,
        revision: Some(5),
        publication_request: None,
        submitted_request: None,
        deadline: Instant::now() + RELAY_TIMEOUT,
    };
    let reversed = start(["Guest", "Host"]);
    assert_eq!(reversed.users, ["Host", "Guest"]);
    assert!(reversed.ready(&session, 5));
    assert!(!start(["Guest", "Other"]).ready(&session, 5));
    session.set_network_player_ready(1, false).unwrap();
    assert!(!reversed.ready(&session, 5), "the guest still accepts");
}

#[test]
fn optimistic_local_choice_waits_for_the_authoritative_options_echo() {
    let mut session = setup_session();
    let accepted = json!({"launch":session.network_setup()});
    assert!(local_choices_match(&session, &accepted));
    let mut launch = session.network_setup().unwrap().clone();
    let random = !launch.local_slot().color_random;
    launch.local_slot_mut().color_random = random;
    session
        .accept_network_setup(launch, true, None, false)
        .unwrap();
    assert!(!local_choices_match(&session, &accepted));
    let expected = serde_json::to_value(session.network_setup().unwrap().local_player()).unwrap();
    assert!(
        !accepted_human_matches(&accepted, &expected),
        "an unrelated old echo does not acknowledge the outgoing request"
    );
    let echoed = json!({"launch":session.network_setup()});
    assert!(local_choices_match(&session, &echoed));
    assert!(accepted_human_matches(&echoed, &expected));
}

#[test]
fn a_host_answering_with_random_settles_a_banned_country_choice() {
    let mut session = setup_session();
    let mut human = session.network_setup().unwrap().local_player().clone();
    human.slot.country_random = false;
    let expected_human = serde_json::to_value(&human).unwrap();
    human.slot.country_random = true;
    let pending = PendingChoices {
        request_id: 1,
        expected_human,
        expected_random: serde_json::to_value(&human).unwrap(),
        deadline: Instant::now() + RELAY_TIMEOUT,
    };
    let mut launch = session.network_setup().unwrap().clone();
    launch.local_player_mut().slot = human.slot.clone();
    session
        .accept_network_setup(launch, true, None, false)
        .unwrap();
    assert!(pending.answered_by(&json!({"launch":session.network_setup()})));
    let mut launch = session.network_setup().unwrap().clone();
    launch.local_slot_mut().color_random = !launch.local_slot().color_random;
    assert!(!pending.answered_by(&json!({"launch":launch})));
}

#[test]
fn a_guests_first_choices_are_answered_in_the_row_the_host_gave_it() {
    // The first choices leave before the host's options seat the guest,
    // so they name row 0; the host keeps the guest in its own row.
    let session = setup_session();
    let launch = session.network_setup().unwrap();
    let mut human = launch.humans[1].clone();
    human.raw_slot_index = Some(0);
    let expected_human = serde_json::to_value(&human).unwrap();
    human.slot.country_random = true;
    let pending = PendingChoices {
        request_id: 1,
        expected_human,
        expected_random: serde_json::to_value(&human).unwrap(),
        deadline: Instant::now() + RELAY_TIMEOUT,
    };
    assert!(pending.answered_by(&json!({"launch":launch})));
    let mut other = launch.clone();
    other.humans[1].slot.color_random = !other.humans[1].slot.color_random;
    assert!(
        !pending.answered_by(&json!({"launch":other})),
        "other choices still wait"
    );
}

#[test]
fn a_room_with_teams_answers_choices_with_the_team_it_gave() {
    // A guest's first choices ask for no team; with Teams set the host
    // keeps the guest on the team it gave.
    use crate::skirmish_launch::LaunchTeam;
    let session = setup_session();
    let mut launch = session.network_setup().unwrap().clone();
    let mut human = launch.humans[1].clone();
    human.slot.team = LaunchTeam::None;
    let expected_human = serde_json::to_value(&human).unwrap();
    human.slot.country_random = true;
    let pending = PendingChoices {
        request_id: 1,
        expected_human,
        expected_random: serde_json::to_value(&human).unwrap(),
        deadline: Instant::now() + RELAY_TIMEOUT,
    };
    launch.humans[1].slot.team = LaunchTeam::Team(1);
    assert!(
        !pending.answered_by(&json!({"launch":launch})),
        "without Teams another team still waits"
    );
    launch.options.teams = Some(2);
    assert!(pending.answered_by(&json!({"launch":launch})));
}

/// What `end_room` stops for a guest who left the room, signed out or
/// lost the lobby before the host answered: the answer's time running out
/// restores no settings and shows no notice. Its call sites need an
/// `AppState`, which unit tests cannot build.
#[test]
fn a_room_this_game_is_done_with_waits_for_no_answer() {
    let (mut service, _input, _output) = control_service(None);
    let deadline = Instant::now() + RELAY_TIMEOUT;
    let waiting = |service: &mut LiveService| {
        service.room.published_options = Some((7, json!({})));
        service.room.pending_choices = Some(PendingChoices {
            request_id: 8,
            expected_human: json!({}),
            expected_random: json!({}),
            deadline,
        });
    };
    service.room.accepted_options = Some(json!({}));
    waiting(&mut service);
    assert!(
        service.room.choices_unanswered(deadline),
        "still in the room"
    );
    waiting(&mut service);
    service.room.stop_waiting();
    assert!(!service.room.choices_unanswered(deadline));
    assert!(
        !service.is_room_publication(7),
        "a late refusal changes nothing"
    );
    assert!(
        service.room.accepted_options.is_some(),
        "a late options echo still compares"
    );
}

#[test]
fn loading_return_while_service_is_taken_keeps_finish_before_lobby_admission() {
    let (service, input, _output) = control_service(Some("loading-match"));
    let mut visit = super::super::WolVisit::open(false);
    visit.provider = WolProvider::Online;
    visit.live_service = Some(service);
    let borrowed = visit.live_service.take().unwrap(); //The actual poll boundary.
    let now = Instant::now();
    visit.requests.push(WolServiceRequest::JoinChannel {
        channel: crate::ui::wol_lobby::WolChannel::default(),
        key: String::new(),
    });
    visit.match_returned((None, None, None), now, false);
    visit.match_returned((None, None, None), now, false); //Repeated abort callbacks keep one intent.
    assert!(matches!(
        visit.take_service_requests().as_slice(),
        [
            WolServiceRequest::MatchReturned,
            WolServiceRequest::JoinChannel { .. }
        ]
    ));
    assert!(
        input.try_recv().is_err(),
        "no second service was manufactured"
    );
    assert_eq!(borrowed.match_id.as_deref(), Some("loading-match"));
    visit.live_service = Some(borrowed);
    visit.match_returned((None, None, None), now, false);
    let http::Request::Command {
        id,
        operation,
        payload,
    } = input.try_recv().unwrap()
    else {
        panic!("same retained service must finish the same match");
    };
    assert_eq!(
        (id, operation, payload),
        (5, "finish", json!({"match_id":"loading-match"}))
    );
    let service = visit.live_service.as_mut().unwrap();
    assert!(service.admission_blocked());
    service.finish_completed(id);
    assert!(!service.admission_blocked());
    assert!(matches!(
        input.try_recv(),
        Ok(http::Request::HoldMatch(None))
    ));
    service.clear_cleanup(); //Definitive Offline has retired the authority.
    visit.match_returned((None, None, None), now, false);
    assert!(
        input.try_recv().is_err(),
        "expired authority cannot issue finish"
    );
}

#[test]
fn a_match_from_a_kept_room_asks_for_the_room_back_on_finish() {
    let (service, input, _output) = control_service(Some("room-match"));
    let mut visit = super::super::WolVisit::open(false);
    visit.provider = WolProvider::Online;
    visit.live_service = Some(service);
    let now = Instant::now();
    visit.match_returned((None, None, None), now, true);
    let sent = |input: &Receiver<http::Request>| {
        let http::Request::Command {
            id,
            operation,
            payload,
        } = input.try_recv().unwrap()
        else {
            panic!("finish command");
        };
        assert_eq!(operation, "finish");
        (id, payload)
    };
    let (id, payload) = sent(&input);
    assert_eq!(
        payload,
        json!({"match_id":"room-match","back_to_room":true})
    );
    // A lobby from before rooms came back refuses the field: the room is
    // not kept and the finish goes again without it.
    let service = visit.live_service.as_mut().unwrap();
    assert!(service.finish_failed(id, http::CommandFailure::Rejected(0x8000_4005), now));
    service.poll_cleanup(now + Duration::from_secs(2));
    assert_eq!(sent(&input).1, json!({"match_id":"room-match"}));
}

#[test]
fn closing_the_game_finishes_its_match_with_the_recorded_result() {
    // The game closes during a match from a kept room, before the shell
    // return finished it: the finish carries the result the game
    // recorded for the player who left, and asks for no room.
    let (service, input, _output) = control_service(Some("closed-match"));
    let mut visit = super::super::WolVisit::open(false);
    visit.provider = WolProvider::Online;
    visit.live_service = Some(service);
    let now = Instant::now();
    visit.match_left_for_exit((Some("quit"), None, Some(now)), now);
    let Ok(http::Request::Command {
        operation: "finish",
        payload,
        ..
    }) = input.try_recv()
    else {
        panic!("finish command");
    };
    assert_eq!(
        payload,
        json!({"match_id":"closed-match","result":"quit","ended_ago":0})
    );
}

#[test]
fn a_leave_for_the_started_match_keeps_its_loading_map() {
    let (mut service, input, _output) = control_service(Some("test-match"));
    service.leave_room(Instant::now());
    let http::Request::Command { operation, .. } = input.try_recv().unwrap() else {
        panic!("leave command");
    };
    assert_eq!(operation, "leave");
    assert!(!service.room_closed(), "its own leave's channel_left");
    assert!(service.room_closed(), "a later one closes the room");
    let (mut service, _input, _output) = control_service(None);
    service.leave_room(Instant::now());
    assert!(service.room_closed(), "a leave before any start");
}

/// The worker holds the match through Quick Match's leave, and a new
/// sign-in lets it finish that match with the bearer it logs out before
/// the service drops it.
#[test]
fn the_worker_holds_the_match_through_a_leave_until_the_next_sign_in() {
    let (mut service, input, _output) = control_service(None);
    service.hold_match(Some("quick-match".into()));
    service.leave_room(Instant::now());
    service.leave_completed(5);
    service.sign_in("Tanya".into(), http::Credential::Guest, String::new());
    let sent: Vec<_> = input
        .try_iter()
        .map(|request| match request {
            http::Request::HoldMatch(id) => format!("hold {id:?}"),
            http::Request::Command { operation, .. } => operation.to_owned(),
            http::Request::OpenSession { .. } => "sign in".to_owned(),
            _ => "other".to_owned(),
        })
        .collect();
    assert_eq!(
        sent,
        [
            "hold Some(\"quick-match\")",
            "leave",
            "sign in",
            "hold None"
        ]
    );
}

#[test]
fn finish_retries_the_same_request_and_retains_match_until_acknowledged() {
    let (mut service, input, _output) = control_service(Some("test-match"));
    let now = Instant::now();
    service.begin_finish(now);
    let http::Request::Command {
        id,
        operation,
        payload,
    } = input.try_recv().unwrap()
    else {
        panic!("finish command");
    };
    assert_eq!((id, operation), (5, "finish"));
    assert_eq!(payload["match_id"], "test-match");
    service.finish_failed(id, http::CommandFailure::Retryable, now);
    assert_eq!(service.match_id.as_deref(), Some("test-match"));
    service.poll_cleanup(now + Duration::from_secs(1));
    assert!(input.try_recv().is_err());
    service.poll_cleanup(now + Duration::from_secs(2));
    let http::Request::Command {
        id: retry_id,
        operation,
        payload: retry_payload,
    } = input.try_recv().unwrap()
    else {
        panic!("retry finish command");
    };
    assert_eq!(
        (retry_id, operation, retry_payload),
        (id, "finish", payload)
    );
    service.finish_completed(id + 1);
    assert!(service.finishing.is_some());
    service.finish_completed(id);
    assert!(service.finishing.is_none() && service.match_id.is_none());
    assert!(matches!(
        input.try_recv(),
        Ok(http::Request::HoldMatch(None))
    ));
    service.poll_cleanup(now + Duration::from_secs(60));
    assert!(input.try_recv().is_err());
    service.begin_leave(now);
    let http::Request::Command {
        id: leave_id,
        operation,
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("leave command");
    };
    assert_eq!(operation, "leave");
    assert!(service.admission_blocked());
    service.leave_failed(leave_id, now);
    service.poll_cleanup(now + Duration::from_secs(2));
    let http::Request::Command { id, operation, .. } = input.try_recv().unwrap() else {
        panic!("retry leave command");
    };
    assert_eq!((id, operation), (leave_id, "leave"));
    service.leave_completed(leave_id + 1);
    assert!(service.admission_blocked());
    service.leave_completed(leave_id);
    assert!(!service.admission_blocked());
    service.begin_logout(now);
    let http::Request::Command {
        id: logout_id,
        operation,
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("logout command");
    };
    assert_eq!(operation, "logout");
    service.logout_failed(logout_id, now);
    service.poll_cleanup(now + Duration::from_secs(2));
    let http::Request::Command { id, operation, .. } = input.try_recv().unwrap() else {
        panic!("retry logout command");
    };
    assert_eq!((id, operation), (logout_id, "logout"));
    service.logout_completed(logout_id);
    assert!(!service.admission_blocked());
}
#[test]
fn online_exit_owner_retries_logout_after_the_visit_is_dropped_and_ignores_old_callbacks() {
    let (service, input, output) = control_service(None);
    let mut visit = super::super::WolVisit::open(false);
    visit.live_service = Some(service);
    let now = Instant::now();
    let mut cleanup = OnlineCleanup::default();
    cleanup.leave(&mut visit, now);
    assert!(visit.live_service.is_none());
    drop(visit);
    let http::Request::Command {
        id,
        operation,
        payload,
    } = input.try_recv().unwrap()
    else {
        panic!("logout");
    };
    assert_eq!((operation, payload), ("logout", json!({})));
    assert!(
        output
            .send(http::Response::CommandFailed {
                id,
                operation: "logout",
                failure: http::CommandFailure::Retryable
            })
            .is_ok()
    );
    let _ = cleanup.poll(now);
    let _ = cleanup.poll(now + Duration::from_secs(1));
    assert!(input.try_recv().is_err());
    assert!(cleanup.pending() && !cleanup.logout_acknowledged());
    let _ = cleanup.poll(now + Duration::from_secs(2));
    let http::Request::Command {
        id: retry,
        operation,
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("logout retry");
    };
    assert_eq!((retry, operation), (id, "logout"));
    assert!(
        output
            .send(http::Response::CommandCompleted {
                id: id + 1,
                operation: "logout"
            })
            .is_ok()
    );
    assert!(
        output
            .send(http::Response::Event(wire::Envelope {
                sequence: 1,
                kind: "channel_left".into(),
                payload: json!({})
            }))
            .is_ok()
    );
    assert!(
        output
            .send(http::Response::Authenticated {
                attempt: 0,
                generated_guest: false,
                player_id: "old-player".into(),
                nickname: "OldVisit".into(),
                motd: None,
                sign_in_token: None,
                lobby_visits: false,
                campaign_rooms: false,
            })
            .is_ok()
    );
    let _ = cleanup.poll(now + Duration::from_secs(2));
    let retired = cleanup.retired.as_ref().unwrap();
    assert!(!cleanup.logout_acknowledged());
    assert!(!retired.service.stopping.load(Ordering::Acquire));
    assert!(
        retired.service.player_id.is_empty(),
        "retired login callbacks never enter the active UI/service"
    );
    assert!(
        output
            .send(http::Response::CommandCompleted {
                id,
                operation: "logout"
            })
            .is_ok()
    );
    let _ = cleanup.poll(now + Duration::from_secs(3));
    assert!(
        cleanup
            .retired
            .as_ref()
            .unwrap()
            .service
            .stopping
            .load(Ordering::Acquire)
    );
    assert!(
        cleanup.retired.is_some(),
        "relogin waits for the old worker to stop"
    );
    assert!(cleanup.pending() && cleanup.logout_acknowledged());
    drop(output);
    assert!(cleanup.poll(now + Duration::from_secs(3)).is_none());
    assert!(!cleanup.pending() && cleanup.logout_acknowledged());
}

#[test]
fn online_exit_owner_acknowledges_finish_and_leave_before_logout() {
    let (mut service, input, output) = control_service(Some("departing-match"));
    let now = Instant::now();
    service.begin_leave(now);
    let mut visit = super::super::WolVisit::open(false);
    visit.live_service = Some(service);
    let mut cleanup = OnlineCleanup::default();
    cleanup.leave(&mut visit, now);
    drop(visit);
    let http::Request::Command {
        id: leave,
        operation: "leave",
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("leave");
    };
    let http::Request::Command {
        id: finish,
        operation: "finish",
        payload,
    } = input.try_recv().unwrap()
    else {
        panic!("finish");
    };
    assert_eq!(payload, json!({"match_id":"departing-match"}));
    for (id, operation) in [(leave, "leave"), (finish, "finish")] {
        assert!(
            output
                .send(http::Response::CommandFailed {
                    id,
                    operation,
                    failure: http::CommandFailure::Retryable
                })
                .is_ok()
        );
    }
    let _ = cleanup.poll(now);
    let _ = cleanup.poll(now + Duration::from_secs(2));
    let http::Request::Command {
        id,
        operation,
        payload,
    } = input.try_recv().unwrap()
    else {
        panic!("finish retry");
    };
    assert_eq!(
        (id, operation, payload),
        (finish, "finish", json!({"match_id":"departing-match"}))
    );
    let http::Request::Command { id, operation, .. } = input.try_recv().unwrap() else {
        panic!("leave retry");
    };
    assert_eq!((id, operation), (leave, "leave"));
    assert!(
        output
            .send(http::Response::CommandCompleted {
                id: finish,
                operation: "finish"
            })
            .is_ok()
    );
    let _ = cleanup.poll(now + Duration::from_secs(3));
    assert!(matches!(
        input.try_recv(),
        Ok(http::Request::HoldMatch(None))
    ));
    assert!(
        input.try_recv().is_err(),
        "leave acknowledgement still owns the room drain"
    );
    assert!(
        output
            .send(http::Response::CommandCompleted {
                id: leave,
                operation: "leave"
            })
            .is_ok()
    );
    let _ = cleanup.poll(now + Duration::from_secs(3));
    let http::Request::Command {
        id: logout,
        operation: "logout",
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("logout after cleanup");
    };
    assert!(
        output
            .send(http::Response::CommandCompleted {
                id: logout,
                operation: "logout"
            })
            .is_ok()
    );
    let _ = cleanup.poll(now + Duration::from_secs(4));
    drop(output);
    assert!(cleanup.poll(now + Duration::from_secs(4)).is_none());
}

#[test]
fn online_exit_finishes_a_late_committed_start_and_bounds_worker_shutdown() {
    const LATE: &str = "22222222-2222-2222-2222-222222222222";
    let (service, input, output) = control_service(None);
    let mut visit = super::super::WolVisit::open(false);
    visit.live_service = Some(service);
    let now = Instant::now();
    let mut cleanup = OnlineCleanup::default();
    cleanup.leave(&mut visit, now);
    drop(visit);
    let http::Request::Command {
        id: logout,
        operation: "logout",
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("initial logout");
    };
    assert!(
        output
            .send(http::Response::Event(wire::Envelope {
                sequence: 1,
                kind: "game_started".into(),
                payload: json!({"match_id":LATE})
            }))
            .is_ok()
    );
    assert!(
        output
            .send(http::Response::CommandFailed {
                id: logout,
                operation: "logout",
                failure: http::CommandFailure::Rejected(0x8000_4005)
            })
            .is_ok()
    );
    let _ = cleanup.poll(now);
    assert!(matches!(
        input.try_recv(),
        Ok(http::Request::HoldMatch(Some(id))) if id == LATE
    ));
    let http::Request::Command {
        operation: "finish",
        payload,
        ..
    } = input.try_recv().unwrap()
    else {
        panic!("late match finish");
    };
    assert_eq!(payload, json!({"match_id":LATE}));
    let _ = cleanup.poll(now + EXIT_CLEANUP_TIMEOUT);
    let stopping = Arc::clone(&cleanup.retired.as_ref().unwrap().service.stopping);
    assert!(stopping.load(Ordering::Acquire));
    assert!(!cleanup.logout_acknowledged());
    assert!(
        input.try_recv().is_err(),
        "deadline cannot issue another cleanup attempt"
    );
    // The actual worker skips queued HTTP commands once the existing owner
    // stops it. No server or bearer is needed to exercise this boundary.
    let (send, commands) = mpsc::channel();
    assert!(
        send.send(http::Request::Command {
            id: 1,
            operation: "logout",
            payload: json!({})
        })
        .is_ok()
    );
    drop(send);
    let (events, received) = mpsc::channel();
    http::run("https://127.0.0.1:1".into(), commands, events, stopping);
    assert!(received.try_recv().is_err());
    // The welcome can already have completed logout before Main Menu.
    // The same worker knows it holds no bearer and completes idempotently
    // without touching an unauthenticated endpoint or waiting 90 seconds.
    let (send, commands) = mpsc::channel();
    send.send(http::Request::Command {
        id: 2,
        operation: "logout",
        payload: json!({}),
    })
    .unwrap();
    drop(send);
    let (events, received) = mpsc::channel();
    http::run(
        "https://127.0.0.1:1".into(),
        commands,
        events,
        Arc::new(AtomicBool::new(false)),
    );
    assert!(matches!(
        received.try_recv(),
        Ok(http::Response::CommandCompleted {
            id: 2,
            operation: "logout"
        })
    ));
    assert!(
        cleanup.retired.is_some(),
        "deadline retains the quiescence barrier"
    );
    drop(output);
    assert!(cleanup.poll(now + EXIT_CLEANUP_TIMEOUT).is_none());
}
