//! VERA's offline WOL test service: a developer stand-in behind the same
//! request/event boundary a real service will use, chosen by the Options
//! page's `[VERA] WolTestService=` box. It is not gamemd behavior: it lets
//! the Internet screens (login `0x10F`, lobby `0x113`, setup C2/C9) be
//! opened and clicked without a server.
//!
//! It answers the visit's and the WOL setup's requests once no shell
//! transition runs: like a real service, a reply arrives after the page that
//! asked has slid out. Any login is accepted. The lobbies, games and users
//! are fixed stand-ins. Two of them say one line each when a chat lobby is
//! joined, and a locate finds them. A created game is joined by one
//! stand-in guest who picks, has the map and accepts; the joined game's
//! host answers the guest's first choices with its options. A start with
//! that guest gets OnGameStart and the connection verify, then the scenario
//! negotiation fails ("Scenarios don't match"): nobody can play a match
//! here. The buddy list starts with four stand-ins whose locates answer a
//! chat lobby, a game, offline and find-disabled; Add and Remove change it
//! for the process.

use super::lobby::WolJoined;
use super::service::WolLadderRecord;
use super::{WolApiStatus, WolLoginAccepted, WolServer, WolServerList, WolServiceEvent};
use super::{WolProvider, WolServiceRequest, service_event};
use crate::app::AppState;
use crate::app::match_runtime::session::SessionMode;
use crate::app::shell_network_setup::{NetworkSetupEvent, NetworkSetupRequest};
use crate::ui::wol_chat::WolChatLine;
use crate::ui::wol_lobby::{
    EX_INFO_VERSION, GAME_CHANNEL, MEMBER_OPERATOR, MEMBER_VOICE, WolChannel, WolMember,
};
use crate::ui::wol_login::WolCredential;
use std::time::Instant;

/// Event 9's status that completes the server-list wait.
const SERVER_STATUS_READY: u32 = 0x0004_012F;

/// Replies per poll; each may queue further requests (a list reply asks to
/// join, a join asks for both lists).
const ROUNDS: usize = 8;

const MOTD: &str = "VERA20k offline test service: these lobbies, games and players are \
                    stand-ins and nobody else is online.\nTurn it off on the Options page.";

/// Answer the queued requests of a visit that uses this service; returns
/// "now" while requests wait for a running transition.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.provider != WolProvider::TestService)
    {
        return None;
    }
    for _ in 0..ROUNDS {
        if state.frontend.shell_exit.is_some() {
            break;
        }
        let requests = state
            .frontend
            .wol
            .as_mut()
            .map(|visit| visit.take_service_requests())
            .unwrap_or_default();
        let setup = setup_requests(state);
        if requests.is_empty() && setup.is_empty() {
            return None;
        }
        for request in requests {
            answer(state, request);
        }
        for request in setup {
            answer_setup(state, request);
        }
    }
    pending(state).then(Instant::now)
}

fn pending(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| !visit.requests.is_empty())
}

/// The WOL setup's own requests; this service answers a locate, the
/// guest's leave and, as the joined game's host, the guest's choices.
fn setup_requests(state: &mut AppState) -> Vec<NetworkSetupRequest> {
    if state.match_state.session_mode() != SessionMode::Wol
        || !state.frontend.shell_route.network_setup()
    {
        return Vec::new();
    }
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .map(|protocol| protocol.take_service_requests())
        .unwrap_or_default()
}

fn answer_setup(state: &mut AppState, request: NetworkSetupRequest) {
    if let NetworkSetupRequest::WolChat(WolChatLine::Locate { name }) = &request {
        let result = locate(&String::from_utf16_lossy(name));
        service_event(state, WolServiceEvent::Located(result));
        return;
    }
    if let NetworkSetupRequest::PlayerChoices { slot, observer, .. } = request {
        if !state.match_state.session.host()
            && let Err(error) = host_options(state, slot, observer)
        {
            log::warn!("WOL test service: host options refused: {error:#}");
        }
        return;
    }
    // C9 Back waits for the leave's OnPart; C2 Back closes at once.
    if request == NetworkSetupRequest::Leave && !state.match_state.session.host() {
        service_event(state, WolServiceEvent::ChannelLeft);
        return;
    }
    if state.match_state.session.host() {
        guest_answer(state, &request);
    }
}

/// The stand-in guest who joins a game this user created.
const GUEST: &str = "Natasha";

/// The stand-in guest of a created game: none, expected once C2 publishes
/// its first options, joined, or kicked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum WolTestGuest {
    #[default]
    None,
    Expected,
    Joined,
    Kicked,
}

/// The stand-in guest joins after C2's first options, sends its first `R`
/// (Korea, colour 3), `K1` and accepts. Each later options parse sends `K1`
/// again, and accepts again when the change cleared its acceptance. A
/// host's kick reaches it as the server's kick echo.
fn guest_answer(state: &mut AppState, request: &NetworkSetupRequest) {
    let local = nickname(state);
    match (state.frontend.wol_test_guest, request) {
        (WolTestGuest::Expected, NetworkSetupRequest::PublishOptions) => {
            state.frontend.wol_test_guest = WolTestGuest::Joined;
            service_event(state, WolServiceEvent::UserJoined(member(GUEST, 0)));
            let slot = crate::skirmish_launch::SkirmishLocalSlot {
                country: crate::skirmish_launch::LaunchCountry::Korea,
                country_random: false,
                color_index: 3,
                color_random: false,
                start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
                team: crate::skirmish_launch::LaunchTeam::None,
            };
            service_event(
                state,
                WolServiceEvent::PlayerChoices {
                    from: GUEST.into(),
                    slot,
                    observer: false,
                },
            );
            guest_map_status(state);
            guest_accepts(state);
        }
        (WolTestGuest::Joined, NetworkSetupRequest::PublishOptions) => {
            guest_map_status(state);
            guest_accepts(state);
        }
        (WolTestGuest::Joined, NetworkSetupRequest::RequestAcceptance) => guest_accepts(state),
        (WolTestGuest::Joined, NetworkSetupRequest::StartGame { users }) => {
            // OnGameStart for the requested users, then Start Game Now: the
            // guest answers the verify, but nobody can play here, so the
            // negotiation fails as for a map the guest lacks.
            let users = users
                .iter()
                .map(|name| crate::app::shell_network_setup::WolStartUser {
                    name: name.clone(),
                    flags: u32::from(*name == local),
                })
                .collect();
            let mut pings = [-1; 8];
            pings[1] = 60;
            for event in [
                NetworkSetupEvent::GameStarted { users, pings },
                NetworkSetupEvent::VerifyStarted { pairs: true },
                NetworkSetupEvent::PeerConnected { node: 1 },
                NetworkSetupEvent::Negotiated { result: 2 },
            ] {
                if let Err(error) = crate::app::shell_network_setup::service_event(state, event) {
                    log::warn!("WOL test service: game start refused: {error:#}");
                    return;
                }
            }
        }
        (WolTestGuest::Joined, NetworkSetupRequest::Kick { name }) if name == GUEST => {
            state.frontend.wol_test_guest = WolTestGuest::Kicked;
            let Some(channel) = super::lobby::current_channel(state) else {
                return;
            };
            service_event(
                state,
                WolServiceEvent::Kicked {
                    channel,
                    user: member(GUEST, 0),
                    by: member(&local, MEMBER_OPERATOR),
                },
            );
        }
        _ => {}
    }
}

/// The guest's options parse finds the map and sends `K1`.
fn guest_map_status(state: &mut AppState) {
    service_event(
        state,
        WolServiceEvent::MapStatus {
            from: GUEST.into(),
            status: 1,
        },
    );
}

fn guest_accepts(state: &mut AppState) {
    let ready = state
        .match_state
        .session
        .network_setup()
        .and_then(|launch| {
            let index = launch.humans.iter().position(|human| human.name == GUEST)?;
            Some(state.match_state.session.network_player_ready(index))
        });
    if ready == Some(false) {
        service_event(
            state,
            WolServiceEvent::PlayerReady {
                from: GUEST.into(),
                value: 1,
            },
        );
    }
}

/// The stand-in host's seed in its full options.
const HOST_SEED: u32 = 0x5EED_0001;

/// The joined game's stand-in host answers the guest's `R` with its full
/// options: the first map for four the mode admits, the host on row 0 with a fixed
/// pick, the guest on row 1 with its choices (start and team as sent).
fn host_options(
    state: &mut AppState,
    slot: crate::skirmish_launch::SkirmishLocalSlot,
    observer: bool,
) -> anyhow::Result<()> {
    let mut launch = state
        .match_state
        .session
        .network_setup()
        .ok_or_else(|| anyhow::anyhow!("no joined setup"))?
        .clone();
    let mode = state
        .frontend
        .skirmish_modes
        .iter()
        .find(|mode| mode.id == launch.mode.id)
        .ok_or_else(|| anyhow::anyhow!("no setup mode"))?;
    let file = state
        .frontend
        .scenario_catalog
        .records()
        .iter()
        .find(|record| {
            crate::map::skirmish_scenarios::record_matches_mode(record, mode)
                && record.player_capacity >= 4
        })
        .map(|record| record.file_name.clone())
        .ok_or_else(|| anyhow::anyhow!("no map for the stand-in host"))?;
    let local = launch.local_player_index;
    let mut next_row = 2;
    for (index, human) in launch.humans.iter_mut().enumerate() {
        if index == local {
            human.raw_slot_index = Some(1);
            human.slot = slot.clone();
            human.set_observer(observer);
        } else if index == 1 {
            human.raw_slot_index = Some(0);
            human.slot = crate::skirmish_launch::SkirmishLocalSlot {
                country: crate::skirmish_launch::LaunchCountry::from_country_index(5),
                country_random: false,
                color_index: 2,
                color_random: false,
                start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
                team: crate::skirmish_launch::LaunchTeam::None,
            };
        } else {
            human.raw_slot_index = Some(next_row);
            next_row += 1;
        }
    }
    launch.selected_map_file = Some(file.clone());
    launch.rebuild_house_roster();
    let map_metadata = crate::app::shell_network_setup::local_map_metadata(state, &file)?;
    crate::app::shell_network_setup::service_event(
        state,
        NetworkSetupEvent::HostSetup {
            launch,
            map_metadata,
            vacancies: [crate::app::match_runtime::session::NetworkVacancy::Open; 8],
            observer_row: None,
            cooperative_difficulty: 0,
            seed: Some(HOST_SEED),
        },
    )
}

fn answer(state: &mut AppState, request: WolServiceRequest) {
    let event = match request {
        WolServiceRequest::EnsureApi => WolServiceEvent::Api(WolApiStatus::Available),
        WolServiceRequest::ListCredentials => {
            WolServiceEvent::Credentials(state.frontend.wol_test_credentials.clone())
        }
        WolServiceRequest::StoreCredential {
            nickname, password, ..
        } => {
            store_credential(&mut state.frontend.wol_test_credentials, nickname, password);
            return;
        }
        WolServiceRequest::ForgetCredential { slot } => {
            state
                .frontend
                .wol_test_credentials
                .retain(|stored| stored.slot != slot);
            return;
        }
        WolServiceRequest::FetchServers { .. } => {
            service_event(state, WolServiceEvent::NetStatus(SERVER_STATUS_READY));
            WolServiceEvent::ServerList(Ok(servers()))
        }
        WolServiceRequest::Connect { .. } => {
            WolServiceEvent::Login(Ok(WolLoginAccepted { motd: MOTD.into() }))
        }
        WolServiceRequest::ListChannels { kind, .. } => WolServiceEvent::Channels {
            kind,
            channels: if kind == GAME_CHANNEL {
                games()
            } else {
                lobbies()
            },
        },
        WolServiceRequest::JoinChannel { mut channel, .. } => {
            let local = nickname(state);
            let lobby = channel.kind != GAME_CHANNEL;
            let mut members = if lobby {
                lobby_members()
            } else {
                vec![member(host_of(&channel.name), MEMBER_OPERATOR)]
            };
            members.push(member(&local, 0));
            channel.users = members.len() as i32;
            service_event(
                state,
                WolServiceEvent::Joined(Ok(WolJoined { channel, members })),
            );
            if lobby {
                lobby_greeting(state);
            }
            return;
        }
        WolServiceRequest::Chat {
            line: WolChatLine::Locate { name },
            ..
        } => WolServiceEvent::Located(locate(&String::from_utf16_lossy(&name))),
        WolServiceRequest::Locate(name) => WolServiceEvent::Located(locate(&name)),
        WolServiceRequest::ListBuddies => {
            WolServiceEvent::Buddies(Ok(state.frontend.wol_test_buddies.clone()))
        }
        // The add/delete result sinks (+0x98/+0x9C) ignore the answer.
        WolServiceRequest::AddBuddy(name) => {
            let buddies = &mut state.frontend.wol_test_buddies;
            if !buddies.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
                buddies.push(name);
            }
            return;
        }
        WolServiceRequest::RemoveBuddy(name) => {
            state.frontend.wol_test_buddies.retain(|known| *known != name);
            return;
        }
        WolServiceRequest::LadderRecords(names) => WolServiceEvent::Ladder(
            names
                .iter()
                .filter(|name| name.eq_ignore_ascii_case("Tanya"))
                .map(|_| tanya_ladder())
                .collect(),
        ),
        WolServiceRequest::Chat {
            line: WolChatLine::Page { name, .. },
            ..
        } => {
            page(state, &String::from_utf16_lossy(&name));
            return;
        }
        WolServiceRequest::Chat {
            line: WolChatLine::Reply { .. },
            ..
        } => {
            let name = super::last_pager(state).unwrap_or_default().to_owned();
            page(state, &name);
            return;
        }
        // OnPart arrives only for a channel this user is in.
        WolServiceRequest::LeaveChannel if super::lobby::in_channel(state) => {
            WolServiceEvent::ChannelLeft
        }
        WolServiceRequest::CreateChannel { .. } => {
            state.frontend.wol_test_guest = WolTestGuest::Expected;
            let local = nickname(state);
            WolServiceEvent::Created(Ok(vec![member(&local, MEMBER_OPERATOR)]))
        }
        // The locale it holds for each name: the stand-ins' own, and for
        // anyone else what the table last received (0 at first, so a new
        // persona is asked with 0xFE).
        WolServiceRequest::RequestLocales { names } => {
            let locales = names
                .into_iter()
                .map(|name| {
                    let locale = stand_in_locale(&name)
                        .or_else(|| {
                            state
                                .frontend
                                .wol
                                .as_ref()
                                .and_then(|visit| visit.locales.get(&name.to_ascii_lowercase()))
                                .copied()
                        })
                        .unwrap_or(0);
                    (name, locale)
                })
                .collect();
            WolServiceEvent::UserLocales(Ok(locales))
        }
        // Every WOL:Locale00..37 the retail CSF holds.
        WolServiceRequest::LocaleCount => WolServiceEvent::LocaleCount(38),
        // Nothing to cancel or deliver: nobody is online.
        WolServiceRequest::SetLocale(_)
        | WolServiceRequest::LeaveChannel
        | WolServiceRequest::Logout
        | WolServiceRequest::CancelWait
        | WolServiceRequest::Chat { .. }
        | WolServiceRequest::Emote { .. }
        // Server-side page/find/language enforcement is not simulated.
        | WolServiceRequest::SetLangFilter(_)
        | WolServiceRequest::SetFindPage { .. } => return,
    };
    service_event(state, event);
}

/// What the stand-ins say in a chat lobby, and what the user tooltip shows
/// for Tanya.
fn lobby_greeting(state: &mut AppState) {
    let line = |from, text: &str| WolServiceEvent::Message {
        from,
        text: text.encode_utf16().collect(),
        private: false,
        action: false,
    };
    for event in [
        WolServiceEvent::Ladder(vec![tanya_ladder()]),
        WolServiceEvent::UserLocales(Ok(vec![("Tanya".into(), 2)])),
        line(
            member("Tanya", MEMBER_OPERATOR),
            "Welcome to the VERA20k test lobby.",
        ),
        line(member("Boris", MEMBER_VOICE), "Anyone up for a game?"),
    ] {
        service_event(state, event);
    }
}

/// Tanya's ladder record, the only one the service holds.
fn tanya_ladder() -> WolLadderRecord {
    WolLadderRecord {
        name: "Tanya".into(),
        wins: 12,
        losses: 3,
        points: 1520,
        rank: 1,
        disconnects: 0,
    }
}

/// The buddy list a fresh process starts with: Tanya (in a chat lobby),
/// Boris (in his game), Ghost (offline) and Ivan (find disabled).
pub(crate) fn default_buddies() -> Vec<String> {
    ["Tanya", "Boris", "Ghost", "Ivan"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Tanya's locale (United States); the other stand-ins have none.
fn stand_in_locale(name: &str) -> Option<i32> {
    name.eq_ignore_ascii_case("Tanya").then_some(2)
}

/// A page reaches a stand-in, who pages back from a current client (the
/// UTF-16 sink); anyone else is gone (`0x40133`).
fn page(state: &mut AppState, name: &str) {
    let Some(stand_in) = lobby_members()
        .into_iter()
        .find(|member| member.name.eq_ignore_ascii_case(name))
    else {
        service_event(state, WolServiceEvent::PageResult(Err(0x0004_0133)));
        return;
    };
    service_event(state, WolServiceEvent::PageResult(Ok(())));
    service_event(
        state,
        WolServiceEvent::Page {
            from: stand_in,
            text: "Got your page.".encode_utf16().collect(),
            ansi: false,
        },
    );
}

/// A locate finds Boris in his game, Ivan with find disabled (`0x40132`)
/// and the other lobby stand-ins in the first chat lobby; anyone else is
/// not logged in (`0x40130`).
fn locate(name: &str) -> Result<WolChannel, u32> {
    if name.eq_ignore_ascii_case("Boris") {
        return Ok(games().remove(1));
    }
    if name.eq_ignore_ascii_case("Ivan") {
        return Err(0x0004_0132);
    }
    if lobby_members()
        .iter()
        .any(|member| member.name.eq_ignore_ascii_case(name))
    {
        Ok(lobbies().remove(0))
    } else {
        Err(0x0004_0130)
    }
}

fn nickname(state: &AppState) -> String {
    state.frontend.wol_account.nickname.clone()
}

fn member(name: &str, flags: u32) -> WolMember {
    WolMember {
        flags,
        name: name.into(),
        clan_name: String::new(),
        clan_tag: String::new(),
    }
}

/// Record 0 is never connected to (`0x0078753D`); record 1 is selected.
fn servers() -> WolServerList {
    WolServerList {
        servers: ["Ladder", "VERA20k Test Server"]
            .into_iter()
            .map(|name| WolServer {
                name: format!("vera20k.test:{name}"),
            })
            .collect(),
        selected: 1,
    }
}

fn lobbies() -> Vec<WolChannel> {
    [(0, 4), (1, 0), (2, 0)]
        .into_iter()
        .map(|(number, users)| WolChannel {
            kind: 0,
            max_users: 100,
            users,
            ping: -1,
            name: format!("Lob_41_{number}"),
            ..Default::default()
        })
        .collect()
}

fn lobby_members() -> Vec<WolMember> {
    vec![
        member("Tanya", MEMBER_OPERATOR),
        member("Boris", MEMBER_VOICE),
        member("Natasha", 0),
    ]
}

/// Open games of other stand-in hosts, and one that is full. Each keeps an
/// observer slot, which the shown capacity leaves out.
fn games() -> Vec<WolChannel> {
    let game = |host: &str, users, max_users, ping| WolChannel {
        kind: GAME_CHANNEL,
        max_users,
        users,
        ping,
        name: format!("{host}'s_game"),
        ex_info: format!("{EX_INFO_VERSION},0,0,0,1,"),
        ..Default::default()
    };
    vec![
        game("Tanya", 1, 7, 40),
        game("Boris", 2, 7, 180),
        game("Natasha", 6, 7, 350),
    ]
}

/// `<host>'s_game` names its host; any other name gets a generic one.
fn host_of(channel: &str) -> &str {
    channel.strip_suffix("'s_game").unwrap_or("Host")
}

/// Remember after a login: the same nickname keeps its slot with the new
/// password, a new one takes the first free slot of 1..32; with none free
/// it is dropped. The stores' real slot policy is WOLAPI's and unknown.
fn store_credential(stored: &mut Vec<WolCredential>, nickname: String, password: String) {
    if let Some(existing) = stored
        .iter_mut()
        .find(|stored| stored.nickname.eq_ignore_ascii_case(&nickname))
    {
        existing.password = password;
        return;
    }
    let Some(slot) = (1..=32).find(|slot| stored.iter().all(|stored| stored.slot != *slot)) else {
        return;
    };
    stored.push(WolCredential {
        slot,
        nickname,
        password,
    });
    stored.sort_by_key(|stored| stored.slot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_accounts_keep_their_slot_and_fill_the_first_free_one() {
        let mut stored = Vec::new();
        store_credential(&mut stored, "Tester".into(), "aaaaaaaa".into());
        store_credential(&mut stored, "Alpha".into(), "bbbbbbbb".into());
        stored.retain(|stored| stored.slot != 1);
        store_credential(&mut stored, "Omega".into(), "cccccccc".into());
        store_credential(&mut stored, "alpha".into(), "dddddddd".into());
        let slots: Vec<(u32, &str, &str)> = stored
            .iter()
            .map(|s| (s.slot, s.nickname.as_str(), s.password.as_str()))
            .collect();
        assert_eq!(slots, [(1, "Omega", "cccccccc"), (2, "Alpha", "dddddddd")]);
    }

    #[test]
    fn stand_in_games_admit_a_join_and_name_their_hosts() {
        use crate::ui::wol_lobby::{AdmissionContext, admit};
        let context = AdmissionContext {
            quick_match: false,
            clan_id: 0,
            local_hash: None,
        };
        let games = games();
        assert_eq!(admit(&games[0], false, context), Ok(()));
        assert_eq!(admit(&games[2], false, context), Err(Some("WOL:GameFull")));
        assert_eq!(host_of(&games[1].name), "Boris");
        assert!(lobbies()[0].lobby_number() == Some(0));
        let list = servers();
        assert_eq!(list.selected, 1);
    }
}
