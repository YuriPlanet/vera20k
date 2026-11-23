use super::{MatchSession, NetworkRoom, SessionMode};

pub(super) fn network_fixture() -> crate::skirmish_launch::SkirmishLaunchSession {
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
    guest.slot.color_index = 2;
    launch.humans.push(guest);
    launch.local_player_index = 1;
    launch
}

#[test]
fn a_connection_watcher_has_no_playing_source_or_load_vote() {
    use crate::app::match_runtime::network_match;

    let launch = network_fixture();
    let followed = launch.local_player_index;
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch.clone(), false, None, false)
        .unwrap();
    let endpoint = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 30_001);
    session.admit_local_network_watcher(endpoint).unwrap();
    assert!(session.set_network_player_ready(followed, true).is_err());
    session.prepare_wol_timing([0; 2], false).unwrap();
    session.freeze_network_start(13, None).unwrap();
    assert!(session.local_connection_watches());
    assert_eq!(network_match::local_source(&session), None);
    assert!(network_match::nodes(&session).is_empty());
    assert_eq!(session.network_setup().unwrap().humans, launch.humans);
    assert_eq!(session.network_connected_source_indices(), &[0, 1]);
    assert_eq!(session.network_source_sender(2), None);
    session.report_network_loading_progress(100);
    assert!(session.take_network_loading_progress_reports().is_empty());

    assert!(session.disconnect_network_participant(followed));
    assert!(session.local_connection_watches());
    assert_eq!(network_match::local_source(&session), None);
    assert_eq!(session.network_connected_source_indices(), &[0]);
    assert!(session.disconnect_network_watcher(endpoint));
    assert!(!session.local_connection_watches());
    assert_eq!(session.network_connected_source_indices(), &[0]);
}

#[test]
fn thirty_player_setup_keeps_last_row_identity_and_cooperative_capacity() {
    let mut launch = network_fixture();
    let template = launch.humans[0].clone();
    launch.humans = (0..30)
        .map(|row| {
            let mut human = template.clone();
            human.name = format!("Player{row}");
            human.raw_slot_index = Some(row);
            human
        })
        .collect();
    launch.local_player_index = 29;
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch.clone(), false, None, false)
        .unwrap();
    session
        .accept_network_host_setup(
            launch.clone(),
            [super::NetworkVacancy::Open; 30],
            120,
            None,
            false,
        )
        .unwrap();
    assert_eq!(session.network_setup_capacity(), Some(30));
    assert!(session.network_free_row(false).is_none());
    let first = session.network_row_choices(0).unwrap();
    let mut last = session.network_row_choices(29).unwrap();
    last.start_position = crate::skirmish_launch::LaunchStartPosition::Position(29);
    session.set_network_row_choices(29, last.clone()).unwrap();
    session.set_network_player_ready(29, true).unwrap();
    session.remove_network_participant(7).unwrap();
    let current = session.network_setup().unwrap();
    assert_eq!(current.local_player_index, 28);
    assert_eq!(current.local_player().name, "Player29");
    assert_eq!(session.network_row_choices(29), Some(last));
    assert_eq!(session.network_row_choices(0), Some(first));
    assert!(session.network_player_ready(28));
    let cooperative = crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(
        &crate::skirmish_modes::stock_skirmish_modes()
            .into_iter()
            .find(|mode| mode.class.uses_cooperative_setup())
            .unwrap(),
    );
    assert_eq!(
        super::row_capacity(SessionMode::Wol, NetworkRoom::default(), &cooperative, 30),
        2
    );
}

#[test]
fn extended_player_admission_stops_at_capacity_and_shrink_closes_hidden_rows() {
    use super::{NetworkRowKind, NetworkVacancy};
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    let template = launch.humans[0].clone();
    let mode = launch.mode.clone();
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch, true, None, false)
        .unwrap();
    session
        .select_network_map(mode.clone(), "NINE.MAP".into(), 9)
        .unwrap();
    for row in 2..9 {
        assert_eq!(session.network_free_row(false), Some(row));
        let mut human = template.clone();
        human.name = format!("Player{row}");
        human.raw_slot_index = Some(row);
        session.add_network_participant(human).unwrap();
    }
    assert!(session.network_free_row(false).is_none());
    assert!(session.network_no_open_player_row());

    session
        .select_network_map(mode.clone(), "THIRTY.MAP".into(), 30)
        .unwrap();
    session
        .set_network_row_kind(
            12,
            NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
        )
        .unwrap();
    let mut last = template;
    last.name = "Last".into();
    last.raw_slot_index = Some(29);
    session.add_network_participant(last).unwrap();
    let removed = session
        .select_network_map(mode.clone(), "NINE.MAP".into(), 9)
        .unwrap();
    assert_eq!(
        removed
            .iter()
            .map(|human| human.name.as_str())
            .collect::<Vec<_>>(),
        ["Last"]
    );
    assert_eq!(session.network_vacancy(12), Some(NetworkVacancy::Closed));
    assert_eq!(session.network_vacancy(29), Some(NetworkVacancy::Closed));
    assert!(session.network_display_ai(12).is_none());
    assert!(session.network_free_row(false).is_none());
    session
        .set_network_row_kind(29, NetworkRowKind::OpenObserver)
        .unwrap();
    assert_eq!(session.network_free_row(true), Some(29));
    session
        .select_network_map(mode, "THIRTY.MAP".into(), 30)
        .unwrap();
    assert_eq!(session.network_free_row(false), Some(9));
    assert_eq!(session.network_free_row(true), Some(29));
}

#[test]
fn extended_host_observer_keeps_the_extra_player_row() {
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    launch.humans[0].set_observer(true);
    let template = launch.humans[1].clone();
    let mode = launch.mode.clone();
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(launch, true, Some(0), false)
        .unwrap();
    session
        .select_network_map(mode, "NINE.MAP".into(), 9)
        .unwrap();
    assert_eq!(session.network_observer_row(), Some(0));
    for row in 2..=9 {
        assert_eq!(session.network_free_row(false), Some(row));
        let mut human = template.clone();
        human.name = format!("Player{row}");
        human.raw_slot_index = Some(row);
        session.add_network_participant(human).unwrap();
    }
    assert!(session.network_free_row(false).is_none());
    assert!(session.network_no_open_player_row());
    assert!(!session.network_over_capacity());
}

/// A VERA room (`several_observers`) on FOUR.MAP whose host keeps a
/// spectator row open; the guest plays from row 1.
fn vera_room() -> MatchSession {
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    let mode = launch.mode.clone();
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch, true, Some(7), true)
        .unwrap();
    session
        .select_network_map(mode, "FOUR.MAP".into(), 4)
        .unwrap();
    session.set_network_spectators(true).unwrap();
    session
}

/// The rows open for an observer.
fn observer_rows(session: &MatchSession) -> Vec<u8> {
    (0..30)
        .filter(|row| session.network_vacancy(*row) == Some(super::NetworkVacancy::OpenObserver))
        .collect()
}

/// Whether the guest watches, and its row.
fn guest(session: &MatchSession) -> (bool, Option<u8>) {
    let guest = &session.network_setup().unwrap().humans[1];
    (guest.is_observer(), guest.raw_slot_index)
}

/// An online guest takes its AI rows from each of the host's options,
/// which stand in for the roster `'L'` (`0x5E8EA0`): an AI the host adds
/// after the guest joined shows on the guest's row, one it removes leaves
/// it, and AI the host does not show stay hidden.
#[test]
fn an_online_guest_shows_the_ai_the_host_adds_and_removes() {
    use super::{NetworkRowKind, NetworkVacancy};
    use crate::skirmish_launch::AiDifficulty;
    let mut host = vera_room();
    let mut guest = MatchSession::default();
    guest.enter_network(super::NetworkReturnContext::Wol);
    guest
        .accept_network_setup(network_fixture(), false, Some(7), true)
        .unwrap();
    // The host's options as the online service sends them: a row
    // without a vacancy (a person or an AI) goes as Closed.
    let send = |host: &MatchSession, guest: &mut MatchSession| {
        let mut launch = host.network_setup().unwrap().clone();
        launch.local_player_index = 1;
        let vacancies: [NetworkVacancy; 30] = std::array::from_fn(|row| {
            host.network_vacancy(row as u8)
                .unwrap_or(NetworkVacancy::Closed)
        });
        guest
            .accept_network_host_setup(launch, vacancies, 4, Some(7), false)
            .unwrap();
    };
    host.choose_network_row_kind(2, NetworkRowKind::Ai(AiDifficulty::Hard))
        .unwrap();
    send(&host, &mut guest);
    assert_eq!(guest.network_display_ai(2), Some(AiDifficulty::Hard));
    assert_eq!(guest.network_vacancy(2), None);
    // Cooperative's start AI (`0x5C30C0`) stays behind the host's Open row.
    let mut opponents = host.network_setup().unwrap().opponents.clone();
    let mut start_ai = opponents[0].clone();
    start_ai.raw_slot_index = 3;
    opponents.push(start_ai);
    host.apply_network_ai_preparation(None, Some(opponents))
        .unwrap();
    send(&host, &mut guest);
    assert_eq!(guest.network_display_ai(3), None);
    assert_eq!(guest.network_vacancy(3), host.network_vacancy(3));
    host.choose_network_row_kind(2, NetworkRowKind::Open)
        .unwrap();
    send(&host, &mut guest);
    assert_eq!(guest.network_display_ai(2), None);
    assert_eq!(guest.network_vacancy(2), Some(NetworkVacancy::Open));
}

/// The spectator row a VERA room keeps open: a guest who watches takes
/// it and the next opens. With no seat open it keeps watching, and a
/// LAN guest plays again from the first open seat.
#[test]
fn a_vera_room_guest_watches_and_plays_again() {
    use super::NetworkRowKind;
    let mut session = vera_room();
    assert_eq!(observer_rows(&session), [4]);
    let slot = session.network_setup().unwrap().humans[1].slot.clone();
    session
        .accept_network_peer_choices(1, slot.clone(), true, None)
        .unwrap();
    assert_eq!(guest(&session), (true, Some(4)));
    assert_eq!(observer_rows(&session), [5]);
    for row in 1..4 {
        session
            .choose_network_row_kind(row, NetworkRowKind::Closed)
            .unwrap();
    }
    session
        .accept_lan_peer_choices(1, slot.clone(), false)
        .unwrap();
    assert_eq!(guest(&session), (true, Some(4)));
    session
        .choose_network_row_kind(2, NetworkRowKind::Open)
        .unwrap();
    session.accept_lan_peer_choices(1, slot, false).unwrap();
    assert_eq!(guest(&session), (false, Some(2)));
    assert_eq!(observer_rows(&session), [4]);
}

/// Spectators who ask to play while no seat is free wait in the join
/// queue in order: the first takes the next free seat, and asking to
/// watch or leaving the room gives up the place.
#[test]
fn a_vera_room_queues_its_spectators_for_the_seats_that_free() {
    use super::NetworkRowKind;
    let mut session = vera_room();
    let slot = session.network_setup().unwrap().humans[1].slot.clone();
    session
        .accept_network_peer_choices(1, slot.clone(), true, None)
        .unwrap();
    let mut third = session.network_setup().unwrap().humans[1].clone();
    third.name = "Third".into();
    third.raw_slot_index = Some(5);
    session.add_network_participant(third).unwrap();
    for row in 1..4 {
        session
            .choose_network_row_kind(row, NetworkRowKind::Closed)
            .unwrap();
    }
    for index in [2, 1] {
        session
            .accept_network_peer_choices(index, slot.clone(), false, None)
            .unwrap();
    }
    assert_eq!(session.network_queue(), ["Third", "Guest"]);
    assert_eq!(session.network_queue_next(), None);
    session
        .choose_network_row_kind(2, NetworkRowKind::Open)
        .unwrap();
    assert_eq!(session.network_queue_next(), Some(2));
    session
        .accept_network_peer_choices(2, slot.clone(), false, None)
        .unwrap();
    assert_eq!(session.network_queue_place(1), Some(1));
    session
        .accept_network_peer_choices(1, slot.clone(), true, None)
        .unwrap();
    assert!(session.network_queue().is_empty());
    session
        .accept_network_peer_choices(1, slot, false, None)
        .unwrap();
    session.remove_network_participant(1).unwrap();
    assert!(session.network_queue().is_empty());
}

/// AI play while people watch, unlike in gamemd's room (`0x5EC9F0`'s
/// TXT_NO_AI_WITH_OBSERVER): an AI keeps the spectator row open, and a
/// guest asking to watch takes it. A room without spectators closes it;
/// it follows a spectator who joins and leaves.
#[test]
fn a_vera_room_keeps_its_spectator_row_for_ai_and_closes_it_for_the_host() {
    use super::NetworkRowKind;
    let mut session = vera_room();
    session
        .choose_network_row_kind(
            2,
            NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
        )
        .unwrap();
    assert_eq!(observer_rows(&session), [4]);
    let slot = session.network_setup().unwrap().humans[1].slot.clone();
    session
        .accept_network_peer_choices(1, slot.clone(), true, None)
        .unwrap();
    assert_eq!(guest(&session), (true, Some(4)));
    assert_eq!(session.network_setup().unwrap().opponents.len(), 1);
    session
        .accept_network_peer_choices(1, slot, false, None)
        .unwrap();
    assert_eq!(guest(&session), (false, Some(1)));
    session
        .choose_network_row_kind(2, NetworkRowKind::Open)
        .unwrap();
    assert_eq!(observer_rows(&session), [4]);
    session.set_network_spectators(false).unwrap();
    assert!(observer_rows(&session).is_empty());
    session.set_network_spectators(true).unwrap();
    assert_eq!(observer_rows(&session), [4]);
    let mut spectator = session.network_setup().unwrap().humans[1].clone();
    spectator.name = "Spectator".into();
    spectator.raw_slot_index = Some(4);
    spectator.set_observer(true);
    let index = session.add_network_participant(spectator).unwrap();
    assert_eq!(observer_rows(&session), [5]);
    session.remove_network_participant(index).unwrap();
    assert_eq!(observer_rows(&session), [4]);
}

/// A VERA room seats its map's players. A slot its host removes takes no
/// joiner, who watches instead, or finds the game full once the room
/// takes no spectators; the next map opens the slot again.
#[test]
fn a_vera_room_seats_its_map_and_reopens_removed_slots_for_the_next() {
    use super::{NetworkRowKind, NetworkVacancy};
    let mut session = vera_room();
    let mode = session.network_setup().unwrap().mode.clone();
    assert_eq!(session.network_player_row_limit(), 4);
    for row in [3, 2] {
        session
            .choose_network_row_kind(row, NetworkRowKind::Closed)
            .unwrap();
    }
    assert_eq!(session.network_join_row(false), Some((4, true)));
    // Its Max players counts the spectators: two people fill it.
    session.set_network_people_limit(2).unwrap();
    assert_eq!(session.network_join_row(false), None);
    assert!(session.network_room_full());
    session.set_network_people_limit(30).unwrap();
    session.set_network_spectators(false).unwrap();
    assert_eq!(session.network_join_row(false), None);
    session
        .select_network_map(mode, "EIGHT.MAP".into(), 8)
        .unwrap();
    assert!((2..8).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Open)));
}

/// A VERA room stays open until its host's Max players: with every seat
/// taken, a newcomer gets the last AI's seat. With spectators off its
/// limit is the people in it and the seats AI hold or leave open.
#[test]
fn a_vera_room_gives_a_newcomer_the_last_ai_seat_until_its_limit() {
    use super::NetworkRowKind;
    let easy = NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy);
    let mut session = vera_room();
    for row in [2, 3] {
        session.choose_network_row_kind(row, easy).unwrap();
    }
    assert!(session.network_no_open_player_row());
    // Spectators fill the rows the people and AI leave.
    assert_eq!(session.network_people_capacity(), Some(28));
    session.set_network_spectators(false).unwrap();
    assert_eq!(session.network_people_capacity(), Some(4));
    // At its Max players the AI keep their seats.
    session.set_network_people_limit(2).unwrap();
    assert_eq!(session.network_people_capacity(), Some(2));
    assert_eq!(session.network_newcomer_ai_seat(), None);
    assert_eq!(session.free_network_seat_for_newcomer().unwrap(), None);
    session.set_network_people_limit(30).unwrap();
    assert_eq!(session.network_newcomer_ai_seat(), Some(3));
    assert_eq!(session.free_network_seat_for_newcomer().unwrap(), Some(3));
    assert_eq!(session.network_free_row(false), Some(3));
    let ai_rows: Vec<u8> = session
        .network_setup()
        .unwrap()
        .opponents
        .iter()
        .map(|ai| ai.raw_slot_index)
        .collect();
    assert_eq!(ai_rows, [2]);
}

/// VERA's host may watch, from the spectator row, and play again from a
/// free seat, as a guest would.
#[test]
fn a_vera_room_host_watches_and_plays_again() {
    let mut session = vera_room();
    let host = |session: &MatchSession| {
        let host = &session.network_setup().unwrap().humans[0];
        (host.is_observer(), host.raw_slot_index)
    };
    let slot = session.network_setup().unwrap().humans[0].slot.clone();
    session.move_network_host(slot.clone(), true).unwrap();
    assert_eq!(host(&session), (true, Some(4)));
    assert_eq!(observer_rows(&session), [5]);
    session.move_network_host(slot, false).unwrap();
    assert_eq!(host(&session), (false, Some(0)));
}

/// A larger map's players take the spectators' rows: they move on past
/// the new players, and stay there for a smaller map.
#[test]
fn a_vera_room_moves_its_spectators_past_new_players() {
    use super::NetworkVacancy;
    let mut session = vera_room();
    let mode = session.network_setup().unwrap().mode.clone();
    let slot = session.network_setup().unwrap().humans[1].slot.clone();
    session
        .accept_network_peer_choices(1, slot, true, None)
        .unwrap();
    let mut spectator = session.network_setup().unwrap().humans[1].clone();
    spectator.name = "Spectator".into();
    spectator.raw_slot_index = Some(5);
    session.add_network_participant(spectator).unwrap();
    let rows = |session: &MatchSession| {
        session.network_setup().unwrap().humans[1..]
            .iter()
            .map(|human| human.raw_slot_index)
            .collect::<Vec<_>>()
    };
    session
        .select_network_map(mode.clone(), "TWELVE.MAP".into(), 12)
        .unwrap();
    assert_eq!(rows(&session), [Some(12), Some(13)]);
    assert!((1..12).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Open)));
    assert_eq!(observer_rows(&session), [14]);
    let removed = session
        .select_network_map(mode, "FOUR.MAP".into(), 4)
        .unwrap();
    assert!(removed.is_empty());
    assert_eq!(rows(&session), [Some(12), Some(13)]);
    assert_eq!(observer_rows(&session), [14]);
}

#[test]
fn a_vera_room_host_who_observes_leaves_every_player_row() {
    use super::NetworkVacancy;
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    launch.humans[0].set_observer(true);
    let mode = launch.mode.clone();
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(launch, true, Some(0), true)
        .unwrap();
    // The host watches from row 0; rows 1..=8 seat eight players.
    assert_eq!(session.network_player_row_limit(), 9);
    assert_eq!(session.network_vacancy(8), Some(NetworkVacancy::Open));
    assert_eq!(session.network_vacancy(9), Some(NetworkVacancy::Closed));
    session
        .select_network_map(mode.clone(), "FOUR.MAP".into(), 4)
        .unwrap();
    assert!((5..30).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Closed)));
    session
        .select_network_map(mode, "TWELVE.MAP".into(), 12)
        .unwrap();
    assert!((5..13).all(|row| session.network_vacancy(row) == Some(NetworkVacancy::Open)));
    assert_eq!(session.network_vacancy(13), Some(NetworkVacancy::Closed));
}

#[test]
fn a_vera_room_profile_keeps_spectator_rows_open() {
    use super::{NetworkRowKind, NetworkVacancy};
    let mut session = vera_room();
    session
        .set_network_row_kind(3, NetworkRowKind::Closed)
        .unwrap();
    // The host closed seat 3; rows 4.., the open one among them, are the
    // room's spectator rows.
    assert_eq!(observer_rows(&session), [4]);
    let remembered = session.network_remembered_vacancies();
    assert_eq!(remembered[3], Some(NetworkVacancy::Closed));
    assert!(
        remembered[4..]
            .iter()
            .all(|row| *row == Some(NetworkVacancy::Open))
    );
}

/// A newcomer in row 2, copied from the guest.
fn newcomer(session: &mut MatchSession) {
    let mut newcomer = session.network_setup().unwrap().humans[1].clone();
    newcomer.name = "Newcomer".into();
    newcomer.raw_slot_index = Some(2);
    session.add_network_participant(newcomer).unwrap();
}

/// The people's teams.
fn teams(session: &MatchSession) -> Vec<crate::skirmish_launch::LaunchTeam> {
    let humans = &session.network_setup().unwrap().humans;
    humans.iter().map(|human| human.slot.team).collect()
}

/// With Teams set, every seated player of a VERA room is on one of its
/// teams: setting it deals them out in row order, a newcomer joins the
/// team with the fewest players, whatever it picked, and a new map keeps
/// them where they are.
#[test]
fn a_vera_room_with_teams_keeps_every_player_on_a_team() {
    use crate::skirmish_launch::LaunchTeam::Team;
    let mut session = vera_room();
    session.set_network_teams(Some(2)).unwrap();
    assert_eq!(teams(&session), [Team(0), Team(1)]);
    let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
    pick.team = Team(0);
    session
        .accept_network_peer_choices(1, pick, false, None)
        .unwrap();
    newcomer(&mut session);
    assert_eq!(teams(&session), [Team(0), Team(0), Team(1)]);
    let mode = session.network_setup().unwrap().mode.clone();
    session
        .select_network_map(mode, "EIGHT.MAP".into(), 8)
        .unwrap();
    assert_eq!(teams(&session), [Team(0), Team(0), Team(1)]);
}

/// With Teams set a VERA room shares bases, so no team is ever full: a
/// spectator asking to play once the map's four seats are taken sits
/// past them, and a smaller map keeps everyone seated and takes an AI
/// past its seats too, with a spectator seat after it. Teams Off makes
/// the people past the seats watch
/// from there, the AI leave and a start two players held go Random.
#[test]
fn a_vera_room_with_teams_seats_more_than_its_map() {
    use crate::skirmish_launch::LaunchTeam::Team;
    let seated = |session: &MatchSession| -> Vec<bool> {
        let humans = &session.network_setup().unwrap().humans;
        humans.iter().map(|human| !human.is_observer()).collect()
    };
    let mut session = vera_room();
    session.set_network_teams(Some(2)).unwrap();
    for (name, row) in [("Third", 2), ("Fourth", 3), ("Fifth", 4)] {
        let mut human = session.network_setup().unwrap().humans[1].clone();
        human.name = name.into();
        human.raw_slot_index = Some(row);
        human.set_observer(row == 4);
        session.add_network_participant(human).unwrap();
    }
    let mut pick = session.network_setup().unwrap().humans[4].slot.clone();
    pick.team = Team(0);
    session
        .accept_network_peer_choices(4, pick, false, None)
        .unwrap();
    assert_eq!(seated(&session), [true; 5]);
    assert_eq!(teams(&session)[4], Team(0));
    let mode = session.network_setup().unwrap().mode.clone();
    session
        .select_network_map(mode, "TWO.MAP".into(), 2)
        .unwrap();
    assert_eq!(seated(&session), [true; 5]);
    let seat = session.network_ai_seat().unwrap();
    assert!(seat >= 2);
    session
        .choose_network_row_kind(
            seat,
            super::NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
        )
        .unwrap();
    // People may still watch, from the row after the AI's.
    assert_eq!(observer_rows(&session), [seat + 1]);
    let start = crate::skirmish_launch::LaunchStartPosition::Position(0);
    session
        .set_network_seat_picks(&[
            (0, super::SeatPick::Start(start)),
            (1, super::SeatPick::Start(start)),
        ])
        .unwrap();
    session.set_network_teams(None).unwrap();
    assert_eq!(seated(&session), [true, true, false, false, false]);
    let launch = session.network_setup().unwrap();
    assert!(launch.opponents.is_empty());
    assert!(launch.humans[..2].iter().all(|human| {
        human.slot.start_position == crate::skirmish_launch::LaunchStartPosition::Auto
    }));
}

/// A VERA room has up to 15 teams, A to O, and a guest's online pick
/// past gamemd's D stays until Teams goes Off, which makes the room
/// free-for-all: everyone on no team, a pick of none stays seated and a
/// pick of a team stays off it.
#[test]
fn a_vera_room_keeps_a_guest_past_d_until_teams_is_off() {
    use crate::skirmish_launch::LaunchTeam::{self, Team};
    let mut session = vera_room();
    session.set_network_teams(Some(15)).unwrap();
    assert_eq!(teams(&session), [Team(0), Team(1)]);
    let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
    pick.team = Team(14);
    assert!(!session.admit_network_team(1, &mut pick, &mut false));
    session
        .accept_network_peer_choices(1, pick, false, None)
        .unwrap();
    assert_eq!(teams(&session), [Team(0), Team(14)]);
    session.set_network_teams(None).unwrap();
    assert_eq!(teams(&session), [LaunchTeam::None, LaunchTeam::None]);
    let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
    let mut observer = false;
    assert!(!session.admit_network_team(1, &mut pick, &mut observer));
    assert!(!observer);
    pick.team = Team(1);
    assert!(session.admit_network_team(1, &mut pick, &mut observer));
    assert_eq!((pick.team, observer), (LaunchTeam::None, false));
}

/// A VERA room with Teams set seats people only on a team: a newcomer
/// comes in watching, a player picking no team starts watching and a
/// spectator asking to play on none keeps watching, and a team's Join
/// seats it on that team.
#[test]
fn a_vera_room_with_teams_seats_people_only_on_a_team() {
    use crate::skirmish_launch::LaunchTeam::{self, Team};
    let mut session = vera_room();
    session.set_network_teams(Some(4)).unwrap();
    assert!(!teams(&session).contains(&LaunchTeam::None));
    assert_eq!(session.network_join_row(false), Some((4, true)));
    let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
    pick.team = LaunchTeam::None;
    for watching in [false, true] {
        let mut observer = false;
        assert!(session.admit_network_team(1, &mut pick, &mut observer));
        assert!(observer);
        session
            .accept_network_peer_choices(1, pick.clone(), observer, None)
            .unwrap();
        assert_eq!(guest(&session), (true, Some(4)), "{watching}");
    }
    pick.team = Team(2);
    let mut observer = false;
    assert!(!session.admit_network_team(1, &mut pick, &mut observer));
    session
        .accept_network_peer_choices(1, pick, observer, None)
        .unwrap();
    assert_eq!((guest(&session).0, teams(&session)[1]), (false, Team(2)));
}

/// In a mode that must ally, a player who watched and is seated again
/// joins the team with the fewest players, not Team A.
#[test]
fn a_vera_team_game_seats_a_returning_player_on_the_smallest_team() {
    use crate::skirmish_launch::LaunchTeam::{self, Team};
    let mut session = vera_room();
    session.editable_network_setup().unwrap().mode.must_ally = true;
    session.set_network_teams(Some(2)).unwrap();
    newcomer(&mut session);
    let mut pick = session.network_setup().unwrap().humans[1].slot.clone();
    session
        .accept_network_peer_choices(1, pick.clone(), true, None)
        .unwrap();
    // The host's Seat, as the join queue's, asks for no team.
    pick.team = LaunchTeam::None;
    session
        .accept_network_peer_choices(1, pick, false, None)
        .unwrap();
    assert_eq!(teams(&session), [Team(0), Team(1), Team(0)]);
}

#[test]
fn online_canonical_roster_preserves_admission_and_local_identity() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    let fixture = network_fixture();
    let host_name = fixture.humans[0].name.clone();
    let guest_name = fixture.humans[1].name.clone();
    session
        .accept_network_setup(fixture, false, None, false)
        .unwrap();
    session.set_network_player_ready(0, true).unwrap();
    session.set_network_map_status(0, 2).unwrap();
    session
        .canonicalize_online_roster(&[guest_name.clone(), host_name.clone()])
        .unwrap();
    let launch = session.network_setup().unwrap();
    assert_eq!(launch.local_player_index, 0);
    assert_eq!(launch.humans[0].name, guest_name);
    assert_eq!(launch.humans[1].name, host_name);
    assert!(session.network_player_ready(1));
    let unchanged = session.clone();
    assert!(
        session
            .canonicalize_online_roster(&[host_name.clone(), host_name])
            .is_err()
    );
    assert_eq!(
        session, unchanged,
        "rejected roster must not partially change admission"
    );
}

#[test]
fn endpoint_bindings_follow_admitted_participants_and_source_compaction() {
    use super::NetworkReturnContext;
    use std::net::{Ipv4Addr, SocketAddrV4};
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::LanLobby);
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234);
    assert!(session.bind_network_endpoint("absent", endpoint).is_err());
    let launch = network_fixture();
    let host = launch.humans[0].name.clone();
    session
        .accept_network_setup(launch, false, None, false)
        .unwrap();
    session.bind_network_endpoint(&host, endpoint).unwrap();
    assert!(session.bind_network_endpoint("Guest", endpoint).is_err());
    assert_eq!(session.network_endpoint_source(endpoint), Some(0));
    assert_eq!(session.network_endpoint(1), None);
    session.remove_network_participant(0).unwrap();
    assert_eq!(session.network_endpoint_source(endpoint), None);
    assert_eq!(session.network_setup().unwrap().local_player_index, 0);
    session.bind_network_endpoint("Guest", endpoint).unwrap();
    session.return_to_network_lobby();
    assert_eq!(session.network_endpoint_source(endpoint), None);
}

#[test]
fn match_chat_shows_the_sending_endpoints_admitted_name_and_colour() {
    use crate::app::match_runtime::network_match::chat_sender;
    use std::net::{Ipv4Addr, SocketAddrV4};
    let mut launch = network_fixture();
    // Longer than the packet's 19-byte name field.
    let host = "TwentyTwoCharacterName";
    launch.humans[0].name = host.into();
    launch.humans[0].slot.color_index = 5;
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    session
        .accept_network_setup(launch, false, None, false)
        .unwrap();
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30_001);
    session.bind_network_endpoint(host, endpoint).unwrap();
    // The packet names the other player, in that player's colour.
    assert_eq!(
        chat_sender(&session, endpoint, "Guest".into(), 2),
        (host.to_owned(), 5)
    );
    let stranger = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30_002);
    assert_eq!(
        chat_sender(&session, stranger, "Guest".into(), 2),
        ("Guest".to_owned(), 2)
    );
}

#[test]
fn network_freeze_keeps_guest_identity_seed_and_return_context_through_loading() {
    use super::NetworkReturnContext;
    for (mode, context) in [
        (SessionMode::Lan, NetworkReturnContext::LanLobby),
        (SessionMode::Wol, NetworkReturnContext::Wol),
    ] {
        let mut session = MatchSession::default();
        session.enter_network(context);
        session
            .accept_network_setup(network_fixture(), false, Some(7), false)
            .unwrap();
        session.set_network_player_ready(0, true).unwrap();
        session.set_network_map_status(0, 2).unwrap();
        assert!(session.network_player_ready(0));
        session.set_network_start_seed(0x1234abcd).unwrap();
        session.prepare_network_timing(2, || 0).unwrap();
        assert!(session.freeze_network_start(123, None).is_err());
        assert_eq!(session.network_start_seed(), Some(0x1234abcd));
        let startup = session.freeze_network_start(0x1234abcd, None).unwrap();
        assert_eq!(startup.session_mode(), mode);
        let crate::app::match_bootstrap::LoadingStartup::Network(prepared) = &startup else {
            panic!("network variant");
        };
        session.assert_network_start(prepared);
        assert_eq!(session.network_connected_source_indices(), &[0, 1]);
        session.report_network_loading_progress(100);
        assert!(
            !session.disconnect_network_participant(1),
            "local cannot time out itself"
        );
        assert!(session.disconnect_network_participant(0));
        assert!(
            !session.disconnect_network_participant(0),
            "disconnect is one-shot"
        );
        assert_eq!(session.network_connected_source_indices(), &[1]);
        assert_eq!(
            prepared.launch_session().humans.len(),
            2,
            "historical launch stays immutable"
        );
        assert_eq!(session.take_network_loading_progress_reports(), [100]);
        session.assert_network_start(prepared);
        assert_eq!(prepared.seed.value, 0x1234abcd);
        assert_eq!(prepared.launch_session().local_player_index, 1);
        assert_eq!(session.local_player_name(), Some("Guest"));
        assert!(!session.host());
        assert!(session.set_network_player_ready(0, false).is_err());
        assert!(
            session
                .accept_network_setup(network_fixture(), true, Some(7), false)
                .is_err()
        );
        assert_eq!(session.return_to_network_lobby(), Some(context));
        assert!(session.network_setup().is_none());
        assert_eq!(session.shell_mode(), Some(mode));
        session.leave_network();
        assert_eq!(session.shell_mode(), Some(SessionMode::Campaign));
        assert!(session.network_return_context().is_none());
    }
}

#[test]
fn host_changes_preserve_peer_ready_until_service_update_and_removal_reindexes_local() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(network_fixture(), true, Some(7), false)
        .unwrap();
    session.set_network_player_ready(0, true).unwrap();
    session.set_network_map_status(0, 2).unwrap();
    let mut options = session.network_setup().unwrap().options.clone();
    options.short_game = !options.short_game;
    session.set_network_options(options).unwrap();
    assert!(session.network_player_ready(0));
    assert_eq!(session.network_map_status(0), Some(2));
    session.set_network_player_ready(0, false).unwrap();
    assert_eq!(session.network_map_status(0), Some(2));
    session.remove_network_participant(0).unwrap();
    assert_eq!(session.network_setup().unwrap().local_player_index, 0);
    assert_eq!(session.local_player_name(), Some("Guest"));
    assert!(
        session
            .network_setup()
            .unwrap()
            .house_roster_is_consistent()
    );
}

#[test]
fn wol_rows_follow_mode_and_room_clears_guest_acceptance_and_times_from_game_speed() {
    let wol_host = || {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let mut launch = network_fixture();
        launch.local_player_index = 0;
        session
            .accept_network_setup(launch, true, Some(7), false)
            .unwrap();
        session
    };
    let mut session = wol_host();
    let launch = session.network_setup().unwrap().clone();
    let file = launch.selected_map_file.clone().unwrap();
    let mut man_battle = launch.mode.clone();
    man_battle.class = crate::skirmish_modes::MpModeClass::ManBattle;
    session
        .select_network_map(man_battle.clone(), file.clone(), 8)
        .unwrap();
    // 5EC3A0 `0x5EC457`: WOL rows stop at six where LAN keeps eight.
    assert_eq!(session.network_setup_capacity(), Some(6));
    assert_eq!(session.network_row_limit(), Some(6));
    // `0x5EC41C..0x5EC430`: Battle and FreeForAll (vtable+40) skip the
    // limit outside tournament and clan rooms.
    session
        .select_network_map(launch.mode.clone(), file.clone(), 8)
        .unwrap();
    assert_eq!(session.network_setup_capacity(), Some(8));
    // A clan room rounds down to even and keeps the limit; a tournament
    // room holds two rows.
    for (room, capacity, expected, limit) in [
        (
            NetworkRoom {
                tournament: false,
                clan: true,
            },
            7,
            6,
            6,
        ),
        (
            NetworkRoom {
                tournament: false,
                clan: true,
            },
            5,
            4,
            6,
        ),
        (
            NetworkRoom {
                tournament: true,
                clan: false,
            },
            8,
            2,
            2,
        ),
        (
            NetworkRoom {
                tournament: true,
                clan: true,
            },
            3,
            2,
            6,
        ),
    ] {
        let mut session = wol_host();
        session.set_network_room(room);
        session
            .select_network_map(launch.mode.clone(), file.clone(), capacity)
            .unwrap();
        assert_eq!(
            (
                session.network_setup_capacity(),
                session.network_row_limit()
            ),
            (Some(expected), Some(limit)),
            "{room:?} on a map for {capacity}"
        );
    }
    // 5E9850 keeps only the node named 0xA8D610: the host's own name on
    // the host, empty on a guest, which therefore clears its own too.
    session.set_network_player_ready(0, true).unwrap();
    session.set_network_player_ready(1, true).unwrap();
    session.clear_guest_readiness().unwrap();
    assert!(session.network_player_ready(0));
    assert!(!session.network_player_ready(1));
    let mut guest = MatchSession::default();
    guest.enter_network(super::NetworkReturnContext::Wol);
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    guest
        .accept_network_setup(launch, false, Some(7), false)
        .unwrap();
    guest.set_network_player_ready(0, true).unwrap();
    guest.set_network_player_ready(1, true).unwrap();
    guest.clear_guest_readiness().unwrap();
    assert!(!guest.network_player_ready(0) && !guest.network_player_ready(1));
    // Start Game Now: MaxAhead follows the agreed speed only.
    session.prepare_wol_timing([0; 8], true).unwrap();
    let speed = session.network_setup().unwrap().options.game_speed;
    assert_eq!(
        session.network_max_ahead_frames(),
        Some(crate::net::lockstep::wol_start_timing(speed, [0; 8], false).max_ahead)
    );
    // The same block seeds LatencyFudge, which FF's Connection trackbar opens on.
    assert_eq!(
        session.latency_fudge(),
        crate::net::lockstep::wol_start_timing(speed, [0; 8], true).latency_fudge
    );

    let mut lan = MatchSession::default();
    lan.enter_network(super::NetworkReturnContext::LanLobby);
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    lan.accept_network_setup(launch.clone(), true, Some(7), false)
        .unwrap();
    lan.select_network_map(launch.mode.clone(), file, 8)
        .unwrap();
    assert_eq!(lan.network_setup_capacity(), Some(8));
    // A LAN host's 0x1B arm keeps only its own acceptance.
    lan.set_network_player_ready(0, true).unwrap();
    lan.set_network_player_ready(1, true).unwrap();
    lan.clear_guest_readiness().unwrap();
    assert!(lan.network_player_ready(0) && !lan.network_player_ready(1));
    assert!(lan.prepare_wol_timing([0; 8], true).is_err());
}

#[test]
fn online_revision_retracts_all_remote_agreements_and_local_guest_acceptance() {
    for host in [false, true] {
        let mut session = MatchSession::default();
        session.enter_network(super::NetworkReturnContext::Wol);
        let launch = network_fixture();
        let local = launch.local_player_index;
        session
            .accept_network_setup(launch, host, None, false)
            .unwrap();
        for index in 0..2 {
            session.set_network_player_ready(index, true).unwrap();
            session.set_network_map_status(index, 1).unwrap();
        }
        session.invalidate_network_agreement().unwrap();
        assert_eq!(session.network_player_ready(local), host);
        assert!(!session.network_player_ready(1 - local));
        assert_eq!(session.network_map_status(1 - local), Some(0));
        assert_eq!(session.network_map_status(local), Some(1));
    }
}

/// Online guests repeat their K with every options revision. The host's
/// room hears it once per map or mode, again only when it changes, and
/// never for RandMap.Sed, which native admission reports present.
#[test]
fn a_repeated_map_report_is_news_once_per_map() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    let launch = network_fixture();
    let guest = 1 - launch.local_player_index;
    let file = launch.selected_map_file.clone().unwrap();
    let mut mode = launch.mode.clone();
    session
        .accept_network_setup(launch, true, None, false)
        .unwrap();
    let mut metadata = super::NetworkMapMetadata {
        display_name: "Room map".into(),
        player_capacity: 8,
        official: false,
        byte_length: 1234,
        digest: "Room".into(),
        source_sha256: "a".repeat(64),
        effective_digest: "b".repeat(64),
    };
    session
        .accept_network_map_metadata(metadata.clone(), None, true)
        .unwrap();
    assert!(session.report_network_map_status(guest, 0).unwrap());
    session.invalidate_network_agreement().unwrap();
    assert!(!session.report_network_map_status(guest, 0).unwrap());
    assert_eq!(session.network_map_status(guest), Some(0));
    assert!(session.report_network_map_status(guest, 1).unwrap());
    metadata.source_sha256 = "c".repeat(64);
    session
        .accept_network_map_metadata(metadata, None, true)
        .unwrap();
    assert!(session.report_network_map_status(guest, 1).unwrap());
    mode.id += 1;
    session.select_network_map(mode.clone(), file, 8).unwrap();
    assert!(session.report_network_map_status(guest, 1).unwrap());
    let random = crate::map::skirmish_scenarios::RANDMAP_SED;
    session.select_network_map(mode, random.into(), 8).unwrap();
    assert!(!session.report_network_map_status(guest, 0).unwrap());
}

#[test]
fn lan_revision_retracts_local_acceptance_and_prior_peer_map_proof() {
    let mut guest = MatchSession::default();
    guest.enter_network(super::NetworkReturnContext::LanLobby);
    let launch = network_fixture();
    let local = launch.local_player_index;
    guest
        .accept_network_setup(launch, false, None, false)
        .unwrap();
    guest.set_network_player_ready(local, true).unwrap();
    guest.set_network_map_status(local, 1).unwrap();
    guest.set_network_map_status(1 - local, 1).unwrap();
    guest.invalidate_network_agreement().unwrap();
    assert!(!guest.network_player_ready(local));
    assert_eq!(guest.network_map_status(local), Some(1));
    assert_eq!(guest.network_map_status(1 - local), Some(0));
}

#[test]
fn kick_unbinds_row_but_departure_owns_source_roster_compaction() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(network_fixture(), true, Some(7), false)
        .unwrap();
    session.set_network_player_ready(0, true).unwrap();
    session.set_network_map_status(0, 2).unwrap();
    let kicked = session
        .set_network_row_kind(
            0,
            super::NetworkRowKind::Ai(crate::skirmish_launch::AiDifficulty::Easy),
        )
        .unwrap()
        .unwrap();
    assert_eq!(kicked.raw_slot_index, Some(0));
    let setup = session.network_setup().unwrap();
    assert_eq!(setup.humans.len(), 2);
    assert_eq!(setup.humans[0].raw_slot_index, None);
    assert_eq!(setup.local_player_index, 1);
    assert_eq!(setup.required_start_count(), 3);
    assert!(setup.house_roster_is_consistent());
    assert!(session.network_player_ready(0));
    assert_eq!(session.network_map_status(0), Some(2));
    let mut rng = crate::sim::rng::SimRng::new(11);
    let resolved = setup.resolve_shell_random_assignments(&mut rng);
    assert_eq!(resolved.humans[0].name, kicked.name);
    session.accept_network_assignments(resolved).unwrap();
    session.remove_network_participant(0).unwrap();
    let setup = session.network_setup().unwrap();
    assert_eq!(setup.humans.len(), 1);
    assert_eq!(setup.local_player_index, 0);
    assert_eq!(setup.required_start_count(), 2);
    assert!(setup.house_roster_is_consistent());
}

#[test]
fn cooperative_country_refresh_can_keep_the_ai_rows() {
    use crate::skirmish_launch::LaunchCountry;
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    let mut launch = network_fixture();
    launch
        .opponents
        .push(crate::skirmish_launch::SkirmishAiSlot {
            raw_slot_index: 2,
            country: LaunchCountry::Yuri,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
            team: crate::skirmish_launch::LaunchTeam::None,
            difficulty: crate::skirmish_launch::AiDifficulty::Easy,
        });
    launch.ai_player_count = 1;
    session
        .accept_network_setup(launch, false, None, false)
        .unwrap();
    let ai_country = |session: &MatchSession| {
        let ai = &session.network_setup().unwrap().opponents[0];
        (ai.country, ai.country_random)
    };
    // BC open's refreshes leave the AI rows to the profile restore.
    session
        .reset_network_countries(Some(LaunchCountry::Russia), false)
        .unwrap();
    let setup = session.network_setup().unwrap();
    assert!(
        setup
            .humans
            .iter()
            .all(|human| human.slot.country == LaunchCountry::Russia && !human.slot.country_random)
    );
    assert_eq!(ai_country(&session), (LaunchCountry::Yuri, false));
    // Use Map's refresh (`+0x18`) takes every row.
    session.reset_network_countries(None, true).unwrap();
    assert_eq!(ai_country(&session), (LaunchCountry::Yuri, true));
}

#[test]
fn select_scenario_keeps_rows_starts_and_teams() {
    use crate::skirmish_launch::{LaunchStartPosition, LaunchTeam};
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    launch.humans[0].slot.start_position = LaunchStartPosition::Position(3);
    launch.humans[1].slot.team = LaunchTeam::Team(2);
    session
        .accept_network_setup(launch.clone(), true, None, false)
        .unwrap();
    let before = session.network_setup().unwrap().clone();
    assert_eq!(
        before.humans[0].slot.start_position,
        LaunchStartPosition::Position(3)
    );
    // 0x005E7BF0 writes the map and [0xA8BB0C], nothing in the rows.
    session
        .select_network_scenario("C1A01MD.MAP".into(), 4)
        .unwrap();
    let after = session.network_setup().unwrap();
    assert_eq!(after.selected_map_file.as_deref(), Some("C1A01MD.MAP"));
    assert_eq!(session.network_setup_capacity(), Some(4));
    assert_eq!(after.humans, before.humans);
    assert_eq!(after.opponents, before.opponents);
}

#[test]
fn open_rows_and_over_capacity_follow_the_wol_slot_tests() {
    // 0x5E9510 and 0x5E95E0 scan min(8, the options' raw capacity).
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    let launch = network_fixture();
    session
        .accept_network_setup(launch.clone(), false, Some(7), false)
        .unwrap();
    // Before any options the raw capacity is 8: rows 2..7 are Open.
    assert!(!session.network_no_open_player_row());
    let options =
        |session: &mut MatchSession, vacancies: [super::NetworkVacancy; 8], capacity: i32| {
            session
                .accept_network_host_setup(launch.clone(), vacancies, capacity, None, false)
                .unwrap();
            let metadata = super::NetworkMapMetadata {
                display_name: "Map".into(),
                player_capacity: capacity,
                official: true,
                byte_length: 1,
                digest: "D".into(),
                source_sha256: String::new(),
                effective_digest: String::new(),
            };
            session
                .accept_network_map_metadata(metadata, None, true)
                .unwrap();
        };
    let open = [super::NetworkVacancy::Open; 8];
    options(&mut session, open, 4);
    assert!(!session.network_no_open_player_row());
    assert!(!session.network_over_capacity());
    // Two humans fill a two-row map: no Open row, not over-full.
    options(&mut session, open, 2);
    assert!(session.network_no_open_player_row());
    assert!(!session.network_over_capacity());
    // Closed rows are not Open.
    let mut closed = open;
    closed[2] = super::NetworkVacancy::Closed;
    closed[3] = super::NetworkVacancy::Closed;
    options(&mut session, closed, 4);
    assert!(session.network_no_open_player_row());
    // A third node on a two-row map is over-full.
    options(&mut session, open, 2);
    let mut third = session.network_setup().unwrap().humans[0].clone();
    third.name = "Third".into();
    third.raw_slot_index = Some(2);
    session.add_network_participant(third).unwrap();
    assert!(session.network_over_capacity());
}

/// VERA shared control: outside Cooperative a player may take a start
/// another player holds, never an AI's (`0x5E9720`).
#[test]
fn a_player_may_share_a_players_start_but_not_an_ais() {
    use crate::skirmish_launch::LaunchStartPosition::Position;
    let mut launch = network_fixture();
    launch.humans[0].slot.start_position = Position(0);
    launch
        .opponents
        .push(crate::skirmish_launch::SkirmishAiSlot {
            raw_slot_index: 2,
            country: crate::skirmish_launch::LaunchCountry::America,
            country_random: false,
            color_index: 3,
            color_random: false,
            start_position: Position(1),
            team: crate::skirmish_launch::LaunchTeam::None,
            difficulty: crate::skirmish_launch::AiDifficulty::Hard,
        });
    let inactive = [super::NetworkAiOptions {
        difficulty: -1,
        country: -2,
        color: -2,
        start: -2,
        team: -2,
    }; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT];
    assert!(!super::network_start_in_use(
        &launch,
        &inactive,
        Position(0)
    ));
    assert!(super::network_start_in_use(&launch, &inactive, Position(1)));
    launch.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
    assert!(super::network_start_in_use(&launch, &inactive, Position(0)));
}

#[test]
fn a_peer_r_follows_the_wol_host_checks() {
    // 0x7A8260 'R': taken colour/start -> Random, team outside 0..3 ->
    // the mode default, observer moves and the vacate on a full session.
    use super::{NetworkRowKind, NetworkVacancy, PeerChoicesChange};
    use crate::skirmish_launch::{LaunchStartPosition, LaunchTeam};
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    launch.humans[0].slot.color_index = 5;
    launch.humans[0].slot.color_random = false;
    launch.humans[0].slot.start_position = LaunchStartPosition::Auto;
    let must_ally = launch.mode.must_ally;
    session
        .accept_network_setup(launch, true, Some(7), false)
        .unwrap();
    session
        .set_network_row_kind(7, NetworkRowKind::OpenObserver)
        .unwrap();
    let guest = |session: &MatchSession| session.network_setup().unwrap().humans[1].clone();
    let mut asked = guest(&session).slot;
    asked.color_index = 5;
    asked.color_random = false;
    asked.start_position = LaunchStartPosition::Position(0);
    asked.team = LaunchTeam::Team(7);
    let change = session
        .accept_network_peer_choices(1, asked, false, None)
        .unwrap();
    let after = guest(&session);
    assert_eq!(
        change,
        PeerChoicesChange {
            roster: false,
            options: true
        }
    );
    assert!(after.slot.color_random, "the host holds colour 5");
    assert_eq!(after.slot.start_position, LaunchStartPosition::Position(0));
    assert_eq!(
        after.slot.team,
        if must_ally {
            LaunchTeam::Team(0)
        } else {
            LaunchTeam::None
        }
    );
    // The same choices as held change nothing and publish nothing.
    let held = session
        .accept_network_peer_choices(1, after.slot.clone(), false, None)
        .unwrap();
    assert_eq!(held, PeerChoicesChange::default());
    // To the observer row and back to the first Open row.
    let change = session
        .accept_network_peer_choices(1, after.slot.clone(), true, None)
        .unwrap();
    assert!(change.roster && change.options);
    let observer = guest(&session);
    assert!(observer.is_observer());
    assert_eq!(observer.raw_slot_index, Some(7));
    assert_eq!(session.network_vacancy(1), Some(NetworkVacancy::Open));
    // Random asked again: the options still go out after the move.
    let change = session
        .accept_network_peer_choices(1, after.slot.clone(), false, None)
        .unwrap();
    assert!(change.roster && change.options);
    let player = guest(&session);
    assert!(!player.is_observer());
    assert_eq!(player.raw_slot_index, Some(1));
    assert_eq!(
        session.network_vacancy(7),
        Some(NetworkVacancy::OpenObserver)
    );
    // An observer asking to play with no Open row loses its row.
    session
        .accept_network_peer_choices(1, after.slot.clone(), true, None)
        .unwrap();
    for row in 1..7 {
        session
            .set_network_row_kind(row, NetworkRowKind::Closed)
            .unwrap();
    }
    assert!(session.network_no_open_player_row());
    let change = session
        .accept_network_peer_choices(1, after.slot.clone(), false, None)
        .unwrap();
    assert!(change.roster && change.options);
    assert_eq!(guest(&session).raw_slot_index, None);
    assert_eq!(
        session.network_vacancy(7),
        Some(NetworkVacancy::OpenObserver)
    );
    // A node without a row is ignored.
    let ignored = session
        .accept_network_peer_choices(1, after.slot, true, None)
        .unwrap();
    assert_eq!(ignored, PeerChoicesChange::default());
}

#[test]
fn map_metadata_has_independent_verification_gate_and_transferred_filename_authority() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    let launch = network_fixture();
    session
        .accept_network_setup(launch.clone(), false, Some(7), false)
        .unwrap();
    let mut metadata = super::NetworkMapMetadata {
        display_name: "Advertised title".into(),
        player_capacity: 8,
        official: false,
        byte_length: 83573,
        digest: "CaseSensitive".into(),
        source_sha256: String::new(),
        effective_digest: String::new(),
    };
    assert!(
        session
            .network_map_requires_verification(&launch, &metadata)
            .unwrap()
    );
    session
        .accept_network_map_metadata(metadata.clone(), Some("Local title"), true)
        .unwrap();
    let mut changed_options = launch.clone();
    changed_options.options.short_game = !changed_options.options.short_game;
    assert!(
        !session
            .network_map_requires_verification(&changed_options, &metadata)
            .unwrap(),
        "general options do not repeat file probing/K publication"
    );
    let mut case_only = launch.clone();
    let file = case_only.selected_map_file.as_mut().unwrap();
    *file = if file.to_ascii_lowercase() == *file {
        file.to_ascii_uppercase()
    } else {
        file.to_ascii_lowercase()
    };
    assert_ne!(case_only.selected_map_file, launch.selected_map_file);
    assert!(
        !session
            .network_map_requires_verification(&case_only, &metadata)
            .unwrap(),
        "native filename comparison ignores case"
    );
    metadata.display_name = "Title-only packet".into();
    assert!(
        !session
            .network_map_requires_verification(&launch, &metadata)
            .unwrap()
    );
    session
        .accept_network_map_metadata(metadata.clone(), None, false)
        .unwrap();
    assert_eq!(
        session.network_map_metadata().unwrap().display_name,
        "Local title"
    );
    metadata.player_capacity = 2;
    assert!(
        session
            .network_map_requires_verification(&launch, &metadata)
            .unwrap()
    );
    session
        .accept_network_host_setup(
            launch.clone(),
            [super::NetworkVacancy::Open; 8],
            2,
            Some(7),
            false,
        )
        .unwrap();
    session
        .accept_network_map_metadata(metadata.clone(), None, true)
        .unwrap();
    assert_eq!(
        session.network_map_metadata().unwrap().display_name,
        "Title-only packet"
    );
    assert!(
        !session
            .network_map_requires_verification(&launch, &metadata)
            .unwrap()
    );
    assert_eq!(metadata.admission("CONTROLLED.MAP", false).status(), 2);
    assert!(
        metadata
            .admission("CONTROLLED.MAP", false)
            .accept_eligible()
    );
    metadata.official = true;
    assert_eq!(metadata.admission("CONTROLLED.MAP", false).status(), 0);
    assert!(
        !metadata
            .admission("CONTROLLED.MAP", false)
            .accept_eligible()
    );
    assert_eq!(metadata.admission("RandMap.Sed", false).status(), 1);
    assert_eq!(metadata.admission("CONTROLLED.MAP", true).status(), 1);
    session
        .accept_prepared_network_map_filename("CONTROLLED.MAP".into())
        .unwrap();
    session.prepare_network_timing(2, || 0).unwrap();
    let startup = session.freeze_network_start(13, None).unwrap();
    let crate::app::match_bootstrap::LoadingStartup::Network(prepared) = startup else {
        panic!("network");
    };
    assert_eq!(
        prepared.launch_session().selected_map_file.as_deref(),
        Some("CONTROLLED.MAP")
    );
    assert!(
        session
            .accept_prepared_network_map_filename("OTHER.MAP".into())
            .is_err()
    );
    session.return_to_network_lobby();
    assert!(session.network_map_metadata().is_none());
}

#[test]
fn verified_download_keeps_advertised_name_until_freeze_and_rejects_stale_identity() {
    use crate::map::source::{TransferResource, install_received_asset, load_received_asset};
    let directory = crate::map::source::test_support::TestDirectory::new("session-map-lease");
    let bytes = b"[RandomMap]\nSeed=123\n";
    let digest = crate::util::sha256::sha256_hex(bytes);
    let source = install_received_asset(
        directory.path(),
        bytes.to_vec(),
        &digest,
        bytes.len(),
        TransferResource::Seed,
    )
    .unwrap()
    .source;
    let pin = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.path().join(format!(".{digest}.sed.pin")))
        .unwrap();
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::Wol);
    let launch = network_fixture();
    session
        .accept_network_setup(launch.clone(), false, None, false)
        .unwrap();
    let mut metadata = super::NetworkMapMetadata {
        display_name: "Shared map".into(),
        player_capacity: 8,
        official: false,
        byte_length: bytes.len() as i32,
        digest: String::new(),
        source_sha256: digest.clone(),
        effective_digest: "c".repeat(64),
    };
    session
        .accept_network_map_metadata(metadata.clone(), None, true)
        .unwrap();
    session
        .accept_verified_network_map_source(source.clone(), "c".repeat(64))
        .unwrap();
    assert_eq!(
        session.network_setup().unwrap().selected_map_file,
        launch.selected_map_file
    );
    assert!(
        !session.network_player_ready(1),
        "receiving bytes never readies the guest"
    );
    metadata.source_sha256 = "b".repeat(64);
    session
        .accept_network_map_metadata(metadata.clone(), None, true)
        .unwrap();
    assert!(session.network_local_map_source().is_none());
    assert!(
        session
            .accept_verified_network_map_source(source, "c".repeat(64))
            .is_err()
    );
    pin.try_lock()
        .expect("a rejected stale completion releases its lease");
    pin.unlock().unwrap();
    metadata.source_sha256 = digest.clone();
    session
        .accept_network_map_metadata(metadata, None, true)
        .unwrap();
    let source = load_received_asset(
        directory.path(),
        &digest,
        bytes.len(),
        TransferResource::Seed,
    )
    .unwrap()
    .source;
    session
        .accept_verified_network_map_source(source, "c".repeat(64))
        .unwrap();
    session.prepare_wol_timing([0; 2], false).unwrap();
    let startup = session.freeze_network_start(13, None).unwrap();
    let crate::app::match_bootstrap::LoadingStartup::Network(prepared) = startup else {
        panic!("network start");
    };
    assert_eq!(
        prepared.launch_session().selected_map_file,
        launch.selected_map_file
    );
    assert!(prepared.local_map_source().is_some());
    session.return_to_network_lobby();
    assert!(session.network_local_map_source().is_none());
    assert!(
        pin.try_lock().is_err(),
        "frozen startup retains the admitted source"
    );
    drop(prepared);
    pin.try_lock()
        .expect("lobby return and final startup drop release the source");
}

#[test]
fn host_packet_preserves_admission_and_rejects_source_identity_changes_atomically() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(network_fixture(), false, Some(7), false)
        .unwrap();
    session.set_network_player_ready(0, true).unwrap();
    session.set_network_player_ready(1, true).unwrap();
    session.set_network_map_status(0, 2).unwrap();
    let mut incoming = session.network_setup().unwrap().clone();
    incoming.options.short_game = !incoming.options.short_game;
    incoming.humans[0].role = crate::skirmish_launch::LaunchHumanRole::Player { group: 2 };
    let vacancies = [super::NetworkVacancy::Closed; 8];
    session
        .accept_network_host_setup(incoming, vacancies, 4, Some(7), false)
        .unwrap();
    assert!(session.network_player_ready(0));
    assert!(session.network_player_ready(1));
    assert_eq!(session.network_map_status(0), Some(2));
    assert_eq!(session.network_setup_capacity(), Some(4));
    assert_eq!(session.network_observer_row(), Some(7));
    assert_eq!(
        session.network_vacancy(7),
        Some(super::NetworkVacancy::Closed)
    );
    let accepted = session.clone();
    let mut reordered = session.network_setup().unwrap().clone();
    reordered.humans.swap(0, 1);
    assert!(
        session
            .accept_network_host_setup(reordered, vacancies, 4, Some(7), false)
            .is_err()
    );
    assert_eq!(session, accepted);
    session.set_network_player_ready(1, false).unwrap();
    assert!(session.network_player_ready(0));
    assert!(!session.network_player_ready(1));
}

#[test]
fn host_options_with_a_speed_the_setup_never_offers_are_refused() {
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(network_fixture(), false, Some(7), false)
        .unwrap();
    let accepted = session.clone();
    let vacancies = [super::NetworkVacancy::Open; 8];
    let with_speed = |speed| {
        let mut incoming = accepted.network_setup().unwrap().clone();
        incoming.options.game_speed = speed;
        incoming
    };
    for speed in [-1, 7, 9] {
        assert!(
            session
                .accept_network_host_setup(with_speed(speed), vacancies, 4, Some(7), false)
                .is_err(),
            "speed {speed}"
        );
        assert_eq!(session, accepted);
    }
    session
        .accept_network_host_setup(with_speed(8), vacancies, 4, Some(7), false)
        .unwrap();
    assert_eq!(session.network_setup().unwrap().options.game_speed, 8);
}

#[test]
fn mode_role_is_distinct_from_team_and_assignment_cannot_change_selected_start() {
    use crate::skirmish_launch::{LaunchHumanRole, LaunchStartPosition};
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(network_fixture(), true, Some(7), false)
        .unwrap();
    let team = session.network_setup().unwrap().humans[0].slot.team;
    session
        .accept_network_human_role(0, LaunchHumanRole::Player { group: 2 })
        .unwrap();
    assert_eq!(session.network_setup().unwrap().humans[0].slot.team, team);
    let mut resolved = session.network_setup().unwrap().clone();
    resolved.humans[0].slot.start_position = LaunchStartPosition::Position(3);
    assert!(session.accept_network_assignments(resolved).is_err());
    session
        .accept_network_human_role(0, LaunchHumanRole::Observer)
        .unwrap();
    assert!(
        session
            .network_setup()
            .unwrap()
            .house_roster_is_consistent()
    );
    assert!(session.network_setup().unwrap().humans[0].is_observer());
    assert!(
        session
            .accept_network_human_role(0, LaunchHumanRole::Player { group: -1 })
            .is_err()
    );
}

#[test]
fn multiplayer_settings_initialization_matches_native_rng_and_survives_session_changes() {
    use crate::sim::rng::{MainRng, SimRng};
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/network_options.json",
    ))
    .unwrap();
    for case in corpus["initialization_cases"]
        .as_array()
        .expect("native initialization cases")
    {
        let main: MainRng =
            SimRng::from_native_state_hex_for_test(case["initial_rng_hex"].as_str().unwrap())
                .into();
        let mut session = MatchSession::default();
        let calls = case["calls"].as_array().unwrap();
        assert!(calls.len() >= 2, "native first/re-entry comparison");
        for (index, call) in calls.iter().enumerate() {
            session.prepare_multiplayer_settings(&main.draws());
            assert_eq!(
                main.native_state_hex(),
                call["rng_hex"].as_str().unwrap(),
                "{} call {index}",
                case["name"]
            );
            session.begin_offline(SessionMode::Skirmish, Some("Commander".into()));
        }
        // Loading another scenario does not reset the process's B07788.
        session.restore_from_save(SessionMode::Campaign, None);
        session.prepare_multiplayer_settings(&main.draws());
        assert_eq!(
            main.native_state_hex(),
            calls.last().unwrap()["rng_hex"].as_str().unwrap()
        );
    }
}

#[test]
fn process_start_is_campaign_with_no_participants() {
    let session = MatchSession::default();
    assert_eq!(session.shell_mode(), Some(SessionMode::Campaign));
    assert_eq!(session.participant_count(), 0);
    assert_eq!(session.local_player_name(), None);
    assert!(!session.host());
}

#[test]
fn offline_skirmish_launch_seats_the_local_player_as_host() {
    let mut session = MatchSession::default();
    session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
    assert_eq!(session.shell_mode(), Some(SessionMode::Skirmish));
    assert_eq!(session.participant_count(), 1);
    assert_eq!(session.local_player_name(), Some("Commander"));
    assert!(session.host());
}

#[test]
fn campaign_launch_replaces_an_outgoing_skirmish_table() {
    let mut session = MatchSession::default();
    session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
    session.begin_offline(SessionMode::Campaign, None);
    assert_eq!(session.shell_mode(), Some(SessionMode::Campaign));
    assert_eq!(session.participant_count(), 0);
    assert_eq!(session.local_player_name(), None);
    assert!(session.host());
}

#[test]
fn saved_game_restore_hands_the_mode_to_the_restored_world() {
    let mut session = MatchSession::default();
    session.restore_from_save(SessionMode::Skirmish, Some("Saved"));
    assert_eq!(session.shell_mode(), None, "the restored world holds it");
    assert_eq!(session.local_player_name(), Some("Saved"));
    session.restore_from_save(SessionMode::Campaign, None);
    assert_eq!(session.shell_mode(), None);
    assert_eq!(session.local_player_name(), None);
}

#[test]
fn restoring_a_save_keeps_only_a_visit_of_its_own_mode() {
    use super::NetworkReturnContext;
    let mut session = MatchSession::default();
    session.enter_network(NetworkReturnContext::LanLobby);
    session.restore_from_save(SessionMode::Lan, None);
    assert_eq!(
        session.network_return_context(),
        Some(NetworkReturnContext::LanLobby)
    );
    session.restore_from_save(SessionMode::Wol, None);
    assert_eq!(session.network_return_context(), None);
}

#[test]
fn game_mode_has_one_holder_through_install_shell_writes_and_drop() {
    let mut session = MatchSession::default();
    session.begin_offline(SessionMode::Skirmish, Some("Commander".to_owned()));
    session.hand_mode_to_world(SessionMode::Skirmish);
    assert_eq!(session.shell_mode(), None);
    // The finished world keeps holding it until the shell writes again.
    session.enter_network(super::NetworkReturnContext::LanLobby);
    assert_eq!(session.shell_mode(), Some(SessionMode::Lan));
    // A load that drops the world without a successor hands it back.
    session.hand_mode_to_world(SessionMode::Lan);
    session.retake_mode_from_world(SessionMode::Lan);
    assert_eq!(session.shell_mode(), Some(SessionMode::Lan));
    session.retake_mode_from_world(SessionMode::Skirmish);
    assert_eq!(session.shell_mode(), Some(SessionMode::Lan));
}

#[test]
fn network_modes_route_queue_ai_to_the_multiplayer_body() {
    use crate::sim::world::CommandQueueRoute;
    for (mode, route) in [
        (SessionMode::Campaign, CommandQueueRoute::SinglePlayer),
        (SessionMode::Skirmish, CommandQueueRoute::SinglePlayer),
        (SessionMode::Lan, CommandQueueRoute::Multiplayer),
        (SessionMode::Wol, CommandQueueRoute::Multiplayer),
        (SessionMode::Other, CommandQueueRoute::SinglePlayer),
    ] {
        assert_eq!(
            mode.execution_topology().command_queue_route(),
            route,
            "{mode:?}"
        );
    }
}

#[test]
fn raw_game_mode_mapping_is_writer_proofed() {
    assert_eq!(SessionMode::from_game_mode(0), SessionMode::Campaign);
    assert_eq!(SessionMode::from_game_mode(3), SessionMode::Lan);
    assert_eq!(SessionMode::from_game_mode(4), SessionMode::Wol);
    assert_eq!(SessionMode::from_game_mode(5), SessionMode::Skirmish);
    for raw in [1, 2, -1, 99] {
        assert_eq!(SessionMode::from_game_mode(raw), SessionMode::Other);
    }
    assert!(SessionMode::Lan.is_network());
    assert!(SessionMode::Wol.is_network());
    assert!(!SessionMode::Campaign.is_network());
    assert!(!SessionMode::Skirmish.is_network());
    assert!(!SessionMode::Other.is_network());
}

#[test]
fn mode_policy_controls_team_room_and_clears_observer_row_when_disallowed() {
    let mut session = vera_room();
    assert!(session.network_team_room());
    assert!(session.network_observer_row().is_some());

    let mut coop = session.network_setup().unwrap().mode.clone();
    coop.class = crate::skirmish_modes::MpModeClass::Cooperative;
    coop.allies_allowed = true;
    session
        .select_network_map(coop, "COOP.MAP".into(), 2)
        .unwrap();
    assert!(!session.network_team_room());
    assert_eq!(session.network_observer_row(), None);
}

/// Cooperative's +0x20 (`0x005C1A10`) clears the observer reservation
/// whenever the mode becomes current, so a host whose restored setup is
/// already Cooperative keeps no observer row from its first map pick.
#[test]
fn a_restored_cooperative_setup_keeps_no_observer_row() {
    let mut launch = network_fixture();
    launch.local_player_index = 0;
    launch.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
    let mode = launch.mode.clone();
    let mut session = MatchSession::default();
    session.enter_network(super::NetworkReturnContext::LanLobby);
    session
        .accept_network_setup(launch, true, Some(7), true)
        .unwrap();
    session
        .select_network_map(mode, "COOP.MAP".into(), 2)
        .unwrap();
    assert_eq!(session.network_observer_row(), None);
}
