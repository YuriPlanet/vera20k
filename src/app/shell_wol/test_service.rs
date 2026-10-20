//! VERA's offline WOL test service: a developer stand-in behind the same
//! request/event boundary a real service will use, chosen by the Options
//! page's `[VERA] WolTestService=` box. It is not gamemd behavior: it lets
//! the Internet screens (login `0x10F`, lobby `0x113`, setup C2/C9) be
//! opened and clicked without a server.
//!
//! It answers the visit's and the WOL setup's requests once no shell
//! transition runs: like a real service, a reply arrives after the page that
//! asked has slid out. Any login is accepted. The lobbies, games and users
//! are fixed stand-ins; nobody else ever joins, chats or starts, so a
//! created game cannot be started and a joined game never receives its
//! host's options.

use super::lobby::WolJoined;
use super::{WolApiStatus, WolLoginAccepted, WolServer, WolServerList, WolServiceEvent};
use super::{WolProvider, WolServiceRequest, service_event};
use crate::app::AppState;
use crate::app::match_runtime::session::SessionMode;
use crate::app::shell_network_setup::{NetworkSetupEvent, NetworkSetupRequest};
use crate::ui::wol_lobby::{
    EX_INFO_VERSION, GAME_CHANNEL, MEMBER_OPERATOR, MEMBER_VOICE, WolChannel, WolMember,
};
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

/// The WOL setup's own requests; this service answers only the guest's
/// leave.
fn setup_requests(state: &mut AppState) -> Vec<NetworkSetupRequest> {
    if state.match_state.session.mode() != SessionMode::Wol
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
    // C9 Back waits for the leave's OnPart; C2 Back closes at once.
    if request == NetworkSetupRequest::Leave
        && !state.match_state.session.host()
        && let Err(error) =
            crate::app::shell_network_setup::service_event(state, NetworkSetupEvent::ChannelLeft)
    {
        log::warn!("WOL test service: channel left refused: {error:#}");
    }
}

fn answer(state: &mut AppState, request: WolServiceRequest) {
    let event = match request {
        WolServiceRequest::EnsureApi => WolServiceEvent::Api(WolApiStatus::Available),
        WolServiceRequest::ListCredentials => WolServiceEvent::Credentials(Vec::new()),
        WolServiceRequest::FetchServers { .. } => {
            service_event(state, WolServiceEvent::ServerStatus(SERVER_STATUS_READY));
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
            let mut members = if channel.kind == GAME_CHANNEL {
                vec![member(host_of(&channel.name), MEMBER_OPERATOR)]
            } else {
                lobby_members()
            };
            members.push(member(&local, 0));
            channel.users = members.len() as i32;
            WolServiceEvent::Joined(Ok(WolJoined { channel, members }))
        }
        // OnPart arrives only for a channel this user is in.
        WolServiceRequest::LeaveChannel if super::lobby::in_channel(state) => {
            WolServiceEvent::ChannelLeft
        }
        WolServiceRequest::CreateChannel { .. } => {
            let local = nickname(state);
            WolServiceEvent::Created(Ok(vec![member(&local, MEMBER_OPERATOR)]))
        }
        // Nothing to cancel, remember, forget or deliver: nobody is online.
        WolServiceRequest::LeaveChannel
        | WolServiceRequest::CancelWait
        | WolServiceRequest::ForgetCredential { .. }
        | WolServiceRequest::StoreCredential { .. }
        | WolServiceRequest::Chat { .. }
        | WolServiceRequest::Emote { .. } => return,
    };
    service_event(state, event);
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

#[cfg(test)]
mod tests {
    use super::*;

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
