//! The custom lobby runner `0x00787770` and dialog `0x113`'s app adapter.
//!
//! The runner creates `0x113` hidden behind the wait box, prints the MOTD,
//! requests the chat-lobby list, joins a lobby picked by `0x00788180` and
//! only then shows the dialog. While it shows, its proc `0x0078AC10` drives
//! the lists, chat, sorts, Create (`0x00789B60`) and Join (`0x00789960`
//! with admission `0x0079BA60` and the join `0x00788800`), and the chat
//! sink's OnJoin (`0x007A5890`) answers joins. Every service exchange is a
//! [`WolServiceRequest`]/[`WolServiceEvent`]; nothing here fabricates a
//! result. No RNG is drawn on these paths.
//!
//! Records the WOL globals hold (`0xB76AE8` games, `0xB73E18` lobbies,
//! `0xB77908` users, `0xB76B44` current and `0xB73BE8` pending channel) live
//! in [`WolLobby`]; the dialog's children in [`WolLobbyState`]. The process
//! globals the runner keeps across visits are [`WolLobbyGlobals`].
//!
//! The Buddy List runner `0x0079F140` (WOL mode 5, [`super::buddy`]) runs
//! the same Create (`0x00789B60`) and Join (`0x00789960`) over the empty
//! backdrop with `0x116` ended, so a [`WolLobby`] with [`Runner::Buddy`]
//! carries its records and its Create/Join steps; `0x113` never shows then,
//! and where that runner's tails differ the buddy owner continues. The
//! Quick Match runner `0x0079FC40` ([`super::quick_match`],
//! [`Runner::QuickMatch`]) likewise keeps its lobby, game channel and users
//! here, with the native mode 1-3 branches of the shared sinks.

use crate::app::shell_route::ShellRoute;
use crate::util::time::{Duration, Instant};

use super::WolPage;
use super::service::{self, WolServiceEvent, WolServiceRequest};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::shell_network_setup as setup;
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::modal::ShellBox;
use crate::ui::wol_chat::{self, WolChatFilter, WolChatLine, WolLobbyChatLine};
use crate::ui::wol_lobby::*;
use crate::ui::wol_shell::WolWaitBox;

mod create;
mod events;
mod help;
mod input;
mod join;
mod runners;
mod visit;
use help::scenario_description;

use super::{
    WOL_LOBBY_DIALOG, WolProvider, WolVisit, buddies, buddy, leaf, page, persona, player_card,
    quick_match, translation,
};
#[cfg(test)]
use create::republished_limit;
#[cfg(test)]
use create::slot_ex_info;
pub(crate) use create::{commit_create, republish_channel};
use create::{
    create_confirmed, create_ended, created_channel, ex_info, leave_then_create, send_create,
};
use events::in_setup_channel;
pub(super) use events::{
    ReceivedMessage, channel_ban, kicked, located, message_received, net_status, page_received,
    page_result, service_event,
};
pub(super) use help::locale_name;
pub(crate) use help::{badge_help, player_badge, status_help};
pub(crate) use input::{create_layout, ime_edit_rect, key_press, popup_scale, sync};
pub(super) use input::{cursor_moved, mouse};
#[cfg(test)]
use join::JoinErrorReply;
#[cfg(test)]
use join::join_error_reply;
pub(crate) use join::{
    KEPT_OUT_JOIN_ERROR, ROOM_NAME_TAKEN, WATCH_MATCH_ACTIVE, WATCH_UNKNOWN_MATCH, WATCHERS_FULL,
};
use join::{WATCH_LIST_TAG, created, enter_game_as_guest, join_failed_tail, joined, request_games};
pub(crate) use runners::host_setup_left;
use runners::new_runner;
#[cfg(test)]
use runners::quick_match_channel;
pub(super) use runners::{
    buddy_dialog_opens, buddy_find_game, buddy_join, buddy_runner_resumes, open_runner,
    quick_match_box, quick_match_create, quick_match_join, quick_match_join_game,
    quick_match_leave_and_return,
};
use visit::{
    back_to_room, selected_admits, take_visit, visit_failed, visit_joined, visit_room_left,
    visit_settles, visit_user_joined, visit_user_left,
};
pub(super) use visit::{page_target, visit_ends};
pub(crate) use visit::{visit_lobby, visiting};

/// The list wait `0x0077F120` gives up after 30 s.
const LIST_WAIT: Duration = Duration::from_millis(30_000);
// The initial VERA public lobby; native diagnostic catalogs keep their rooms.
pub(super) const PUBLIC_LOBBY_NAME: &str = "Lob_41_2";
/// The modern UI's name for `PUBLIC_LOBBY_NAME` in place of its base name
/// `TXT_LOB_3` (index `2 % 8`, retail "German Bunker"); the legacy UI keeps
/// the retail names.
const PUBLIC_LOBBY_TITLE: &str = "VERA20K Online";
const PUBLIC_LOBBY_BASE: usize = 2;
/// `0x00788800`/`0x00789B60`: at most 150 x 100 ms for the leave.
const LEAVE_WAIT: Duration = Duration::from_millis(15_000);
/// The game list. VERA asks every 5 s, so a game someone creates shows up
/// soon; native `SetTimer(0, 30000)` asked every 30.
const GAMES_PERIOD: Duration = Duration::from_millis(5_000);
/// `SetTimer(1, 53000)`: the lobbies.
const LOBBIES_PERIOD: Duration = Duration::from_millis(53_000);
/// Refresh `0x55D` disables itself for `SetTimer(0x55D, 3000)`.
const REFRESH_HOLD: Duration = Duration::from_millis(3000);
/// RESIDUAL: a custom game's channel takes 7 users natively (`0xC3`'s proc
/// `0x0078DB70` sets `[0xB77DC4] = 7` at `0x0078DC57`, which `0x00789B60`
/// copies into the record); VERA's takes every network slot, for 30 players.
const CREATE_MAX_USERS: i32 = crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32;
/// The create result `0x800401F8` shows `TXT_BAD_CHANKEY`.
const BAD_CHANNEL_KEY: u32 = 0x8004_01F8;

/// Globals the lobby keeps for the process (BSS 0 at start; the WOL reset
/// `0x00780110` leaves them alone).
#[derive(Debug, Clone, Default)]
pub(crate) struct WolLobbyGlobals {
    /// `[0xB77DD4]`: the chat lobby number the picker rejoins.
    pub(crate) lobby_number: i32,
    /// `[0xB77E44]` and `[0xB77E48]`.
    pub(crate) game_sort: GameSort,
    pub(crate) player_sort: PlayerSort,
    /// `[0xA8B540]`: Create's Limit Screen Resolution.
    pub(crate) limit_resolution: bool,
    /// `[0xB77E8C]`: the `0xCF` control that last notified, never reset.
    pub(crate) page_last_control: u16,
    /// `[0xB76478..0xB76480]`: the in-match page's reply prefill window.
    pub(crate) page_reply_window: crate::ui::wol_page::PageReplyWindow,
    /// The ladder table `0xB77D80` (`[0xB77E0C]` outside mode 3; `0x00780110`
    /// clears only the clan and mode-3 tables) and the high-score table
    /// `0xB76640`, keyed by the ASCII-lowered name the lookups use
    /// (`0x007B5B30`).
    /// RESIDUAL: Quick Match's mode-3 ladder table `0xB73AF8`, which the
    /// WOL reset clears, shares this table.
    pub(crate) ladder: std::collections::HashMap<String, service::WolLadderRecord>,
    pub(crate) high_scores: std::collections::HashMap<String, service::WolHighScoreRecord>,
    /// VERA: the VERA20K Online season the tables were filled in, `None`
    /// before its first `ladder` event ([`Self::ladder_season`]).
    pub(crate) ladder_season: Option<Option<u32>>,
    /// VERA: what the online page's games hide, as the player last ticked it
    /// (Hide Full, Hide Locked).
    pub(crate) room_filter: RoomFilter,
}

impl WolLobbyGlobals {
    /// VERA: a `ladder` event from VERA20K Online's `season` (`None` when
    /// none runs). A season other than the one the tables were filled in
    /// empties both, so the season before's ranks and badges go; true then,
    /// for the listed players to be asked again (docs/lobby-protocol.md).
    pub(crate) fn ladder_season(&mut self, season: Option<u32>) -> bool {
        let changed = self.ladder_season.is_some_and(|held| held != season);
        self.ladder_season = Some(season);
        if changed {
            self.ladder.clear();
            self.high_scores.clear();
        }
        changed
    }
}

/// The user-list sink sends its ladder batch once it holds more than 20
/// names (`0x007A74E3`) and takes names from the first 99 users only
/// (`0x007A7454`).
const LADDER_BATCH_LIMIT: usize = 20;
const LADDER_LISTED_USERS: usize = 100;

/// The decoded result of a joined channel: its record and its users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolJoined {
    pub(crate) channel: WolChannel,
    pub(crate) members: Vec<WolMember>,
}

#[derive(Debug, Clone)]
enum Step {
    /// The runner's first list (`ListChannels(0, 0)`) behind the wait box.
    ListingLobbies {
        deadline: Instant,
    },
    /// A chat-lobby join; `entry` shows the dialog afterwards.
    JoiningLobby {
        entry: bool,
        request: Option<OnlineRequest>,
    },
    Ready,
    /// A leave before a join or create (`WOL:LeavingChannel`).
    Leaving {
        deadline: Instant,
        then: AfterLeave,
    },
    Creating {
        observe: bool,
        channel: WolChannel,
        request: Option<OnlineRequest>,
    },
    JoiningGame {
        request: Option<OnlineRequest>,
    },
    /// The visitor keeps their seat until the online lobby commits the join.
    SwitchingRoom {
        request: Option<OnlineRequest>,
    },
    /// The Buddy List runner's second game-list request for Join.
    FindingGame {
        name: String,
        deadline: Instant,
    },
    /// VERA: a watch row's `watch` behind the wait box, until the lobby
    /// grants it ([`watch_admitted`]) or refuses it.
    Watching {
        match_id: String,
        request: Option<OnlineRequest>,
    },
    /// VERA: the room's Lobby, until the public lobby's `joined` marked
    /// `visit` ([`visit_lobby`]).
    Visiting {
        request: Option<OnlineRequest>,
    },
}

/// Correlation for the online provider only. The lobby step continues to own
/// admission; this ID prevents an old HTTP error from ending a later attempt.
#[derive(Debug, Clone, Copy)]
struct OnlineRequest {
    id: u64,
    deadline: Instant,
}

impl Step {
    fn online_request(&self) -> Option<OnlineRequest> {
        match self {
            Self::JoiningLobby { request, .. }
            | Self::JoiningGame { request }
            | Self::SwitchingRoom { request }
            | Self::Creating { request, .. }
            | Self::Watching { request, .. }
            | Self::Visiting { request } => *request,
            _ => None,
        }
    }

    fn online_request_mut(&mut self) -> Option<&mut Option<OnlineRequest>> {
        match self {
            Self::JoiningLobby { request, .. }
            | Self::JoiningGame { request }
            | Self::SwitchingRoom { request }
            | Self::Creating { request, .. }
            | Self::Watching { request, .. }
            | Self::Visiting { request } => Some(request),
            _ => None,
        }
    }

    fn bind_online_request(&mut self, id: u64, now: Instant) -> bool {
        let Some(request) = self.online_request_mut() else {
            return false;
        };
        *request = Some(OnlineRequest {
            id,
            deadline: now + LIST_WAIT,
        });
        true
    }

    fn accepts_online_result(&self, id: u64) -> bool {
        self.online_request()
            .is_some_and(|request| request.id == id)
    }

    // A switch is retried under its original request ID. A timed leave
    // would evict the player from their original room on a lost reply.
    fn timed_request(&self) -> Option<OnlineRequest> {
        if matches!(self, Self::SwitchingRoom { .. }) {
            None
        } else {
            self.online_request()
        }
    }

    fn take_leave_continuation(&mut self) -> Option<AfterLeave> {
        if !matches!(self, Self::Leaving { .. }) {
            return None;
        }
        let Self::Leaving { then, .. } = std::mem::replace(self, Self::Ready) else {
            unreachable!("checked leaving step");
        };
        Some(then)
    }
}

/// Bind the transport request to the existing join/create step before sending.
pub(super) fn bind_online_request(state: &mut AppState, id: u64) -> bool {
    lobby_mut(state).is_some_and(|lobby| lobby.step.bind_online_request(id, Instant::now()))
}

/// Whether a response still belongs to the current lobby admission.
pub(super) fn online_request_pending(state: &AppState, id: u64) -> bool {
    lobby(state).is_some_and(|lobby| lobby.step.accepts_online_result(id))
}

pub(super) fn switching_room(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| matches!(lobby.step, Step::SwitchingRoom { .. }))
}

/// A rejected online command enters the same reducer as a service callback.
/// Stale failures cannot close a different attempt, even for the same room.
pub(super) fn online_request_failed(state: &mut AppState, id: u64, code: u32) -> bool {
    let Some(lobby) = lobby(state) else {
        return false;
    };
    if !lobby.step.accepts_online_result(id) {
        return false;
    }
    let event = match lobby.step {
        Step::Creating { .. } => WolServiceEvent::Created(Err(code)),
        Step::Watching { .. } => WolServiceEvent::WatchRefused(code),
        Step::Visiting { .. } => {
            visit_failed(state);
            return true;
        }
        _ => WolServiceEvent::Joined(Err(code)),
    };
    service::service_event(state, event);
    true
}

#[derive(Debug, Clone)]
enum AfterLeave {
    Join {
        channel: WolChannel,
        key: String,
        game: bool,
        entry: bool,
    },
    Create {
        channel: WolChannel,
        key: String,
        observe: bool,
        ranked: bool,
    },
    /// The Quick Match runner's exit (`0x007A03A0..0x007A0410`): it
    /// returns 0 whether or not the leave finished.
    Return,
}

/// What closing a lobby box continues with.
#[derive(Debug, Clone)]
enum BoxThen {
    /// `0x007A5890`'s failure tail; `close` when it set event 5.
    JoinFailed { close: bool },
    /// Create's boxes: the lobby back with Join disabled.
    CreateFailed,
    /// VERA: a refused watch's box: the lobby's commands back.
    WatchFailed,
    /// `0x00789960`'s `WOL:ResolutionLimited*` Yes/No.
    Resolution { channel: WolChannel },
    /// The runner gives up and returns 0: the welcome.
    Close,
    /// `0x007AB670`'s box for this user's kick out of a chat lobby.
    Kicked,
    /// A box of the Quick Match runner.
    QuickMatch(QuickMatchBoxThen),
}

/// What closing a Quick Match runner's box continues with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickMatchBoxThen {
    /// The runner returns 0 at once (the lobby-list boxes,
    /// `0x0079FD37..0x0079FE44`).
    Return,
    /// The runner's exit tail, then 0 (`0x007A0282`).
    Exit,
    /// Create's boxes: `0x00789B60` returned 0.
    CreateFailed,
}

/// Which runner owns the records and the Create/Join steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Runner {
    /// The custom lobby runner `0x00787770` with `0x113`.
    Lobby,
    /// The Buddy List runner `0x0079F140`: `0x113` never exists.
    Buddy,
    /// The Quick Match runner `0x0079FC40` with `0x117`.
    QuickMatch,
}

/// Result of an ordinary refresh of the public lobby's watch list.
pub(super) enum WatchListRefresh {
    Failed,
    Listed,
    Unsupported,
}

/// One runner of the custom lobby.
#[derive(Debug)]
pub(crate) struct WolLobby {
    pub(crate) runner: Runner,
    pub(crate) ui: WolLobbyState,
    games: Vec<WolChannel>,
    lobbies: Vec<WolChannel>,
    members: Vec<WolMember>,
    current: Option<WolChannel>,
    pending: Option<WolChannel>,
    /// `0x113` shows (it is created hidden).
    pub(crate) shown: bool,
    /// `Shell__DrawEmptyBackdrop` painted over the dialog for a popup or a
    /// Create/Join wait.
    pub(crate) covered: bool,
    pub(crate) wait: Option<WolWaitBox>,
    message: Option<(ShellBox, BoxThen)>,
    step: Step,
    postmatch: bool,
    games_at: Instant,
    lobbies_at: Instant,
    refresh_at: Option<Instant>,
    /// A game join's result that arrived while the dialog slid out.
    deferred: Option<Result<WolJoined, u32>>,
    /// `0x00789960`'s record while its password popup `0xE1` runs.
    joining: Option<WolChannel>,
    /// VERA: the running matches the modern list offers to watch.
    matches: Vec<service::WolMatch>,
    /// This public lobby has already warned about a failed watch-list refresh.
    watch_list_stale: bool,
    /// VERA: the public lobby this player looks round from their game room
    /// ([`visit_lobby`]).
    visit: Option<LobbyVisit>,
}

/// VERA: the public lobby a game room's member visits (docs/lobby-protocol.md
/// "Visiting the lobby"). The runner's channel and members stay the room's,
/// which goes on hidden; the lobby page shows these.
#[derive(Debug)]
struct LobbyVisit {
    channel: WolChannel,
    members: Vec<WolMember>,
}

impl WolLobby {
    fn watch_refresh_notice(&mut self, refresh: WatchListRefresh) -> Option<&'static str> {
        // Quick Match and the buddy runner have no public watch list.
        if self.runner != Runner::Lobby || !self.ui.is_modern() {
            return None;
        }
        let stale = matches!(refresh, WatchListRefresh::Failed);
        let previously_stale = std::mem::replace(&mut self.watch_list_stale, stale);
        match (refresh, previously_stale) {
            (WatchListRefresh::Failed, false) => Some("VERA:WatchListStale"),
            (WatchListRefresh::Listed, true) => Some("VERA:WatchListUpdated"),
            _ => None,
        }
    }

    /// The modern lobby lists only VERA20K Online; the Quick Match runner,
    /// which shows no list, looks for its `Lob_40_` lobby in all of them.
    fn set_lobbies(&mut self, mut channels: Vec<WolChannel>) {
        if self.ui.is_modern() && self.runner != Runner::QuickMatch {
            channels.retain(|channel| channel.name == PUBLIC_LOBBY_NAME);
        }
        self.lobbies = channels;
    }

    /// `[0xB76B10] == 0`: the current channel record's kind is 0, or no
    /// channel is current.
    fn chat_kind_current(&self) -> bool {
        self.current
            .as_ref()
            .is_none_or(|channel| channel.kind == 0)
    }

    /// This user's join of `channel` with its user list. The join sink
    /// `0x007A5890` copies the record into `[0xB76B10]` before the user-list
    /// sink `0x007A7110` lists the users anew: while that kind is 0 each
    /// joins by the player sort, otherwise `0x007ACA00` appends each at the
    /// count, in the listed order (`0x007A72A6..0x007A7346`).
    fn users_listed(
        &mut self,
        channel: WolChannel,
        listed: Vec<WolMember>,
        sort: PlayerSort,
        rank: &dyn Fn(&str) -> i32,
    ) {
        self.current = Some(channel);
        self.members.clear();
        if !self.chat_kind_current() {
            self.members = listed;
            return;
        }
        for member in listed {
            insert_member(&mut self.members, member, sort, rank);
        }
    }

    /// The join sink's insert of another user (`0x007A66B3`): by the player
    /// sort into a channel of kind 0, otherwise [`insert_game_member`].
    fn user_joined(&mut self, member: WolMember, sort: PlayerSort, rank: &dyn Fn(&str) -> i32) {
        if self.chat_kind_current() {
            insert_member(&mut self.members, member, sort, rank);
        } else {
            insert_game_member(&mut self.members, member);
        }
    }

    /// The grant answered the watch command. The map transfer and relay
    /// admission now own their deadlines while the lobby keeps the wait box.
    pub(super) fn watch_granted(&mut self, granted: &str) {
        if let Step::Watching { match_id, request } = &mut self.step
            && match_id == granted
        {
            *request = None;
        }
    }

    pub(crate) fn message(&self) -> Option<&ShellBox> {
        self.message.as_ref().map(|(message, _)| message)
    }

    /// The dialog or its popup takes input: no box, no wait.
    pub(crate) fn interactive(&self) -> bool {
        (self.shown || self.ui.popup.is_some()) && self.message.is_none() && self.wait.is_none()
    }
}

pub(crate) fn layout(state: &AppState) -> WolLobbyLayout {
    shell_layout::on(state, |w, h, scale| match lobby(state) {
        Some(lobby) => lobby.ui.layout(w, h, scale),
        None => WolLobbyLayout::new(w, h, scale, &PAGE),
    })
}

/// [`layout`] as the online page's chat view lays it out, whichever view
/// shows: the log the lobby's chat wraps its messages to.
pub(crate) fn chat_layout(state: &AppState) -> WolLobbyLayout {
    shell_layout::on(state, |w, h, scale| match lobby(state) {
        Some(lobby) => lobby.ui.chat_layout(w, h, scale),
        None => WolLobbyLayout::new(w, h, scale, &PAGE),
    })
}

fn lobby(state: &AppState) -> Option<&WolLobby> {
    state.frontend.wol.as_ref()?.lobby.as_deref()
}

/// The custom lobby runner, whose `0x113` is in the shell stack (also
/// while hidden under C2/C9 or Options).
fn lobby_dialog(state: &AppState) -> Option<&WolLobby> {
    lobby(state).filter(|lobby| lobby.runner == Runner::Lobby)
}

/// The Buddy List runner holds the records.
fn buddy_runner(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.runner == Runner::Buddy)
}

/// The Quick Match runner holds the records.
pub(crate) fn quick_match_runner(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.runner == Runner::QuickMatch)
}

/// The current channel's users `[0xB77908]`, in list order.
pub(super) fn members(state: &AppState) -> &[WolMember] {
    lobby(state).map_or(&[], |lobby| &lobby.members)
}

/// The current channel's user names, in list order (`[0xB77900]` of them).
pub(crate) fn member_names(state: &AppState) -> impl DoubleEndedIterator<Item = &str> {
    members(state).iter().map(|member| member.name.as_str())
}

/// How many users the current channel has (`[0xB77900]`).
pub(crate) fn member_count(state: &AppState) -> usize {
    members(state).len()
}

/// The dialog a runner's closed box or wait returns the input to.
fn runner_dialog(state: &AppState) -> crate::ui::shell::descriptor::DialogResourceId {
    if super::quick_match::page_open(state) {
        crate::ui::wol_quick_match::DIALOG
    } else {
        super::WOL_LOBBY_DIALOG
    }
}

/// The game list `[0xB76AE8]` of the running runner.
pub(super) fn games(state: &AppState) -> &[WolChannel] {
    lobby(state).map_or(&[], |lobby| &lobby.games)
}

/// The chat-lobby list `[0xB73E18]` of the running runner.
pub(super) fn lobbies(state: &AppState) -> &[WolChannel] {
    lobby(state).map_or(&[], |lobby| &lobby.lobbies)
}

/// VERA: the running matches the modern list offers to watch; a watch
/// row's `record` indexes them.
pub(crate) fn watch_matches(state: &AppState) -> &[service::WolMatch] {
    lobby(state).map_or(&[], |lobby| &lobby.matches)
}

/// This user is in a channel (`[0xB77DD0]`): a leave will see its OnPart.
pub(crate) fn in_channel(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.current.is_some())
}

pub(crate) fn current_channel(state: &AppState) -> Option<WolChannel> {
    lobby(state)?.current.clone()
}

/// A leave is out (`WOL:LeavingChannel`): VERA keeps its channel current
/// until the leave lands, where native's wait (`0x00792BE0`) runs nothing
/// else first.
pub(crate) fn leaving(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| matches!(lobby.step, Step::Leaving { .. }))
}

fn lobby_mut(state: &mut AppState) -> Option<&mut WolLobby> {
    state.frontend.wol.as_mut()?.lobby.as_deref_mut()
}

/// The request can finish after the player entered a room. The retained public
/// transcript owns its notice even while the room covers it.
pub(super) fn watch_list_refresh(state: &mut AppState, refresh: WatchListRefresh) {
    if let Some(key) = lobby_mut(state).and_then(|lobby| lobby.watch_refresh_notice(refresh)) {
        crate::app::shell_lan::append_wol_lobby_chat(
            state,
            LanChatLine {
                text: crate::i18n::tr(key).encode_utf16().collect(),
                color: u32::MAX,
            },
            None,
        );
    }
}

fn request(state: &mut AppState, request: WolServiceRequest) {
    service::request(state, request);
}

fn units(state: &AppState, key: &str) -> Vec<u16> {
    state.csf_text(key).encode_utf16().collect()
}

/// `0x005E2700` with color -1 into the lobby's transcript.
fn message_line(state: &mut AppState, text: String) {
    crate::app::shell_lan::append_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

fn echo(state: &mut AppState, text: Vec<u16>, color: u32) {
    crate::app::shell_lan::append_chat(state, LanChatLine { text, color });
}

/// A line of the game room's, which goes to it also while its player visits
/// the lobby ([`crate::app::shell_lan::append_room_chat`]).
fn room_line(state: &mut AppState, text: String) {
    crate::app::shell_lan::append_room_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
}

/// The login nickname `[0xB73E40]` (the MultiPlayer handle).
fn nickname(state: &AppState) -> String {
    state.frontend.wol_account.nickname.clone()
}

fn admission_context(state: &AppState) -> AdmissionContext {
    AdmissionContext {
        quick_match: super::quick_match::mode_active(state),
        clan_id: squad_id(state),
        local_hash: local_ini_hash(state),
        vera_rooms: modern_lobby(state),
    }
}

/// `[0xB77E04] ^ [0xB77E08] ^ [0xB77E00]`.
pub(crate) fn local_ini_hash(state: &AppState) -> Option<i32> {
    state
        .process_assets
        .native_ini_hashes()
        .map(|hashes| hashes.combined())
}

/// The current channel's kind for the room tests `0x0077D940`/`0x0077D970`:
/// tournament `+0x14` and the clan bit of `+0x20` (`[0xB76B24]`,
/// `[0xB76B30]`), copied from the server's record on create and join.
pub(crate) fn room(state: &AppState) -> crate::app::match_runtime::session::NetworkRoom {
    let current = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.lobby.as_deref())
        .and_then(|lobby| lobby.current.as_ref());
    crate::app::match_runtime::session::NetworkRoom {
        tournament: current.is_some_and(|channel| channel.tournament != 0),
        clan: current.is_some_and(|channel| channel.game_flags & GAME_CLAN != 0),
    }
}

/// What the matched OnGameStart and Start Game Now hand the match.
/// `[0xB779C4]` is the current channel's tournament value, 2 in a clan room
/// (`0x007A7D0E..0x007A7D49`). In a clan room Start Game Now splits the
/// channel users, in list order, into two name lists by squad
/// (`0x00795250..0x0079546E`): the first squad seen, or a user while it is
/// 0, joins the first list; the next squad the second; a third is left out.
/// RESIDUAL: native builds the lists when the lobby runner calls Start Game
/// Now (`0x007800EC`), after OnGameStart; VERA takes them at the matched
/// OnGameStart, so a user parting in between stays listed. The WDT builder
/// for `[0xA8B244]` 3 (`0x0079546E..0x00795574`) is dormant for custom games.
pub(crate) fn start_terms(state: &AppState) -> crate::app::match_runtime::session::WolStartTerms {
    let mut terms = lobby(state)
        .and_then(|lobby| Some(start_terms_for(lobby.current.as_ref()?, &lobby.members)))
        .unwrap_or_default();
    terms.quick_coop = super::quick_match::quick_coop(state);
    terms
}

fn start_terms_for(
    current: &WolChannel,
    members: &[WolMember],
) -> crate::app::match_runtime::session::WolStartTerms {
    let clan = current.game_flags & GAME_CLAN != 0;
    let mut squad_lists: [Vec<String>; 2] = Default::default();
    if clan {
        let mut squads = [0; 2];
        for member in members {
            let list = if squads[0] == 0 || member.squad == squads[0] {
                0
            } else if squads[1] == 0 || member.squad == squads[1] {
                1
            } else {
                continue;
            };
            squads[list] = member.squad;
            squad_lists[list].push(member.name.clone());
        }
    }
    crate::app::match_runtime::session::WolStartTerms {
        tournament: if clan { 2 } else { current.tournament },
        squad_lists,
        quick_coop: false,
    }
}

/// C2's clan start test (`0x0079096A..0x00790B18`) over the channel users.
pub(crate) fn clan_teams_even(state: &AppState, observer: Option<&str>) -> bool {
    lobby(state).is_some_and(|lobby| clan_split_even(&lobby.members, observer))
}

/// Every user but the observer row's node is in a squad, there are exactly
/// two squads, and they have as many users each. The observer's name
/// compares under `_stricmp` (`0x007C8D20` at `0x00790A22`).
/// RESIDUAL: native narrows the observer row's name to 9 characters first
/// (`0x007C91D0` at `0x007909E6`); WOL nicknames are no longer.
fn clan_split_even(members: &[WolMember], observer: Option<&str>) -> bool {
    let mut squads: [(i32, usize); 2] = [(0, 0); 2];
    for member in members {
        if observer
            .is_some_and(|name| crate::util::native_string::crt_strcmpi(&member.name, name).is_eq())
        {
            continue;
        }
        if member.squad == 0 {
            return false;
        }
        match squads
            .iter_mut()
            .find(|(id, _)| *id == member.squad || *id == 0)
        {
            Some(entry) => *entry = (member.squad, entry.1 + 1),
            None => return false,
        }
    }
    squads[0].1 == squads[1].1
}

/// `[0xB77DE4]`: the user's squad id.
pub(crate) fn squad_id(state: &AppState) -> i32 {
    state
        .frontend
        .wol
        .as_ref()
        .map_or(0, |visit| visit.squad_id)
}

/// API+0xB0 for squad `id`, counted in `[0xB77DDC]`; 0 asks for the user's
/// own, which `[0xB77DE0]` marks.
fn request_squad(state: &mut AppState, id: i32) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.squad_requests += 1;
    if id == 0 {
        visit.squad_own_pending = true;
    }
    request(state, WolServiceRequest::SquadInfo { id });
}

/// The squad sink `+0x80` (`0x007A4EC0`). The user's own request takes the
/// id even when it failed (`0x007A4EE8..0x007A4F2D`); a failure then ends
/// here (`0x007A4EFC`/`0x007A4F38`). Each channel user of that squad takes
/// its tag and name and is sorted again; a success that leaves no request
/// outstanding refills the player list (`0x007A51F3..0x007A5200`).
/// RESIDUAL: the clan-ladder request for the tag (API2+0x14) is the
/// backend's.
pub(super) fn squad(
    state: &mut AppState,
    id: i32,
    squad: Result<crate::ui::wol_lobby::WolSquad, u32>,
) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.squad_requests = visit.squad_requests.saturating_sub(1);
    let last = visit.squad_requests == 0;
    if visit.squad_own_pending {
        visit.squad_own_pending = false;
        visit.squad_id = id;
    }
    let Ok(squad) = squad else {
        return;
    };
    if let Some(lobby) = lobby_mut(state) {
        let (mut matched, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut lobby.members)
            .into_iter()
            .partition(|member| member.squad == id);
        lobby.members = rest;
        for member in &mut matched {
            member.clan_tag = squad.tag.clone();
            member.clan_name = squad.name.clone();
        }
        insert_members(state, matched);
    }
    if last {
        refill_players(state);
    }
}

/// One request of the user-list sink, in its order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ListedRequest {
    Squad(i32),
    /// Ladder and high-score requests with the same lowered names.
    Ladder(Vec<String>),
}

/// The user-list sink's requests (`0x007A7187..0x007A781A`), user by user
/// in the listed order (the user included): a squad whose tag is not
/// known yet is asked (`0x007A7193..0x007A71C6`); the lowered name of each
/// of the first 99 users the ladder table lacks joins the batch, which
/// goes out whenever it holds more than 20 names, and once more at the
/// end when not empty.
/// RESIDUAL: the clan-ladder request for a known tag (API+0x14 with SKU
/// `0x802100`, `0x007A71D1..0x007A7267`) is the backend's.
fn listed_requests(listed: &[WolMember], ranked: impl Fn(&str) -> bool) -> Vec<ListedRequest> {
    let mut requests = Vec::new();
    let mut batch = Vec::new();
    for (position, member) in listed.iter().enumerate() {
        if member.squad != 0 && member.clan_tag.is_empty() {
            requests.push(ListedRequest::Squad(member.squad));
        }
        let key = member.name.to_ascii_lowercase();
        if !ranked(&key) && position + 1 < LADDER_LISTED_USERS {
            batch.push(key);
        }
        if batch.len() > LADDER_BATCH_LIMIT {
            requests.push(ListedRequest::Ladder(std::mem::take(&mut batch)));
        }
    }
    if !batch.is_empty() {
        requests.push(ListedRequest::Ladder(batch));
    }
    requests
}

/// VERA: the modern lobby asks for every listed user, not only those its
/// table lacks, since VERA20K Online's records change with each rated game
/// while the table lasts the process; the native lobby keeps its records.
fn request_listed(state: &mut AppState, listed: &[WolMember]) {
    let modern = modern_lobby(state);
    let ladder = &state.frontend.wol_lobby_globals.ladder;
    let requests = listed_requests(listed, |key| !modern && ladder.contains_key(key));
    for listed in requests {
        match listed {
            ListedRequest::Squad(squad) => request_squad(state, squad),
            ListedRequest::Ladder(names) => request_ladders(state, names),
        }
    }
}

/// API vt+0x14 and vt+0x38 with the same names.
fn request_ladders(state: &mut AppState, names: Vec<String>) {
    request(state, WolServiceRequest::LadderRecords(names.clone()));
    request(state, WolServiceRequest::HighScores(names));
}

/// The ladder rank `0x00782320` sorts by: the ladder table's, else -1.
fn ladder_rank(
    ladder: &std::collections::HashMap<String, service::WolLadderRecord>,
    name: &str,
) -> i32 {
    ladder
        .get(&name.to_ascii_lowercase())
        .map_or(-1, |record| record.rank)
}

/// The runner with the player sort `[0xB77E48]` and the ladder rank
/// `0x00782320` sorts by.
fn sort_lobby(
    state: &mut AppState,
    then: impl FnOnce(&mut WolLobby, PlayerSort, &dyn Fn(&str) -> i32),
) {
    let globals = &state.frontend.wol_lobby_globals;
    let Some(lobby) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.lobby.as_deref_mut())
    else {
        return;
    };
    then(lobby, globals.player_sort, &|name: &str| {
        ladder_rank(&globals.ladder, name)
    });
}

/// Each member joins the lobby's users by the player sort.
fn insert_members(state: &mut AppState, members: impl IntoIterator<Item = WolMember>) {
    sort_lobby(state, |lobby, sort, rank| {
        for member in members {
            insert_member(&mut lobby.members, member, sort, rank);
        }
    });
}

/// VERA: a `ladder` event's season ([`WolLobbyGlobals::ladder_season`]).
/// When it changed, the player list drops the ranks and badges it showed
/// and, outside a match, asks again for its users' records; after a match
/// the lobby's list asks anyway.
pub(super) fn ladder_season(state: &mut AppState, season: Option<u32>) {
    if !state.frontend.wol_lobby_globals.ladder_season(season) {
        return;
    }
    refill_players(state);
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game)
    {
        return;
    }
    let listed = lobby(state).map_or_else(Vec::new, |lobby| lobby.members.clone());
    request_listed(state, &listed);
}

/// The ladder sink's re-sort (`0x007A3966..0x007A39C4`): under a rank
/// sort, while the current channel's kind (`[0xB76B10]`) is 0, the user of a
/// stored record leaves the users and joins them again.
pub(super) fn rank_changed(state: &mut AppState, name: &str) {
    let sort = state.frontend.wol_lobby_globals.player_sort;
    if !matches!(sort, 2 | 3) || !chat_kind_current(state) {
        return;
    }
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let Some(at) = lobby
        .members
        .iter()
        .position(|member| crate::util::native_string::crt_strcmpi(&member.name, name).is_eq())
    else {
        return;
    };
    let member = lobby.members.remove(at);
    insert_members(state, [member]);
}

/// `[0xB76B10] == 0`: the current channel record's kind is 0, or no
/// channel is current.
fn chat_kind_current(state: &AppState) -> bool {
    lobby(state).is_none_or(WolLobby::chat_kind_current)
}

/// The high-score sink `0x007A3B30`: each record replaces the table's
/// record of its lowered name. A failure is only logged. Nothing is
/// refreshed.
pub(super) fn high_scores_received(
    state: &mut AppState,
    result: Result<Vec<service::WolHighScoreRecord>, u32>,
) {
    let records = match result {
        Ok(records) => records,
        Err(code) => {
            log::debug!("WOL high scores failed: {code:#x}");
            return;
        }
    };
    let table = &mut state.frontend.wol_lobby_globals.high_scores;
    for record in records {
        table.insert(record.name.to_ascii_lowercase(), record);
    }
}

/// `[0xA8EB84]`: Options' ScreenWidth, the running shell width while the
/// profile holds none.
pub(super) fn options_screen(state: &AppState) -> (i32, i32) {
    let profile = &state.persistence.options_profile;
    if profile.screen_width > 0 && profile.screen_height > 0 {
        (profile.screen_width, profile.screen_height)
    } else {
        let (w, h) = shell_layout::shell_screen_size(state);
        (w as i32, h as i32)
    }
}

/// The `Lob_41_<n>` base names `TXT_LOB_1..8` as this visit shows them.
pub(crate) fn base_names(state: &AppState) -> [String; 8] {
    let modern = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.modern_ui());
    lobby_base_names(modern, |key| state.csf_text(key))
}

fn lobby_base_names(modern: bool, text: impl Fn(&str) -> String) -> [String; 8] {
    std::array::from_fn(|index| match index {
        PUBLIC_LOBBY_BASE if modern => PUBLIC_LOBBY_TITLE.into(),
        _ => text(&format!("TXT_LOB_{}", index + 1)),
    })
}

/// `0x00780110`'s view of the runner's wait box `0x792BE0(0)`; an open one
/// only takes the new text.
fn show_wait(state: &mut AppState, key: &str) {
    let fresh = service::wait_box(state, key);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    match lobby.wait.as_mut() {
        Some(open) => open.shell_box.body = fresh.shell_box.body,
        None => lobby.wait = Some(fresh),
    }
    lobby.ui.release_buttons();
    state
        .frontend
        .shell_controller
        .reset_to(crate::ui::wol_shell::WAIT_DIALOG);
}

/// `0x00792BE0(0)`: only an open box takes the text.
fn retext_wait(state: &mut AppState, key: &str) {
    let text = state.csf_text(key);
    if let Some(wait) = lobby_mut(state).and_then(|lobby| lobby.wait.as_mut()) {
        wait.shell_box.body = text;
    }
}

fn close_wait(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.wait = None;
    }
    let dialog = runner_dialog(state);
    state.frontend.shell_controller.reset_to(dialog);
}

fn show_box(state: &mut AppState, key: &str, yes_no: bool, then: BoxThen) {
    let message = service::message_box(state, state.csf_text(key), yes_no);
    put_box(state, message, then);
}

/// Replace the open lobby box's text, for a VERA-specific reason.
pub(super) fn replace_box_body(state: &mut AppState, body: &str) -> bool {
    let Some((message, _)) = lobby_mut(state).and_then(|lobby| lobby.message.as_mut()) else {
        return false;
    };
    message.body = body.to_owned();
    true
}

fn put_box(state: &mut AppState, message: ShellBox, then: BoxThen) {
    let dialog = message.dialog;
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.message = Some((message, then));
    lobby.ui.release_buttons();
    state.frontend.shell_controller.reset_to(dialog);
}

/// The runner `0x00787770` entered after the login (`postmatch` false) or
/// after a match (true). A hidden lobby kept while C2/C9 ran is shown again
/// instead: setup Back left its game channel and set `[0xB77DD8]`, so the
/// runner rejoins its chat lobby.
pub(crate) fn open(state: &mut AppState, postmatch: bool) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let motd = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.motd.take());
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    // Runner entry clears [0xB77DC1] (0x00787973).
    visit.in_game = false;
    if visit.disconnected {
        // 0x007879B6: after a lost connection the runner logs in again
        // first, and a failed login ends it (the welcome).
        visit.relogin = Some(postmatch);
        // The lost connection took the kept game room with it.
        state.match_state.session.take_network_room_return();
        visit.lobby = None;
        visit.page = WolPage::Backdrop { wait: None };
        super::leaf::free_art(state, DIALOG);
        state.match_state.session.leave_network();
        state.frontend.shell_route = ShellRoute::Wol;
        service::begin_flow(state);
        return;
    }
    if state.match_state.session.network_return_context().is_none() {
        state
            .match_state
            .session
            .enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
    }
    visit.page = WolPage::Lobby;
    // Disjoint from the visit's borrow.
    let login = crate::ui::wol_login::DIALOG;
    state.frontend.wol_art.retain(|(id, _)| *id != login);
    state.frontend.shell_route = ShellRoute::WolLobby;
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG);
    let now = Instant::now();
    let returning = postmatch && state.match_state.session.awaiting_wol_room_return();
    if !returning {
        state.match_state.session.take_network_room_return();
    }
    let resume = !postmatch && visit.lobby.is_some();
    if resume {
        let lobby = visit.lobby.as_deref_mut().expect("checked");
        // C2 Back (0x790060..0x7900BE) leaves without waiting; the runner's
        // 0x788A30 (0x787ED8) then leaves the game channel again before
        // joining (0x788800, 0x7888AD), whose OnPart shows TXT_YOULEFT
        // here. A channel C9 already left is gone.
        if lobby
            .current
            .as_ref()
            .is_none_or(|current| current.kind != GAME_CHANNEL)
        {
            lobby.current = None;
            lobby.members.clear();
        }
        lobby.shown = true;
        lobby.covered = false;
        lobby.ui.set_observer(observer);
        lobby.step = Step::Ready;
        refill_players(state);
        switch_lobby(state);
        return;
    }
    visit.lobby = Some(new_runner(
        Runner::Lobby,
        observer,
        postmatch,
        now,
        !visit.legacy_ui,
    ));
    let _ = App::ensure_skirmish_shell_chrome(state);
    super::leaf::open_art(state, DIALOG);
    // The MOTD [0xB73AE0], and the global is cleared.
    if let Some(motd) = motd.filter(|motd| !motd.is_empty()) {
        let modern = state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| visit.modern_ui());
        for row in motd_rows(&motd, modern) {
            message_line(state, row);
        }
    }
    show_wait(state, "GUI:LoginComplete");
    // 0x00787BC8: the buddy mirror restarts (API+0xDC, 0x0079CE70 outside
    // Quick Match, 0x0079D2F0) before the chat-lobby list.
    // 0x00787B33..0x00787B53: the user's own squad.
    request_squad(state, 0);
    super::buddies::restart(state);
    if returning {
        await_room(state);
    } else {
        list_lobbies(state);
    }
}

/// The MOTD's transcript rows (runner `0x00787770`, `[0xB73AE0]`): one per
/// `strtok("\n")` token, then a blank row. VERA20K Online's chat lobby
/// leaves the blank row out; the native shell UI keeps it.
fn motd_rows(motd: &str, modern: bool) -> Vec<String> {
    let mut rows: Vec<String> = crate::rules::ini_value::strtok(motd, &['\n'])
        .map(str::to_owned)
        .collect();
    if !modern {
        rows.push("\n".to_owned());
    }
    rows
}

/// The runner's first list (`ListChannels(0, 0)`) behind `TXT_REQ_CHANLIST`.
fn list_lobbies(state: &mut AppState) {
    show_wait(state, "TXT_REQ_CHANLIST");
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
}

/// VERA: the match came from VERA's own room, which stayed joined. The
/// lobby gives it back once every player has finished; until then a wait
/// box shows, whose button gives up the room for the chat lobby.
fn await_room(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        // MatchSession owns the room return. This list only fills the rows.
        lobby.step = Step::Ready;
    }
    // The chat lobbies still come, so leaving the room later finds one.
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    show_wait(state, "TXT_REQ_CHANLIST");
    let cancel = state.csf_text("GUI:Cancel");
    if let Some(wait) = lobby_mut(state).and_then(|lobby| lobby.wait.as_mut()) {
        wait.shell_box.body = crate::i18n::tr("VERA:ReturningToRoom");
        wait.shell_box
            .set_caption(crate::ui::wol_shell::WAIT_DISCONNECT, cancel);
    }
}

/// VERA: the lobby refused to keep the room (one from before rooms came
/// back), so a runner waiting for it goes on to the chat lobby.
pub(super) fn room_refused(state: &mut AppState) {
    if awaiting_room(state) {
        room_not_returned(state, false);
    }
}

pub(super) fn awaiting_room(state: &AppState) -> bool {
    state.match_state.session.awaiting_wol_room_return()
}

/// VERA: the kept room is gone (it closed, or the player gave up waiting).
/// The runner goes on as after any match: the chat-lobby list, then a join.
/// The join itself leaves the room on the lobby if it is still there.
fn room_not_returned(state: &mut AppState, closed: bool) {
    state.match_state.session.take_network_room_return();
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.current = None;
    lobby.members.clear();
    // The next waits take their own box, with its Disconnect again.
    lobby.wait = None;
    lobby.step = Step::ListingLobbies {
        deadline: Instant::now() + LIST_WAIT,
    };
    if closed {
        message_line(state, crate::i18n::tr("VERA:GameRoomClosed"));
    }
    list_lobbies(state);
}

/// VERA: the lobby gave back the room this player stayed in, owner first.
/// Its owner opens the host's setup again from the settings it started
/// with, or on the campaign's next stage after BC6's Continue; the others
/// enter it as after a join.
fn room_reopened(state: &mut AppState, joined: WolJoined) {
    if !awaiting_room(state) {
        // The player gave up waiting; the chat-lobby join leaves the room.
        log::debug!("WOL room reopened after the wait ended");
        return;
    }
    let (resume, campaign) = match state.match_state.session.take_network_room_return() {
        crate::app::match_runtime::session::NetworkRoomReturn::Wol {
            cooperative,
            campaign,
        } => (cooperative, campaign),
        _ => (None, None),
    };
    let listed = joined.members.clone();
    let channel = joined.channel.clone();
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::Ready;
    }
    sort_lobby(state, |lobby, sort, rank| {
        lobby.users_listed(channel, joined.members, sort, rank);
    });
    request_listed(state, &listed);
    close_wait(state);
    let observe = state
        .frontend
        .multiplayer_shell_runtime
        .lan_create_observer();
    if open_game_room(state, &listed, observe, resume)
        && let Some(campaign) = campaign
        && let Err(error) = setup::campaign_room::resume_after_wol_result(
            state,
            *campaign,
            listed.iter().map(|member| member.name.as_str()),
        )
    {
        log::warn!("WOL campaign continuation refused: {error:#}");
        message_line(state, crate::i18n::tr("VERA:CampaignRoomUnavailable"));
    }
}

/// The others in a room this game hosts again take rows as joiners would,
/// in the lobby's order, so the host's options name them all; VERA's room
/// does not tell them as joins. Everyone still here stays: past this host's
/// own Max players, or with no seat free, the room grows and they watch.
fn seat_again(state: &mut AppState, listed: &[WolMember], me: &str) {
    if let Some(protocol) = state.frontend.network_setup_protocol.as_mut() {
        protocol.quiet_joins();
    }
    for name in listed
        .iter()
        .map(|member| member.name.clone())
        .filter(|name| !name.eq_ignore_ascii_case(me))
    {
        let session = &mut state.match_state.session;
        if session.network_several_observers() && session.network_join_row(false).is_none() {
            let people = session
                .network_setup()
                .map_or(0, |launch| launch.humans.len() + 1);
            let grown = session
                .set_network_people_limit(u8::try_from(people).unwrap_or(u8::MAX))
                .and_then(|()| session.set_network_spectators(true));
            if let Err(error) = grown {
                log::warn!("WOL room grows for {name}: {error:#}");
            }
        }
        if let Err(error) = setup::user_joined(state, name) {
            log::warn!("WOL room seats its players again: {error:#}");
        }
    }
}

/// Opens the game room this player just came into or back to: as its host
/// when the lobby lists it as the owner, from this game's own room
/// settings (on the campaign's next stage with `resume`), seating everyone
/// else listed, and otherwise as a guest.
fn open_game_room(
    state: &mut AppState,
    listed: &[WolMember],
    observe: bool,
    resume: Option<crate::app::match_runtime::session::CooperativeResume>,
) -> bool {
    let me = nickname(state);
    let host = listed
        .iter()
        .any(|member| member.flags & MEMBER_OPERATOR != 0 && member.name.eq_ignore_ascii_case(&me));
    if host {
        commit_create(state, observe);
        // Before anyone is seated again: the Cooperative mode admits only
        // its two players.
        if let Some(resume) = resume.filter(|_| !observe)
            && let Err(error) = crate::app::shell_network_setup::resume_cooperative(state, resume)
        {
            log::warn!("WOL reopened room keeps its mode: {error:#}");
        }
        seat_again(state, listed, &me);
    } else {
        enter_game_as_guest(state);
    }
    host
}

/// Makes `name` the host in a room's member list, first, without the old
/// host, where a guest's game takes its host from
/// (`setup::guest_users_listed`); returns the old host's name.
fn pass_room(members: &mut Vec<WolMember>, name: &str) -> Option<String> {
    let old = members
        .iter()
        .find(|member| member.flags & MEMBER_OPERATOR != 0)
        .map(|member| member.name.clone());
    members.retain(|member| Some(&member.name) != old.as_ref());
    for member in members.iter_mut() {
        member.flags &= !MEMBER_OPERATOR;
    }
    if let Some(index) = members
        .iter()
        .position(|member| member.name.eq_ignore_ascii_case(name))
    {
        let mut host = members.remove(index);
        host.flags |= MEMBER_OPERATOR;
        members.insert(0, host);
    }
    old
}

/// VERA: the host left, and the lobby made `name` the room's host, renaming
/// a room it named for the old host (`host_changed`, docs/lobby-protocol.md
/// "Host handover"); the old host's `member_left` follows. The room opens
/// again as after a match: the new host's game from its own room settings,
/// seating everyone still here, and the others' games enter it as after a
/// join. The chat stays. A join still sliding in opens the room as it is
/// now ([`joined`]).
fn host_changed(state: &mut AppState, name: String, channel: Option<WolChannel>) {
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    if !runner
        .current
        .as_ref()
        .is_some_and(|current| current.kind == GAME_CHANNEL)
    {
        if let Some(Ok(joined)) = runner.deferred.as_mut() {
            pass_room(&mut joined.members, &name);
            if let Some(channel) = channel {
                joined.channel = channel;
            }
        }
        return;
    }
    if let Some(channel) = channel {
        runner.current = Some(channel);
    }
    let old = pass_room(&mut runner.members, &name);
    refill_players(state);
    if !in_setup_channel(state) || quick_match_runner(state) {
        return;
    }
    let chat = state
        .frontend
        .multiplayer_setup
        .as_mut()
        .map(|ui| std::mem::take(&mut ui.chat));
    let observe = state
        .match_state
        .session
        .network_setup()
        .is_some_and(|launch| launch.local_player().is_observer());
    let listed = lobby(state)
        .map(|lobby| lobby.members.clone())
        .unwrap_or_default();
    // A visitor's room opens again hidden: the lobby page stays as it is,
    // with any box or page open over it.
    let page = visiting(state).then(|| {
        let shown = lobby(state).map(|lobby| (lobby.shown, lobby.covered));
        (std::mem::take(&mut state.frontend.shell_controller), shown)
    });
    let host = open_game_room(state, &listed, observe, None);
    if let (Some(chat), Some(ui)) = (chat, state.frontend.multiplayer_setup.as_mut()) {
        ui.chat = chat;
    }
    if let Some((controller, shown)) = page {
        crate::app::shell_network_setup::hide_room(state);
        state.frontend.shell_controller = controller;
        let join = selected_admits(state);
        if let (Some(lobby), Some((shown, covered))) = (lobby_mut(state), shown) {
            lobby.shown = shown;
            lobby.covered = covered;
            lobby.ui.set_commands(true, join, false);
        }
    }
    if let Some(old) = old {
        room_line(
            state,
            crate::i18n::tr_fill("VERA:PlayerLeftRoom", &[("name", &old)]),
        );
    }
    room_line(
        state,
        if host {
            crate::i18n::tr("VERA:NowHost")
        } else {
            crate::i18n::tr_fill("VERA:NewHost", &[("name", &name)])
        },
    );
}

fn refill_games(state: &mut AppState) {
    let sort = state.frontend.wol_lobby_globals.game_sort;
    let Some(lobby) = lobby(state) else {
        return;
    };
    let mut rows = game_rows(
        &lobby.games,
        sort,
        lobby.ui.observer(),
        lobby.ui.is_modern(),
    );
    // VERA: the modern list tags a room whose exInfo names a co-op
    // campaign's map with the Cooperative mode's name (MPModesMD.ini).
    let tag = state
        .frontend
        .skirmish_modes
        .iter()
        .find(|mode| mode.class == crate::skirmish_modes::MpModeClass::Cooperative)
        .map(|mode| state.csf_text(&mode.ui_name_key));
    for row in &mut rows {
        let channel = &lobby.games[row.record];
        if lobby.ui.is_modern() {
            let fallback_map = if channel
                .summary
                .as_ref()
                .and_then(|s| s.map.as_ref())
                .is_none()
            {
                scenario_description(state, &channel.ex_info().scenario)
            } else {
                String::new()
            };
            if let Some(heading) =
                room_heading(&channel.name, channel.summary.as_ref(), &fallback_map)
            {
                row.text = heading;
            }
        }
        if cooperative_campaign(state, channel).is_some() {
            row.tag = tag.clone();
        }
    }
    if lobby.ui.is_modern() {
        rows.extend(watch_rows(&lobby.matches, &crate::i18n::tr(WATCH_LIST_TAG)));
        // VERA: the online page's filter row hides the rest.
        let filter = state.frontend.wol_lobby_globals.room_filter;
        let search = lobby.ui.search_text();
        rows.retain(|row| filter.shows(row, &search));
    }
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.replace_games(rows);
    }
}

/// Build a modern row only from published facts. Older listings can still
/// supply a known map. With no added information, keep the original count.
fn room_heading(
    name: &str,
    summary: Option<&WolRoomSummary>,
    fallback_map: &str,
) -> Option<String> {
    let format = summary.and_then(|summary| match &summary.teams {
        _ if summary.forming => Some(crate::i18n::tr("VERA:LobbyTeamsForming")),
        Some(teams) if teams.is_empty() => Some(crate::i18n::tr("VERA:LobbyFfa")),
        Some(teams) => Some(
            teams
                .iter()
                .filter(|&&seats| seats > 0)
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join("v"),
        ),
        None => None,
    });
    let map = summary
        .and_then(|summary| summary.map.as_deref())
        .unwrap_or(fallback_map);
    if format.is_none() && (name.is_empty() || map.is_empty() || map == name) {
        return None;
    }
    let parts = [name, format.as_deref().unwrap_or_default(), map];
    Some(
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" - "),
    )
}

/// Matches follow open rooms. The match ID keys selection across refreshes.
/// Older listings without a descriptive header retain their viewer counts.
fn watch_rows(matches: &[service::WolMatch], tag: &str) -> Vec<WolGameRow> {
    matches
        .iter()
        .enumerate()
        .map(|(record, watch)| {
            let name = if watch.name.is_empty() {
                &watch.map
            } else {
                &watch.name
            };
            WolGameRow {
                record,
                name: watch.match_id.clone(),
                text: room_heading(&watch.name, watch.summary.as_ref(), &watch.map)
                    .unwrap_or_else(|| format!("{name} {}/{}", watch.watchers, watch.max_watchers)),
                greyed: watch.watchers >= watch.max_watchers,
                kind: GameTypeIcon::Plain,
                keyed: false,
                limit_icon: None,
                ping: -1,
                tag: Some(tag.to_owned()),
                watch: true,
            }
        })
        .collect()
}

/// The co-op campaign whose map a game's exInfo names.
fn cooperative_campaign(state: &AppState, channel: &WolChannel) -> Option<usize> {
    state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_campaign_for_scenario(&channel.ex_info().scenario)
}

fn refill_lobbies(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let current = lobby.current.as_ref().map(|channel| channel.name.clone());
    let rows = lobby_rows(&lobby.lobbies);
    lobby.ui.replace_lobbies(rows, current.as_deref());
}

/// `0x00783C80`: the chat lobby's users with their ladder ranks and
/// high-score cells; in a game channel it shows nothing the custom lobby
/// can see.
/// RESIDUAL: the colors of mode 4 with `[0xA8B244]` 3 (`0x00783F02`) are
/// not ported; the cells' tooltips (`0x4A8` records) have no reader, as
/// nothing sends the list `0x4AD`.
pub(super) fn refill_players(state: &mut AppState) {
    let campaigns = state
        .frontend
        .multiplayer_shell_runtime
        .cooperative_campaign_count();
    let globals = &state.frontend.wol_lobby_globals;
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let Some(lobby) = visit.lobby.as_deref_mut() else {
        return;
    };
    // A visitor's lobby page lists the lobby they visit.
    let (current, members) = match &lobby.visit {
        Some(visit) => (Some(&visit.channel), &visit.members),
        None => (lobby.current.as_ref(), &lobby.members),
    };
    let rows = match current {
        Some(channel) if channel.kind == 0 => members
            .iter()
            .map(|member| {
                let key = member.name.to_ascii_lowercase();
                let icons = globals
                    .high_scores
                    .get(&key)
                    .map_or_else(Vec::new, |record| {
                        score_icons(
                            PlayerScore {
                                points: record.points,
                                rank: record.rank,
                                coop: record.coop,
                            },
                            campaigns,
                        )
                    });
                WolPlayerRow::from_member(
                    member,
                    globals.ladder.get(&key).map(|record| record.rank),
                    icons,
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    lobby.ui.replace_players(rows);
    if lobby.ui.is_modern() {
        refill_lobbies(state);
    }
}

/// `0x0079BA60` for the selected game row (`show` prints its refusal).
/// VERA: a match to watch passes; its `watch` is the lobby's to refuse.
fn admit_row(state: &mut AppState, row: usize, observer: bool, show: bool) -> bool {
    let Some(lobby) = lobby(state) else {
        return false;
    };
    let Some(row) = lobby.ui.game_rows.get(row) else {
        return false;
    };
    // A visitor cannot watch or rejoin their own room. Another room's
    // admission is responsible for switching their membership.
    if row.watch {
        return lobby.visit.is_none();
    }
    let Some(channel) = lobby.games.get(row.record) else {
        return false;
    };
    if lobby.visit.is_some()
        && lobby
            .current
            .as_ref()
            .is_some_and(|room| room.name == channel.name)
    {
        return false;
    }
    match admit(channel, observer, admission_context(state)) {
        Ok(()) => true,
        Err(key) => {
            if let (true, Some(key)) = (show, key) {
                let text = if key == crate::ui::wol_lobby::RULES_DIFFER {
                    crate::app::shell_network_setup::agreement::rules_differ_text(state)
                } else {
                    state.csf_text(key)
                };
                message_line(state, text);
            }
            false
        }
    }
}

fn selected_channel(lobby: &WolLobby) -> Option<WolChannel> {
    lobby
        .ui
        .selected_game()
        .filter(|row| !row.watch)
        .and_then(|row| lobby.games.get(row.record))
        .cloned()
}

/// The runner's lobby switch (`[0xB77DD8]`): controls off, the picker
/// without skipping crowded lobbies, the join; no lobby closes the runner.
fn switch_lobby(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.set_commands(false, false, true);
    }
    if !join_picked_lobby(state, false, false) {
        if let Some(lobby) = lobby_mut(state) {
            lobby.ui.set_enabled(BACK, true);
        }
        close_runner(state);
    }
}

/// `0x00788A30`: pick (`0x00788180`) and join with the lobby key. False
/// when no chat lobby exists.
fn join_picked_lobby(state: &mut AppState, skip_full: bool, entry: bool) -> bool {
    let number = state.frontend.wol_lobby_globals.lobby_number;
    let Some(lobby) = lobby(state) else {
        return false;
    };
    let Some((record, number)) = pick_lobby(&lobby.lobbies, number, skip_full) else {
        return false;
    };
    let channel = lobby.lobbies[record].clone();
    state.frontend.wol_lobby_globals.lobby_number = number;
    join_channel(state, channel, LOBBY_KEY, false, entry);
    // 0x00788A30 refills the game list after its join.
    refill_games(state);
    true
}

/// `0x00788800`: leave the current channel first (`WOL:LeavingChannel`
/// through `0x00792BE0(1)`, up to 150 x 100 ms), then copy the key
/// (`+0xD7`), remember the pending channel `[0xB73BE8]` and request the join
/// behind the box retexted `TXT_JOINLOB`. A channel already joined or
/// pending is not joined again (it returns 0 without a request). When visiting
/// from a VERA room, send the join directly so refusal preserves that room.
fn join_channel(state: &mut AppState, channel: WolChannel, key: &str, game: bool, entry: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let named = |known: &Option<WolChannel>| {
        known
            .as_ref()
            .is_some_and(|known| known.name == channel.name)
    };
    if named(&lobby.current) || named(&lobby.pending) {
        // The game list never holds the current chat lobby and the commands
        // are off while a join is pending: unreachable for a game.
        log::debug!(
            "WOL join of {} skipped: already joined or pending",
            channel.name
        );
        if game {
            join_failed_locally(state, true);
        }
        return;
    }
    if lobby.current.is_some() && !(game && lobby.visit.is_some()) {
        lobby.step = Step::Leaving {
            deadline: Instant::now() + LEAVE_WAIT,
            then: AfterLeave::Join {
                channel,
                key: key.to_owned(),
                game,
                entry,
            },
        };
        request(state, WolServiceRequest::LeaveChannel);
        show_wait(state, "WOL:LeavingChannel");
        return;
    }
    send_join(state, channel, key.to_owned(), game, entry);
}

fn send_join(state: &mut AppState, channel: WolChannel, key: String, game: bool, entry: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.pending = Some(channel.clone());
    let switching = game && lobby.visit.is_some();
    lobby.step = if switching {
        Step::SwitchingRoom { request: None }
    } else if game {
        Step::JoiningGame { request: None }
    } else {
        Step::JoiningLobby {
            entry,
            request: None,
        }
    };
    show_wait(state, "TXT_JOINLOB");
    request(state, WolServiceRequest::JoinChannel { channel, key });
    if game && !switching && lobby_dialog(state).is_some() {
        // 0x00789960 returned true: 0x0078C281 slides the dialog out
        // (0x006071E0), draws the empty backdrop and hides it; C9 comes from
        // the join sink afterwards.
        App::leave_shell_dialog(
            state,
            shell_transition::ShellSlideKind::WolLobby,
            crate::app::shell_wol::lobby::commit_hide,
        );
    }
}

/// `0x00785740` for this user: `TXT_YOULEFT` with the room text (a
/// `Lob_41_<n>` name as `TXT_JOINED_S` shows it, else the raw name; not in
/// Quick Match), then the channel record and its users cleared. The line
/// reaches the lobby's transcript under any wait box (`0x005E2700` walks
/// the dialog stack). The modern lobby never names its one chat lobby.
fn left_channel(state: &mut AppState) {
    let Some(current) = lobby_mut(state).and_then(|lobby| {
        lobby.members.clear();
        lobby.current.take()
    }) else {
        return;
    };
    // 0x785764..0x785788: PlayerLeft in a channel of a nonzero type.
    if current.kind != 0 {
        setup::play_player_left(state);
    }
    if super::quick_match::mode_active(state) {
        return;
    }
    let room = match current.lobby_number() {
        Some(_) if modern_lobby(state) => return,
        Some(number) => WolLobbyRow::joined_name(number, &base_names(state)),
        None => current.name,
    };
    let text = crate::assets::csf_file::format_csf(
        &state.csf_text("TXT_YOULEFT"),
        &[crate::assets::csf_file::CsfArg::Str(&room)],
    );
    message_line(state, text);
}

/// The Room name VERA's online Create Game starts with; OK with it unchanged
/// creates gamemd's own name.
fn default_room_name(nickname: &str) -> String {
    crate::i18n::tr_fill("VERA:DefaultRoomName", &[("nickname", &nickname)])
}

/// VERA's line for a guest entering a modern-lobby game room (gamemd's C9
/// prints none): it names the room's operator, its host, and the room's own
/// name when the host gave it one, and says who starts the game, since the
/// room accepts for its guests. `users` is the room's user list without the
/// local player, as `(name, operator)`.
fn joined_game_line(users: &[(String, bool)], room: &str) -> Option<String> {
    let (host, _) = users.iter().find(|(_, operator)| *operator)?;
    // gamemd's own name is "%s's_game" (`0x00789B60`).
    Some(if room.eq_ignore_ascii_case(&format!("{host}'s_game")) {
        crate::i18n::tr_fill("VERA:JoinedGame", &[("host", host)])
    } else {
        crate::i18n::tr_fill("VERA:JoinedNamedGame", &[("host", host), ("room", &room)])
    })
}

/// The label of VERA's line for the host of a modern-lobby game room
/// (gamemd's C2 prints none): its guests accept on their own
/// (`setup::service::auto_accept`).
const HOST_ROOM_LINE: &str = "VERA:HostRoomHint";

/// VERA's own online lobby, not gamemd's.
pub(crate) fn modern_lobby(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.ui.is_modern())
}

/// `0x00785740` for this user while C2/C9 is up: its `TXT_YOULEFT` goes to
/// that dialog (`[0x7EEF28]`) and closes with it; the channel record and its
/// users clear, then the dialog closes (`0x785970`, `0x7859F9`) with
/// PlayerLeft.
fn setup_channel_left(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.members.clear();
        lobby.current = None;
    }
    setup::local_left(state);
}

/// The leave finished (the sink cleared `[0xB77DD0]`) or its 15 s ran out.
fn leave_finished(state: &mut AppState, left: bool) {
    if left {
        left_channel(state);
    }
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let Some(then) = lobby.step.take_leave_continuation() else {
        if left {
            refill_players(state);
            refill_lobbies(state);
        }
        return;
    };
    refill_players(state);
    refill_lobbies(state);
    match then {
        AfterLeave::Join {
            channel,
            key,
            game,
            entry,
        } => {
            if left {
                send_join(state, channel, key, game, entry);
            } else {
                // Still in the channel: 0x00788800 returns 1. The box keeps
                // flag 1 ([0xB77DC8] only loses 0x40), so it stays open.
                join_failed_locally(state, game);
            }
        }
        // 0x00789B60 closes the box (0x00792EC0) and creates whether or
        // not the leave finished.
        AfterLeave::Create {
            channel,
            key,
            observe,
            ranked,
        } => {
            close_wait(state);
            send_create(state, channel, key, observe, ranked);
        }
        AfterLeave::Return => {
            close_wait(state);
            close_runner(state);
        }
    }
}

/// `0x00789960` returned false: every command back, Join as admitted.
/// The Buddy List runner goes back to its dialog loop.
fn join_failed_locally(state: &mut AppState, game: bool) {
    if buddy_runner(state) {
        super::buddy::stopped(state);
        return;
    }
    if quick_match_runner(state) {
        if let Some(lobby) = lobby_mut(state) {
            lobby.step = Step::Ready;
        }
        if game {
            super::quick_match::game_joined(state, false);
        } else {
            super::quick_match::lobby_join_failed(state);
        }
        return;
    }
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.covered = false;
    lobby.step = Step::Ready;
    if game {
        lobby.ui.set_commands(true, true, false);
    } else {
        lobby.ui.set_commands(true, false, true);
    }
}

/// The runner returns 0 (`LAB_00787F98`): the dialog closes without a
/// slide and `WOL_Main` shows the welcome (mode 0), or the online lobby
/// again for a Quick Match runner that lobby started.
fn close_runner(state: &mut AppState) {
    take_visit(state);
    let quick_match = quick_match_runner(state);
    if quick_match {
        super::quick_match::runner_closed(state);
    }
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.lobby = None;
    }
    super::leaf::free_art(state, DIALOG);
    state.match_state.session.leave_network();
    if quick_match && super::quick_match::return_to_lobby(state) {
        return;
    }
    App::reopen_wol_welcome(state);
}

/// Start Game Now's abort: its leave (`0x00794E00`), then event 6's
/// (`0x00787F95`), and the runner returns 0.
pub(super) fn start_aborted(state: &mut AppState) {
    request(state, WolServiceRequest::LeaveChannel);
    if quick_match_runner(state) {
        // Event 6 ends the Quick Match runner through its exit tail.
        super::quick_match::start_aborted(state);
        return;
    }
    request(state, WolServiceRequest::LeaveChannel);
    close_runner(state);
}

/// Back's teardown has run: event 6 leaves the channel (API+0x24) and the
/// runner returns 0.
pub(crate) fn commit_back(state: &mut AppState) {
    request(state, WolServiceRequest::LeaveChannel);
    close_runner(state);
}

/// Buddy List's teardown has run (`0x0078BF7D`): the runner ends as for
/// Back with mode 5 and return mode 4. The mode-loop pass applies and the
/// login category is unchanged, so `WOL_Main` runs the Buddy List runner at
/// once (`0x0077B482`).
/// VERA: the online page's Quick Match ends the runner the same way with
/// mode 1, so the Quick Match runner `0x0079FC40` follows with the login kept.
pub(crate) fn commit_runner_switch(state: &mut AppState, mode: crate::ui::wol_shell::WolMode) {
    request(state, WolServiceRequest::LeaveChannel);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.lobby = None;
        visit.mode = Some(mode);
        visit.return_mode = Some(crate::ui::wol_shell::WolMode::CustomMatch);
    }
    super::leaf::free_art(state, DIALOG);
    super::apply_wol_preferences(state);
    super::open_lobby(state, false);
}

/// The runner's records end with it (the Buddy List runner returned).
pub(super) fn drop_runner(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.lobby = None;
    }
}

/// The Join slide has run: the empty backdrop shows under the wait, and
/// `0x0078C2A0` enables the command column again. A join result that came
/// during the slide is delivered now (the native slide pumps no messages).
pub(crate) fn commit_hide(state: &mut AppState) {
    let deferred = lobby_mut(state).and_then(|lobby| {
        lobby.shown = false;
        lobby.covered = false;
        lobby.ui.set_commands(true, true, false);
        lobby.deferred.take()
    });
    if let Some(result) = deferred {
        joined(state, result);
    }
}

/// The Join button (`show` when the row differs from `[0x84A23C]`) and a
/// double-click both reach `0x00789960` after admission.
fn begin_join(state: &mut AppState, row: usize, show: bool) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let admitted = admit_row(state, row, observer, show);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.join_row = Some(row);
    lobby.ui.set_enabled(JOIN, admitted);
    if !admitted {
        return;
    }
    if let Some(match_id) = selected_match(lobby) {
        begin_watch(state, match_id);
        return;
    }
    let Some(channel) = selected_channel(lobby) else {
        return;
    };
    lobby.ui.set_commands(false, false, false);
    continue_join(state, channel);
}

/// VERA: Join Any Game joins the first room the list shows that admits this
/// player without a password, as Join on its row does. With none, Create Game
/// opens instead.
fn join_any(state: &mut AppState) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let Some(lobby) = lobby(state) else {
        return;
    };
    let open: Vec<usize> = (lobby.ui.game_rows.iter().enumerate())
        .filter(|(_, row)| !row.watch && !row.keyed)
        .map(|(row, _)| row)
        .collect();
    let Some(row) = open
        .into_iter()
        .find(|&row| admit_row(state, row, observer, false))
    else {
        begin_create(state);
        return;
    };
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.select_row(GAMES, Some(row));
    lobby.ui.admitted_name = lobby.ui.game_rows.get(row).map(|row| row.name.clone());
    begin_join(state, row, false);
}

/// VERA: the match of the selected watch row.
fn selected_match(lobby: &WolLobby) -> Option<String> {
    lobby
        .ui
        .selected_game()
        .filter(|row| row.watch)
        .and_then(|row| lobby.matches.get(row.record))
        .map(|watch| watch.match_id.clone())
}

/// VERA: Join on a watch row sends the lobby's `watch` for its match
/// behind the wait box (docs/lobby-protocol.md "Watching"). The player
/// stays in the chat lobby meanwhile.
fn begin_watch(state: &mut AppState, match_id: String) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.set_commands(false, false, false);
    lobby.step = Step::Watching {
        match_id: match_id.clone(),
        request: None,
    };
    show_wait(state, "VERA:ContactingOnline");
    request(state, WolServiceRequest::Watch { match_id });
}

/// VERA: whether the lobby page waits for a watch's grant.
pub(crate) fn awaits_watch(state: &AppState, match_id: &str) -> bool {
    lobby(state).is_some_and(|lobby| matches!(&lobby.step, Step::Watching { match_id: awaited, .. } if awaited == match_id))
}

/// VERA: the relay admitted the watch that [`Step::Watching`] waits for:
/// the wait closes as a joined game's does, and the match loads. A watch
/// keeps no room, so the lobby comes back after it as after any match.
/// False when no watch was waiting.
pub(crate) fn watch_admitted(state: &mut AppState) -> bool {
    let Some(lobby) = lobby_mut(state) else {
        return false;
    };
    if !matches!(lobby.step, Step::Watching { .. }) {
        return false;
    }
    lobby.step = Step::Ready;
    close_wait(state);
    state.match_state.session.take_network_room_return();
    true
}

/// VERA: a granted watch could not start (its map, version or relay): the
/// wait closes, when it is still up, and a box says why.
pub(crate) fn watch_failed(state: &mut AppState, key: &str) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if matches!(lobby.step, Step::Watching { .. }) {
        lobby.step = Step::Ready;
        close_wait(state);
    }
    show_box(state, key, false, BoxThen::WatchFailed);
}

/// VERA: the lobby refused the watch, or it went unanswered: the wait
/// closes and a box says why.
fn watch_refused(state: &mut AppState, code: u32) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if !matches!(lobby.step, Step::Watching { .. }) {
        log::debug!("stale WOL watch refusal");
        return;
    }
    lobby.step = Step::Ready;
    close_wait(state);
    show_box(state, watch_refusal_key(code), false, BoxThen::WatchFailed);
}

/// The text of a refused `watch`; other failures show the lobby's usual
/// refusal.
pub(super) fn watch_refusal_key(code: u32) -> &'static str {
    match code {
        WATCH_UNKNOWN_MATCH => "VERA:WatchUnknownMatch",
        WATCH_MATCH_ACTIVE => "VERA:WatchMatchActive",
        WATCHERS_FULL => "VERA:WatchersFull",
        _ => "VERA:OnlineRequestRefused",
    }
}

/// `0x00789960` past admission: the resolution limit, then the password.
fn continue_join(state: &mut AppState, channel: WolChannel) {
    let limit = channel.screen_limit();
    if limit < options_screen(state).0 {
        let key = if channel.game_flags & GAME_LIMIT_800 != 0 {
            "WOL:ResolutionLimited800"
        } else {
            "WOL:ResolutionLimited640"
        };
        show_box(state, key, true, BoxThen::Resolution { channel });
        return;
    }
    if channel.keyed() {
        // Shell__DrawEmptyBackdrop, then 0xE1.
        if let Some(lobby) = lobby_mut(state) {
            lobby.covered = true;
            lobby.ui.open_password();
            lobby.joining = Some(channel);
        }
        state.frontend.shell_controller.reset_to(PASSWORD_DIALOG);
        sync(state);
        return;
    }
    join_channel(state, channel, "", true, false);
}

fn answer_resolution(state: &mut AppState, channel: WolChannel, yes: bool) {
    let limit = channel.screen_limit();
    if yes {
        // OptionsClass::WriteToINI 0x5FAD10 with the limited size.
        let (width, height) = limited_screen(limit);
        let profile = &mut state.persistence.options_profile;
        profile.screen_width = width;
        profile.screen_height = height;
        crate::app::persistence::options::persist_options_profile(state);
    }
    if limit < options_screen(state).0 {
        join_failed_locally(state, true);
        return;
    }
    if channel.keyed() {
        continue_join(state, channel);
    } else {
        join_channel(state, channel, "", true, false);
    }
}

fn popup_action(state: &mut AppState, action: WolPopupAction) {
    // The popup's destruction repaints the dialog under it.
    if let Some(lobby) = lobby_mut(state) {
        lobby.covered = false;
    }
    match action {
        WolPopupAction::CreateConfirmed {
            password,
            clan,
            limit,
            observe,
            room,
            name,
            ranked,
            watching,
        } => {
            if let Some(room) = room {
                state.frontend.room_limits = room;
            }
            if let Some(ranked) = ranked {
                state.frontend.wol_room_unranked = !ranked;
            }
            // The room takes it when its setup opens.
            if let Some(watching) = watching {
                state.frontend.wol_room_watching = watching;
            }
            // An untouched Room name keeps gamemd's "%s's_game".
            let default = default_room_name(&nickname(state));
            let name = name.filter(|name| *name != default);
            state.frontend.wol_room_name = name.clone();
            let ranked = ranked.unwrap_or(true);
            create_confirmed(state, password, name, clan, limit, observe, ranked)
        }
        WolPopupAction::CreateCancelled => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.close_popup();
            }
            create_ended(state);
        }
        WolPopupAction::PasswordConfirmed(password) => {
            let channel = lobby_mut(state).and_then(|lobby| {
                lobby.ui.close_popup();
                lobby.joining.take()
            });
            state
                .frontend
                .shell_controller
                .reset_to(super::WOL_LOBBY_DIALOG);
            match channel {
                // Spaces become '_' (0x00789AB6).
                Some(channel) => {
                    join_channel(state, channel, &password.replace(' ', "_"), true, false)
                }
                None => join_failed_locally(state, true),
            }
        }
        WolPopupAction::PasswordCancelled => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.close_popup();
                lobby.joining = None;
            }
            state
                .frontend
                .shell_controller
                .reset_to(super::WOL_LOBBY_DIALOG);
            join_failed_locally(state, true);
        }
    }
}

/// `0x0078AC10`'s 0x540 Enter after `0x005EDD00`.
fn send_chat(state: &mut AppState, text: Vec<u16>) {
    let commands = wol_chat::WolChatCommands {
        page: units(state, "TXT_PAGE_COMMAND"),
        reply: units(state, "TXT_REPLY_COMMAND"),
        reply_short: units(state, "TXT_REPLY_COMMAND_SHORT"),
        online: units(state, "TXT_ONLINE_COMMAND"),
    };
    let filters = wol_chat::WolChatFilterCommands {
        buddy: units(state, "TXT_BUDDY_FILTER"),
        clan: units(state, "TXT_CLAN_FILTER"),
    };
    let last_pager = super::last_pager(state).map(str::to_owned);
    let nickname = nickname(state);
    let line = match wol_chat::parse_lobby(&text, &commands, &filters, last_pager.is_some()) {
        WolLobbyChatLine::Filter(filter) => {
            // 0x78B3F4/0x78B4DF flip the live DisplayBuddies/DisplayClan
            // bytes without saving; the next WOL entry re-reads VERA.INI.
            let live = &mut state.frontend.wol_preferences;
            let on = match filter {
                WolChatFilter::Buddy => {
                    live.display_buddies = !live.display_buddies;
                    live.display_buddies
                }
                WolChatFilter::Clan => {
                    live.display_clan = !live.display_clan;
                    live.display_clan
                }
            };
            let notice = units(state, filter.notice(on));
            echo(state, notice, wol_chat::EMOTE_COLOR);
            return;
        }
        WolLobbyChatLine::Line(line) => line,
    };
    let to = lobby(state)
        .map(|lobby| lobby.ui.private_targets())
        .unwrap_or_default();
    let in_channel = lobby(state).is_some_and(|lobby| lobby.current.is_some());
    let echo_line = match &line {
        WolChatLine::Notice(key) => {
            let text = state.csf_text(key);
            message_line(state, text);
            return;
        }
        WolChatLine::Locate { .. } => None,
        WolChatLine::Page { name, text } => Some((
            wol_chat::page_echo(&nickname, name, text),
            wol_chat::PRIVATE_COLOR,
        )),
        WolChatLine::Reply { text } => {
            let target: Vec<u16> = last_pager.unwrap_or_default().encode_utf16().collect();
            Some((
                wol_chat::page_echo(&nickname, &target, text),
                wol_chat::PRIVATE_COLOR,
            ))
        }
        WolChatLine::Say(text) => {
            if !in_channel {
                let text = state.csf_text("TXT_NOT_IN_CHAN");
                message_line(state, text);
                return;
            }
            // 0x0077E1D0 sends privately to the selected users.
            Some((
                wol_chat::say_echo(&nickname, text),
                if to.is_empty() {
                    wol_chat::SAY_COLOR
                } else {
                    wol_chat::PRIVATE_COLOR
                },
            ))
        }
    };
    let to = if matches!(line, WolChatLine::Say(_)) {
        to
    } else {
        Vec::new()
    };
    super::chat_sent(state, &line);
    // A visitor's lobby page talks in the lobby they visit.
    let chat = if visiting(state) {
        WolServiceRequest::VisitChat { line, to }
    } else {
        WolServiceRequest::Chat { line, to }
    };
    request(state, chat);
    if let Some((text, color)) = echo_line {
        echo(state, text, color);
    }
}

/// `0x3ED`: outside a channel `TXT_NOT_IN_CHAN`, an empty edit
/// `TXT_ENTER_MESSAGE`, else `0x0077E300` (private to the selected users).
fn send_emote(state: &mut AppState, text: Vec<u16>) {
    let Some(lobby) = lobby(state) else {
        return;
    };
    let to = lobby.ui.private_targets();
    if lobby.current.is_none() {
        let text = state.csf_text("TXT_NOT_IN_CHAN");
        message_line(state, text);
        return;
    }
    if text.is_empty() {
        let text = state.csf_text("TXT_ENTER_MESSAGE");
        message_line(state, text);
        return;
    }
    let nickname = nickname(state);
    let color = if to.is_empty() {
        wol_chat::EMOTE_COLOR
    } else {
        wol_chat::PRIVATE_EMOTE_COLOR
    };
    echo(state, wol_chat::emote_echo(&nickname, &text), color);
    let emote = if visiting(state) {
        WolServiceRequest::VisitEmote { text, to }
    } else {
        WolServiceRequest::Emote { text, to }
    };
    request(state, emote);
}

/// `0x00789B60` up to its popup: Limit Screen Resolution is cleared on a
/// wide screen, the commands go off and `0xC3` shows over the empty
/// backdrop.
pub(super) fn begin_create(state: &mut AppState) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let (width, _) = options_screen(state);
    let globals = &mut state.frontend.wol_lobby_globals;
    if width > 0x3FF {
        globals.limit_resolution = false;
    }
    let limit = globals.limit_resolution;
    let squad = squad_id(state);
    let room = state.frontend.room_limits;
    let ranked = !state.frontend.wol_room_unranked;
    let watching = state.frontend.wol_room_watching;
    let name = state
        .frontend
        .wol_room_name
        .clone()
        .unwrap_or_else(|| default_room_name(&nickname(state)));
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.set_commands(false, false, false);
    lobby.ui.select_row(GAMES, None);
    lobby.covered = true;
    // +0x13 starts as [0xAC10C8] == 1. VERA's lobby also offers its room's
    // limits, name, Ranked game and Watching, from the host's last choice.
    let room = lobby
        .ui
        .is_modern()
        .then_some((room, name.as_str(), ranked, watching));
    lobby.ui.open_create(width, limit, squad, observer, room);
    state.frontend.shell_controller.reset_to(CREATE_DIALOG);
    // Its widgets on their layout before it paints.
    sync(state);
}

fn dispatch(state: &mut AppState, action: WolLobbyAction) {
    match action {
        WolLobbyAction::Create => begin_create(state),
        WolLobbyAction::Join => {
            let Some(lobby) = lobby(state) else {
                return;
            };
            let Some(row) = lobby.ui.selected_row(GAMES) else {
                return;
            };
            let show = lobby.ui.join_row != Some(row);
            begin_join(state, row, show);
        }
        // A visitor's Back goes back to their room.
        WolLobbyAction::Back if visiting(state) => back_to_room(state),
        WolLobbyAction::Back => {
            App::leave_shell_dialog(
                state,
                shell_transition::ShellSlideKind::WolLobby,
                crate::app::shell_wol::lobby::commit_back,
            );
        }
        WolLobbyAction::Refresh => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.set_enabled(REFRESH, false);
                lobby.refresh_at = Some(Instant::now() + REFRESH_HOLD);
            }
            request_games(state);
        }
        WolLobbyAction::GameSort(control) => {
            let globals = &mut state.frontend.wol_lobby_globals;
            if let Some(sort) = toggle_game_sort(control, globals.game_sort) {
                globals.game_sort = sort;
                refill_games(state);
            }
        }
        WolLobbyAction::PlayerSort(control) => {
            let globals = &mut state.frontend.wol_lobby_globals;
            if let Some(sort) = toggle_player_sort(control, globals.player_sort) {
                globals.player_sort = sort;
                let globals = &state.frontend.wol_lobby_globals;
                if let Some(lobby) = state
                    .frontend
                    .wol
                    .as_mut()
                    .and_then(|visit| visit.lobby.as_deref_mut())
                {
                    let rank = |name: &str| ladder_rank(&globals.ladder, name);
                    match lobby.visit.as_mut() {
                        Some(visit) => resort_members(&mut visit.members, sort, &rank),
                        None => resort_members(&mut lobby.members, sort, &rank),
                    }
                }
                refill_players(state);
            }
        }
        WolLobbyAction::SelectGame { row, double } => {
            let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
            let Some(lobby) = lobby(state) else {
                return;
            };
            let name = lobby.ui.game_rows.get(row).map(|row| row.name.clone());
            let show = double || name != lobby.ui.admitted_name;
            let admitted = admit_row(state, row, observer, show);
            let Some(lobby) = lobby_mut(state) else {
                return;
            };
            lobby.ui.admitted_name = name;
            if double && admitted {
                begin_join(state, row, false);
            } else {
                lobby.ui.set_enabled(JOIN, admitted);
            }
        }
        WolLobbyAction::SelectLobby(row) => {
            let Some(lobby) = lobby(state) else {
                return;
            };
            let Some(record) = lobby.ui.lobby_rows.get(row) else {
                return;
            };
            let named = |known: &Option<WolChannel>| {
                known
                    .as_ref()
                    .is_some_and(|known| known.name == record.name)
            };
            if named(&lobby.current) || named(&lobby.pending) {
                return;
            }
            state.frontend.wol_lobby_globals.lobby_number = record.number;
            switch_lobby(state);
        }
        WolLobbyAction::Chat(text) => send_chat(state, text),
        WolLobbyAction::Emote(text) => send_emote(state, text),
        WolLobbyAction::ObserverChanged(observer) => state
            .frontend
            .multiplayer_shell_runtime
            .set_lan_observer(observer),
        // 0x0078B829: 0x113 slides out over the empty backdrop before 0x2BC.
        WolLobbyAction::Options => {
            App::leave_shell_dialog(
                state,
                shell_transition::ShellSlideKind::WolLobby,
                crate::app::shell_wol::options::commit_open,
            );
        }
        WolLobbyAction::Page => super::page::open_page_dialog(state),
        // 0x0078BF7D: mode 5 with return mode 4; event 6 then ends the
        // runner as Back does, and WOL_Main runs the Buddy List runner.
        WolLobbyAction::Unported(BUDDY) => App::leave_shell_dialog(
            state,
            shell_transition::ShellSlideKind::WolLobby,
            move |state| {
                crate::app::shell_wol::lobby::commit_runner_switch(
                    state,
                    crate::ui::wol_shell::WolMode::PlayBuddy,
                )
            },
        ),
        WolLobbyAction::BackToRoom => back_to_room(state),
        // VERA: the other view's lists and edit take the pointer's place.
        WolLobbyAction::View => {
            sync(state);
            cursor_moved(state);
        }
        WolLobbyAction::JoinAny => join_any(state),
        // VERA: a shorter list keeps its top row within the rows it shows.
        WolLobbyAction::FilterToggled(control) => {
            state.frontend.wol_lobby_globals.room_filter.toggle(control);
            refill_games(state);
            sync(state);
        }
        WolLobbyAction::SearchChanged => {
            refill_games(state);
            sync(state);
        }
        // VERA: the online page's Quick Match ends the runner as Buddy List
        // does, with return mode 4, and the Quick Match runner follows.
        WolLobbyAction::QuickMatch => App::leave_shell_dialog(
            state,
            shell_transition::ShellSlideKind::WolLobby,
            move |state| {
                crate::app::shell_wol::lobby::commit_runner_switch(
                    state,
                    crate::ui::wol_shell::WolMode::QuickMatch,
                )
            },
        ),
        WolLobbyAction::Unported(control) => {
            // Clan/Tournament/News (0x00773E90 web pages) need the
            // embedded browser.
            log::warn!("WOL lobby control {control:#x} is not implemented; 0x113 stays");
        }
        WolLobbyAction::Popup(action) => popup_action(state, action),
    }
}

/// A lobby box or wait-box button released: its answer (OK or Yes 0, No
/// 1; the wait box's Disconnect 0, WOL Check Status 1).
pub(super) fn box_command(state: &mut AppState, answer: usize) {
    let returning_to_room = awaiting_room(state);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if let Some((_, then)) = lobby.message.take() {
        let dialog = runner_dialog(state);
        state.frontend.shell_controller.reset_to(dialog);
        match then {
            BoxThen::QuickMatch(then) => super::quick_match::box_closed(state, then),
            BoxThen::JoinFailed { close } => join_failed_tail(state, close),
            BoxThen::CreateFailed => create_ended(state),
            BoxThen::WatchFailed => {
                if let Some(lobby) = lobby_mut(state) {
                    lobby
                        .ui
                        .set_commands(true, lobby.ui.selected_game().is_some(), false);
                }
            }
            BoxThen::Resolution { channel } => answer_resolution(state, channel, answer == 0),
            BoxThen::Close => close_runner(state),
            BoxThen::Kicked => {
                // The next lobby pick moves on, then 0x00785740.
                state.frontend.wol_lobby_globals.lobby_number += 1;
                left_channel(state);
                if let Some(lobby) = lobby_mut(state) {
                    lobby.step = Step::Ready;
                }
                refill_players(state);
                refill_lobbies(state);
                refill_games(state);
            }
        }
        return;
    }
    if answer == 1 {
        log::warn!("WOL Check Status needs the embedded browser; not opened");
        return;
    }
    if !lobby.wait.as_ref().is_some_and(|wait| wait.shown) {
        return;
    }
    // Disconnect, event 0, ends the runner's wait: LAB_00787F98.
    match lobby.step {
        // 0x0077F120 failed during Join's second list: the Buddy List
        // runner searches again, finds nothing and loops.
        Step::FindingGame { .. } => {
            request(state, WolServiceRequest::CancelWait);
            super::buddy::stopped(state);
        }
        _ if returning_to_room => room_not_returned(state, false),
        // Event 0: the Quick Match runner's exit.
        _ if lobby.runner == Runner::QuickMatch => {
            request(state, WolServiceRequest::CancelWait);
            super::quick_match::exit(state);
        }
        _ => {
            request(state, WolServiceRequest::CancelWait);
            close_runner(state);
        }
    }
}

/// The runner's timeouts, the wait box reveal, the dialog's timers (only
/// while it is the current window) and the shared scrollbar repeats.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    // Every pass polls: build the layout only while a lobby runs.
    lobby(state)?;
    let now = Instant::now();
    let page_open = super::page::page_dialog(state).is_some();
    let lobby = lobby_mut(state)?;
    if let Some(wait) = lobby.wait.as_mut() {
        wait.poll(now);
    }
    if let Some(request_id) = lobby
        .step
        .timed_request()
        .filter(|request| now >= request.deadline && lobby.deferred.is_none())
        .map(|request| request.id)
    {
        // An uncertain HTTP outcome may have joined on the server. Queue a
        // normal leave behind that command before allowing another attempt.
        // A watch joins no channel: the player is still in the chat lobby.
        // A lobby visit keeps the room, which a leave would give up.
        if !matches!(lobby.step, Step::Watching { .. } | Step::Visiting { .. }) {
            request(state, WolServiceRequest::LeaveChannel);
        }
        online_request_failed(state, request_id, 0x8000_4005);
        return None;
    }
    match lobby.step {
        Step::ListingLobbies { deadline } if now >= deadline => {
            // 0x0077F120 gave up: the runner's box, then the welcome. The
            // Buddy List runner returns 0 with return mode 0 and no box.
            lobby.step = Step::Ready;
            // The Quick Match runner clears its away text and returns 0
            // without a box too (0x0079FD5C..0x0079FD75).
            let buddy = matches!(lobby.runner, Runner::Buddy | Runner::QuickMatch);
            close_wait(state);
            if buddy {
                close_runner(state);
            } else {
                show_box(state, "TXT_CANT_CONNECT", false, BoxThen::Close);
            }
            return None;
        }
        Step::FindingGame { deadline, .. } if now >= deadline => {
            lobby.step = Step::Ready;
            close_wait(state);
            super::buddy::stopped(state);
            return None;
        }
        Step::Leaving { deadline, .. } if now >= deadline => {
            leave_finished(state, false);
            return None;
        }
        _ => {}
    }
    let mut deadlines: Vec<Instant> = Vec::new();
    deadlines.extend(lobby.step.timed_request().map(|request| request.deadline));
    deadlines.extend(
        lobby
            .wait
            .as_ref()
            .filter(|wait| !wait.shown)
            .map(WolWaitBox::reveal_at),
    );
    if let Step::ListingLobbies { deadline }
    | Step::Leaving { deadline, .. }
    | Step::FindingGame { deadline, .. } = lobby.step
    {
        deadlines.push(deadline);
    }
    if let Some(at) = lobby.refresh_at {
        if now >= at {
            lobby.refresh_at = None;
            lobby.ui.set_enabled(REFRESH, true);
        } else {
            deadlines.push(at);
        }
    }
    // The timers run only while 0x113 is the current window.
    let current_window = lobby.interactive() && lobby.ui.popup.is_none() && !page_open;
    let mut requests = Vec::new();
    if current_window {
        if now >= lobby.games_at {
            lobby.games_at = now + GAMES_PERIOD;
            requests.push(WolServiceRequest::ListChannels {
                kind: GAME_CHANNEL,
                flags: 1,
            });
            if lobby.ui.is_modern() {
                requests.push(WolServiceRequest::ListMatches);
            }
        }
        if now >= lobby.lobbies_at {
            lobby.lobbies_at = now + LOBBIES_PERIOD;
            requests.push(WolServiceRequest::ListChannels { kind: 0, flags: 0 });
        }
        deadlines.push(lobby.games_at);
        deadlines.push(lobby.lobbies_at);
        // A held scroll arrow's repeat.
        deadlines.extend(lobby.ui.poll());
    }
    for request in requests {
        self::request(state, request);
    }
    deadlines.into_iter().min()
}

#[cfg(all(test, tests_app))]
mod tests;
