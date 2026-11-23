//! Main Menu -> Internet: one Westwood Online visit, `WOL_Main`
//! (`0x0077B2A0`) from the welcome `0x10E` until WOL is left. Custom Match
//! runs the login flow `WOL__RunLoginFlow` (`0x00787010`): the WOLAPI
//! object, the server list behind the wait box `0xC4`, the login `0x10F`,
//! the connect and then the custom lobby runner (`0x00787770`, [`open_lobby`]);
//! Quick Match runs the same login and then its own runner
//! (`0x0079FC40`, [`quick_match`]).
//!
//! The normal online visit has no welcome: Custom Match is its mode, so
//! Internet and every return to mode 0 go straight to its sign-in `0x10F`,
//! whose Back leaves WOL. Its lobby's Quick Match runs the Quick Match runner
//! and comes back to the lobby. Native diagnostics keep the retail welcome.
//!
//! The WOLAPI COM object, its servers and its credential store are outside
//! the shell: they meet it only through [`WolServiceRequest`] /
//! [`WolServiceEvent`]. Production uses VERA's authenticated HTTPS lobby and
//! UDP relay through [`WolProvider::Online`]. The unavailable and controlled
//! providers preserve the original missing-component path and test boundaries.

pub(crate) mod buddies;
pub(crate) mod buddy;
pub(crate) mod leaf;
mod live_service;
pub(crate) use live_service::lobby_get;
pub(crate) mod lobby;
mod lobby_phrases;
mod login;
pub(crate) mod login_info;
pub(crate) mod my_info;
pub(crate) mod options;
pub(crate) mod page;
pub(crate) mod persona;
pub(crate) mod player_card;
pub(crate) mod quick_match;
mod service;
mod test_service;
pub(crate) mod translation;

pub(crate) use live_service::OnlineCleanup;
#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use live_service::discard_spawned_visit;
pub(crate) use live_service::keep_network_report;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use live_service::retire_visit_for_exit;
pub(crate) use live_service::{
    DecodedRelayGrant, begin_spawned_match, decode_external_relay_grant,
    decode_external_watch_grant, end_spawned_match, relay_ticket_is_watch,
};
pub(crate) use live_service::{
    local_choices_agreed, online_cleanup_pending, online_match_id, relay_pass_lapsed,
};
pub(crate) use login::{
    account_form_layout, login_layout, status_key as login_status_key,
};
pub(crate) use service::{
    NET_STATUS_LOST, SERVER_STATUS_READY, WolApiStatus, WolHighScoreRecord, WolLadderRecord,
    WolLoginAccepted, WolMatch, WolProvider, WolServer, WolServerList, WolServiceEvent,
    WolServiceRequest, service_event,
};
pub(crate) use test_service::{
    WolTestGuest, default_buddies as test_service_buddies,
    matchbot_options as test_service_matchbot_options,
};

/// The wait box's reveal timer and the login drop list's scroll repeats;
/// the caller schedules the earliest deadline with the other shell timers.
pub(crate) fn poll(state: &mut AppState) -> Option<crate::util::time::Instant> {
    [
        service::poll_wait(state),
        leaf::poll(state, leaf_lists_open(state)),
        buddy::poll(state),
        page::poll_scroll_repeat(state),
        lobby::poll(state),
        quick_match::poll(state),
        live_service::poll(state),
        test_service::poll(state),
    ]
    .into_iter()
    .flatten()
    .min()
}

/// The login, persona and My Information pages' scrollbar repeats run.
fn leaf_lists_open(state: &AppState) -> bool {
    matches!(
        state.frontend.wol.as_ref().map(|visit| &visit.page),
        Some(
            WolPage::Login(_)
                | WolPage::AutoLoginQuery
                | WolPage::Locale(_)
                | WolPage::AddBuddyQuery(_)
                | WolPage::MyInformation(_)
        )
    ) || options::translate_popup(state).is_some()
}

/// `0x0077DED0`, called only at `WOL_Main` entry (`0x0077B307`): the
/// nine `[WOnline]` values from VERA.INI, then `[MultiPlayer] ObserverMode`
/// (`0x005EE190`). The profile holds what VERA.INI says, so the chat
/// filter toggles of the last visit revert here.
pub(crate) fn read_wol_settings(state: &mut AppState) {
    state.frontend.wol_preferences = state.persistence.options_profile.wol_preferences;
    state
        .frontend
        .multiplayer_shell_runtime
        .reload_observer_mode();
}

/// `0x0077E110`: at `WOL_Main` entry (`0x0077B31D`, whose music half runs
/// inside `enter_wol_lobby_music`), at every mode-loop pass (`0x0077B402`)
/// and after WOL Options closes. `0x005EE350(0)` re-applies the observer
/// toggle to the open `0x116`/`0x113`/`0xBB`; at these points none is open
/// (a returning `0x113` is rebuilt from the toggle), so it is not run.
pub(crate) fn apply_wol_preferences(state: &mut AppState) {
    send_wol_preferences(state);
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    let lobby_music = state.frontend.wol_preferences.lobby_music != 0;
    state.audio.apply_wol_lobby_music_rule(lobby_music, now_ms);
}

/// `0x0077E110`'s first half: with the WOLAPI object it sends the language
/// filter (API+0x7C) and then find/page (API+0x68), whose results are
/// ignored.
fn send_wol_preferences(state: &mut AppState) {
    let live = state.frontend.wol_preferences;
    if state.frontend.wol.as_ref().is_some_and(|visit| visit.api) {
        service::request(state, WolServiceRequest::SetLangFilter(live.lang_filter));
        service::request(
            state,
            WolServiceRequest::SetFindPage {
                find: live.allow_find,
                page: live.allow_page,
            },
        );
    }
}

use super::{App, AppState};
use crate::app::frontend::shell_layout;
use crate::app::shell_route::ShellRoute;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::page::PageEvent;
use crate::ui::wol_login::{WolCredential, WolLoginState};
use crate::ui::wol_shell::{
    WOL_WELCOME_DIALOG, WolMode, WolWaitBox, WolWelcomeAction, action_for_control,
};

/// The custom lobby `0x113`'s controller identity.
pub(crate) const WOL_LOBBY_DIALOG: DialogResourceId = DialogResourceId(0x0113);

/// What one WOL page is showing.
#[derive(Debug, Clone)]
pub(crate) enum WolPage {
    /// The welcome `0x10E`.
    Welcome,
    /// A dialog has closed and the flow runs over the empty shell backdrop
    /// (`Shell__DrawEmptyBackdrop`), with the wait box `0xC4` while a
    /// service request is outstanding.
    Backdrop { wait: Option<WolWaitBox> },
    /// The login `0x10F`.
    Login(Box<WolLoginState>),
    /// A message box over the backdrop and what its close leads to.
    Message {
        message: crate::ui::shell::modal::ShellBox,
        then: WolBoxThen,
    },
    /// The custom lobby `0x113` (its runner belongs to the lobby chain).
    Lobby,
    /// WOL Options `0x2BC` (and its Sound Options `0xD6`) over the hidden
    /// lobby.
    Options(Box<options::WolOptionsPage>),
    /// The auto-login query `0x112` after a dialog login.
    AutoLoginQuery,
    /// The locale page `0xFE` during the connect.
    Locale(Box<crate::ui::wol_persona::WolLocalePage>),
    /// My Information `0x114` (welcome mode 6).
    MyInformation(Box<crate::ui::wol_my_info::WolMyInfoPage>),
    /// The Buddy List `0x116` (mode 5); its runner's records stay in the
    /// visit's [`lobby::WolLobby`].
    Buddy(Box<buddy::BuddyDialog>),
    /// Quick Match's `0x117` (mode 1); its runner's records stay in the
    /// visit's [`lobby::WolLobby`].
    QuickMatch(Box<quick_match::QuickMatchDialog>),
    /// Quick Match's postmatch add-buddy query `0x122`.
    AddBuddyQuery(Box<persona::AddBuddyQuery>),
}

/// What follows a WOL message box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolBoxThen {
    /// `WOL_Main` returns this result (-2 or -1) and leaves WOL.
    Leave(i32),
    /// The login flow returned 0: the welcome runs again.
    Welcome,
    /// The silent auto-login connect failed without a Yes/No result: the
    /// flow goes on to `0x10F`.
    Login,
}

/// One visit to Westwood Online, from Internet until `WOL_Main` returns.
#[derive(Debug)]
pub(crate) struct WolVisit {
    /// The shuffle flag `WOL_Main` saved on entry (`0x0077B30C`); restored
    /// only when the visit ends ([`App::return_from_wol`]).
    saved_shuffle: bool,
    /// `[0xA8B244]`; `None` is mode 0.
    mode: Option<WolMode>,
    /// `[0xA8B248]`: the mode the Buddy List runner returns to (0, 4 or 6).
    /// VERA: also 4 for a Quick Match runner the online lobby started.
    return_mode: Option<WolMode>,
    /// Login category `[0xB73678]`: set once the login flow returned 1.
    authenticated: bool,
    /// `[0xAC116C]`: the WOLAPI object exists. Created by `0x00786390`
    /// and released only when the visit ends (`0x0077B532` -> `0x00786080`).
    api: bool,
    pub(crate) page: WolPage,
    /// Server records of the last list (`0xB778D8` count, `0xF8` records)
    /// and the selected record `[0xB76468]` (-1 for none).
    servers: Vec<WolServer>,
    selected_server: i32,
    awaiting: service::Awaiting,
    /// The login flow between its connect and its return.
    flow: Option<service::LoginFlow>,
    provider: WolProvider,
    /// Full retail controls belong to explicit native UI captures only.
    legacy_ui: bool,
    live_service: Option<live_service::LiveService>,
    requests: Vec<WolServiceRequest>,
    /// Latest account form submission, including one still queued for the next poll.
    account_attempt: u64,
    /// The WOLAPI credential store as the service last reported it.
    credentials: Vec<WolCredential>,
    /// `[0xB77AD0]`: the text the login sinks leave for `0x00786EF0`.
    last_error: Option<&'static str>,
    /// `[0xB77DF8]`: the login result code.
    last_login_result: Option<u32>,
    /// `[0xB73AE0]`: the message of the day from a successful login.
    motd: Option<String>,
    /// `[0xB77E30]`: the last user who paged us, the `/reply` target.
    last_pager: Option<String>,
    /// `[0xB77DC1]`: a WOL match runs. Set when the runner hands over to
    /// the match and cleared on runner entry (`0x00787973`); the chat sinks
    /// show nothing while it is set.
    in_game: bool,
    /// VERA: why the last online match ended, until the chat the player
    /// lands in exists.
    problem_line: Option<String>,
    /// VERA: whether the online lobby lets players watch matches, as its
    /// list of them last answered (docs/lobby-protocol.md "Watching"); `None`
    /// before the first answer.
    lobby_watches: Option<bool>,
    /// VERA: the lobby lets a room's players visit the public lobby, as
    /// its sign-in answered (docs/lobby-protocol.md "Visiting the lobby").
    lobby_visits: bool,
    /// VERA: the lobby takes Multiplayer Campaign rooms: its sign-in offered
    /// `campaign_rooms` and it has refused no campaign room's options since.
    lobby_campaign_rooms: bool,
    /// `![0xB77DC0]`: connected to a server. A WOLAPI creation
    /// (`0x00786485`) and a failed login clear it, a connect sets it, and a
    /// lost connection outside a match clears it.
    connected: bool,
    /// `[0xB77DC2]`: the connection was lost. The next runner entry logs in
    /// again (`0x007879B6`); a successful login clears it (`0x007A559D`).
    disconnected: bool,
    /// A re-login on runner entry is running; it continues that runner,
    /// post-match or not. A failed login ends the runner instead.
    relogin: Option<bool>,
    /// `[0xB76E28]`: the name the last locate (`0x0078A470`) asked for; the
    /// find sink formats its answer with it and clears it.
    locate_name: Option<String>,
    /// The locale table `0xB76530`, keyed by the ASCII-lowered name the
    /// tooltip `0x00780A90` looks up with (`0x007B5B30`); `0x00780110`
    /// clears it.
    locales: std::collections::HashMap<String, i32>,
    /// The custom lobby runner; kept hidden while C2/C9 run over it.
    lobby: Option<Box<lobby::WolLobby>>,
    /// A chat lobby was joined earlier in this visit: the picker passes
    /// over crowded lobbies (`0x00788180`).
    lobby_joined_before: bool,
    /// `[0xB77DE4]`: the user's squad id, written by the squad sink while
    /// its own request is out (`[0xB77DE0]`). `[0xB77DDC]` counts the
    /// requests out. The tag `[0xB77A94]` has no reader.
    squad_id: i32,
    squad_own_pending: bool,
    squad_requests: u32,
    /// The page dialog `0xCF` over `0x113`, C2 or C9.
    page_dialog: Option<Box<crate::ui::wol_page::WolPageDialog>>,
    /// The modeless page-received dialog `0x11E`.
    page_received: Option<Box<crate::ui::wol_page::WolPageReceived>>,
    /// The Quick Match runner's globals; the opponent outlives the runner.
    quick_match: quick_match::QuickMatchGlobals,
    /// An external launch's relay match (docs/spawn.md) instead of a visit
    /// the Internet button opened.
    spawned: Option<live_service::SpawnedVisit>,
}

impl WolVisit {
    fn open(saved_shuffle: bool) -> Self {
        Self {
            saved_shuffle,
            mode: None,
            return_mode: None,
            authenticated: false,
            api: false,
            page: WolPage::Welcome,
            servers: Vec::new(),
            selected_server: 0,
            awaiting: service::Awaiting::Nothing,
            flow: None,
            provider: WolProvider::Unavailable,
            legacy_ui: false,
            live_service: None,
            requests: Vec::new(),
            account_attempt: 0,
            credentials: Vec::new(),
            last_error: None,
            last_login_result: None,
            motd: None,
            last_pager: None,
            in_game: false,
            problem_line: None,
            lobby_watches: None,
            lobby_visits: false,
            lobby_campaign_rooms: false,
            connected: false,
            disconnected: false,
            relogin: None,
            locate_name: None,
            locales: Default::default(),
            lobby: None,
            lobby_joined_before: false,
            squad_id: 0,
            squad_own_pending: false,
            squad_requests: 0,
            page_dialog: None,
            page_received: None,
            quick_match: Default::default(),
            spawned: None,
        }
    }

    pub(crate) fn modern_ui(&self) -> bool {
        !self.legacy_ui
    }

    /// Diagnostics select their presentation independently of the service.
    pub(crate) fn set_legacy_ui(&mut self, legacy: bool) -> bool {
        let changed = self.legacy_ui != legacy;
        self.legacy_ui = legacy;
        changed
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<WolServiceRequest> {
        std::mem::take(&mut self.requests)
    }

    /// Pending shell requests only; diagnostics never inspect credentials or payloads.
    pub(crate) fn pending_service_request_count(&self) -> usize {
        self.requests.len()
    }

    pub(crate) fn mode(&self) -> Option<WolMode> {
        self.mode
    }

    pub(crate) fn authenticated(&self) -> bool {
        self.authenticated
    }

    pub(crate) fn motd(&self) -> Option<&str> {
        self.motd.as_deref()
    }

    pub(crate) fn return_mode(&self) -> Option<WolMode> {
        self.return_mode
    }

    pub(crate) fn wait_box(&self) -> Option<&WolWaitBox> {
        match &self.page {
            WolPage::Backdrop { wait } => wait.as_ref(),
            WolPage::Lobby | WolPage::QuickMatch(_) => self.lobby.as_ref()?.wait.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn message(&self) -> Option<&crate::ui::shell::modal::ShellBox> {
        match &self.page {
            WolPage::Message { message, .. } => Some(message),
            WolPage::Lobby | WolPage::QuickMatch(_) => self.lobby.as_ref()?.message(),
            _ => None,
        }
    }

    /// The custom lobby runner while the visit has one.
    pub(crate) fn lobby(&self) -> Option<&lobby::WolLobby> {
        self.lobby.as_deref()
    }

    pub(crate) fn lobby_mut(&mut self) -> Option<&mut lobby::WolLobby> {
        self.lobby.as_deref_mut()
    }

    /// A wait box owns input (shown or still hidden): the pages under it
    /// take none.
    fn waiting(&self) -> bool {
        match &self.page {
            WolPage::Backdrop { .. } => true,
            WolPage::Lobby | WolPage::QuickMatch(_) => self.wait_box().is_some(),
            _ => false,
        }
    }

    pub(crate) fn login(&self) -> Option<&WolLoginState> {
        match &self.page {
            WolPage::Login(login) => Some(login),
            _ => None,
        }
    }
}

/// `[0xB77E30]`, the `/reply` target of every WOL chat edit.
pub(crate) fn last_pager(state: &AppState) -> Option<&str> {
    state.frontend.wol.as_ref()?.last_pager.as_deref()
}

/// A WOL chat edit sent `line`: a locate (`0x0078A470`) keeps its name in
/// `[0xB76E28]` for the find sink.
pub(crate) fn chat_sent(state: &mut AppState, line: &crate::ui::wol_chat::WolChatLine) {
    if let crate::ui::wol_chat::WolChatLine::Locate { name } = line
        && let Some(visit) = state.frontend.wol.as_mut()
    {
        visit.locate_name = Some(String::from_utf16_lossy(name));
    }
}

/// The online visit is an external launch's relay match: its shell takes no
/// input, and its end reports the result instead of re-entering the lobby.
pub(crate) fn spawned_visit(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.spawned.is_some())
}

/// `[0xB77DC1]`: a WOL match runs.
pub(crate) fn in_match(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.in_game)
}

/// VERA: whether the online lobby lets players watch matches, as its list of
/// them last answered (docs/lobby-protocol.md "Watching"); `None` before the
/// first answer.
pub(crate) fn lobby_watches(state: &AppState) -> Option<bool> {
    state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.lobby_watches)
}

/// VERA: whether the online lobby takes Multiplayer Campaign rooms.
pub(crate) fn lobby_campaign_rooms(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.lobby_campaign_rooms)
}

/// VERA: whether the online lobby lets a room's players visit the public
/// lobby, as its sign-in answered; a lobby from before it does not.
pub(crate) fn lobby_visits(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.lobby_visits)
}

/// A WOL box (`0x005E2CD0` style 0) with a CSF body.
pub(crate) fn message_box(state: &AppState, key: &str) -> crate::ui::shell::modal::ShellBox {
    service::message_box(state, state.csf_text(key), false)
}

/// Start Game Now's abort (`0x00794D13..0x00794E64`): `0x130` closes,
/// `[0xB77DC1]` is set and the game channel is left (API+0x24 at
/// `0x00794E00`); event 6 then ends the runner (`0x00787F49..0x00787F95`,
/// one more leave), every dialog closes and `WOL_Main` runs mode 0, whose
/// category change logs out before the welcome.
/// RESIDUAL: the Buddy List runner's handling of event 6 is not traced; it
/// ends the same way here.
pub(crate) fn start_aborted(state: &mut AppState) {
    crate::app::shell_network_setup::abandon_for_wol_runner(state);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.in_game = true;
    }
    lobby::start_aborted(state);
}

/// `0x00785EA0`: `[0xB77DC0]` set (not connected), the WOLAPI logout
/// (API+0x34), `0x11E` and the wait box close. The caller clears the
/// login category `[0xB73678]`.
/// RESIDUAL: the logout's wait (`TXT_WOL_LOGGING_OUT` behind the hidden
/// `0xC4`, up to five events for event 0 or 9) is not modelled; it shows
/// only after 7 s.
fn logout(visit: &mut WolVisit) {
    visit.connected = false;
    visit.requests.push(service::WolServiceRequest::Logout);
    visit.page_received = None;
}

/// VERA: show `text` in the chat this online visit next lands in.
pub(crate) fn hold_problem_line(state: &mut AppState, text: String) {
    if let Some(visit) = state.frontend.wol.as_mut()
        && visit.provider == WolProvider::Online
    {
        visit.problem_line = Some(text);
    }
}

/// VERA: add `text` to the line waiting for the chat this online visit next
/// lands in, or let it wait alone.
pub(crate) fn add_problem_line(state: &mut AppState, text: String) {
    if let Some(visit) = state.frontend.wol.as_mut()
        && visit.provider == WolProvider::Online
    {
        visit.problem_line = Some(match visit.problem_line.take() {
            Some(held) => format!("{held} {text}"),
            None => text,
        });
    }
}

/// The runner handed over to a WOL match (`[0xB77DC1]` set).
pub(crate) fn match_started(state: &mut AppState) {
    quick_match::match_starts(state);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.in_game = true;
    }
}

/// The runner of the visit's mode after a successful login or setup Back
/// (`postmatch` false) or after a match (`postmatch` true, the state 0x11
/// flag `WOL_Main` passes on): the custom lobby runner `0x00787770`, or for
/// mode 5 the Buddy List runner `0x0079F140`, for mode 1 the Quick Match
/// runner `0x0079FC40`, or after its match `0x007A2970` (`0x0077B439`).
/// Setup Back under the Buddy List runner is its stop path back to `0x116`.
pub(crate) fn open_lobby(state: &mut AppState, postmatch: bool) {
    let Some(visit) = state.frontend.wol.as_ref() else {
        return;
    };
    if matches!(visit.mode, Some(WolMode::QuickMatch | WolMode::QuickCoop)) {
        if postmatch {
            quick_match::postmatch(state);
        } else {
            quick_match::run(state);
        }
        return;
    }
    let buddy_mode = visit.mode == Some(WolMode::PlayBuddy);
    let buddy_runner = visit
        .lobby()
        .is_some_and(|lobby| lobby.runner == lobby::Runner::Buddy);
    match (buddy_mode, postmatch, buddy_runner) {
        (true, false, true) => buddy::stopped(state),
        (true, _, _) => lobby::open_runner(state, lobby::Runner::Buddy),
        (false, _, _) => lobby::open(state, postmatch),
    }
}

/// `WOL_Main` with return mode 4 after a runner returned 0
/// (`0x0077B491..0x0077B4A6`): the login is kept, the mode-loop pass applies
/// and the custom lobby runner opens.
pub(super) fn reopen_lobby(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.mode = Some(WolMode::CustomMatch);
    }
    apply_wol_preferences(state);
    lobby::open(state, false);
}

/// PrepareSession state 0x11 runs `WOL_Main` again. Its game-start exit
/// (`0x0077B5B5`) kept the WOLAPI object and left the shuffle flag forced
/// on, so this entry saves the flag as it is now. The settings re-read,
/// the entry apply (`0x0077B31D`, which now has the WOLAPI object), the
/// mode-loop apply (`0x0077B402`) and the lobby runner with postmatch set
/// (`0x0077B44B`) follow.
pub(crate) fn reenter_after_match(state: &mut AppState) {
    if let Some(visit) = state.frontend.wol.as_mut()
        && matches!(
            visit.mode,
            Some(WolMode::QuickMatch | WolMode::QuickCoop | WolMode::PlayBuddy)
        )
    {
        // VERA's kept game room returns only to the custom lobby runner.
        state.match_state.session.take_network_room_return();
    }
    live_service::match_returned(state);
    if !App::prepare_multiplayer_settings(
        state,
        crate::app::match_runtime::session::SessionMode::Wol,
    ) {
        return;
    }
    persona::enter_wol_main(state);
    read_wol_settings(state);
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    let lobby_music = state.frontend.wol_preferences.lobby_music != 0;
    let saved_shuffle = state.audio.enter_wol_lobby_music(lobby_music, now_ms);
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.saved_shuffle = saved_shuffle;
    }
    send_wol_preferences(state);
    apply_wol_preferences(state);
    open_lobby(state, true);
}

impl App {
    /// State 2 (`0x0052DD57`) then `WOL_Main`: the lobby music takes over
    /// (`0x0077B2D7..0x0077B31D`) and `0x10E` is created after the empty
    /// backdrop (`0x00798DE0`); the normal online visit signs in instead.
    pub(super) fn open_wol_welcome_page(state: &mut AppState) {
        crate::app::shell_main_menu::clear_typed_codes_for_network_session(state);
        Self::destroy_current_shell_dialog(state);
        // The message boxes paint from the skirmish chrome atlas.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        read_wol_settings(state);
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        let lobby_music = state.frontend.wol_preferences.lobby_music != 0;
        let saved_shuffle = state.audio.enter_wol_lobby_music(lobby_music, now_ms);
        let mut visit = WolVisit::open(saved_shuffle);
        visit.provider = state
            .frontend
            .wol_capture_provider
            .unwrap_or(WolProvider::Online);
        visit.set_legacy_ui(state.frontend.native_shell_ui);
        state.frontend.wol = Some(visit);
        // Persona I/O is selected by the new visit's provider, never a
        // previous visit (or the absence of one).
        persona::enter_wol_main(state);
        // The first mode-loop pass (0x0077B402) applies again.
        apply_wol_preferences(state);
        state.frontend.shell_route = ShellRoute::Wol;
        Self::open_wol_home(state);
    }

    /// Mode 0's page: the welcome `0x10E` (`0x00798DE0`), or for the normal
    /// online visit, which has no welcome, its Custom Match sign-in. The
    /// flow starts over the empty backdrop, as after the welcome's button.
    fn open_wol_home(state: &mut AppState) {
        let Some(visit) = state.frontend.wol.as_mut() else {
            return;
        };
        if visit.modern_ui() {
            Self::commit_wol_action(state, WolMode::CustomMatch);
            return;
        }
        visit.page = WolPage::Welcome;
        leaf::open_art(state, WOL_WELCOME_DIALOG);
        state
            .frontend
            .shell_controller
            .reset_to(WOL_WELCOME_DIALOG);
        let filled = persona::welcome_texts(state);
        Self::wol_welcome_sync(state, &filled);
    }

    /// Builds `0x10E`'s widgets once (`filled`: the persona texts its init
    /// writes, `0x007989AA`) and puts them on the layout; the root.
    pub(crate) fn wol_welcome_sync(
        state: &mut AppState,
        filled: &[(u16, String)],
    ) -> Option<vera20k_gui::NodeId> {
        let layout = crate::app::frontend::shell_layout::wol_welcome_layout(state);
        let spec = || crate::ui::wol_shell::spec(filled);
        leaf::sync(state, WOL_WELCOME_DIALOG, layout.page.scale, spec, |id| {
            layout.rect_of(id)
        })
    }

    /// `0x10E`'s persona texts as its statics show them, by control id.
    pub(crate) fn wol_welcome_texts(state: &AppState) -> Vec<(u16, String)> {
        let ids = [0x06EF, 0x06F4, 0x06F5, 0x06F0, 0x06F1, 0x0701, 0x0797];
        ids.map(|id| (id, leaf::text(state, WOL_WELCOME_DIALOG, id)))
            .into()
    }

    /// The control the top dialog's pointer holds (`SetCapture`), by id.
    pub(crate) fn wol_held(state: &AppState) -> Option<u16> {
        let controller = &state.frontend.shell_controller;
        let node = controller.pressed_node_in(controller.top_id()?)?;
        controller
            .gui()
            .id_of(node)
            .and_then(|id| u16::try_from(id).ok())
    }

    /// The pointer on the box over the page, else on the welcome. Owner-draw
    /// buttons play GUIMainButtonSound on the press (`0x00613667..0x00613771`).
    fn wol_box_or_welcome_pointer(state: &mut AppState, at: leaf::Pointer) {
        let open = state.frontend.wol.as_ref().and_then(|visit| match visit.message() {
            Some(message) => Some(Some(message.clone())),
            // A hidden wait box takes no pointer.
            None if visit.waiting() => Some(
                visit.wait_box().filter(|wait| wait.shown).map(|wait| wait.shell_box.clone()),
            ),
            None => None,
        });
        if let Some(open) = open {
            let top = state.frontend.shell_controller.top_id();
            if let Some(shell_box) = open.filter(|open| top == Some(open.dialog)) {
                let parts = shell_layout::shell_box_parts(state, &shell_box);
                let down = match at {
                    leaf::Pointer::Move => None,
                    leaf::Pointer::Down => Some(true),
                    leaf::Pointer::Up => Some(false),
                };
                if let Some(answer) = Self::shell_box_pointer(state, &shell_box, &parts, down)
                    .and_then(|id| shell_box.answer(id))
                {
                    service::box_command(state, answer);
                }
            }
            return;
        }
        if Self::wol_welcome_sync(state, &[]).is_none() {
            return;
        }
        // The dialog's hit test writes the status help on a move
        // (`0x00622CCB`); a pressed button holds the mouse, so none arrives
        // until the next move after its release.
        let held = state
            .frontend
            .shell_controller
            .pressed_node_in(WOL_WELCOME_DIALOG);
        let events = match at {
            leaf::Pointer::Move if held.is_some() => {
                state.frontend.shell_status_line.repaint();
                Vec::new()
            }
            leaf::Pointer::Move => leaf::pointer(state, at),
            _ => leaf::pointer_quiet(state, at),
        };
        for event in events {
            let PageEvent::Activated(id) = event else {
                continue;
            };
            match action_for_control(id) {
                Some(WolWelcomeAction::MainMenu) => Self::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::WolWelcome,
                    App::return_from_wol,
                ),
                Some(WolWelcomeAction::Mode(mode)) => Self::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::WolWelcome,
                    move |state| App::commit_wol_action(state, mode),
                ),
                // No `Community` URL in the registry: nothing happens.
                Some(WolWelcomeAction::Community) | None => {}
            }
        }
    }

    pub(super) fn wol_page_is_lobby(state: &AppState) -> bool {
        matches!(
            state.frontend.wol.as_ref().map(|visit| &visit.page),
            Some(WolPage::Lobby)
        )
    }

    /// A box or the wait box (shown or still hidden) owns the input.
    pub(super) fn wol_box_owns_input(state: &AppState) -> bool {
        let visit = state.frontend.wol.as_ref();
        visit.is_some_and(|visit| visit.message().is_some() || visit.waiting())
    }

    /// Start Game Now's `0x130` over Quick Match's backdrop takes the input.
    fn wol_quick_match_verify(state: &AppState) -> bool {
        crate::app::shell_network_setup::quick_match(state)
            && crate::app::shell_network_setup::verify(state).is_some()
    }

    pub(super) fn handle_wol_mouse_move(state: &mut AppState) {
        Self::wol_pointer(state, leaf::Pointer::Move);
    }

    pub(super) fn handle_wol_mouse_down(state: &mut AppState) {
        Self::wol_pointer(state, leaf::Pointer::Down);
    }

    pub(super) fn handle_wol_mouse_up(state: &mut AppState) {
        Self::wol_pointer(state, leaf::Pointer::Up);
    }

    /// The pointer on `0x130`, a page dialog, the page under it, or else on
    /// a box or the welcome. Quick Match and the lobby take none under a box.
    fn wol_pointer(state: &mut AppState, at: leaf::Pointer) {
        if Self::wol_quick_match_verify(state) {
            let down = match at {
                leaf::Pointer::Move => None,
                at => Some(at == leaf::Pointer::Down),
            };
            return crate::app::shell_network_setup::verify_pointer(state, down);
        }
        type Handlers = (fn(&mut AppState), fn(&mut AppState, bool));
        let boxed = Self::wol_box_owns_input(state);
        let shown = state.frontend.wol.as_ref().map(|visit| &visit.page);
        let handlers: Option<Handlers> = match shown {
            _ if page::open(state) => Some((page::cursor_moved, page::mouse)),
            Some(WolPage::Options(_)) => Some((options::cursor_moved, options::mouse)),
            Some(WolPage::AutoLoginQuery | WolPage::Locale(_) | WolPage::AddBuddyQuery(_)) => {
                Some((persona::cursor_moved, persona::mouse))
            }
            Some(WolPage::MyInformation(_)) => Some((my_info::cursor_moved, my_info::mouse)),
            Some(WolPage::Buddy(_)) => Some((buddy::cursor_moved, buddy::mouse)),
            Some(WolPage::QuickMatch(_)) if !boxed => {
                Some((quick_match::cursor_moved, quick_match::mouse))
            }
            Some(WolPage::Lobby) if !boxed => Some((lobby::cursor_moved, lobby::mouse)),
            Some(WolPage::Login(_)) => Some((login::cursor_moved, login::mouse)),
            _ => None,
        };
        match (handlers, at) {
            (Some((moved, _)), leaf::Pointer::Move) => moved(state),
            (Some((_, mouse)), at) => mouse(state, at == leaf::Pointer::Down),
            (None, at) => Self::wol_box_or_welcome_pointer(state, at),
        }
    }

    /// VERA's Esc menu (`app::shell_menu`): the boxes, popups and player card
    /// over a page keep Esc.
    pub(super) fn wol_leaves_escape(state: &AppState) -> bool {
        if Self::wol_quick_match_verify(state) || page::open(state) {
            return false;
        }
        let Some(visit) = state.frontend.wol.as_ref() else {
            return false;
        };
        !visit.waiting()
            && match &visit.page {
                WolPage::Login(login) => !login.choosing_entry() && login.account.is_none(),
                WolPage::Lobby => visit.lobby().is_some_and(|lobby| {
                    lobby.interactive() && lobby.ui.popup.is_none() && lobby.ui.card.is_none()
                }),
                WolPage::Buddy(dialog) => dialog.ui.add.is_none(),
                _ => true,
            }
    }

    /// Keys while a WOL page shows. `0x10E` ignores IDOK/IDCANCEL
    /// (`0x007988C0`); the boxes' loop (`0x007759E0`) and the hidden or shown
    /// `0xC4` take none. `0x10F`'s and `0x113`'s NewEdits read keys.
    pub(super) fn handle_wol_key(
        state: &mut AppState,
        code: Option<winit::keyboard::KeyCode>,
        pressed: bool,
        text: Option<&str>,
    ) {
        // 0x130 has no key handling.
        if !pressed || Self::wol_quick_match_verify(state) {
            return;
        }
        type KeyPress = fn(&mut AppState, Option<winit::keyboard::KeyCode>, Option<&str>);
        let shown = state.frontend.wol.as_ref().map(|visit| &visit.page);
        let handler: Option<KeyPress> = match shown {
            _ if page::open(state) => Some(page::key_press),
            Some(WolPage::Login(_)) => Some(login::key_press),
            Some(WolPage::Lobby) => Some(lobby::key_press),
            Some(WolPage::Buddy(_)) => Some(buddy::key_press),
            Some(WolPage::Options(_)) => Some(options::key_press),
            _ => None,
        };
        if let Some(key_press) = handler {
            key_press(state, code, text);
        }
    }

    /// `0x10E`'s teardown has run for a WOL action: the mode is written
    /// (`0x007988C0`) and the flow continues over the empty backdrop.
    pub(super) fn commit_wol_action(state: &mut AppState, mode: WolMode) {
        let Some(visit) = state.frontend.wol.as_mut() else {
            return;
        };
        visit.mode = Some(mode);
        // 0x00798DB5: the welcome's Buddy List returns to mode 0.
        visit.return_mode = None;
        visit.page = WolPage::Backdrop { wait: None };
        // Mode 6 needs no login (0x0077B3AA): its driver 0x00799D30 only
        // ensures the WOLAPI object and waits for the server list.
        if mode == WolMode::MyInformation {
            service::begin_my_information(state);
        } else {
            service::begin_flow(state);
        }
    }

    /// The login flow returned 0 (Back, or a failed connect after its box):
    /// mode 0 (`0x0077B3F6`), and the loop runs a fresh `0x10E` with its
    /// entry slide, or the online sign-in again.
    pub(super) fn reopen_wol_welcome(state: &mut AppState) {
        let Some(visit) = state.frontend.wol.as_mut() else {
            return;
        };
        visit.mode = None;
        visit.return_mode = None;
        visit.relogin = None;
        visit.flow = None;
        // 0x0077B4D3..0x0077B519: mode 0 needs no login category, so a
        // held one logs out and clears.
        if visit.authenticated {
            logout(visit);
            visit.authenticated = false;
        }
        page::close_stale(state);
        // The mode-loop pass (0x0077B402) before the welcome runs.
        apply_wol_preferences(state);
        leaf::free_art(state, crate::ui::wol_login::DIALOG);
        state.frontend.shell_route = ShellRoute::Wol;
        Self::open_wol_home(state);
    }

    /// Main Menu's teardown has run (result 0), or `WOL_Main` returned -2
    /// or -1: the WOLAPI object is released (`0x00786080`), the shuffle flag
    /// restored, and PrepareSession plays INTRO and builds a new `0xE2`
    /// (`0x0052E28E`).
    pub(super) fn return_from_wol(state: &mut AppState) {
        // The UI visit ends here. Its same HTTPS cleanup owner moves into the
        // process frontend until logout acknowledgement/deadline + worker stop;
        // old callbacks never enter a later WOL visit.
        live_service::leave_visit(state);
        state
            .frontend
            .multiplayer_shell_runtime
            .release_online_handle();
        if let Some(visit) = state.frontend.wol.take() {
            state.audio.leave_wol_lobby_music(visit.saved_shuffle);
        }
        page::close_stale(state);
        state.frontend.wol_art.clear();
        state.frontend.shell_route = ShellRoute::MainMenu;
        Self::destroy_current_shell_dialog(state);
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::shell::descriptor::DialogResourceId(0x00E2));
    }

    pub(super) fn commit_wol_login_close(state: &mut AppState, result: u16) {
        login::commit_close(state, result);
    }
}
