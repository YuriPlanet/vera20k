use super::*;

/// `0x0078F000` counts Open rows only within the setup's rows (one more
/// with the observer preference) but open observer rows anywhere.
#[test]
fn republished_limit_counts_open_rows_within_the_setup() {
    use crate::app::match_runtime::session::{MatchSession, NetworkReturnContext, NetworkVacancy};
    let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.command-bar.example.json",
    ))
    .unwrap();
    let mut launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    launch.opponents.clear();
    launch.ai_player_count = 0;
    let mut guest = launch.local_player().clone();
    guest.name = "Guest".into();
    guest.raw_slot_index = Some(1);
    launch.humans.push(guest);
    launch.local_player_index = 1;
    let mut vacancies = [NetworkVacancy::Closed; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT];
    vacancies[2] = NetworkVacancy::Open;
    vacancies[3] = NetworkVacancy::Open;
    vacancies[5] = NetworkVacancy::OpenObserver;
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch.clone(), false, None, false)
        .unwrap();
    session
        .accept_network_host_setup(launch, vacancies, 3, None, false)
        .unwrap();
    assert_eq!(session.network_setup_capacity(), Some(3));
    // Host, guest, row 2 and the observer row; row 3 is past the rows.
    assert_eq!(republished_limit(&session, false), Some(4));
    assert_eq!(republished_limit(&session, true), Some(5));
    assert_eq!(slot_ex_info(&session), (0, 0, 1));
}

#[test]
fn the_modern_lobby_prints_no_blank_row_after_the_welcome() {
    let welcome = "Welcome, General Commander.\nHave fun.";
    assert_eq!(
        motd_rows(welcome, true),
        ["Welcome, General Commander.", "Have fun."]
    );
    assert_eq!(
        motd_rows(welcome, false),
        ["Welcome, General Commander.", "Have fun.", "\n"]
    );
}

#[test]
fn a_joined_game_line_names_the_operator_not_the_first_user() {
    let users = [("Guest".to_owned(), false), ("Host".to_owned(), true)];
    assert_eq!(
        joined_game_line(&users, "Host's_game").as_deref(),
        Some("You have joined Host's game. Host starts it when everyone is here.")
    );
    assert_eq!(
        joined_game_line(&users, "No rush").as_deref(),
        Some("You have joined Host's game (No rush). Host starts it when everyone is here.")
    );
    assert_eq!(joined_game_line(&users[..1], "No rush"), None);
}

#[test]
fn normal_test_service_keeps_the_simplified_ui_and_only_vera20k_online() {
    let mut visit = crate::app::shell_wol::WolVisit::open(false);
    visit.provider = service::WolProvider::TestService;
    assert!(visit.modern_ui());
    let channels: Vec<_> = ["Lob_41_0", "Lob_41_1", "Lob_41_2"]
        .into_iter()
        .map(|name| WolChannel {
            name: name.into(),
            ..Default::default()
        })
        .collect();
    let mut lobby = new_runner(
        Runner::Lobby,
        false,
        false,
        Instant::now(),
        !visit.legacy_ui,
    );
    lobby.set_lobbies(channels.clone());
    assert_eq!(lobby.lobbies.len(), 1);
    assert_eq!(lobby.lobbies[0].name, "Lob_41_2");
    assert_eq!(lobby.ui.page_spec().stacked.len(), 5);
    let names = lobby_base_names(visit.modern_ui(), str::to_owned);
    assert_eq!(WolLobbyRow::joined_name(2, &names), "VERA20K Online");
    visit.set_legacy_ui(true);
    let mut legacy = new_runner(
        Runner::Lobby,
        false,
        false,
        Instant::now(),
        !visit.legacy_ui,
    );
    legacy.set_lobbies(channels);
    assert_eq!(legacy.lobbies.len(), 3);
    let names = lobby_base_names(visit.modern_ui(), str::to_owned);
    assert_eq!(WolLobbyRow::joined_name(2, &names), "TXT_LOB_3");
    assert_eq!(legacy.ui.page_spec().stacked.len(), 8);
}

/// The online Quick Match lobby `Lob_40_0`: the modern lobby leaves it
/// out, and the native list (`0x007806C0`) takes only `Lob_41_` rows,
/// but the modern Quick Match runner must find it.
#[test]
fn only_the_quick_match_runner_lists_the_quick_match_lobby() {
    let channels: Vec<_> = ["Lob_41_2", "Lob_40_0"]
        .into_iter()
        .map(|name| WolChannel {
            name: name.into(),
            ..Default::default()
        })
        .collect();
    let listed = |runner| {
        let mut lobby = new_runner(runner, false, false, Instant::now(), true);
        lobby.set_lobbies(channels.clone());
        lobby
            .lobbies
            .iter()
            .any(|channel| channel.name == "Lob_40_0")
    };
    assert!(!listed(Runner::Lobby));
    assert!(listed(Runner::QuickMatch));
    let rows = crate::ui::wol_lobby::lobby_rows(&channels);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Lob_41_2");
}

fn user(name: &str, flags: u32) -> WolMember {
    WolMember {
        name: name.into(),
        flags,
        ..Default::default()
    }
}

/// A runner whose user list for a channel of `kind` was listed
/// operator first, then by arrival, under the descending alpha sort 1.
fn listed_runner(kind: u32) -> Box<WolLobby> {
    let mut lobby = new_runner(Runner::Lobby, false, false, Instant::now(), false);
    let listed = vec![
        user("Host", MEMBER_OPERATOR),
        user("Mike", 0),
        user("Zed", 0),
    ];
    let channel = WolChannel {
        kind,
        ..Default::default()
    };
    lobby.users_listed(channel, listed, 1, &|_| -1);
    lobby
}

fn names(lobby: &WolLobby) -> Vec<&str> {
    lobby.members.iter().map(|m| m.name.as_str()).collect()
}

#[test]
fn only_the_public_watch_list_reports_its_outage_and_recovery() {
    use WatchListRefresh::*;
    let new = |runner| new_runner(runner, false, false, Instant::now(), true);
    let mut lobby = new(Runner::Lobby);
    // The dialog is hidden while a room covers it.
    assert!(!lobby.shown);
    assert_eq!(
        lobby.watch_refresh_notice(Failed),
        Some("VERA:WatchListStale")
    );
    assert_eq!(lobby.watch_refresh_notice(Failed), None);
    assert_eq!(
        lobby.watch_refresh_notice(Listed),
        Some("VERA:WatchListUpdated")
    );
    assert_eq!(lobby.watch_refresh_notice(Listed), None);
    assert_eq!(
        lobby.watch_refresh_notice(Failed),
        Some("VERA:WatchListStale")
    );
    assert_eq!(lobby.watch_refresh_notice(Unsupported), None);
    assert_eq!(lobby.watch_refresh_notice(Listed), None);
    assert_eq!(new(Runner::QuickMatch).watch_refresh_notice(Failed), None);
    assert_eq!(
        new(Runner::Lobby).watch_refresh_notice(Failed),
        Some("VERA:WatchListStale")
    );
}

/// VERA lists the games again five seconds on, so a new game shows up
/// soon; the lobbies keep native `SetTimer(1, 53000)`.
#[test]
fn a_runner_lists_the_games_again_after_five_seconds() {
    let now = Instant::now();
    let lobby = new_runner(Runner::Lobby, false, false, now, true);
    assert_eq!(lobby.games_at - now, Duration::from_secs(5));
    assert_eq!(lobby.lobbies_at - now, Duration::from_secs(53));
}

/// The user-list sink sorts a chat lobby's users but keeps a game
/// channel's in the listed order, whatever the sort mode.
#[test]
fn game_channel_users_keep_the_listed_order() {
    assert_eq!(names(&listed_runner(0)), ["Host", "Zed", "Mike"]);
    assert_eq!(names(&listed_runner(GAME_CHANNEL)), ["Host", "Mike", "Zed"]);
}

/// VERA online: a page goes to the player the visited lobby, else the
/// channel, lists, under the listed name, and nowhere in a match.
#[test]
fn a_page_targets_only_a_listed_player_outside_a_match() {
    let mut visit = crate::app::shell_wol::WolVisit::open(false);
    let mut lobby = listed_runner(GAME_CHANNEL);
    lobby.visit = Some(LobbyVisit {
        channel: WolChannel::default(),
        members: vec![user("Bob", 0), user("Zed", 0)],
    });
    visit.lobby = Some(lobby);
    assert_eq!(page_target(&visit, "mike"), Some(("Mike", false)));
    assert_eq!(page_target(&visit, "BOB"), Some(("Bob", true)));
    // A roommate who visits too reads the lobby page.
    assert_eq!(page_target(&visit, "zed"), Some(("Zed", true)));
    assert_eq!(page_target(&visit, "Ann"), None);
    visit.in_game = true;
    assert_eq!(page_target(&visit, "Mike"), None);
}

/// A game channel's joiner goes before the first user who weighs less
/// (operator 4, voice 2), or as much with a later name.
#[test]
fn game_channel_joiners_insert_by_weight_then_name() {
    let mut lobby = listed_runner(GAME_CHANNEL);
    lobby.user_joined(user("alice", 0), 1, &|_| -1);
    lobby.user_joined(user("Vic", MEMBER_VOICE), 1, &|_| -1);
    assert_eq!(names(&lobby), ["Host", "Vic", "alice", "Mike", "Zed"]);
}

#[test]
fn online_admission_matches_only_its_current_request_and_bounds_the_wait() {
    let now = Instant::now();
    let mut step = Step::JoiningGame { request: None };
    assert!(step.bind_online_request(7, now));
    assert!(step.accepts_online_result(7));
    assert_eq!(
        step.online_request().unwrap().deadline,
        now + Duration::from_secs(30)
    );
    assert!(step.bind_online_request(8, now));
    assert!(!step.accepts_online_result(7));
    assert!(step.accepts_online_result(8));
    assert!(step.take_leave_continuation().is_none());
    assert!(
        step.accepts_online_result(8),
        "late ChannelLeft must preserve a newer join"
    );
    *step.online_request_mut().unwrap() = None;
    assert!(!step.accepts_online_result(8));
    step = Step::Ready;
    assert!(!step.bind_online_request(9, now));
    // A watch's refusal or silence reaches its step the same way.
    step = Step::Watching {
        match_id: "match".into(),
        request: None,
    };
    assert!(step.bind_online_request(10, now));
    assert!(step.accepts_online_result(10));
}

/// VERA's modern list: each match to watch by its room's name, or the
/// map's when the room has none, with its watchers; a full one greyed.
#[test]
fn a_watch_grant_ends_only_its_request_deadline() {
    let mut lobby = listed_runner(0);
    lobby.step = Step::Watching {
        match_id: "current".into(),
        request: None,
    };
    assert!(lobby.step.bind_online_request(17, Instant::now()));
    lobby.watch_granted("earlier");
    assert!(lobby.step.accepts_online_result(17));
    lobby.watch_granted("current");
    assert!(!lobby.step.accepts_online_result(17));
    assert!(matches!(&lobby.step, Step::Watching { match_id, .. } if match_id == "current"));
}

#[test]
fn a_forming_room_never_advertises_its_partial_teams_as_the_matchup() {
    let mut room = WolRoomSummary {
        teams: Some(vec![0, 2, 0, 1]),
        forming: true,
        ..Default::default()
    };
    fn heading(summary: Option<&WolRoomSummary>) -> Option<String> {
        room_heading("Friday Night", summary, "Oasis")
    }
    assert_eq!(
        heading(Some(&room)),
        Some(format!(
            "Friday Night - {} - Oasis",
            crate::i18n::tr("VERA:LobbyTeamsForming")
        ))
    );
    room.teams = Some(Vec::new());
    room.forming = false;
    assert_eq!(
        heading(Some(&room)),
        Some(format!(
            "Friday Night - {} - Oasis",
            crate::i18n::tr("VERA:LobbyFfa")
        ))
    );
    assert_eq!(heading(None).as_deref(), Some("Friday Night - Oasis"));
}

#[test]
fn watch_rows_name_each_match_with_its_watchers() {
    let watch = |match_id: &str, name: &str, watchers| service::WolMatch {
        match_id: match_id.into(),
        name: name.into(),
        map: "Little Big Lake".into(),
        watchers,
        max_watchers: 10,
        names: Vec::new(),
        summary: None,
    };
    let mut matches = [watch("m1", "Ann's game", 9), watch("m2", "", 10)];
    matches[0].summary = Some(WolRoomSummary {
        map: Some("Snow Valley".into()),
        teams: Some(vec![0, 4, 0, 4]),
        forming: false,
        observers: Some(2),
        computers: Some(1),
    });
    let rows = watch_rows(&matches, "Watch");
    let shown: Vec<_> = rows
        .iter()
        .map(|row| (row.name.as_str(), row.text.as_str(), row.greyed))
        .collect();
    assert_eq!(
        shown,
        [
            ("m1", "Ann's game - 4v4 - Snow Valley", false),
            ("m2", "Little Big Lake 10/10", true)
        ]
    );
    assert!(rows.iter().enumerate().all(|(record, row)| {
        row.record == record && row.watch && row.tag.as_deref() == Some("Watch")
    }));
}

#[test]
fn join_errors_follow_the_on_join_sink() {
    assert_eq!(join_error_reply(0x8004_006C, true), JoinErrorReply::Ignore);
    assert_eq!(
        join_error_reply(0x8004_0070, true),
        JoinErrorReply::Box {
            key: "TXT_GAME_CLOSED",
            close: false,
            forget: true,
            next_lobby: false
        }
    );
    assert_eq!(
        join_error_reply(0x8004_0071, true),
        JoinErrorReply::Box {
            key: "TXT_BADPASS",
            close: false,
            forget: false,
            next_lobby: false
        }
    );
    // A full channel ends the runner only when nothing was pending.
    for (pending, close) in [(true, false), (false, true)] {
        assert_eq!(
            join_error_reply(0x8004_006E, pending),
            JoinErrorReply::Box {
                key: "TXT_CHANNEL_FULL",
                close,
                forget: false,
                next_lobby: false
            }
        );
    }
    assert_eq!(
        join_error_reply(0x8004_0072, true),
        JoinErrorReply::Box {
            key: "TXT_JOINBAN",
            close: true,
            forget: false,
            next_lobby: true
        }
    );
    assert_eq!(
        join_error_reply(KEPT_OUT_JOIN_ERROR, true),
        JoinErrorReply::Box {
            key: "TXT_JOINBAN",
            close: false,
            forget: false,
            next_lobby: false
        }
    );
    assert_eq!(join_error_reply(0x8000_4005, true), JoinErrorReply::Chat);
    assert_eq!(join_error_reply(0x8004_006D, false), JoinErrorReply::Chat);
}

#[test]
fn start_game_now_splits_users_by_their_first_two_squads() {
    let member = |name: &str, squad| WolMember {
        name: name.to_string(),
        squad,
        ..WolMember::default()
    };
    let members = [
        member("a", 0),
        member("b", 7),
        member("c", 9),
        member("d", 7),
        member("e", 11),
        member("f", 9),
    ];
    let mut channel = created_channel("Tester", true, false, 1024, 0, String::new());
    let terms = start_terms_for(&channel, &members);
    assert_eq!(terms.tournament, 2);
    assert_eq!(
        terms.squad_lists,
        [
            vec!["a".to_string(), "b".into(), "d".into()],
            vec!["c".to_string(), "f".into()]
        ]
    );
    // Outside a clan room the lists stay empty and the value is raw.
    channel.game_flags = 0;
    channel.tournament = 3;
    let terms = start_terms_for(&channel, &members);
    assert_eq!(terms.tournament, 3);
    assert!(terms.squad_lists.iter().all(Vec::is_empty));
}

#[test]
fn clan_start_needs_two_equal_squads_without_the_observer() {
    let member = |name: &str, squad| WolMember {
        name: name.to_string(),
        squad,
        ..WolMember::default()
    };
    let two = [member("a", 7), member("b", 9)];
    assert!(clan_split_even(&two, None));
    // An observer outside any squad is skipped by name, without case.
    let watched = [member("a", 7), member("Obs", 0), member("b", 9)];
    assert!(clan_split_even(&watched, Some("obs")));
    assert!(!clan_split_even(&watched, None));
    // One squad, a third squad, or uneven sides refuse the start.
    assert!(!clan_split_even(&[member("a", 7), member("b", 7)], None));
    assert!(!clan_split_even(
        &[member("a", 7), member("b", 9), member("c", 11)],
        None
    ));
    assert!(!clan_split_even(
        &[member("a", 7), member("b", 7), member("c", 9)],
        None
    ));
}

#[test]
fn the_user_list_batches_unranked_names_with_its_squad_requests() {
    let member = |name: String, squad| WolMember {
        name,
        squad,
        ..WolMember::default()
    };
    let mut listed: Vec<WolMember> = (1..=104).map(|n| member(format!("User{n}"), 0)).collect();
    listed[1].squad = 7;
    listed[23].squad = 9;
    listed[23].clan_tag = "KN".into();
    listed[30].squad = 11;
    let requests = listed_requests(&listed, |key| key == "user3");
    let names = |range: std::ops::RangeInclusive<usize>, skip: usize| {
        range
            .filter(|n| *n != skip)
            .map(|n| format!("user{n}"))
            .collect::<Vec<_>>()
    };
    // A batch leaves at its 21st name; users from the 100th on and a
    // ranked user add none, and a known tag asks for no squad.
    assert_eq!(
        requests,
        [
            ListedRequest::Squad(7),
            ListedRequest::Ladder(names(1..=22, 3)),
            ListedRequest::Squad(11),
            ListedRequest::Ladder(names(23..=43, 0)),
            ListedRequest::Ladder(names(44..=64, 0)),
            ListedRequest::Ladder(names(65..=85, 0)),
            ListedRequest::Ladder(names(86..=99, 0)),
        ]
    );
}

#[test]
fn a_ladder_event_from_another_season_empties_the_tables() {
    let mut globals = WolLobbyGlobals::default();
    let fill = |globals: &mut WolLobbyGlobals| {
        globals
            .ladder
            .insert("tanya".into(), service::WolLadderRecord::default());
        globals
            .high_scores
            .insert("tanya".into(), service::WolHighScoreRecord::default());
    };
    // The first event only notes its season; the same season keeps the records.
    assert!(!globals.ladder_season(Some(1)));
    fill(&mut globals);
    assert!(!globals.ladder_season(Some(1)));
    assert_eq!((globals.ladder.len(), globals.high_scores.len()), (1, 1));
    // A new season, and ranked play stopping, each drop the season before's.
    assert!(globals.ladder_season(Some(2)));
    assert!(globals.ladder.is_empty() && globals.high_scores.is_empty());
    fill(&mut globals);
    assert!(globals.ladder_season(None));
    assert!(globals.ladder.is_empty() && globals.high_scores.is_empty());
}

#[test]
fn the_created_record_carries_name_users_and_flags() {
    let channel = created_channel("Tester", false, false, 1024, 3, "25,7,0,0,1,x".into());
    assert_eq!(channel.name, "Tester's_game");
    assert_eq!((channel.kind, channel.max_users), (GAME_CHANNEL, 30));
    assert_eq!((channel.game_flags, channel.flags), (3, 0));
    assert_eq!(channel.ex_info, "25,7,0,0,1,x");
    let limited = created_channel("Tester", true, true, 800, 0, String::new());
    assert_eq!(limited.game_flags, GAME_CLAN | GAME_LIMIT_800);
    let limited = created_channel("Tester", false, true, 640, 1, String::new());
    assert_eq!(limited.game_flags, GAME_LIMIT_640 + 1);
    // A limited game's screen admission matches its bit.
    assert_eq!(
        created_channel("a", false, true, 800, 0, String::new()).screen_limit(),
        800
    );
    assert_eq!(
        created_channel("a", false, true, 640, 0, String::new()).screen_limit(),
        640
    );
}

/// The Quick Match game carries Create's exInfo, which the guest's
/// `0x0079BA60` checks before it joins.
#[test]
fn a_quick_match_guest_admits_the_hosts_game() {
    let ex_info = ExInfo {
        version: EX_INFO_VERSION,
        hash: 7,
        ai_players: 0,
        observer_taken: 0,
        observer_open: 0,
        scenario: "x".into(),
    };
    let channel = quick_match_channel("Host", 1, 2, ex_info.write());
    assert_eq!(channel.name, "Host's_game");
    assert_eq!(
        (channel.max_users, channel.tournament, channel.game_flags),
        (2, 1, GAME_QUICK_MATCH + 2)
    );
    let context = AdmissionContext {
        quick_match: true,
        clan_id: 0,
        local_hash: Some(7),
        vera_rooms: false,
    };
    assert_eq!(admit(&channel, false, context), Ok(()));
}
