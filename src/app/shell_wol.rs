//! Main Menu -> Internet: one Westwood Online visit, `WOL_Main`
//! (`0x0077B2A0`) from the welcome `0x10E` until WOL is left. Custom Match
//! runs the login flow `WOL__RunLoginFlow` (`0x00787010`): the WOLAPI
//! object, the server list behind the wait box `0xC4`, the login `0x10F`,
//! the connect and then the custom lobby runner (`0x00787770`, [`open_lobby`]);
//! Quick Match runs the same login and then its own runner
//! (`0x0079FC40`, [`quick_match`]).
//!
//! The WOLAPI COM object, its servers and its credential store are outside
//! the shell: they meet it only through [`WolServiceRequest`] /
//! [`WolServiceEvent`]. Production has no WOLAPI ([`WolProvider::Unavailable`]):
//! every WOL action shows `TXT_APIMISSING` in `0xD0` and returns to a new
//! `0xE2` (`0x0052E28E`), as on a retail install without the online
//! components, unless the Options page's developer box picks VERA's offline
//! stand-in ([`WolProvider::TestService`]).

pub(crate) mod buddies;
pub(crate) mod buddy;
pub(crate) mod lobby;
mod login;
pub(crate) mod my_info;
pub(crate) mod options;
pub(crate) mod page;
pub(crate) mod persona;
pub(crate) mod quick_match;
mod service;
mod test_service;

pub(crate) use login::{combo_popup, login_layout, open_login_art};
pub(crate) use service::{
    NET_STATUS_LOST, WolApiStatus, WolHighScoreRecord, WolLadderRecord, WolLoginAccepted,
    WolProvider, WolServer, WolServerList, WolServiceEvent, WolServiceRequest, service_event,
};
pub(crate) use test_service::{
    WolTestGuest, default_buddies as test_service_buddies,
    matchbot_options as test_service_matchbot_options,
};

/// The wait box's reveal timer and the login drop list's scroll repeats;
/// the caller schedules the earliest deadline with the other shell timers.
pub(crate) fn poll(state: &mut AppState) -> Option<std::time::Instant> {
    [
        service::poll_wait(state),
        login::poll_scroll_repeat(state),
        persona::poll_scroll_repeat(state),
        my_info::poll_scroll_repeat(state),
        buddy::poll(state),
        page::poll_scroll_repeat(state),
        lobby::poll(state),
        quick_match::poll(state),
        test_service::poll(state),
    ]
    .into_iter()
    .flatten()
    .min()
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
use crate::app::frontend::shell_transition::ShellExitThen;
use crate::app::shell_route::ShellRoute;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::layout::LaidOutControl;
use crate::ui::wol_login::{WolCredential, WolLoginState};
use crate::ui::wol_shell::{
    BOX_NO, BOX_OK, BOX_YES, WAIT_CHECK_STATUS, WAIT_DISCONNECT, WOL_WELCOME_DIALOG, WolBoxButtons,
    WolMessageBox, WolMode, WolWaitBox, WolWelcomeAction, WolWelcomeLayout, action_for_control,
};

/// The custom lobby `0x113`'s controller identity.
pub(crate) const WOL_LOBBY_DIALOG: DialogId = DialogId(0x0113);

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
        message: WolMessageBox,
        then: WolBoxThen,
    },
    /// The custom lobby `0x113` (its runner belongs to the lobby chain).
    Lobby,
    /// WOL Options `0x2BC` (and its Sound Options `0xD6`) over the hidden
    /// lobby.
    Options(Box<options::WolOptionsPage>),
    /// The auto-login query `0x112` after a dialog login.
    AutoLoginQuery(Box<crate::ui::wol_persona::WolAutoLoginQuery>),
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
    return_mode: Option<WolMode>,
    /// Login category `[0xB73678]`: set once the login flow returned 1.
    authenticated: bool,
    /// `[0xAC116C]`: the WOLAPI object exists. Created by `0x00786390`
    /// and released only when the visit ends (`0x0077B532` -> `0x00786080`).
    api: bool,
    pub(crate) page: WolPage,
    /// The status help the welcome's last hit test wrote.
    pub(crate) status_help: Option<&'static str>,
    /// Server records of the last list (`0xB778D8` count, `0xF8` records)
    /// and the selected record `[0xB76468]` (-1 for none).
    servers: Vec<WolServer>,
    selected_server: i32,
    awaiting: service::Awaiting,
    /// The login flow between its connect and its return.
    flow: Option<service::LoginFlow>,
    /// `0x10E`'s persona heading and stats (`0x007989AA`), by control id;
    /// empty without a nickname.
    pub(crate) welcome_texts: Vec<(u16, String)>,
    provider: WolProvider,
    requests: Vec<WolServiceRequest>,
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
            status_help: None,
            servers: Vec::new(),
            selected_server: 0,
            awaiting: service::Awaiting::Nothing,
            flow: None,
            welcome_texts: Vec::new(),
            provider: WolProvider::Unavailable,
            requests: Vec::new(),
            credentials: Vec::new(),
            last_error: None,
            last_login_result: None,
            motd: None,
            last_pager: None,
            in_game: false,
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
        }
    }

    /// Diagnostics and tests only: a controlled service will drain the
    /// requests and inject events. Production keeps `Unavailable` unless
    /// the Options page chose the test service.
    pub(crate) fn set_provider(&mut self, provider: WolProvider) {
        self.provider = provider;
    }

    pub(crate) fn take_service_requests(&mut self) -> Vec<WolServiceRequest> {
        std::mem::take(&mut self.requests)
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

    pub(crate) fn message(&self) -> Option<&WolMessageBox> {
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

/// `[0xB77DC1]`: a WOL match runs.
pub(crate) fn in_match(state: &AppState) -> bool {
    state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.in_game)
}

/// A WOL box (`0x005E2CD0` style 0) with a CSF body.
pub(crate) fn message_box(state: &AppState, key: &str) -> crate::ui::wol_shell::WolMessageBox {
    service::message_box(state, key, false)
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
        (true, _, _) => buddy::open(state),
        (false, _, _) => lobby::open(state, postmatch),
    }
}

/// PrepareSession state 0x11 runs `WOL_Main` again. Its game-start exit
/// (`0x0077B5B5`) kept the WOLAPI object and left the shuffle flag forced
/// on, so this entry saves the flag as it is now. The settings re-read,
/// the entry apply (`0x0077B31D`, which now has the WOLAPI object), the
/// mode-loop apply (`0x0077B402`) and the lobby runner with postmatch set
/// (`0x0077B44B`) follow.
pub(crate) fn reenter_after_match(state: &mut AppState) {
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
    /// backdrop (`0x00798DE0`).
    pub(super) fn open_wol_welcome_page(state: &mut AppState) {
        crate::app::shell_main_menu::clear_typed_codes_for_network_session(state);
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        // The message boxes paint from the skirmish chrome atlas.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        Self::ensure_wol_welcome_art(state);
        persona::enter_wol_main(state);
        read_wol_settings(state);
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        let lobby_music = state.frontend.wol_preferences.lobby_music != 0;
        let saved_shuffle = state.audio.enter_wol_lobby_music(lobby_music, now_ms);
        let mut visit = WolVisit::open(saved_shuffle);
        if state.persistence.options_profile.wol_test_service {
            visit.provider = WolProvider::TestService;
        }
        state.frontend.wol = Some(visit);
        persona::fill_welcome(state);
        // The first mode-loop pass (0x0077B402) applies again.
        apply_wol_preferences(state);
        state.frontend.shell_route = ShellRoute::Wol;
        state
            .frontend
            .shell_controller
            .reset_to(WOL_WELCOME_DIALOG, false);
    }

    /// `0x0072C7E0` loads the page's art (the background only at 800 wide).
    pub(crate) fn ensure_wol_welcome_art(state: &mut AppState) -> bool {
        if state.frontend.wol_welcome_art.is_some() {
            return true;
        }
        let Some(assets) = state.process_assets.manager() else {
            return false;
        };
        let icons: Vec<&str> = crate::ui::wol_shell::WOL_ICONS
            .iter()
            .map(|icon| icon.file)
            .collect();
        state.frontend.wol_welcome_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            assets,
            crate::app::frontend::shell_layout::art_key_screen_width(state),
            ("MultiplaySelection.shp", "MultiplaySelection.pal"),
            &icons,
        );
        state.frontend.wol_welcome_art.is_some()
    }

    /// Any WOL page owns the shell's input and paint.
    pub(super) fn wol_active(state: &AppState) -> bool {
        state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
            && state.frontend.shell_route.wol_visit()
            && state.frontend.wol.is_some()
    }

    pub(crate) fn wol_welcome_layout(state: &AppState) -> WolWelcomeLayout {
        crate::app::frontend::shell_layout::wol_welcome_layout(state)
    }

    /// The page's buttons in hit-test (Z) order.
    fn wol_button_feed(layout: &WolWelcomeLayout) -> Vec<LaidOutControl> {
        layout
            .buttons_in_z_order()
            .into_iter()
            .map(|button| LaidOutControl {
                id: button.id,
                rect: button.rect,
            })
            .collect()
    }

    /// The buttons of the box that owns input, if one does.
    fn wol_box_feed(state: &AppState) -> Option<Vec<LaidOutControl>> {
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let visit = state.frontend.wol.as_ref()?;
        if let Some(message) = visit.message() {
            return Some(match message.buttons {
                WolBoxButtons::Ok(_) => vec![LaidOutControl {
                    id: BOX_OK,
                    rect: crate::ui::shell::modal::body_ok_layout(width, height).ok,
                }],
                WolBoxButtons::YesNo { .. } => {
                    let layout = crate::ui::shell::modal::yes_no_layout(width, height);
                    vec![
                        LaidOutControl {
                            id: BOX_NO,
                            rect: layout.no,
                        },
                        LaidOutControl {
                            id: BOX_YES,
                            rect: layout.yes,
                        },
                    ]
                }
            });
        }
        if visit.waiting() {
            // A hidden wait box takes no pointer.
            return Some(match visit.wait_box().filter(|wait| wait.shown) {
                Some(_) => {
                    let layout = crate::ui::shell::modal::wait_box_layout(width, height);
                    vec![
                        LaidOutControl {
                            id: WAIT_DISCONNECT,
                            rect: layout.disconnect,
                        },
                        LaidOutControl {
                            id: WAIT_CHECK_STATUS,
                            rect: layout.check_status,
                        },
                    ]
                }
                None => Vec::new(),
            });
        }
        None
    }

    fn wol_page_is_login(state: &AppState) -> bool {
        state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| matches!(visit.page, WolPage::Login(_)))
    }

    fn wol_page_is_options(state: &AppState) -> bool {
        state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| matches!(visit.page, WolPage::Options(_)))
    }

    pub(super) fn wol_page_is_lobby(state: &AppState) -> bool {
        state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| matches!(visit.page, WolPage::Lobby))
    }

    /// Start Game Now's `0x130` over Quick Match's backdrop takes the input.
    fn wol_quick_match_verify(state: &AppState) -> bool {
        crate::app::shell_network_setup::quick_match(state)
            && crate::app::shell_network_setup::verify(state).is_some()
    }

    pub(super) fn handle_wol_mouse_move(state: &mut AppState) {
        if Self::wol_quick_match_verify(state) {
            crate::app::shell_network_setup::verify_cursor_moved(state);
            return;
        }
        if page::open(state) {
            page::cursor_moved(state);
            return;
        }
        if Self::wol_page_is_options(state) {
            options::cursor_moved(state);
            return;
        }
        if persona::page_open(state) {
            persona::cursor_moved(state);
            return;
        }
        if my_info::page_open(state) {
            my_info::cursor_moved(state);
            return;
        }
        if buddy::page_open(state) {
            buddy::cursor_moved(state);
            return;
        }
        if quick_match::page_open(state) && Self::wol_box_feed(state).is_none() {
            quick_match::cursor_moved(state);
            return;
        }
        if Self::wol_page_is_lobby(state) && Self::wol_box_feed(state).is_none() {
            lobby::cursor_moved(state);
            return;
        }
        if Self::wol_page_is_login(state) {
            login::cursor_moved(state);
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        if let Some(feed) = Self::wol_box_feed(state) {
            state.frontend.shell_controller.on_pointer_move(x, y, &feed);
            return;
        }
        let layout = Self::wol_welcome_layout(state);
        let feed = Self::wol_button_feed(&layout);
        state
            .frontend
            .shell_controller
            .ensure_active(WOL_WELCOME_DIALOG, false);
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        // The dialog's hit test writes the status help (`0x00622CCB`); a
        // pressed button holds the mouse, so none arrives meanwhile.
        if state.frontend.shell_controller.pressed().is_none()
            && let Some(visit) = state.frontend.wol.as_mut()
        {
            visit.status_help = layout.status_help_key(x, y);
        }
        state.frontend.shell_status_line.repaint();
    }

    pub(super) fn handle_wol_mouse_down(state: &mut AppState) {
        if Self::wol_quick_match_verify(state) {
            crate::app::shell_network_setup::verify_mouse(state, true);
            return;
        }
        if page::open(state) {
            page::mouse(state, true);
            return;
        }
        if Self::wol_page_is_options(state) {
            options::mouse(state, true);
            return;
        }
        if persona::page_open(state) {
            persona::mouse(state, true);
            return;
        }
        if my_info::page_open(state) {
            my_info::mouse(state, true);
            return;
        }
        if buddy::page_open(state) {
            buddy::mouse(state, true);
            return;
        }
        if quick_match::page_open(state) && Self::wol_box_feed(state).is_none() {
            quick_match::mouse(state, true);
            return;
        }
        if Self::wol_page_is_lobby(state) && Self::wol_box_feed(state).is_none() {
            lobby::mouse(state, true);
            return;
        }
        if Self::wol_page_is_login(state) {
            login::mouse(state, true);
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        let feed = match Self::wol_box_feed(state) {
            Some(feed) => feed,
            None => {
                state
                    .frontend
                    .shell_controller
                    .ensure_active(WOL_WELCOME_DIALOG, false);
                Self::wol_button_feed(&Self::wol_welcome_layout(state))
            }
        };
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // Owner-draw buttons play GUIMainButtonSound on the press
        // (`0x00613667..0x00613771`).
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_wol_mouse_up(state: &mut AppState) {
        if Self::wol_quick_match_verify(state) {
            crate::app::shell_network_setup::verify_mouse(state, false);
            return;
        }
        if page::open(state) {
            page::mouse(state, false);
            return;
        }
        if Self::wol_page_is_options(state) {
            options::mouse(state, false);
            return;
        }
        if persona::page_open(state) {
            persona::mouse(state, false);
            return;
        }
        if my_info::page_open(state) {
            my_info::mouse(state, false);
            return;
        }
        if buddy::page_open(state) {
            buddy::mouse(state, false);
            return;
        }
        if quick_match::page_open(state) && Self::wol_box_feed(state).is_none() {
            quick_match::mouse(state, false);
            return;
        }
        if Self::wol_page_is_lobby(state) && Self::wol_box_feed(state).is_none() {
            lobby::mouse(state, false);
            return;
        }
        if Self::wol_page_is_login(state) {
            login::mouse(state, false);
            return;
        }
        let (x, y) = state.match_state.input.cursor_px();
        if let Some(feed) = Self::wol_box_feed(state) {
            if let Some(control) = state.frontend.shell_controller.on_pointer_up(x, y, &feed) {
                service::box_command(state, control);
            }
            return;
        }
        let feed = Self::wol_button_feed(&Self::wol_welcome_layout(state));
        let Some(activated) = state.frontend.shell_controller.on_pointer_up(x, y, &feed) else {
            return;
        };
        match action_for_control(activated) {
            Some(WolWelcomeAction::MainMenu) => {
                Self::leave_shell_dialog(state, ShellExitThen::WolBack)
            }
            Some(WolWelcomeAction::Mode(mode)) => {
                Self::leave_shell_dialog(state, ShellExitThen::WolAction(mode));
            }
            // No `Community` URL in the registry: nothing happens.
            Some(WolWelcomeAction::Community) | None => {}
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
        if Self::wol_quick_match_verify(state) {
            return;
        }
        if pressed && page::open(state) {
            page::key_press(state, code, text);
        } else if pressed && Self::wol_page_is_login(state) {
            login::key_press(state, code, text);
        } else if pressed && Self::wol_page_is_lobby(state) {
            lobby::key_press(state, code, text);
        } else if pressed && buddy::page_open(state) {
            buddy::key_press(state, code, text);
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
        visit.status_help = None;
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
    /// entry slide.
    pub(crate) fn reopen_wol_welcome(state: &mut AppState) {
        let Some(visit) = state.frontend.wol.as_mut() else {
            return;
        };
        visit.mode = None;
        visit.return_mode = None;
        visit.status_help = None;
        visit.relogin = None;
        visit.flow = None;
        // 0x0077B4D3..0x0077B519: mode 0 needs no login category, so a
        // held one logs out and clears.
        if visit.authenticated {
            logout(visit);
            visit.authenticated = false;
        }
        visit.page = WolPage::Welcome;
        persona::fill_welcome(state);
        // The mode-loop pass (0x0077B402) before the welcome runs.
        apply_wol_preferences(state);
        state.frontend.wol_login_art = None;
        state.frontend.shell_route = ShellRoute::Wol;
        Self::ensure_wol_welcome_art(state);
        state
            .frontend
            .shell_controller
            .reset_to(WOL_WELCOME_DIALOG, false);
        state.platform.window.request_redraw();
    }

    /// Main Menu's teardown has run (result 0), or `WOL_Main` returned -2
    /// or -1: the WOLAPI object is released (`0x00786080`), the shuffle flag
    /// restored, and PrepareSession plays INTRO and builds a new `0xE2`
    /// (`0x0052E28E`).
    pub(super) fn return_from_wol(state: &mut AppState) {
        if let Some(visit) = state.frontend.wol.take() {
            state.audio.leave_wol_lobby_music(visit.saved_shuffle);
        }
        state.frontend.wol_welcome_art = None;
        state.frontend.wol_login_art = None;
        state.frontend.wol_lobby_art = None;
        state.frontend.wol_options_art = None;
        state.frontend.wol_sound_options_art = None;
        state.frontend.wol_auto_login_art = None;
        state.frontend.wol_my_info_art = None;
        state.frontend.wol_buddy_art = None;
        state.frontend.wol_quick_match_art = None;
        state.frontend.shell_route = ShellRoute::MainMenu;
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::shell::descriptor::DialogId(0x00E2), false);
    }

    /// Status help of the welcome for its status line.
    pub(crate) fn wol_status_key(state: &AppState) -> Option<&'static str> {
        state
            .frontend
            .wol
            .as_ref()
            .filter(|visit| matches!(visit.page, WolPage::Welcome))
            .and_then(|visit| visit.status_help)
    }

    pub(super) fn commit_wol_login_close(state: &mut AppState, result: u16) {
        login::commit_close(state, result);
    }
}
