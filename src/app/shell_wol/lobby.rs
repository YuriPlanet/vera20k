//! The custom lobby runner `0x00787770` and dialog `0x113`'s app adapter.
//!
//! The runner creates `0x113` hidden behind the wait box, prints the MOTD,
//! requests the chat-lobby list, joins a lobby picked by `0x00788180` and
//! only then shows the dialog. While it shows, its proc `0x0078AC10` drives
//! the lists, chat, sorts, Create (`0x00789B60`) and Join (`0x00789960`
//! with admission `0x0079BA60` and the join `0x00788800`), and the chat
//! sink's OnJoin (`0x007A5E40`) answers joins. Every service exchange is a
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

use crate::util::time::{Duration, Instant};

use super::WolPage;
use super::service::{self, WolServiceEvent, WolServiceRequest};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::shell_network_setup as setup;
use crate::app::{App, AppState};
use crate::ui::lan_shell::{LanChatLine, LanEditKey};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::wol_chat::{self, WolChatFilter, WolChatLine, WolLobbyChatLine};
use crate::ui::wol_lobby::*;
use crate::ui::wol_shell::{BOX_NO, BOX_OK, BOX_YES, WAIT_CHECK_STATUS, WAIT_DISCONNECT};
use crate::ui::wol_shell::{WolMessageBox, WolWaitBox};
use winit::keyboard::KeyCode;

/// The list wait `0x0077F120` gives up after 30 s.
const LIST_WAIT: Duration = Duration::from_millis(30_000);
// The initial VERA public lobby; native diagnostic catalogs keep their rooms.
const PUBLIC_LOBBY_NAME: &str = "Lob_41_2";
/// `0x00788800`/`0x00789B60`: at most 150 x 100 ms for the leave.
const LEAVE_WAIT: Duration = Duration::from_millis(15_000);
/// `SetTimer(0, 30000)`: the game list; `SetTimer(1, 53000)`: the lobbies.
const GAMES_PERIOD: Duration = Duration::from_millis(30_000);
const LOBBIES_PERIOD: Duration = Duration::from_millis(53_000);
/// Refresh `0x55D` disables itself for `SetTimer(0x55D, 3000)`.
const REFRESH_HOLD: Duration = Duration::from_millis(3000);
/// `0x00789B60`: `[0xB77DC4] = 7` players.
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
    /// `[0xB78608]`: the options sink's preview-request stamp in 16 ms
    /// buckets, first set by its one-time init (`0x007A7EA0..0x007A7ED3`).
    pub(crate) preview_stamp: Option<u32>,
    /// The ladder table `0xB77D80` (`[0xB77E0C]` outside mode 3; `0x00780110`
    /// clears only the clan and mode-3 tables) and the high-score table
    /// `0xB76640`, keyed by the ASCII-lowered name the lookups use
    /// (`0x007B5B30`).
    /// RESIDUAL: Quick Match's mode-3 ladder table `0xB73AF8`, which the
    /// WOL reset clears, shares this table.
    pub(crate) ladder: std::collections::HashMap<String, service::WolLadderRecord>,
    pub(crate) high_scores: std::collections::HashMap<String, service::WolHighScoreRecord>,
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
    /// The Buddy List runner's second game-list request for Join.
    FindingGame {
        name: String,
        deadline: Instant,
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
            | Self::Creating { request, .. } => *request,
            _ => None,
        }
    }

    fn online_request_mut(&mut self) -> Option<&mut Option<OnlineRequest>> {
        match self {
            Self::JoiningLobby { request, .. }
            | Self::JoiningGame { request }
            | Self::Creating { request, .. } => Some(request),
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

/// A rejected online command enters the same reducer as a service callback.
/// Stale failures cannot close a different attempt, even for the same room.
pub(super) fn online_request_failed(state: &mut AppState, id: u64, code: u32) -> bool {
    let Some(lobby) = lobby(state) else {
        return false;
    };
    if !lobby.step.accepts_online_result(id) {
        return false;
    }
    let event = if matches!(lobby.step, Step::Creating { .. }) {
        WolServiceEvent::Created(Err(code))
    } else {
        WolServiceEvent::Joined(Err(code))
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
    },
    /// The Quick Match runner's exit (`0x007A03A0..0x007A0410`): it
    /// returns 0 whether or not the leave finished.
    Return,
}

/// What closing a lobby box continues with.
#[derive(Debug, Clone)]
enum BoxThen {
    /// `0x007A5E40`'s failure tail; `close` when it set event 5.
    JoinFailed { close: bool },
    /// Create's boxes: the lobby back with Join disabled.
    CreateFailed,
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
    message: Option<(WolMessageBox, BoxThen)>,
    step: Step,
    postmatch: bool,
    games_at: Instant,
    lobbies_at: Instant,
    refresh_at: Option<Instant>,
    /// A game join's result that arrived while the dialog slid out.
    deferred: Option<Result<WolJoined, u32>>,
    /// `0x00789960`'s record while its password popup `0xE1` runs.
    joining: Option<WolChannel>,
}

impl WolLobby {
    fn set_lobbies(&mut self, mut channels: Vec<WolChannel>) {
        if self.ui.is_modern() {
            channels.retain(|channel| channel.name == PUBLIC_LOBBY_NAME);
        }
        self.lobbies = channels;
    }

    pub(crate) fn message(&self) -> Option<&WolMessageBox> {
        self.message.as_ref().map(|(message, _)| message)
    }

    /// The dialog or its popup takes input: no box, no wait.
    pub(crate) fn interactive(&self) -> bool {
        (self.shown || self.ui.popup.is_some()) && self.message.is_none() && self.wait.is_none()
    }
}

pub(crate) fn layout(state: &AppState) -> WolLobbyLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if let Some(lobby) = lobby(state) {
        return lobby.ui.layout(
            width as i32,
            height as i32,
            shell_layout::fills_window(state),
        );
    }
    if shell_layout::fills_window(state) {
        WolLobbyLayout::responsive(width as i32, height as i32, &crate::ui::wol_lobby::PAGE)
    } else {
        WolLobbyLayout::new(width as i32, height as i32, &crate::ui::wol_lobby::PAGE)
    }
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
pub(super) fn quick_match_runner(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.runner == Runner::QuickMatch)
}

/// The current channel's users `[0xB77908]`, in list order.
pub(super) fn members(state: &AppState) -> Vec<WolMember> {
    lobby(state).map_or_else(Vec::new, |lobby| lobby.members.clone())
}

/// The current channel's user names, in list order (`[0xB77900]` of them).
pub(crate) fn member_names(state: &AppState) -> Vec<String> {
    lobby(state).map_or_else(Vec::new, |lobby| {
        lobby
            .members
            .iter()
            .map(|member| member.name.clone())
            .collect()
    })
}

/// The dialog a runner's closed box or wait returns the input to.
fn runner_dialog(state: &AppState) -> crate::ui::shell::descriptor::DialogId {
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

/// This user is in a channel (`[0xB77DD0]`): a leave will see its OnPart.
pub(crate) fn in_channel(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.current.is_some())
}

pub(crate) fn current_channel(state: &AppState) -> Option<WolChannel> {
    lobby(state)?.current.clone()
}

fn lobby_mut(state: &mut AppState) -> Option<&mut WolLobby> {
    state.frontend.wol.as_mut()?.lobby.as_deref_mut()
}

fn request(state: &mut AppState, request: WolServiceRequest) {
    service::request(state, request);
}

fn csf(state: &AppState, key: &str) -> String {
    App::csf_label(state, key, key)
}

fn units(state: &AppState, key: &str) -> Vec<u16> {
    csf(state, key).encode_utf16().collect()
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

/// The login nickname `[0xB73E40]` (the MultiPlayer handle).
fn nickname(state: &AppState) -> String {
    state.frontend.wol_account.nickname.clone()
}

fn admission_context(state: &AppState) -> AdmissionContext {
    AdmissionContext {
        quick_match: super::quick_match::mode_active(state),
        clan_id: squad_id(state),
        local_hash: local_ini_hash(state),
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

fn request_listed(state: &mut AppState, listed: &[WolMember]) {
    let ladder = &state.frontend.wol_lobby_globals.ladder;
    let requests = listed_requests(listed, |key| ladder.contains_key(key));
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

/// Each member joins the lobby's users by the player sort.
fn insert_members(state: &mut AppState, members: impl IntoIterator<Item = WolMember>) {
    let globals = &state.frontend.wol_lobby_globals;
    let Some(lobby) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.lobby.as_deref_mut())
    else {
        return;
    };
    let (sort, rank) = (globals.player_sort, |name: &str| {
        ladder_rank(&globals.ladder, name)
    });
    for member in members {
        insert_member(&mut lobby.members, member, sort, &rank);
    }
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
    lobby(state)
        .and_then(|lobby| lobby.current.as_ref())
        .is_none_or(|channel| channel.kind == 0)
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

pub(super) fn base_names(state: &AppState) -> [String; 8] {
    std::array::from_fn(|index| csf(state, &format!("TXT_LOB_{}", index + 1)))
}

/// `0x00780110`'s view of the runner's wait box `0x792BE0(0)`; an open one
/// only takes the new text.
fn show_wait(state: &mut AppState, key: &str) {
    let fresh = service::wait_box(state, key);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    match lobby.wait.as_mut() {
        Some(open) => open.text = fresh.text,
        None => lobby.wait = Some(fresh),
    }
    lobby.ui.cancel_pointer();
    state
        .frontend
        .shell_controller
        .reset_to(crate::ui::wol_shell::WAIT_DIALOG, false);
}

/// `0x00792BE0(0)`: only an open box takes the text.
fn retext_wait(state: &mut AppState, key: &str) {
    let text = csf(state, key);
    if let Some(wait) = lobby_mut(state).and_then(|lobby| lobby.wait.as_mut()) {
        wait.text = text;
    }
}

fn close_wait(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.wait = None;
    }
    let dialog = runner_dialog(state);
    state.frontend.shell_controller.reset_to(dialog, false);
}

fn show_box(state: &mut AppState, key: &str, yes_no: bool, then: BoxThen) {
    let message = service::message_box(state, key, yes_no);
    put_box(state, message, then);
}

fn put_box(state: &mut AppState, message: WolMessageBox, then: BoxThen) {
    let dialog = message.dialog();
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.message = Some((message, then));
    lobby.ui.cancel_pointer();
    state.frontend.shell_controller.reset_to(dialog, false);
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
        visit.lobby = None;
        visit.page = WolPage::Backdrop { wait: None };
        state.frontend.wol_lobby_art = None;
        state.match_state.session.leave_network();
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
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
    state.frontend.wol_login_art = None;
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::WolLobby;
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
    let now = Instant::now();
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
        lobby.ui.observer = observer;
        lobby.step = Step::Ready;
        refill_players(state);
        switch_lobby(state);
        state.platform.window.request_redraw();
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
    open_art(state);
    // The MOTD [0xB73AE0]: one line per strtok("\n") token, a blank row,
    // and the global is cleared.
    if let Some(motd) = motd.filter(|motd| !motd.is_empty()) {
        for line in crate::rules::ini_value::strtok(&motd, &['\n']) {
            message_line(state, line.to_owned());
        }
        message_line(state, "\n".to_owned());
    }
    show_wait(state, "GUI:LoginComplete");
    // 0x00787BC8: the buddy mirror restarts (API+0xDC, 0x0079CE70 outside
    // Quick Match, 0x0079D2F0) before the chat-lobby list.
    // 0x00787B33..0x00787B53: the user's own squad.
    request_squad(state, 0);
    super::buddies::restart(state);
    show_wait(state, "TXT_REQ_CHANLIST");
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    state.platform.window.request_redraw();
}

fn new_runner(
    runner: Runner,
    observer: bool,
    postmatch: bool,
    now: Instant,
    modern: bool,
) -> Box<WolLobby> {
    Box::new(WolLobby {
        runner,
        ui: WolLobbyState::new(observer, modern),
        games: Vec::new(),
        lobbies: Vec::new(),
        members: Vec::new(),
        current: None,
        pending: None,
        shown: false,
        covered: false,
        wait: None,
        message: None,
        step: Step::ListingLobbies {
            deadline: now + LIST_WAIT,
        },
        postmatch,
        games_at: now + GAMES_PERIOD,
        lobbies_at: now + LOBBIES_PERIOD,
        refresh_at: None,
        deferred: None,
        joining: None,
    })
}

/// The Buddy List runner's entry (`0x0079F140`, steps 1-2): a runner with
/// no `0x113` and the chat-lobby list behind `TXT_REQ_CHANLIST`
/// (`0x0077EF90(0, 0)`, the 30 s wait `0x0077F120`).
pub(super) fn open_buddy_runner(state: &mut AppState) {
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.in_game = false;
    visit.page = WolPage::Lobby;
    visit.lobby = Some(new_runner(
        Runner::Buddy,
        observer,
        false,
        Instant::now(),
        !visit.legacy_ui,
    ));
    if state.match_state.session.network_return_context().is_none() {
        state
            .match_state
            .session
            .enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
    }
    state.frontend.wol_login_art = None;
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    show_wait(state, "TXT_REQ_CHANLIST");
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    state.platform.window.request_redraw();
}

/// The Quick Match runner's lobby list (`0x0079FCF0..0x0079FD35`): a
/// runner with no `0x113` and the chat-lobby list behind
/// `TXT_REQ_CHANLIST` (`0x0077EF90(0, 0)`, the 30 s wait `0x0077F120`).
pub(super) fn open_quick_match_runner(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Lobby;
    visit.lobby = Some(new_runner(
        Runner::QuickMatch,
        false,
        false,
        Instant::now(),
        !visit.legacy_ui,
    ));
    if state.match_state.session.network_return_context().is_none() {
        state
            .match_state
            .session
            .enter_network(crate::app::match_runtime::session::NetworkReturnContext::Wol);
    }
    state.frontend.wol_login_art = None;
    state.frontend.shell_route = crate::app::shell_route::ShellRoute::Wol;
    let _ = App::ensure_skirmish_shell_chrome(state);
    show_wait(state, "TXT_REQ_CHANLIST");
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    state.platform.window.request_redraw();
}

/// `0x00788800` for the Quick Match lobby (stage 2): a lobby already
/// joined or pending answers 0 without a request.
pub(super) fn quick_match_join(state: &mut AppState, channel: WolChannel, key: &str) {
    join_channel(state, channel, key, false, false);
}

/// `0x00789960(record, DL=0)` for the host's game the guest found: the
/// admission `0x0079BA60` with Quick Match's ranked test, then the join.
/// RESIDUAL: `DL=0`'s internals are not traced; the Quick Match game has
/// no key and no resolution bits, so neither popup can follow.
pub(super) fn quick_match_join_game(state: &mut AppState, channel: WolChannel) {
    if admit(&channel, false, admission_context(state)).is_err() {
        super::quick_match::game_joined(state, false);
        return;
    }
    join_channel(state, channel, "", true, false);
}

/// `0x00789B60`'s Quick Match branch (`0x00789BA8..0x00789D60`,
/// `0x00789EC4..0x00789EEE`): no `0xC3`, two users, no key,
/// `"%s's_game"`, tournament 1 (Quick Co-op 0, `0x00789EC9`), game flags
/// `0x800` plus `[0xB77DD4]` and no resolution bits; the leave and the
/// create then run as for Create.
pub(super) fn quick_match_create(state: &mut AppState) {
    let nickname = nickname(state);
    let tournament = i32::from(!super::quick_match::quick_coop(state));
    let (width, _) = options_screen(state);
    let globals = &mut state.frontend.wol_lobby_globals;
    if width >= 1024 {
        globals.limit_resolution = false;
    }
    let channel = WolChannel {
        kind: GAME_CHANNEL,
        max_users: 2,
        tournament,
        name: format!("{nickname}'s_game"),
        game_flags: GAME_QUICK_MATCH.wrapping_add(globals.lobby_number as u32),
        ..Default::default()
    };
    leave_then_create(state, channel, String::new(), false);
}

/// A box of the Quick Match runner over `0x117` or the backdrop.
pub(super) fn quick_match_box(state: &mut AppState, key: &str, then: QuickMatchBoxThen) {
    show_box(state, key, false, BoxThen::QuickMatch(then));
}

/// The runner's exit leave (`0x007A03A0..0x007A0410`): a channel this user
/// is in is left behind `WOL:LeavingChannel` (at most 150 x 100 ms);
/// then the runner returns 0.
pub(super) fn quick_match_leave_and_return(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state) else {
        close_runner(state);
        return;
    };
    lobby.pending = None;
    if lobby.current.is_none() {
        close_runner(state);
        return;
    }
    lobby.step = Step::Leaving {
        deadline: Instant::now() + LEAVE_WAIT,
        then: AfterLeave::Return,
    };
    request(state, WolServiceRequest::LeaveChannel);
    show_wait(state, "WOL:LeavingChannel");
}

/// The Buddy List runner's dialog loop takes over: its records stay, the
/// runner's own boxes are closed and the backdrop shows under `0x116`.
pub(super) fn buddy_dialog_opens(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.wait = None;
        lobby.message = None;
        lobby.step = Step::Ready;
        lobby.covered = false;
        lobby.ui.popup = None;
        lobby.joining = None;
    }
}

/// The Buddy List runner's dialog loop after a match or a stopped
/// Create/Join (C2/C9 Back left the game channel).
pub(super) fn buddy_runner_resumes(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        // A game channel C2 Back is still leaving stays until its OnPart.
        if lobby
            .current
            .as_ref()
            .is_none_or(|current| current.kind != GAME_CHANNEL)
        {
            lobby.current = None;
            lobby.members.clear();
        }
        lobby.pending = None;
        lobby.shown = false;
    }
}

/// C2 Back's own leave (API+0x24 at `0x007900B8`), after the dialog closed.
pub(crate) fn host_setup_left(state: &mut AppState) {
    if current_channel(state).is_some_and(|current| current.kind == GAME_CHANNEL) {
        request(state, WolServiceRequest::LeaveChannel);
    }
}

/// Join from `0x116` for a buddy in a listed game: `0x00789960` with the
/// record, over the empty backdrop. Its own admission `0x0079BA60` runs
/// first: `0x116`'s observer box does not recompute Join's enable, so the
/// game may refuse here; then `0x00789960` returns false and the loop goes
/// on. RESIDUAL: its refusal line (`show` = 1) goes to a list that is not
/// traced while `0x116` is down; none is printed.
pub(super) fn buddy_join(state: &mut AppState, channel: WolChannel) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::Ready;
        lobby.shown = false;
    }
    let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
    if admit(&channel, observer, admission_context(state)).is_err() {
        join_failed_locally(state, true);
        return;
    }
    continue_join(state, channel);
}

/// Join from `0x116` for a game the list lacks: type `0x29` again behind
/// `TXT_REQ_CHANLIST` (`0x0077EF90(0x29, 0)`, `0x0077F120`), then a
/// second search.
pub(super) fn buddy_find_game(state: &mut AppState, name: String) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::FindingGame {
            name,
            deadline: Instant::now() + LIST_WAIT,
        };
    }
    show_wait(state, "TXT_REQ_CHANLIST");
    request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 0,
        },
    );
}

/// `0x0072CD60`: MnScrnLCustomMatchLobby.shp through its own palette (800
/// wide only), with the list and button PCX images.
pub(crate) fn open_art(state: &mut AppState) -> bool {
    if state.frontend.wol_lobby_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_lobby_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("MnScrnLCustomMatchLobby.shp", "MnScrnLCustomMatchLobby.pal"),
        &PCX_FILES,
    );
    state.frontend.wol_lobby_art.is_some()
}

fn refill_games(state: &mut AppState) {
    let sort = state.frontend.wol_lobby_globals.game_sort;
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let rows = game_rows(&lobby.games, sort, lobby.ui.observer);
    lobby.ui.replace_games(rows);
}

fn refill_lobbies(state: &mut AppState) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let current = lobby.current.as_ref().map(|channel| channel.name.clone());
    lobby
        .ui
        .replace_lobbies(lobby_rows(&lobby.lobbies), current.as_deref());
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
    let rows = match &lobby.current {
        Some(channel) if channel.kind == 0 => lobby
            .members
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
}

/// `0x0079BA60` for the selected game row (`show` prints its refusal).
fn admit_row(state: &mut AppState, row: usize, observer: bool, show: bool) -> bool {
    let Some(lobby) = lobby(state) else {
        return false;
    };
    let Some(channel) = lobby
        .ui
        .game_rows
        .get(row)
        .and_then(|row| lobby.games.get(row.record))
    else {
        return false;
    };
    match admit(channel, observer, admission_context(state)) {
        Ok(()) => true,
        Err(key) => {
            if let (true, Some(key)) = (show, key) {
                let text = csf(state, key);
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
        .and_then(|row| lobby.games.get(row.record))
        .cloned()
}

/// The runner's lobby switch (`[0xB77DD8]`): controls off, the picker
/// without skipping crowded lobbies, the join; no lobby closes the runner.
fn switch_lobby(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.begin_lobby_switch();
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
/// pending is not joined again (it returns 0 without a request).
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
    if lobby.current.is_some() {
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
    lobby.step = if game {
        Step::JoiningGame { request: None }
    } else {
        Step::JoiningLobby {
            entry,
            request: None,
        }
    };
    show_wait(state, "TXT_JOINLOB");
    request(state, WolServiceRequest::JoinChannel { channel, key });
    if game && lobby_dialog(state).is_some() {
        // 0x00789960 returned true: 0x0078C281 slides the dialog out
        // (0x006071E0), draws the empty backdrop and hides it; C9 comes from
        // the join sink afterwards.
        App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolLobbyHide);
    }
}

/// `0x00785740` for this user: `TXT_YOULEFT` with the room text (a
/// `Lob_41_<n>` name as `TXT_JOINED_S` shows it, else the raw name; not in
/// Quick Match), then the channel record and its users cleared. The line
/// reaches the lobby's transcript under any wait box (`0x005E2700` walks
/// the dialog stack).
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
        Some(number) => WolLobbyRow::joined_name(number, &base_names(state)),
        None => current.name,
    };
    let text = crate::assets::csf_file::format_csf(
        &csf(state, "TXT_YOULEFT"),
        &[crate::assets::csf_file::CsfArg::Str(&room)],
    );
    message_line(state, text);
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
        } => {
            close_wait(state);
            send_create(state, channel, key, observe);
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
        lobby.ui.enable_commands(true);
    } else {
        lobby.ui.lobby_switched(false);
    }
}

/// The runner returns 0 (`LAB_00787F98`): the dialog closes without a
/// slide and `WOL_Main` shows the welcome (mode 0).
fn close_runner(state: &mut AppState) {
    if quick_match_runner(state) {
        super::quick_match::runner_closed(state);
    }
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.lobby = None;
    }
    state.frontend.wol_lobby_art = None;
    state.match_state.session.leave_network();
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

/// Buddy List's teardown has run: the runner ends as for Back, but
/// `WOL_Main` sees mode 5 (`0x0077B482`) and runs the Buddy List runner.
pub(crate) fn commit_buddy(state: &mut AppState) {
    request(state, WolServiceRequest::LeaveChannel);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.lobby = None;
    }
    state.frontend.wol_lobby_art = None;
    super::buddy::enter_from(state, crate::ui::wol_shell::WolMode::CustomMatch);
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
        lobby.ui.enable_commands(true);
        lobby.deferred.take()
    });
    if let Some(result) = deferred {
        joined(state, result);
    }
}

/// Create's slide has run: `UI__BeginDialog(0xC2)` over a hidden `0x113`.
pub(crate) fn commit_create(state: &mut AppState, observe: bool) {
    commit_hide(state);
    App::open_host_setup(state, observe);
}

/// API+0x1C, then `GUI:CreatingChannel` in a box created with flag 1 whose
/// modal loop waits for the result. The session preparation `0x00789B60`
/// runs before it (GameMode options, the host node, `[0x8316A4]`) belongs
/// to the setup owner that creates C2 ([`App::open_host_setup`]).
fn send_create(state: &mut AppState, channel: WolChannel, key: String, observe: bool) {
    // 0x00789F55..0x00789FB4: with registry Options bit 1 clear (VERA reads
    // no registry), the nickname's persona Country and Color become the
    // MultiPlayer picks the host node then takes (0x0069B700/0x0069B720).
    let nickname = nickname(state);
    super::persona::take_side_and_color(state, &nickname);
    if let Some(lobby) = lobby_mut(state) {
        lobby.step = Step::Creating {
            observe,
            channel: channel.clone(),
            request: None,
        };
    }
    request(state, WolServiceRequest::CreateChannel { channel, key });
    show_wait(state, "GUI:CreatingChannel");
}

/// `0x00789B60`'s record (`0x10C` bytes): type `0x29`, `"%s's_game"` of the
/// nickname, 7 users, the 0x629 byte (absent, 0) as tournament, the clan
/// and resolution bits plus the chat-lobby number `[0xB77DD4]` as game
/// flags. `width` is Options' ScreenWidth `[0xA8EB84]`; `ex_info` is
/// `0x0077E7A0`'s text (`0x00789D05`). `+0x04 = 1` has no field here.
fn created_channel(
    nickname: &str,
    clan: bool,
    limit: bool,
    width: i32,
    lobby_number: i32,
    ex_info: String,
) -> WolChannel {
    let mut game_flags = 0u32;
    if clan {
        game_flags |= GAME_CLAN;
    }
    if limit {
        game_flags |= if width == 800 {
            GAME_LIMIT_800
        } else {
            GAME_LIMIT_640
        };
    }
    WolChannel {
        kind: GAME_CHANNEL,
        max_users: CREATE_MAX_USERS,
        name: format!("{nickname}'s_game"),
        game_flags: game_flags.wrapping_add(lobby_number as u32),
        ex_info,
        ..Default::default()
    }
}

/// `0x0077E7A0`: the hash, then `0x005E94D0`/`0x005E9580`/`!0x005E9560`
/// over the slot array `[0xA8DA90]`, and the scenario `[0xA8B8E0]` (the
/// record at `[0xA8B254]`, as `SelectScenario` `0x005E7BF0` copies it). At
/// Create the WOL entry (`0x00786390` -> `0x005E8B60`) has filled every slot
/// with the Open or Closed sentinel, and no setup exists yet: no AI, no
/// observer and no open observer slot.
/// RESIDUAL: a setup earlier in the visit leaves its own slots, which
/// native's Create publishes instead.
fn ex_info(state: &AppState) -> String {
    let index = state.frontend.multiplayer_shell_runtime.scenario_index();
    let scenario = state
        .frontend
        .scenario_catalog
        .records()
        .get(index)
        .map(|record| record.file_name.clone())
        .unwrap_or_default();
    let (ai_players, observer_taken, observer_open) = slot_ex_info(&state.match_state.session);
    ExInfo {
        version: EX_INFO_VERSION,
        hash: local_ini_hash(state).unwrap_or(0),
        ai_players,
        observer_taken,
        observer_open,
        scenario,
    }
    .write()
}

/// The slot words of [`ex_info`]: the AI rows (`0x005E94D0`), a human
/// observer on the observer row `[0x8316A4]` (`0x005E9580`) and an open
/// observer row (`!0x005E9560`).
fn slot_ex_info(session: &crate::app::match_runtime::session::MatchSession) -> (i32, i32, i32) {
    use crate::app::match_runtime::session::NetworkVacancy;
    let Some(launch) = session.network_setup() else {
        return (0, 0, 0);
    };
    let rows = 0..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8;
    let ai = rows
        .clone()
        .filter(|row| session.network_display_ai(*row).is_some())
        .count();
    let observer_row = session.network_observer_row();
    let taken = observer_row.is_some()
        && launch
            .humans
            .iter()
            .any(|human| human.is_observer() && human.raw_slot_index == observer_row);
    let open = rows
        .clone()
        .any(|row| session.network_vacancy(row) == Some(NetworkVacancy::OpenObserver));
    (ai as i32, i32::from(taken), i32::from(open))
}

/// `0x0078F000`'s channel limit (`"mode #%s +l %d"`, API+0x9C): the host,
/// then over the setup's rows (`0x005E64C0`, one more with the observer
/// preference `[0xAC10C8]` at 1) every human and every Open or open
/// observer row, and past them every human and open observer row.
fn republished_limit(
    session: &crate::app::match_runtime::session::MatchSession,
    observer_preference: bool,
) -> Option<i32> {
    use crate::app::match_runtime::session::NetworkVacancy;
    let launch = session.network_setup()?;
    let rows = session.network_setup_capacity()? + i32::from(observer_preference);
    let mut limit = 1;
    for row in 1..crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as u8 {
        let human = launch
            .humans
            .iter()
            .any(|human| human.raw_slot_index == Some(row));
        let counted = match session.network_vacancy(row) {
            Some(NetworkVacancy::OpenObserver) => true,
            Some(NetworkVacancy::Open) => i32::from(row) < rows,
            _ => human,
        };
        limit += i32::from(counted);
    }
    Some(limit)
}

/// `0x0078F000`, the host's republish after its setup changed: the channel
/// limit and a fresh exInfo (API+0xA8) replace the Create record's.
/// RESIDUAL: API+0xFC's count (the limit plus the AI rows) has no field in
/// VERA's channel record.
pub(crate) fn republish_channel(state: &mut AppState) -> Option<WolChannel> {
    let observer = state
        .frontend
        .multiplayer_shell_runtime
        .lan_create_observer();
    let max_users = republished_limit(&state.match_state.session, observer)?;
    let ex_info = ex_info(state);
    let current = lobby_mut(state)?.current.as_mut()?;
    current.max_users = max_users;
    current.ex_info = ex_info;
    Some(current.clone())
}

/// `0x00789B60` after `0xC3` OK.
fn create_confirmed(
    state: &mut AppState,
    password: String,
    clan: bool,
    limit: bool,
    observe: bool,
) {
    let nickname = nickname(state);
    let (width, _) = options_screen(state);
    let globals = &mut state.frontend.wol_lobby_globals;
    globals.limit_resolution = limit;
    let lobby_number = globals.lobby_number;
    let ex_info = ex_info(state);
    let channel = created_channel(&nickname, clan, limit, width, lobby_number, ex_info);
    // The key goes to +0xD7 with spaces made '_'; the popup's observe byte
    // becomes the observer preference [0xAC10C8].
    let key = password.replace(' ', "_");
    state
        .frontend
        .multiplayer_shell_runtime
        .set_lan_observer(observe);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.popup = None;
    lobby.ui.observer = observe;
    leave_then_create(state, channel, key, observe);
}

/// `0x00789B60`'s leave and create: API+0x24 runs unconditionally, and
/// once more with the wait (a `0x792BE0(0)` retext: no box is created)
/// while still joined.
fn leave_then_create(state: &mut AppState, channel: WolChannel, key: String, observe: bool) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let joined = lobby.current.is_some();
    if joined {
        lobby.step = Step::Leaving {
            deadline: Instant::now() + LEAVE_WAIT,
            then: AfterLeave::Create {
                channel: channel.clone(),
                key: key.clone(),
                observe,
            },
        };
    }
    request(state, WolServiceRequest::LeaveChannel);
    if joined {
        request(state, WolServiceRequest::LeaveChannel);
        retext_wait(state, "WOL:LeavingChannel");
    } else {
        send_create(state, channel, key, observe);
    }
}

/// Create ended without C2: the commands back with Join disabled
/// (`0x0078AC10`'s 0x62B tail), focus on the chat edit. The Buddy List
/// runner takes its stop path back to `0x116`.
fn create_ended(state: &mut AppState) {
    if buddy_runner(state) {
        super::buddy::stopped(state);
        return;
    }
    if let Some(lobby) = lobby_mut(state) {
        lobby.covered = false;
        lobby.step = Step::Ready;
        lobby.ui.enable_commands(false);
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
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
    let Some(channel) = selected_channel(lobby) else {
        return;
    };
    lobby.ui.disable_commands();
    continue_join(state, channel);
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
            lobby.ui.popup = Some(WolLobbyPopup::password());
            lobby.joining = Some(channel);
        }
        state
            .frontend
            .shell_controller
            .reset_to(PASSWORD_DIALOG, false);
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
        } => create_confirmed(state, password, clan, limit, observe),
        WolPopupAction::CreateCancelled => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.popup = None;
            }
            create_ended(state);
        }
        WolPopupAction::PasswordConfirmed(password) => {
            let channel = lobby_mut(state).and_then(|lobby| {
                lobby.ui.popup = None;
                lobby.joining.take()
            });
            state
                .frontend
                .shell_controller
                .reset_to(super::WOL_LOBBY_DIALOG, false);
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
                lobby.ui.popup = None;
                lobby.joining = None;
            }
            state
                .frontend
                .shell_controller
                .reset_to(super::WOL_LOBBY_DIALOG, false);
            join_failed_locally(state, true);
        }
    }
}

/// `0x0078AC10`'s 0x540 Enter after `0x005EDD00`.
fn send_chat(state: &mut AppState, text: Vec<u16>) {
    if super::translation::command(state, &text) {
        return;
    }
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
            let text = csf(state, key);
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
                let text = csf(state, "TXT_NOT_IN_CHAN");
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
    request(state, WolServiceRequest::Chat { line, to });
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
        let text = csf(state, "TXT_NOT_IN_CHAN");
        message_line(state, text);
        return;
    }
    if text.is_empty() {
        let text = csf(state, "TXT_ENTER_MESSAGE");
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
    request(state, WolServiceRequest::Emote { text, to });
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
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    lobby.ui.disable_commands();
    lobby.ui.game_list.selected = None;
    lobby.covered = true;
    // +0x13 starts as [0xAC10C8] == 1.
    lobby.ui.popup = Some(WolLobbyPopup::create(width, limit, squad, observer));
    state
        .frontend
        .shell_controller
        .reset_to(CREATE_DIALOG, false);
    state.platform.window.request_redraw();
}

fn dispatch(state: &mut AppState, action: WolLobbyAction) {
    match action {
        WolLobbyAction::Create => begin_create(state),
        WolLobbyAction::Join => {
            let Some(lobby) = lobby(state) else {
                return;
            };
            let Some(row) = lobby.ui.game_list.selected else {
                return;
            };
            let show = lobby.ui.join_row != Some(row);
            begin_join(state, row, show);
        }
        WolLobbyAction::Back => {
            App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolLobbyBack);
        }
        WolLobbyAction::Refresh => {
            if let Some(lobby) = lobby_mut(state) {
                lobby.ui.set_enabled(REFRESH, false);
                lobby.refresh_at = Some(Instant::now() + REFRESH_HOLD);
            }
            request(
                state,
                WolServiceRequest::ListChannels {
                    kind: GAME_CHANNEL,
                    flags: 1,
                },
            );
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
                    resort_members(&mut lobby.members, sort, &rank);
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
            App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolOptionsOpen);
        }
        WolLobbyAction::Page => super::page::open_page_dialog(state),
        // 0x0078BF7D: mode 5 with return mode 4; event 6 then ends the
        // runner as Back does, and WOL_Main runs the Buddy List runner.
        WolLobbyAction::Unported(BUDDY) => {
            App::leave_shell_dialog(state, shell_transition::ShellExitThen::WolLobbyBuddy);
        }
        WolLobbyAction::Unported(control) => {
            // Clan/Tournament/News (0x00773E90 web pages) need the
            // embedded browser.
            log::warn!("WOL lobby control {control:#x} is not implemented; 0x113 stays");
        }
        WolLobbyAction::Popup(action) => popup_action(state, action),
    }
    state.platform.window.request_redraw();
}

/// A lobby box or wait-box button released.
pub(super) fn box_command(state: &mut AppState, control: u16) {
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if let Some((_, then)) = lobby
        .message
        .take_if(|_| matches!(control, BOX_OK | BOX_YES | BOX_NO))
    {
        let dialog = runner_dialog(state);
        state.frontend.shell_controller.reset_to(dialog, false);
        match then {
            BoxThen::QuickMatch(then) => super::quick_match::box_closed(state, then),
            BoxThen::JoinFailed { close } => join_failed_tail(state, close),
            BoxThen::CreateFailed => create_ended(state),
            BoxThen::Resolution { channel } => {
                answer_resolution(state, channel, control == BOX_YES)
            }
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
    match control {
        // Event 0 ends the runner's wait: LAB_00787F98.
        // 0x0077F120 failed during Join's second list: the Buddy List
        // runner searches again, finds nothing and loops.
        WAIT_DISCONNECT
            if lobby.wait.as_ref().is_some_and(|wait| wait.shown)
                && matches!(lobby.step, Step::FindingGame { .. }) =>
        {
            request(state, WolServiceRequest::CancelWait);
            super::buddy::stopped(state);
        }
        WAIT_DISCONNECT
            if lobby.wait.as_ref().is_some_and(|wait| wait.shown)
                && lobby.runner == Runner::QuickMatch =>
        {
            // Event 0: the runner's exit.
            request(state, WolServiceRequest::CancelWait);
            super::quick_match::exit(state);
        }
        WAIT_DISCONNECT if lobby.wait.as_ref().is_some_and(|wait| wait.shown) => {
            request(state, WolServiceRequest::CancelWait);
            close_runner(state);
        }
        WAIT_CHECK_STATUS => {
            log::warn!("WOL Check Status needs the embedded browser; not opened");
        }
        _ => {}
    }
}

/// `0x007A5E40`'s failure tail: rejoin flag from the pending channel, the
/// pending and current channels cleared, the wait closed, the lists
/// refilled and `0x113` shown; event 5 then ends the runner.
fn join_failed_tail(state: &mut AppState, close: bool) {
    if quick_match_runner(state) {
        // Event 5 acts only at stage 0, so `close` is the stage timeout's
        // business; a failed game join returns 0 from 0x00789960.
        let game = lobby(state).is_some_and(|lobby| matches!(lobby.step, Step::JoiningGame { .. }));
        if let Some(lobby) = lobby_mut(state) {
            lobby.pending = None;
            lobby.current = None;
            lobby.members.clear();
            lobby.step = Step::Ready;
        }
        close_wait(state);
        if game {
            super::quick_match::game_joined(state, false);
        }
        return;
    }
    if buddy_runner(state) {
        // The Join wait takes event 5 or 0xA as a stop: back to 0x116.
        if let Some(lobby) = lobby_mut(state) {
            lobby.pending = None;
            lobby.current = None;
            lobby.members.clear();
        }
        super::buddy::stopped(state);
        return;
    }
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let rejoin = lobby.pending.take().is_some();
    lobby.current = None;
    lobby.members.clear();
    let entry = matches!(lobby.step, Step::JoiningLobby { entry: true, .. });
    lobby.step = Step::Ready;
    lobby.shown = true;
    lobby.covered = false;
    close_wait(state);
    refill_games(state);
    refill_players(state);
    if close {
        close_runner(state);
    } else if entry {
        // The entry wait takes this event as success; [0xB77DD8] is reset.
        entry_shown(state);
    } else if rejoin {
        switch_lobby(state);
    }
}

/// The runner after its first join: `[0xB77DD8] = 0`, message `0x593`
/// (both lists requested), the wait closed and `0x113` shown.
fn entry_shown(state: &mut AppState) {
    if let Some(lobby) = lobby_mut(state) {
        lobby.shown = true;
        lobby.step = Step::Ready;
        let now = Instant::now();
        lobby.games_at = now + GAMES_PERIOD;
        lobby.lobbies_at = now + LOBBIES_PERIOD;
    }
    close_wait(state);
    request(state, WolServiceRequest::ListChannels { kind: 0, flags: 0 });
    request(
        state,
        WolServiceRequest::ListChannels {
            kind: GAME_CHANNEL,
            flags: 1,
        },
    );
}

/// `0x007A5E40` (CChatEventSink vt+0x28) for this user, or a result.
fn joined(state: &mut AppState, result: Result<WolJoined, u32>) {
    let online = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == super::WolProvider::Online);
    let Some(runner) = lobby_mut(state) else {
        return;
    };
    if runner.pending.is_none()
        || (online
            && (!matches!(
                runner.step,
                Step::JoiningGame { .. } | Step::JoiningLobby { .. }
            ) || runner.step.online_request().is_none()))
    {
        log::debug!("stale WOL join result");
        return;
    }
    if matches!(runner.step, Step::JoiningGame { .. }) && runner.shown {
        runner.deferred = Some(result);
        return;
    }
    if let Some(request) = runner.step.online_request_mut() {
        *request = None;
    }
    let joined = match result {
        Ok(joined) => joined,
        Err(code) => {
            join_error(state, code);
            return;
        }
    };
    let runner = lobby_mut(state).expect("checked");
    runner.pending = None;
    let entry = matches!(runner.step, Step::JoiningLobby { entry: true, .. });
    runner.step = Step::Ready;
    runner.members.clear();
    let listed = joined.members.clone();
    insert_members(state, joined.members);
    let runner = lobby_mut(state).expect("checked");
    runner.current = Some(joined.channel.clone());
    request_listed(state, &listed);
    if joined.channel.kind == GAME_CHANNEL {
        // The guest dialog C9 over a hidden 0x113; the observer preference
        // [0xAC10C8] makes the local node an observer.
        close_wait(state);
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        let mut local = state.frontend.multiplayer_shell_runtime.local_participant();
        if observer {
            local.role = crate::skirmish_launch::LaunchHumanRole::Observer;
        }
        // [0x8316A4] = 7 until the host's "O<n>" arrives.
        App::open_wol_joined_setup(state, local, Some(7));
        // The user-list sink 0x7A7110: the others as nodes, then the
        // persona's Country and Color as the MultiPlayer picks
        // (0x69B760/0x69B7E0) and the first R.
        let me = nickname(state);
        let users: Vec<(String, bool)> = lobby(state)
            .map(|lobby| {
                lobby
                    .members
                    .iter()
                    .filter(|member| !member.name.eq_ignore_ascii_case(&me))
                    .map(|member| (member.name.clone(), member.flags & MEMBER_OPERATOR != 0))
                    .collect()
            })
            .unwrap_or_default();
        if setup::guest_users_listed(state, &users) {
            super::persona::take_side_and_color(state, &me);
            setup::send_first_choices(state);
            if quick_match_runner(state) {
                super::quick_match::game_joined(state, true);
            }
        } else {
            // 0x7A782A..0x7A788D: no operator, the local tail 0x785740.
            setup_channel_left(state);
        }
        return;
    }
    // A chat lobby: its number (0x7A5EFD, any type-0 name), the
    // TXT_JOINED_S line, the controls back with Join as the selected game
    // admits for either role.
    state.frontend.wol_lobby_globals.lobby_number = joined.channel.joined_lobby_number();
    if quick_match_runner(state) {
        // 0x7A5E86..0x7A5EC8: no TXT_JOINED_S in modes 1-3; the wait closes
        // and the lobby's user list sends the request (0x7A7B0D..0x7A7B3B).
        close_wait(state);
        super::quick_match::lobby_users_listed(state);
        return;
    }
    let number = joined.channel.lobby_number();
    let name = match number {
        Some(number) => WolLobbyRow::joined_name(number, &base_names(state)),
        None => joined.channel.name.clone(),
    };
    let text = crate::assets::csf_file::format_csf(
        &csf(state, "TXT_JOINED_S"),
        &[crate::assets::csf_file::CsfArg::Str(&name)],
    );
    message_line(state, text);
    let selected = lobby(state).and_then(|lobby| lobby.ui.game_list.selected);
    let join = selected.is_some_and(|row| {
        admit_row(state, row, false, false) || admit_row(state, row, true, false)
    });
    if let Some(lobby) = lobby_mut(state) {
        lobby.ui.lobby_switched(join);
    }
    refill_lobbies(state);
    refill_players(state);
    if entry {
        entry_shown(state);
    } else {
        close_wait(state);
    }
}

/// `0x007A5E40`'s HRESULT branches in Custom Match.
/// How `0x007A5E40` answers a failed join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoinErrorReply {
    /// `0x8004006C`: nothing happens.
    Ignore,
    /// A box, then the failure tail; `close` sets event 5 (the runner
    /// returns 0). `forget` drops the pending channel's records first;
    /// `next_lobby` increments `[0xB77DD4]`.
    Box {
        key: &'static str,
        close: bool,
        forget: bool,
        next_lobby: bool,
    },
    /// `TXT_CANT_JOINCHAN` (`%X` of the code) through `0x005E2700`, then
    /// the tail with event 5.
    Chat,
}

fn join_error_reply(code: u32, pending: bool) -> JoinErrorReply {
    let reply = |key, close, forget, next_lobby| JoinErrorReply::Box {
        key,
        close,
        forget,
        next_lobby,
    };
    match code {
        0x8004_006C => JoinErrorReply::Ignore,
        0x8004_0070 => reply("TXT_GAME_CLOSED", false, true, false),
        0x8004_0071 => reply("TXT_BADPASS", false, false, false),
        0x8004_006E => reply("TXT_CHANNEL_FULL", !pending, false, false),
        0x8004_0072 => reply("TXT_JOINBAN", true, false, true),
        _ => JoinErrorReply::Chat,
    }
}

fn join_error(state: &mut AppState, code: u32) {
    let pending = lobby(state).is_some_and(|lobby| lobby.pending.is_some());
    let reply = join_error_reply(code, pending);
    if quick_match_runner(state)
        && let JoinErrorReply::Box {
            key: key @ ("TXT_GAME_CLOSED" | "TXT_BADPASS" | "TXT_CHANNEL_FULL"),
            ..
        } = reply
    {
        // 0x7A5961..0x7A5D2A: modes 1-3 print these as lines (colour -1)
        // instead of the boxes; every failure sets event 5.
        let text = csf(state, key);
        message_line(state, text);
        join_failed_tail(state, true);
        return;
    }
    match reply {
        JoinErrorReply::Ignore => {}
        JoinErrorReply::Box {
            key,
            close,
            forget,
            next_lobby,
        } => {
            if let Some(lobby) = lobby_mut(state)
                && forget
                && let Some(name) = lobby.pending.as_ref().map(|channel| channel.name.clone())
            {
                lobby.lobbies.retain(|channel| channel.name != name);
                lobby.games.retain(|channel| channel.name != name);
            }
            if next_lobby {
                let globals = &mut state.frontend.wol_lobby_globals;
                globals.lobby_number = globals.lobby_number.wrapping_add(1);
            }
            show_box(state, key, false, BoxThen::JoinFailed { close });
        }
        JoinErrorReply::Chat => {
            let text = csf(state, "TXT_CANT_JOINCHAN").replacen("%X", &format!("{code:X}"), 1);
            message_line(state, text);
            join_failed_tail(state, true);
        }
    }
}

fn created(state: &mut AppState, result: Result<Vec<WolMember>, u32>) {
    let online = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == super::WolProvider::Online);
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    let Step::Creating {
        observe,
        channel,
        request,
    } = lobby.step.clone()
    else {
        log::debug!("stale WOL create result");
        return;
    };
    if online && request.is_none() {
        return;
    }
    let quick_match = lobby.runner == Runner::QuickMatch;
    match result {
        Err(code) => {
            // The rejected create no longer owns a live admission. Keep the
            // normal error box/return tail, but ignore a late success callback.
            lobby.step = Step::Ready;
            close_wait(state);
            let key = if code == BAD_CHANNEL_KEY {
                "TXT_BAD_CHANKEY"
            } else {
                "TXT_CANTCREATE_CHAN"
            };
            let then = if quick_match {
                BoxThen::QuickMatch(QuickMatchBoxThen::CreateFailed)
            } else {
                BoxThen::CreateFailed
            };
            show_box(state, key, false, then);
        }
        Ok(members) => {
            close_wait(state);
            let nickname = nickname(state);
            let found = members.iter().any(|member| {
                crate::util::native_string::crt_strcmpi(&member.name, &nickname).is_eq()
            });
            if !found {
                // Not among the users: 0x00789B60 returns 0 silently.
                if quick_match {
                    if let Some(lobby) = lobby_mut(state) {
                        lobby.step = Step::Ready;
                    }
                    super::quick_match::created(state, false);
                } else {
                    create_ended(state);
                }
                return;
            }
            let lobby = lobby_mut(state).expect("checked");
            // 0x7A57C7..0x7A57EB: the created channel is the current one
            // ([0xB76B10]) and this user its host ([0xB77E4C]).
            let listed = members.clone();
            lobby.current = Some(channel);
            lobby.members = members;
            lobby.step = Step::Ready;
            let buddy = lobby.runner == Runner::Buddy;
            request_listed(state, &listed);
            if quick_match {
                // 0x0078A36C..0x0078A386: modes 1-3 skip C2; the game
                // channel's nodes run without a dialog.
                App::open_host_setup(state, false);
                super::quick_match::created(state, true);
            } else if buddy {
                // 0x116 has ended: C2 opens over the empty backdrop.
                commit_create(state, observe);
            } else {
                App::leave_shell_dialog(
                    state,
                    shell_transition::ShellExitThen::WolCreateAccepted { observe },
                );
            }
        }
    }
}

/// Lobby events of the service. Events nothing waits for are stale.
pub(super) fn service_event(state: &mut AppState, event: WolServiceEvent) {
    if lobby(state).is_none() {
        log::debug!("WOL lobby event without a lobby: {event:?}");
        return;
    }
    match event {
        WolServiceEvent::Channels { kind, channels } => {
            let lobby = lobby_mut(state).expect("checked");
            if kind == GAME_CHANNEL && lobby.runner == Runner::QuickMatch {
                lobby.games = channels.clone();
                super::quick_match::games_listed(state, &channels);
                state.platform.window.request_redraw();
                return;
            }
            if kind == GAME_CHANNEL {
                lobby.games = channels;
                refill_games(state);
                // The channel-list sink rebuilds an open 0x116's rows.
                super::buddy::buddies_changed(state, false);
                let lobby = lobby_mut(state).expect("checked");
                if let Step::FindingGame { name, .. } = &lobby.step {
                    let name = name.clone();
                    lobby.step = Step::Ready;
                    close_wait(state);
                    super::buddy::game_listed(state, &name, true);
                }
                state.platform.window.request_redraw();
                return;
            }
            lobby.set_lobbies(channels);
            refill_lobbies(state);
            let lobby = lobby_mut(state).expect("checked");
            if let (Step::ListingLobbies { .. }, Runner::QuickMatch) = (&lobby.step, lobby.runner) {
                lobby.step = Step::Ready;
                let lobbies = lobby.lobbies.clone();
                close_wait(state);
                super::quick_match::lobbies_listed(state, &lobbies);
            } else if let (Step::ListingLobbies { .. }, Runner::Buddy) = (&lobby.step, lobby.runner)
            {
                lobby.step = Step::Ready;
                close_wait(state);
                super::buddy::lobbies_listed(state);
            } else if let Step::ListingLobbies { .. } = lobby.step {
                show_wait(state, "TXT_JOINLOB");
                let skip_full = state
                    .frontend
                    .wol
                    .as_ref()
                    .is_some_and(|visit| visit.lobby_joined_before)
                    && !lobby_postmatch(state);
                let joined = join_picked_lobby(state, skip_full, true);
                if let Some(visit) = state.frontend.wol.as_mut() {
                    visit.lobby_joined_before = true;
                }
                if !joined {
                    close_runner(state);
                }
            }
        }
        WolServiceEvent::Joined(result) => joined(state, result),
        WolServiceEvent::UserJoined(member) => {
            let lobby = lobby_mut(state).expect("checked");
            if lobby.current.is_some() {
                let name = member.name.clone();
                // 0x7A62B7..0x7A62EE: a joiner in a squad whose tag is not
                // known yet.
                if member.squad != 0 && member.clan_tag.is_empty() {
                    request_squad(state, member.squad);
                }
                // 0x7A6542..0x7A662D: the lowered name's ladder record and
                // high score, each unless its table holds it, before the
                // insert.
                // RESIDUAL: native kicks a game-channel joiner while a start
                // is pending ([0xB77DEC]/[0xB77E28]/[0xB77E27]) before these
                // requests and the insert; VERA kicks in setup::user_joined.
                let key = name.to_ascii_lowercase();
                let globals = &state.frontend.wol_lobby_globals;
                let ladder = !globals.ladder.contains_key(&key);
                let high_score = !globals.high_scores.contains_key(&key);
                if ladder {
                    request(state, WolServiceRequest::LadderRecords(vec![key.clone()]));
                }
                if high_score {
                    request(state, WolServiceRequest::HighScores(vec![key]));
                }
                insert_members(state, [member]);
                refill_players(state);
                if in_setup_channel(state)
                    && let Err(error) = setup::user_joined(state, name)
                {
                    log::warn!("WOL game-channel join: {error:#}");
                }
            }
        }
        WolServiceEvent::UserLeft(name) => {
            let lobby = lobby_mut(state).expect("checked");
            let operator = lobby
                .members
                .iter()
                .any(|member| member.name == name && member.flags & MEMBER_OPERATOR != 0);
            lobby.members.retain(|member| member.name != name);
            refill_players(state);
            if in_setup_channel(state)
                && let Err(error) = setup::user_left(state, &name, operator)
            {
                log::warn!("WOL game-channel leave: {error:#}");
            }
        }
        WolServiceEvent::ChannelLeft if in_setup_channel(state) => setup_channel_left(state),
        WolServiceEvent::ChannelLeft => leave_finished(state, true),
        WolServiceEvent::PlayerChoices {
            from,
            slot,
            observer,
        } => {
            // 0x7A82F8..0x7A833C: only from a user of this channel.
            let member = lobby_mut(state)
                .expect("checked")
                .members
                .iter()
                .any(|member| member.name.eq_ignore_ascii_case(&from));
            if member
                && in_setup_channel(state)
                && let Err(error) = setup::player_choices(state, &from, slot, observer)
            {
                log::warn!("WOL choices from {from}: {error:#}");
            }
        }
        WolServiceEvent::PlayerReady { from, value } => {
            if in_setup_channel(state)
                && let Err(error) = setup::player_ready(state, &from, value)
            {
                log::warn!("WOL readiness from {from}: {error:#}");
            }
        }
        WolServiceEvent::PreviewRequested { from } => {
            // 0x7A82F8..0x7A833C: only from a user of this channel.
            let member = lobby_mut(state)
                .expect("checked")
                .members
                .iter()
                .any(|member| member.name.eq_ignore_ascii_case(&from));
            if member
                && in_setup_channel(state)
                && let Err(error) = setup::preview_request(state)
            {
                log::warn!("WOL preview request from {from}: {error:#}");
            }
        }
        WolServiceEvent::MapStatus { from, status } => {
            if in_setup_channel(state)
                && let Err(error) = setup::map_status(state, &from, status)
            {
                log::warn!("WOL map status from {from}: {error:#}");
            }
        }
        WolServiceEvent::Created(result) => created(state, result),
        other => log::debug!("WOL event not for the lobby: {other:?}"),
    }
    state.platform.window.request_redraw();
}

/// The runner is in a game channel with C2/C9 open over the hidden `0x113`,
/// or with Quick Match's setup under `0x117`.
fn in_setup_channel(state: &AppState) -> bool {
    (state.frontend.shell_route.network_setup() || setup::quick_match(state))
        && lobby(state)
            .and_then(|lobby| lobby.current.as_ref())
            .is_some_and(|current| current.kind == GAME_CHANNEL)
}

fn lobby_postmatch(state: &AppState) -> bool {
    lobby(state).is_some_and(|lobby| lobby.postmatch)
}

fn sender(member: &WolMember) -> wol_chat::WolSender<'_> {
    wol_chat::WolSender {
        operator: member.flags & MEMBER_OPERATOR != 0,
        name: &member.name,
        clan: &member.clan_name,
    }
}

/// The two gates every chat, action and page sink runs: the language gate
/// (`0x0077D2E0`, inlined) and `0x0077D4C0`. The runner holds `0x113` in
/// the shell stack, also while C2/C9 run over it, until it hands over to a
/// match (it closes every dialog first); C2/C9 run when the setup route is
/// up. The local record is found by the login nickname
/// (`[0xB73E40]`) among the channel's users.
fn sender_passes(state: &AppState, from: &WolMember, text: &[u16]) -> bool {
    let preferences = &state.frontend.wol_preferences;
    let filters = wol_chat::WolDisplayFilters {
        latin: preferences.display_latin != 0,
        asian: preferences.display_asian != 0,
        buddies: preferences.display_buddies,
        clan: preferences.display_clan,
    };
    let sender = sender(from);
    let setup_open = state.frontend.shell_route.network_setup();
    let local = nickname(state);
    let in_game = state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game);
    let lobby = lobby_dialog(state).filter(|_| !in_game);
    let local_clan = lobby
        .and_then(|lobby| {
            lobby
                .members
                .iter()
                .find(|member| member.name.eq_ignore_ascii_case(&local))
        })
        .map(|member| member.clan_name.as_str());
    wol_chat::language_gate(&sender, setup_open, text, filters)
        && wol_chat::buddy_clan_gate(
            &sender,
            local_clan,
            state.frontend.wol_buddies.names(),
            filters,
            lobby.is_some(),
        )
}

/// One chat or action line of [`WolServiceEvent::Message`].
pub(super) struct ReceivedMessage {
    pub(super) from: WolMember,
    pub(super) text: Vec<u16>,
    pub(super) private: bool,
    pub(super) action: bool,
    pub(super) ansi: bool,
    pub(super) options: Option<Box<setup::HostOptions>>,
}

/// The chat sinks (`0x007A8DA0`, `0x007A8FC0`, `0x007AB170`, `0x007AAFB0`
/// and their UTF-16 forms): nothing while a match runs (`[0xB77DC1]`);
/// then the gates; a line under this user's own name (`strcmpi` with
/// `[0xA8B380]`) is not shown, the send already echoed it. The line goes to
/// the current dialog's transcript (`0x005E2700`), the lobby's, C2/C9's or
/// `0x117`'s. In modes 1-3 the two ANSI message sinks route to the Quick
/// Match runner instead (`0x007A8DBB..0x007A8DEB`, `0x007A8FDB..0x007A9024`);
/// the UTF-16 and action sinks have no mode test.
pub(super) fn message_received(state: &mut AppState, message: ReceivedMessage) {
    let ReceivedMessage {
        from,
        text,
        private,
        action,
        ansi,
        options,
    } = message;
    let in_game = state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game);
    if ansi && !action && !in_game && super::quick_match::mode_active(state) {
        let text = String::from_utf16_lossy(&text);
        super::quick_match::ansi_message(state, &from, &text, private, options);
        return;
    }
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game)
        || !sender_passes(state, &from, &text)
        || from.name.eq_ignore_ascii_case(&nickname(state))
    {
        return;
    }
    let (line, color) = wol_chat::received_line(&sender(&from), &text, private, action);
    echo(state, line, color);
    let (prefix, _) = wol_chat::received_line(&sender(&from), &[], private, action);
    super::translation::received(
        state,
        &text,
        super::translation::TranslatedLine::Wol { prefix, color },
    );
    state.platform.window.request_redraw();
}

/// The find sink `0x007A9970` for a locate this user sent (`[0xB76E28]`),
/// outside a buddy-status locate and an open `0xCF`:
/// `TXT_USER_FOUND` with a chat lobby's room text (type 0 named
/// `Lob_41_<n>`, as `TXT_JOINED_S` shows it) or the channel's own name;
/// otherwise the code's `TXT_USER_*` text. The line goes, white, to the
/// current dialog's transcript and the sink returns; only the path without
/// a transcript clears the name (`0x007AA685`).
/// RESIDUAL: in a match the line is a MessageList row (WOL in-match chain).
pub(super) fn located(state: &mut AppState, result: Result<WolChannel, u32>) {
    // 0x007A9986: a pending buddy locate takes any answer first.
    if super::buddies::located(state, &result) {
        state.platform.window.request_redraw();
        return;
    }
    // 0x007A997A: with 0xCF in the stack its status takes the answer.
    if super::page::located(state, &result) {
        return;
    }
    let Some(visit) = state.frontend.wol.as_ref() else {
        return;
    };
    let name = visit.locate_name.clone().unwrap_or_default();
    use crate::assets::csf_file::{CsfArg, format_csf};
    let text = match result {
        Ok(channel) => {
            let room = match channel.lobby_number().filter(|_| channel.kind == 0) {
                Some(number) => WolLobbyRow::joined_name(number, &base_names(state)),
                None => channel.name.clone(),
            };
            format_csf(
                &csf(state, "TXT_USER_FOUND"),
                &[CsfArg::Str(&name), CsfArg::Str(&room)],
            )
        }
        Err(code) => {
            let key = match code {
                0x0004_0131 => "TXT_USER_NOT_IN_CHANNEL",
                0x0004_0132 => "TXT_USER_FIND_DISABLED",
                _ => "TXT_USER_NOT_ONLINE",
            };
            format_csf(&csf(state, key), &[CsfArg::Str(&name)])
        }
    };
    // 0x007AA5D5..0x007AA685: colour [0x7EEF28] = -1; `[0xB76E28]` is
    // cleared either way.
    let line = LanChatLine {
        text: text.encode_utf16().collect(),
        color: u32::MAX,
    };
    if !super::page::current_window_line(state, line) {
        log::info!("WOL find result outside a transcript: {text}");
    }
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.locate_name = None;
    }
    state.platform.window.request_redraw();
}

/// The page-send result sink `0x007A9800`: success writes `TXT_PAGESENT`
/// through `0x005E2790`, so only an open `0xCF` shows it. A failure goes
/// to the current window only: its `0x53F` (`0x113`, C2/C9, `0x11E`), else
/// its `0x5B5` (`0xCF`).
/// RESIDUAL: with neither, a match posts a MessageList row (in-match chain).
pub(super) fn page_result(state: &mut AppState, result: Result<(), u32>) {
    let key = match result {
        Ok(()) => "TXT_PAGESENT",
        Err(0x0004_0134) => "TXT_PAGEDISABLED",
        Err(0x0004_0133) => "TXT_PAGEUSERGONE",
        Err(_) => "TXT_PAGEFAILED",
    };
    let line = LanChatLine {
        text: units(state, key),
        color: u32::MAX,
    };
    if result.is_ok() {
        super::page::append_page_line(state, line);
    } else {
        super::page::current_window_line(state, line);
    }
    state.platform.window.request_redraw();
}

/// The NetStatus sink `0x007AA6D0` outside the server-list wait: only
/// [`service::NET_STATUS_LOST`] while connected (`[0xB77DC0]` clear) does
/// anything. In a match it posts one `TXT_WOL_CONNLOST` row (`[0xB77DC2]`
/// once) and clears the channel records, leaving `[0xB77DC0]` alone; the
/// runner logs in again on its next entry. Outside a match, with `0x113`
/// in the stack, `TXT_YOURE_DISCON` shows from inside the sink; then
/// `[0xB77DC0]` and `[0xB77DC2]` are set, the channel records cleared and
/// event 0 ends the runner, whose `[0xB77AD0]` box is empty (the welcome).
/// RESIDUAL: native shows that box over a running C2/C9; here the setup is
/// dropped first and the box shows over `0x113`. A native slide pumps no
/// messages, so the sink never runs mid-slide; here a status arriving during
/// a C2/C9 Back slide is handled at once and the slide's end then starts a
/// re-login over the box.
pub(super) fn net_status(state: &mut AppState, status: u32) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if !visit.connected || status != service::NET_STATUS_LOST {
        return;
    }
    if visit.in_game {
        if visit.disconnected {
            return;
        }
        visit.disconnected = true;
        if let Some(lobby) = visit.lobby_mut() {
            lobby.current = None;
            lobby.pending = None;
            lobby.members.clear();
        }
        crate::app::input::messages::post_wol_connection_lost(state);
        return;
    }
    visit.connected = false;
    visit.disconnected = true;
    let Some(lobby) = visit.lobby_mut() else {
        return;
    };
    lobby.current = None;
    lobby.pending = None;
    lobby.members.clear();
    if let (Runner::Buddy | Runner::QuickMatch, Step::ListingLobbies { .. }) =
        (lobby.runner, &lobby.step)
    {
        // The entry's list wait 0x0077F120 fails without a box: mode 0,
        // the welcome.
        close_runner(state);
        return;
    }
    if lobby.runner == Runner::QuickMatch {
        // 0x7AA7E3: TXT_YOURE_DISCON needs 0x113 open; the Quick Match
        // runner only takes event 0, and its exit has no channel to leave.
        super::quick_match::exit(state);
        state.platform.window.request_redraw();
        return;
    }
    if lobby.runner == Runner::Buddy {
        // The Buddy List runner's pump takes event 0: 0x116 ends without a
        // slide and TXT_YOURE_DISCON shows before the welcome. Over C2/C9
        // the setup is dropped first, as for the lobby.
        if state.frontend.shell_route.network_setup() {
            crate::app::shell_network_setup::abandon_for_wol_runner(state);
        }
        super::buddy::dialog_ended(state);
        show_box(state, "TXT_YOURE_DISCON", false, BoxThen::Close);
        state.platform.window.request_redraw();
        return;
    }
    if state.frontend.shell_route.network_setup() {
        crate::app::shell_network_setup::abandon_for_wol_runner(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::WolLobby;
        if let Some(lobby) = lobby_mut(state) {
            lobby.shown = true;
            lobby.covered = false;
        }
        open_art(state);
    }
    // The records are zeroed without a list refill; the box shows over the
    // lists as they were, and the runner then closes.
    show_box(state, "TXT_YOURE_DISCON", false, BoxThen::Close);
    state.platform.window.request_redraw();
}

/// The kick sink `0x007AB670` in a chat lobby; nothing while a match runs.
/// This user: `TXT_YOURE_KICKED` with the channel's name and the kicker in
/// a box, whose close moves the lobby pick on (`[0xB77DD4]`) and runs the
/// leave tail `0x00785740`. Another user: `TXT_USER_KICKED` in the
/// transcript, then its leave from the users. In a game channel this user's
/// line (`TXT_CHANNEL_FULL` when the session is over-full) goes to the
/// hidden `0x113`'s transcript (`0x7AB824..0x7AB850`) and the local leave
/// closes C2/C9; another user's line goes to C2/C9.
/// RESIDUAL: in a chat lobby another user's line is skipped natively when
/// `0x005E9510` finds no open slot among the session nodes, which there
/// depends on what the last setup left; it is always shown here.
pub(super) fn kicked(state: &mut AppState, channel: WolChannel, user: WolMember, by: WolMember) {
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game)
    {
        return;
    }
    let Some(lobby) = lobby(state) else {
        return;
    };
    use crate::assets::csf_file::{CsfArg, format_csf};
    if lobby
        .current
        .as_ref()
        .is_some_and(|current| current.kind != 0)
    {
        if !state.frontend.shell_route.network_setup() {
            return;
        }
        if user.flags & MEMBER_LOCAL != 0 {
            let key = setup::kicked_key(state);
            let text = format_csf(
                &csf(state, key),
                &[CsfArg::Str(&channel.name), CsfArg::Str(&by.name)],
            );
            crate::app::shell_lan::append_wol_lobby_chat(
                state,
                LanChatLine {
                    text: text.encode_utf16().collect(),
                    color: u32::MAX,
                },
            );
            setup_channel_left(state);
            return;
        }
        let text = format_csf(
            &csf(state, "TXT_USER_KICKED"),
            &[
                CsfArg::Str(&user.name),
                CsfArg::Str(&channel.name),
                CsfArg::Str(&by.name),
            ],
        );
        if let Some(lobby) = lobby_mut(state) {
            lobby.members.retain(|member| member.name != user.name);
        }
        if let Err(error) = setup::user_kicked(state, &user.name, text) {
            log::warn!("WOL game-channel kick: {error:#}");
        }
        return;
    }
    if user.flags & MEMBER_LOCAL != 0 {
        let text = format_csf(
            &csf(state, "TXT_YOURE_KICKED"),
            &[CsfArg::Str(&channel.name), CsfArg::Str(&by.name)],
        );
        let message = service::message_box_text(state, text, false);
        put_box(state, message, BoxThen::Kicked);
    } else {
        let text = format_csf(
            &csf(state, "TXT_USER_KICKED"),
            &[
                CsfArg::Str(&user.name),
                CsfArg::Str(&channel.name),
                CsfArg::Str(&by.name),
            ],
        );
        message_line(state, text);
        if let Some(lobby) = lobby_mut(state) {
            lobby.members.retain(|member| member.name != user.name);
        }
        refill_players(state);
    }
    state.platform.window.request_redraw();
}

/// The ban sink `0x007AB3C0`: outside a match, `TXT_CHAN_BAN` or
/// `TXT_CHAN_UNBAN` with the name in the transcript.
pub(super) fn channel_ban(state: &mut AppState, name: &str, banned: bool) {
    if state
        .frontend
        .wol
        .as_ref()
        .is_none_or(|visit| visit.in_game)
    {
        return;
    }
    let key = if banned {
        "TXT_CHAN_BAN"
    } else {
        "TXT_CHAN_UNBAN"
    };
    let text = crate::assets::csf_file::format_csf(
        &csf(state, key),
        &[crate::assets::csf_file::CsfArg::Str(name)],
    );
    message_line(state, text);
    state.platform.window.request_redraw();
}

/// The page sinks after their gates: the ANSI one (`0x007A9200`) keeps the
/// sender as the `/reply` target `[0xB77E30]`; the UTF-16 one (`0x007A9550`)
/// does not. Outside a match the line `u"(%hs) %s"` walks the shell stack
/// from the current window: `0xCF`'s list `0x5B5` takes it and the walk goes
/// on (`0x005E2790`); the first transcript `0x53F` (`0x113`, C2/C9 or an
/// open `0x11E`) takes it and the walk stops; with none, `0x11E` opens.
/// In a match (`[0xB77DC1]`, `0x007A92F9..0x007A93DC`) the line is a Grey
/// MessageList row, the ANSI sink adds `TXT_HOW_TO_REPLY` under it, an open
/// `0xCF` takes the line too, and the ANSI sink arms the reply window
/// (now, 300) that prefills the next `0xCF`. `[AudioVisual] IncomingMessage`
/// plays last in both.
/// RESIDUAL: the auto-reply `[0xB77E40]`/`[0xB76230]` has no writer here.
pub(super) fn page_received(state: &mut AppState, from: WolMember, text: Vec<u16>, ansi: bool) {
    if state.frontend.wol.is_none() || !sender_passes(state, &from, &text) {
        return;
    }
    let visit = state.frontend.wol.as_mut().expect("checked");
    if ansi {
        visit.last_pager = Some(from.name.clone());
    }
    let in_game = visit.in_game;
    let mut units: Vec<u16> = format!("({}) ", from.name).encode_utf16().collect();
    units.extend_from_slice(&text);
    let line = LanChatLine {
        text: units,
        color: wol_chat::PAGE_COLOR,
    };
    if in_game {
        let row = String::from_utf16_lossy(&line.text);
        crate::app::input::messages::post_wol_page_rows(state, &row, ansi);
        super::page::append_page_line(state, line);
        if ansi {
            let now = crate::ui::wol_page::reply_window_now(
                crate::app::match_runtime::frame_pacer::wall_clock_ms(),
            );
            state
                .frontend
                .wol_lobby_globals
                .page_reply_window
                .arm(now, crate::ui::wol_page::REPLY_WINDOW_TICKS);
        }
    } else {
        super::page::append_page_line(state, line.clone());
        // The walk down the stack stops at the first `0x53F`: the lobby's
        // (also while Options `0x2BC` hides it), C2/C9's, or `0x11E`'s.
        let route = state.frontend.shell_route;
        let transcript = super::page::page_received(state).is_none()
            && (route.network_setup()
                || lobby_dialog(state).is_some()
                || super::quick_match::page_open(state));
        if transcript {
            crate::app::shell_lan::append_chat(state, line);
        } else {
            super::page::page_received_line(state, line);
        }
    }
    // 0x7A94ED..0x7A9510: with [0xB77E40] the pager gets the Quick Match
    // away text (API+0xEC).
    super::quick_match::page_reply(state, &from.name);
    let sound = state
        .rules()
        .and_then(|rules| rules.general.incoming_message_sound())
        .map(str::to_owned);
    App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
    let font = &state.renderer.bit_font;
    let Some(lobby) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.lobby.as_deref_mut())
    else {
        return;
    };
    let ui = &mut lobby.ui;
    let scale = layout.page.scale;
    for (list, rect, count) in [
        (&mut ui.lobby_list, layout.lobbies, ui.lobby_rows.len()),
        (&mut ui.game_list, layout.games, ui.game_rows.len()),
        (&mut ui.player_list, layout.players, ui.player_rows.len()),
        (&mut ui.chat.list, layout.chat_log, ui.chat.lines.len()),
    ] {
        list.reconcile_geometry(rect, count, scale);
    }
    ui.chat
        .edit
        .scroll_to_caret((scale.base_rect(layout.chat).w - 3).max(0) as u32, |text| {
            font.text_width(text)
        });
    if let Some(popup) = ui.popup.as_mut() {
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let rect = match popup.kind {
            WolPopupKind::Create(_) => {
                crate::ui::shell::modal::wol_create_game_layout(width, height).password
            }
            WolPopupKind::Password(_) => {
                crate::ui::shell::modal::wol_channel_password_layout(width, height).password
            }
        };
        let available = (rect.w - 3).max(0) as u32;
        match &mut popup.kind {
            WolPopupKind::Create(create) => create
                .password
                .scroll_to_caret(available, |text| font.text_width(text)),
            WolPopupKind::Password(edit) => {
                edit.scroll_to_caret(available, |text| font.text_width(text))
            }
        }
    }
}

/// The chat NewEdit while it takes keys, for the input method
/// (`app::input::ime`): `handle_wol_key` reaches [`key_press`] with the lobby
/// page up, no page dialog and no popup.
pub(crate) fn ime_chat_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    let routed = App::wol_active(state)
        && !shell_transition::blocks_shell_input(state)
        && !App::wol_quick_match_verify(state)
        && !super::page::open(state)
        && App::wol_page_is_lobby(state);
    lobby(state)
        .filter(|lobby| routed && lobby.interactive() && lobby.ui.popup.is_none())
        .map(|_| layout(state).chat)
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state).is_some_and(WolLobby::interactive)
    {
        return;
    }
    let key = match code {
        Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
        Some(KeyCode::Delete) => Some(LanEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
        Some(KeyCode::Home) => Some(LanEditKey::Home),
        Some(KeyCode::End) => Some(LanEditKey::End),
        Some(KeyCode::Tab) => Some(LanEditKey::Tab),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
        // The NewEdits consume Escape; 0x113 has no IDCANCEL case.
        Some(KeyCode::Escape) => return,
        _ => None,
    };
    let lobby = lobby_mut(state).expect("checked");
    let action = match key {
        Some(key) => lobby.ui.edit_key(key),
        None => {
            lobby.ui.insert_text(text.unwrap_or_default());
            None
        }
    };
    update_geometry(state);
    if let Some(action) = action {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

/// The popup control under the pointer.
fn popup_control(state: &AppState, popup: &WolLobbyPopup) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    let (width, height) = (
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    );
    let controls = match popup.kind {
        WolPopupKind::Create(_) => {
            let layout = crate::ui::shell::modal::wol_create_game_layout(width, height);
            vec![
                (POPUP_OK, layout.ok),
                (POPUP_CANCEL, layout.cancel),
                (CREATE_CLAN, layout.clan),
                (CREATE_LIMIT, layout.limit),
                (CREATE_OBSERVE, layout.observe),
                (POPUP_PASSWORD, layout.password),
            ]
        }
        WolPopupKind::Password(_) => {
            let layout = crate::ui::shell::modal::wol_channel_password_layout(width, height);
            vec![
                (POPUP_OK, layout.ok),
                (POPUP_CANCEL, layout.cancel),
                (POPUP_PASSWORD, layout.password),
            ]
        }
    };
    controls
        .into_iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(id, _)| id)
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let popup_over = lobby(state)
        .and_then(|lobby| lobby.ui.popup.as_ref())
        .map(|popup| popup_control(state, popup));
    let Some(lobby) = lobby_mut(state) else {
        return;
    };
    if let (Some(over), Some(popup)) = (popup_over, lobby.ui.popup.as_mut()) {
        popup.buttons.hovered = over;
        state.platform.window.request_redraw();
        return;
    }
    let ui = &mut lobby.ui;
    ui.buttons.hovered = layout.control_at(x, y);
    let scale = layout.page.scale;
    for (list, rect, count) in [
        (&mut ui.lobby_list, layout.lobbies, ui.lobby_rows.len()),
        (&mut ui.game_list, layout.games, ui.game_rows.len()),
        (&mut ui.player_list, layout.players, ui.player_rows.len()),
        (&mut ui.chat.list, layout.chat_log, ui.chat.lines.len()),
    ] {
        let geometry = list.geometry(rect, count, scale);
        list.hot_row = geometry.row_at(count, list.top_index, x, y);
        list.scroll
            .pointer_moved(geometry, &mut list.top_index, x, y);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

enum Press {
    Nothing,
    Button,
    Cue(LauncherCue),
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    if shell_transition::blocks_shell_input(state)
        || !lobby(state).is_some_and(WolLobby::interactive)
    {
        return;
    }
    update_geometry(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let scale = layout.page.scale;
    if let Some(popup) = lobby(state).and_then(|lobby| lobby.ui.popup.clone()) {
        let over = popup_control(state, &popup);
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let lobby = lobby_mut(state).expect("checked");
        let popup = lobby.ui.popup.as_mut().expect("checked");
        if !pressed {
            let action = popup
                .buttons
                .release(over)
                .and_then(|control| popup.activate(control));
            if let Some(action) = action {
                dispatch(state, WolLobbyAction::Popup(action));
            }
            state.platform.window.request_redraw();
            return;
        }
        let press = match over {
            Some(control @ (CREATE_CLAN | CREATE_LIMIT | CREATE_OBSERVE)) => {
                let rect = crate::ui::shell::modal::wol_create_game_layout(width, height);
                let window = match control {
                    CREATE_CLAN => rect.clan,
                    CREATE_LIMIT => rect.limit,
                    _ => rect.observe,
                };
                if crate::ui::skirmish_shell::checkbox_icon_rect(
                    window,
                    crate::ui::shell::geom::ShellScale::RETAIL,
                )
                .contains(x, y)
                    && popup.toggle(control)
                {
                    Press::Cue(LauncherCue::Checkbox)
                } else {
                    Press::Nothing
                }
            }
            Some(control @ (POPUP_OK | POPUP_CANCEL)) if popup.enabled(control) => {
                popup.buttons.press(Some(control));
                Press::Button
            }
            _ => {
                popup.buttons.press(None);
                Press::Nothing
            }
        };
        play(state, press);
        state.platform.window.request_redraw();
        return;
    }
    let over = layout.control_at(x, y);
    let lobby = lobby_mut(state).expect("checked");
    let ui = &mut lobby.ui;
    if !pressed {
        for list in [
            &mut ui.lobby_list,
            &mut ui.game_list,
            &mut ui.player_list,
            &mut ui.chat.list,
        ] {
            list.scroll.cancel();
        }
        let action = ui
            .buttons
            .release(over)
            .and_then(|control| ui.activate(control));
        if let Some(action) = action {
            dispatch(state, action);
        }
        state.platform.window.request_redraw();
        return;
    }
    let now = Instant::now();
    let mut action = None;
    let mut press = Press::Nothing;
    let mut scrolled = false;
    let enabled = [LOBBIES, GAMES, PLAYERS, CHAT_LOG].map(|id| ui.enabled(id));
    for (enabled, list, rect, count) in [
        (
            enabled[0],
            &mut ui.lobby_list,
            layout.lobbies,
            ui.lobby_rows.len(),
        ),
        (
            enabled[1],
            &mut ui.game_list,
            layout.games,
            ui.game_rows.len(),
        ),
        (
            enabled[2],
            &mut ui.player_list,
            layout.players,
            ui.player_rows.len(),
        ),
        (
            enabled[3],
            &mut ui.chat.list,
            layout.chat_log,
            ui.chat.lines.len(),
        ),
    ] {
        if enabled {
            let geometry = list.geometry(rect, count, scale);
            if let Some(part) = geometry.scroll_part_at(x, y) {
                list.scroll
                    .press(part, geometry, &mut list.top_index, y, now);
                scrolled = true;
                break;
            }
        }
    }
    if !scrolled {
        match over {
            Some(control @ (LOBBIES | GAMES | PLAYERS)) => {
                let (rect, count, top) = match control {
                    LOBBIES => (layout.lobbies, ui.lobby_rows.len(), ui.lobby_list.top_index),
                    GAMES => (layout.games, ui.game_rows.len(), ui.game_list.top_index),
                    _ => (
                        layout.players,
                        ui.player_rows.len(),
                        ui.player_list.top_index,
                    ),
                };
                let geometry = crate::ui::shell::list::ShellListGeometry::new(
                    rect,
                    count,
                    top,
                    crate::ui::shell::list::ListMetrics::scaled(scale),
                );
                if let Some(row) = geometry.row_at(count, top, x, y) {
                    action = match control {
                        LOBBIES => ui.select_lobby(row),
                        GAMES => {
                            let double = crate::ui::shell::list::is_double_click(
                                &mut ui.game_list.last_press,
                                now,
                                x,
                                y,
                                crate::ui::shell::saved_file_input::host_double_click_limits(),
                            );
                            ui.select_game(row, double)
                        }
                        _ => {
                            ui.toggle_player(row);
                            None
                        }
                    };
                    press = Press::Cue(LauncherCue::GenericClick);
                }
            }
            Some(control @ (OBSERVER_IMAGE | OBSERVER_TOGGLE)) => {
                let icon_hit = control == OBSERVER_IMAGE
                    || crate::ui::skirmish_shell::checkbox_icon_rect(layout.observer_toggle, scale)
                        .contains(x, y);
                if icon_hit {
                    action = ui.activate(control);
                    press = Press::Cue(LauncherCue::Checkbox);
                }
            }
            Some(control) if control != CHAT && control != CHAT_LOG && ui.enabled(control) => {
                ui.buttons.press(Some(control));
                press = Press::Button;
            }
            _ => ui.buttons.press(None),
        }
    }
    play(state, press);
    if let Some(action) = action {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

fn play(state: &mut AppState, press: Press) {
    match press {
        Press::Cue(cue) => App::play_launcher_options_cue(state, cue),
        // Owner-draw buttons play GUIMainButtonSound on the press.
        Press::Button => App::play_main_menu_button_sound(state),
        Press::Nothing => {}
    }
}

/// The runner's timeouts, the wait box reveal, the dialog's timers (only
/// while it is the current window) and the shared scrollbar repeats.
pub(super) fn poll(state: &mut AppState) -> Option<Instant> {
    let now = Instant::now();
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let mut redraw = false;
    let page_open = super::page::page_dialog(state).is_some();
    let lobby = lobby_mut(state)?;
    redraw |= lobby.wait.as_mut().is_some_and(|wait| wait.poll(now));
    if let Some(request_id) = lobby
        .step
        .online_request()
        .filter(|request| now >= request.deadline && lobby.deferred.is_none())
        .map(|request| request.id)
    {
        // An uncertain HTTP outcome may have joined on the server. Queue a
        // normal leave behind that command before allowing another attempt.
        request(state, WolServiceRequest::LeaveChannel);
        online_request_failed(state, request_id, 0x8000_4005);
        state.platform.window.request_redraw();
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
            state.platform.window.request_redraw();
            return None;
        }
        Step::FindingGame { deadline, .. } if now >= deadline => {
            lobby.step = Step::Ready;
            close_wait(state);
            super::buddy::stopped(state);
            state.platform.window.request_redraw();
            return None;
        }
        Step::Leaving { deadline, .. } if now >= deadline => {
            leave_finished(state, false);
            state.platform.window.request_redraw();
            return None;
        }
        _ => {}
    }
    let mut deadlines: Vec<Instant> = Vec::new();
    deadlines.extend(lobby.step.online_request().map(|request| request.deadline));
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
            redraw = true;
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
        }
        if now >= lobby.lobbies_at {
            lobby.lobbies_at = now + LOBBIES_PERIOD;
            requests.push(WolServiceRequest::ListChannels { kind: 0, flags: 0 });
        }
        deadlines.push(lobby.games_at);
        deadlines.push(lobby.lobbies_at);
        let ui = &mut lobby.ui;
        let scale = layout.page.scale;
        for (list, rect, count) in [
            (&mut ui.lobby_list, layout.lobbies, ui.lobby_rows.len()),
            (&mut ui.game_list, layout.games, ui.game_rows.len()),
            (&mut ui.player_list, layout.players, ui.player_rows.len()),
            (&mut ui.chat.list, layout.chat_log, ui.chat.lines.len()),
        ] {
            let geometry = list.geometry(rect, count, scale);
            redraw |= list.scroll.poll(geometry, &mut list.top_index, x, y, now);
            deadlines.extend(list.scroll.repeat_at());
        }
    }
    for request in requests {
        self::request(state, request);
    }
    if redraw {
        state.platform.window.request_redraw();
    }
    deadlines.into_iter().min()
}

/// The status help of what the pointer is over: the table's key, or a
/// list's `0x4E9` answer (`0x00780E30` for a game, `0x00781090` for a user).
pub(crate) fn status_help(state: &AppState) -> String {
    let Some(lobby) = lobby(state) else {
        return String::new();
    };
    if let Some(popup) = lobby.ui.popup.as_ref() {
        return match popup.kind {
            WolPopupKind::Create(_) => popup.buttons.hovered.and_then(create_tooltip_key),
            WolPopupKind::Password(_) => None,
        }
        .map(|key| csf(state, key))
        .unwrap_or_default();
    }
    let ui = &lobby.ui;
    match ui.buttons.hovered {
        Some(GAMES) => match ui.game_list.hot_row.and_then(|row| ui.game_rows.get(row)) {
            None => csf(state, "STT:LobbyListGames"),
            Some(row) => game_help(state, row),
        },
        Some(PLAYERS) => match ui
            .player_list
            .hot_row
            .and_then(|row| ui.player_rows.get(row))
        {
            None => csf(state, "STT:LobbyListUsers"),
            Some(row) if ui.is_modern() => row.name.clone(),
            Some(row) => {
                let member = lobby.members.iter().find(|member| member.name == row.name);
                user_help(state, &row.name, member)
            }
        },
        Some(control) => tooltip_key(control, ui.page_spec())
            .map(|key| csf(state, key))
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// `0x00780A90`: `"<nick> : "`, then `TXT_RANK` and the ladder rank or
/// `TXT_UNRANKED`, then `": "` and the clan name (`+0x2E`, else the tag
/// `+0x57`) when there is one, then `" : "` and `WOL:Locale%02d` for a
/// known nonzero locale. Both tables are looked up by the ASCII-lowered
/// name (`0x007B5B30`). A player row passes its user record; a game row's
/// record (`0x00780E30`) holds only the host's name, so no clan.
/// RESIDUAL: the lobby's locale queue `0xB765F8` (filled at `0x007A73C8`
/// and `0x007A6495`) is not ported; only the user's own locale request
/// fills the locale table.
fn user_help(state: &AppState, name: &str, member: Option<&WolMember>) -> String {
    let Some(visit) = state.frontend.wol.as_ref() else {
        return String::new();
    };
    let key = name.to_ascii_lowercase();
    let mut help = format!("{name} : ");
    match state.frontend.wol_lobby_globals.ladder.get(&key) {
        Some(record) => {
            help.push_str(&csf(state, "TXT_RANK"));
            help.push(' ');
            help.push_str(&record.rank.to_string());
        }
        None => help.push_str(&csf(state, "TXT_UNRANKED")),
    }
    if let Some(member) = member {
        let clan = if member.clan_name.is_empty() {
            &member.clan_tag
        } else {
            &member.clan_name
        };
        if !clan.is_empty() {
            help.push_str(": ");
            help.push_str(clan);
        }
    }
    if visit.api
        && let Some(locale) = visit.locales.get(&key).filter(|locale| **locale != 0)
    {
        help.push_str(" : ");
        help.push_str(&csf(state, &format!("WOL:Locale{locale:02}")));
    }
    help
}

/// `0x00780D90`: `TXT_RANDOM_MAP_DESCRIPTION` for `RandMap.Sed`, else the
/// description (`+0x00`) of the first scenario record (`[0xA8B8CC]`)
/// whose file name (`+0x58`) equals `scenario` without case; empty when
/// none does.
fn scenario_description(state: &AppState, scenario: &str) -> String {
    use crate::util::native_string::crt_strcmpi;
    use std::cmp::Ordering;
    if crt_strcmpi(scenario, crate::app::shell_random_map::RANDMAP_SED_FILE) == Ordering::Equal {
        return csf(
            state,
            crate::app::shell_random_map::RANDOM_MAP_DESCRIPTION_KEY,
        );
    }
    state
        .frontend
        .scenario_catalog
        .records()
        .iter()
        .find(|record| crt_strcmpi(&record.file_name, scenario) == Ordering::Equal)
        .map(|record| record.display_name.clone())
        .unwrap_or_default()
}

/// `0x00780E30`: the host (the name up to `'`, at most 10 characters), the
/// ping, then `", "` and the map when exInfo names a scenario `0x00780D90`
/// knows.
fn game_help(state: &AppState, row: &WolGameRow) -> String {
    let host: String = row
        .name
        .split('\'')
        .next()
        .unwrap_or_default()
        .chars()
        .take(10)
        .collect();
    let ping = if row.ping == -1 {
        csf(state, "TXT_UNKNOWN_PING")
    } else {
        crate::assets::csf_file::format_csf(
            &csf(state, "MSG:PingInfo"),
            &[crate::assets::csf_file::CsfArg::Int(i64::from(row.ping))],
        )
    };
    let mut help = format!("{}, {ping}", user_help(state, &host, None));
    let scenario = lobby(state)
        .and_then(|lobby| lobby.games.get(row.record))
        .map(|channel| channel.ex_info().scenario)
        .unwrap_or_default();
    if !scenario.is_empty() {
        let name = scenario_description(state, &scenario);
        if !name.is_empty() {
            help.push_str(", ");
            help.push_str(&name);
        }
    }
    help
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x0078F000` counts Open rows only within the setup's rows (one more
    /// with the observer preference) but open observer rows anywhere.
    #[test]
    fn republished_limit_counts_open_rows_within_the_setup() {
        use crate::app::match_runtime::session::{
            MatchSession, NetworkReturnContext, NetworkVacancy,
        };
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
        launch.rebuild_house_roster();
        let mut vacancies =
            [NetworkVacancy::Closed; crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT];
        vacancies[2] = NetworkVacancy::Open;
        vacancies[3] = NetworkVacancy::Open;
        vacancies[5] = NetworkVacancy::OpenObserver;
        let mut session = MatchSession::default();
        session.enter_network(NetworkReturnContext::Wol);
        session
            .accept_network_setup(launch.clone(), false, None)
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
    fn saved_test_preference_cannot_load_demo_games_in_ordinary_play() {
        let saved_test_service = true;
        assert_eq!(
            super::super::provider_for_visit(saved_test_service, None),
            service::WolProvider::Online
        );
        assert_eq!(
            super::super::provider_for_visit(
                saved_test_service,
                Some(winit::dpi::PhysicalSize::new(800, 600)),
            ),
            service::WolProvider::TestService
        );
    }

    #[test]
    fn normal_test_service_keeps_the_simplified_ui_and_only_german_bunker() {
        let mut visit = crate::app::shell_wol::WolVisit::open(false);
        visit.set_provider(service::WolProvider::TestService);
        assert_eq!(visit.welcome_page_spec().stacked.len(), 1);
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
        assert_eq!(lobby.ui.page_spec().stacked.len(), 3);
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
        assert_eq!(legacy.ui.page_spec().stacked.len(), 8);
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
}
