//! Production WOL Custom Match checkpoints: Main Menu -> Internet -> Custom
//! Match through the real `0x10E`/`0x10F`/`0xC4`/box handlers, with a
//! controlled WOL service that answers the visit's own requests. Nothing
//! here builds a page; the service events are decoded results, not a
//! transport or backend. The accepted login WRITES the MultiPlayer handle:
//! use an isolated RA2_DIR.

use super::*;
use crate::app::match_runtime::session::SessionMode;
use crate::app::shell_network_setup::{
    self as setup, NetworkSetupEvent, NetworkSetupRequest, WolStartUser,
};
use crate::app::shell_wol::lobby::WolJoined;
use crate::app::shell_wol::{
    WolApiStatus, WolLoginAccepted, WolProvider, WolServer, WolServerList, WolServiceEvent,
    WolServiceRequest, service_event,
};
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_chat::WolChatLine;
use crate::ui::wol_lobby::{
    self as lobby_ui, CHANNEL_KEYED, GAME_CHANNEL, GAME_LIMIT_800, LOBBY_KEY, MEMBER_OPERATOR,
    MEMBER_VOICE, WolChannel, WolMember,
};
use crate::ui::wol_login::{BACK, LOGIN};
use crate::ui::wol_options::WolPreferences;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WolTarget {
    /// `0x10F` settled after a controlled server list, empty fields.
    LoginSteady,
    /// A typed nickname and an eight-character password: Login enabled.
    LoginTyped,
    /// Back on `0x10F`: a fresh `0x10E` settled.
    LoginBack,
    /// The server-list wait box `0xC4` once its 7000 ms timer showed it.
    ServerListWait,
    /// Disconnect on that box: WOL left, a new `0xE2` settled.
    ServerListDisconnect,
    /// A failed server list: WOL left without a box, a new `0xE2` settled.
    ServerListFailure,
    /// Login rejected with `TXT_BADPASS`: the box `0xD0`.
    LoginRejected,
    /// OK on that box: a fresh `0x10E` settled.
    LoginRejectedReturn,
    /// Login accepted: the custom lobby `0x113` settled with its lists.
    LoginAccepted,
    /// The ping sort header pressed: the game list re-sorted.
    LobbySort,
    /// A typed chat line and a received page in the transcript.
    LobbyChat,
    /// A version-mismatched and a full game selected: their refusals.
    JoinAdmission,
    /// Join on a keyed game: the password popup `0xE1`.
    JoinPassword,
    /// Join on an open game: leave, join, the slide, then C9 settled.
    JoinAccepted,
    /// Create Game: the popup `0xC3`.
    CreatePopup,
    /// `0xC3` OK: leave, create, the slide, then C2 settled.
    CreateAccepted,
    /// C2 Back: the chat lobby rejoined and `0x113` settled again.
    SetupBack,
    /// C2 with a joined guest: its row, ping icon and the join's requests.
    SetupGuestJoined,
    /// C2 Start with an unready guest: RequestAcceptance and the host line.
    SetupStartNotReady,
    /// C2 chat: a page echo and an emote.
    SetupChat,
    /// C2 Start with a ready guest (an option edit cleared it once):
    /// StartGame, OnGameStart, the slide, then the first loading frame.
    SetupStart,
    /// C9 Accept: Ready(true) and Accept disabled.
    GuestAccept,
    /// C9 Back: Leave, then the channel-left event rejoins the chat lobby.
    GuestBack,
    /// A started WOL match left through FF/B6: the chat lobby rejoined.
    AbortReturn,
    /// A started WOL match's FF menu after a physical speed-trackbar jump.
    InternetMenu,
    /// FF's Game Controls press opens BBB over the running WOL match.
    GameControls,
    /// `0x113` Back: the channel left and a fresh `0x10E` settled.
    LobbyBack,
    /// The Options page's offline test service: login to a settled 0x113.
    TestServiceLobby,
    /// The same, then Create Game: C2 settled.
    TestServiceCreate,
    /// The same, then Join on the first game: C9 settled.
    TestServiceJoin,
    /// `0x113` Options: WOL Options `0x2BC` settled, help under Pages.
    OptionsSteady,
    /// `0x2BC` Play Music unchecked: the preview queues -3, nothing saved.
    OptionsMusicOff,
    /// Pages and Buddies toggled, then Cancel: the lobby again, applied
    /// unchanged, RA2MD.INI untouched.
    OptionsCancel,
    /// Pages unchecked and Buddies checked, then OK: `[WOnline]` written,
    /// applied, and a reopened `0x2BC` shows them.
    OptionsOk,
    /// Sound Options: `0xD6` settled after a music rail press.
    SoundOptions,
    /// `0xD6` Back: `0x2BC` settled again.
    SoundOptionsBack,
}

impl WolTarget {
    /// The login is accepted and the custom lobby runs.
    fn lobby(self) -> bool {
        !matches!(
            self,
            Self::LoginSteady
                | Self::LoginTyped
                | Self::LoginBack
                | Self::ServerListWait
                | Self::ServerListDisconnect
                | Self::ServerListFailure
                | Self::LoginRejected
                | Self::LoginRejectedReturn
        )
    }

    /// The production test service answers instead of the capture.
    fn test_service(self) -> bool {
        matches!(
            self,
            Self::TestServiceLobby | Self::TestServiceCreate | Self::TestServiceJoin
        )
    }

    /// The chat lobby's Join leads to C9 instead of Create to C2.
    fn guest(self) -> bool {
        matches!(
            self,
            Self::JoinAccepted | Self::GuestAccept | Self::GuestBack
        )
    }

    /// C2 with the controlled guest joined.
    fn guest_joined(self) -> bool {
        matches!(
            self,
            Self::SetupGuestJoined
                | Self::SetupStartNotReady
                | Self::SetupChat
                | Self::SetupStart
                | Self::AbortReturn
                | Self::InternetMenu
                | Self::GameControls
        )
    }

    /// The lobby's Options page runs.
    fn options(self) -> bool {
        matches!(
            self,
            Self::OptionsSteady
                | Self::OptionsMusicOff
                | Self::OptionsCancel
                | Self::OptionsOk
                | Self::SoundOptions
                | Self::SoundOptionsBack
        )
    }

    /// The started match runs past loading.
    fn in_match(self) -> bool {
        matches!(
            self,
            Self::AbortReturn | Self::InternetMenu | Self::GameControls
        )
    }
}

pub(crate) const WOL_CHECKPOINTS: &[(&str, WolTarget)] = &[
    ("wol-0x10f-steady", WolTarget::LoginSteady),
    ("wol-0x10f-typed", WolTarget::LoginTyped),
    ("wol-0x10f-back", WolTarget::LoginBack),
    ("wol-0xc4-server-list-wait", WolTarget::ServerListWait),
    ("wol-0xc4-disconnect", WolTarget::ServerListDisconnect),
    ("wol-server-list-failure", WolTarget::ServerListFailure),
    ("wol-0xd0-login-rejected", WolTarget::LoginRejected),
    ("wol-login-rejected-return", WolTarget::LoginRejectedReturn),
    ("wol-0x113-login-accepted", WolTarget::LoginAccepted),
    ("wol-0x113-sort", WolTarget::LobbySort),
    ("wol-0x113-chat", WolTarget::LobbyChat),
    ("wol-0x113-join-admission", WolTarget::JoinAdmission),
    ("wol-0xe1-password", WolTarget::JoinPassword),
    ("wol-c9-join-accepted", WolTarget::JoinAccepted),
    ("wol-0xc3-create", WolTarget::CreatePopup),
    ("wol-c2-create-accepted", WolTarget::CreateAccepted),
    ("wol-c2-back", WolTarget::SetupBack),
    ("wol-0x113-back", WolTarget::LobbyBack),
    ("wol-c2-guest-joined", WolTarget::SetupGuestJoined),
    ("wol-c2-start-not-ready", WolTarget::SetupStartNotReady),
    ("wol-c2-chat", WolTarget::SetupChat),
    ("wol-c2-start-loading", WolTarget::SetupStart),
    ("wol-c9-accept", WolTarget::GuestAccept),
    ("wol-c9-back", WolTarget::GuestBack),
    ("wol-abort-return", WolTarget::AbortReturn),
    ("wol-0xff-menu", WolTarget::InternetMenu),
    ("wol-game-controls", WolTarget::GameControls),
    ("wol-test-service-lobby", WolTarget::TestServiceLobby),
    ("wol-test-service-create", WolTarget::TestServiceCreate),
    ("wol-test-service-join", WolTarget::TestServiceJoin),
    ("wol-0x2bc-steady", WolTarget::OptionsSteady),
    ("wol-0x2bc-music-off", WolTarget::OptionsMusicOff),
    ("wol-0x2bc-cancel", WolTarget::OptionsCancel),
    ("wol-0x2bc-ok", WolTarget::OptionsOk),
    ("wol-0xd6-sound-options", WolTarget::SoundOptions),
    ("wol-0xd6-back", WolTarget::SoundOptionsBack),
];

/// The controlled guest of the C2 targets, the host of the C9 targets.
const PEER: &str = "Alpha";

const NICKNAME: &str = "Tester";
const PASSWORD: &str = "secret12";
const BAD_PASSWORD: u32 = 0x8004_0065;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WolPhase {
    Main,
    Welcome,
    /// Custom Match pressed: its teardown, then the API request.
    Custom,
    /// The server list is outstanding.
    Servers,
    Login,
    /// Login or Back pressed on `0x10F`.
    LoginClosing,
    Rejected,
    /// A new `0x10E` after the login flow returned 0.
    WelcomeAgain,
    /// WOL left: a new `0xE2`.
    MainReturn,
    /// The runner's chat-lobby list and first join behind the wait box.
    Lobby,
    /// `0x113` shown: its entry slide runs.
    LobbyShown,
    /// Join or Create asked to leave the chat lobby first.
    Leaving,
    /// The game join was requested and `0x113` slides out.
    Joining,
    /// The create was requested.
    Creating,
    /// C2/C9 opening.
    Setup,
    /// C2/C9 Back: the lobby rejoins.
    Rejoin,
    /// OnGameStart accepted: the slide, then the network loading screen.
    Loading,
    /// The started match runs: B5/B6 input until EXIT is queued.
    AbortInput,
    /// EXIT queued: the match returns to the chat lobby.
    MatchLeaving,
    /// Test service: Custom Match pressed, waiting for 0x10F.
    ServiceLogin,
    /// Test service: Login pressed, waiting for 0x113.
    ServiceLobby,
    /// Test service: 0xC3 OK pressed, waiting for C2.
    ServiceSetup,
    /// `0x113` Options pressed: waiting for `0x2BC`.
    OptionsOpening,
    /// `0x2BC` closed: waiting for the lobby.
    OptionsReturn,
    /// OK's result checked: `0x2BC` opened again.
    OptionsReopened,
    /// Sound Options pressed: waiting for `0xD6`.
    SoundOpening,
    /// `0xD6` Back pressed: waiting for `0x2BC`.
    SoundReturn,
    Settle(u32),
}

pub(super) struct WolCapture {
    target: WolTarget,
    phase: WolPhase,
    route: Vec<Value>,
    /// Setup Back or the match return rejoined the chat lobby.
    rejoined: bool,
    abort_input: Option<abort_return::AbortReturnInput>,
    exit_consumed: bool,
    /// RA2MD.INI and the live WOL preferences when Options was pressed.
    options_before: Option<(Option<Vec<u8>>, WolPreferences)>,
}

impl WolCapture {
    pub(super) fn new(target: WolTarget) -> Self {
        Self {
            target,
            phase: WolPhase::Main,
            route: Vec::new(),
            rejoined: false,
            abort_input: None,
            exit_consumed: false,
            options_before: None,
        }
    }

    pub(super) fn timeout(&self) -> Duration {
        // The abort return loads and runs a retail map first.
        Duration::from_secs(if self.target.in_match() { 120 } else { 60 })
    }

    fn event(&mut self, frame: u32, action: impl Into<Value>) {
        self.route
            .push(json!({"frame": frame, "action": action.into()}));
    }

    fn settle(&mut self) {
        self.phase = WolPhase::Settle(SETTLE_FRAMES);
    }

    fn requests(&mut self, state: &mut AppState, frame: u32) -> Result<Vec<WolServiceRequest>> {
        let requests = state
            .frontend
            .wol
            .as_mut()
            .context("WOL visit")?
            .take_service_requests();
        self.route.push(json!({"frame": frame,
            "requests": requests.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        Ok(requests)
    }

    fn page_settled(state: &AppState, kind: ShellSlideKind) -> bool {
        MoviesCapture::slide_settled(state, kind)
            && state.frontend.shell_exit.is_none()
            && state.frontend.shell_page_title.is_terminal()
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "WOL capture encountered the shell fallback"
        );
        if self.target.in_match()
            && self.phase == WolPhase::Loading
            && state.frontend.screen == GameScreen::Loading
        {
            self.complete_peer_loading(state, frame)?;
        }
        match self.phase {
            WolPhase::Main
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                // The Options box, read when Internet opens the visit; the
                // capture never writes the profile out.
                state.persistence.options_profile.wol_test_service = self.target.test_service();
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                        crate::ui::main_menu_shell::MainMenuShellAction::WwOnline,
                    ),
                );
                self.event(frame, "0xE2 Internet");
                self.phase = WolPhase::Welcome;
            }
            WolPhase::Welcome
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                if self.target.test_service() {
                    let rect = App::wol_welcome_layout(state)
                        .page
                        .button_rect(crate::ui::wol_shell::CUSTOM_MATCH)
                        .context("Custom Match")?;
                    welcome_click(state, rect);
                    self.event(frame, "0x10E Custom Match (Options test service)");
                    self.phase = WolPhase::ServiceLogin;
                    return Ok(());
                }
                state
                    .frontend
                    .wol
                    .as_mut()
                    .context("WOL visit")?
                    .set_provider(WolProvider::Controlled);
                let rect = App::wol_welcome_layout(state)
                    .page
                    .button_rect(crate::ui::wol_shell::CUSTOM_MATCH)
                    .context("Custom Match")?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Custom Match did not tear 0x10E down"
                );
                self.event(frame, "0x10E Custom Match (controlled service)");
                self.phase = WolPhase::Custom;
            }
            WolPhase::Custom if state.frontend.shell_exit.is_none() => {
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests == [WolServiceRequest::EnsureApi],
                    "the flow did not start with the WOLAPI object"
                );
                ensure!(
                    state.frontend.wol.as_ref().and_then(|visit| visit.mode())
                        == Some(crate::ui::wol_shell::WolMode::CustomMatch),
                    "Custom Match did not write mode 4"
                );
                service_event(state, WolServiceEvent::Api(WolApiStatus::Available));
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [
                            WolServiceRequest::ListCredentials,
                            WolServiceRequest::FetchServers { flags: 0x2900 }
                        ],
                    "the server list was not requested"
                );
                let wait = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.wait_box())
                    .context("no 0xC4 wait box")?;
                ensure!(!wait.shown, "0xC4 must be created hidden");
                self.event(frame, "controlled WOLAPI available");
                match self.target {
                    WolTarget::ServerListWait | WolTarget::ServerListDisconnect => {
                        self.phase = WolPhase::Servers;
                    }
                    WolTarget::ServerListFailure => {
                        service_event(state, WolServiceEvent::ServerList(Err(())));
                        self.event(frame, "controlled server list failure");
                        self.phase = WolPhase::MainReturn;
                    }
                    _ => {
                        service_event(state, WolServiceEvent::Credentials(Vec::new()));
                        service_event(state, WolServiceEvent::ServerStatus(0x0004_012F));
                        service_event(state, WolServiceEvent::ServerList(Ok(server_list())));
                        ensure!(
                            state
                                .frontend
                                .wol
                                .as_ref()
                                .is_some_and(|visit| visit.login().is_some()),
                            "the server list did not open 0x10F"
                        );
                        self.event(frame, "controlled server list: 3 records, selected 1");
                        self.phase = WolPhase::Login;
                    }
                }
            }
            WolPhase::Servers => {
                let wait = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.wait_box())
                    .context("the wait box closed")?;
                if !wait.shown {
                    return Ok(());
                }
                let shown_after = wait.opened_at.elapsed().as_millis() as u64;
                self.route
                    .push(json!({"frame": frame, "action": "0xC4 shown",
                    "after_ms": shown_after}));
                if self.target == WolTarget::ServerListWait {
                    MoviesCapture::restore_neutral_pointer(state);
                    self.settle();
                    return Ok(());
                }
                let layout = crate::ui::shell::modal::wait_box_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                );
                welcome_click(state, layout.disconnect);
                ensure!(state.frontend.wol.is_none(), "Disconnect did not leave WOL");
                self.event(frame, "0xC4 Disconnect");
                self.phase = WolPhase::MainReturn;
            }
            WolPhase::Login
                if rendered == PresentedShell::WolLogin
                    && Self::page_settled(state, ShellSlideKind::WolLogin) =>
            {
                let layout = crate::app::shell_wol::login_layout(state);
                match self.target {
                    WolTarget::LoginSteady => {
                        MoviesCapture::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.settle();
                        return Ok(());
                    }
                    WolTarget::LoginBack => {
                        login_click(state, button(&layout, BACK)?);
                        self.event(frame, "0x10F Back");
                        self.phase = WolPhase::LoginClosing;
                        return Ok(());
                    }
                    _ => {}
                }
                login_click(state, layout.nickname_face);
                App::handle_wol_key(state, None, true, Some(NICKNAME));
                login_click(state, layout.password);
                App::handle_wol_key(state, None, true, Some(PASSWORD));
                let login = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.login())
                    .context("0x10F")?;
                ensure!(
                    login.login_enabled && login.remember_enabled && !login.forget_enabled,
                    "typed account did not enable Login and Remember"
                );
                self.event(frame, "typed nickname and password");
                if self.target == WolTarget::LoginTyped {
                    MoviesCapture::restore_neutral_pointer(state);
                    App::handle_wol_mouse_move(state);
                    self.settle();
                    return Ok(());
                }
                login_click(state, button(&layout, LOGIN)?);
                self.event(frame, "0x10F Login");
                self.phase = WolPhase::LoginClosing;
            }
            WolPhase::LoginClosing if state.frontend.shell_exit.is_none() => {
                if self.target == WolTarget::LoginBack {
                    self.phase = WolPhase::WelcomeAgain;
                    return Ok(());
                }
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [WolServiceRequest::Connect {
                            server: server_list().servers[1].clone(),
                            nickname: NICKNAME.into(),
                            password: PASSWORD.into(),
                            changed: true,
                        }],
                    "Login did not connect to the selected record"
                );
                if self.target.lobby() {
                    service_event(
                        state,
                        WolServiceEvent::Login(Ok(WolLoginAccepted {
                            motd: "Controlled message of the day".into(),
                        })),
                    );
                    self.event(frame, "controlled login accepted");
                    self.phase = WolPhase::Lobby;
                } else {
                    service_event(state, WolServiceEvent::Login(Err(BAD_PASSWORD)));
                    self.event(frame, "controlled login rejected 0x80040065");
                    self.phase = WolPhase::Rejected;
                }
            }
            WolPhase::Rejected => {
                let expected = App::csf_label(state, "TXT_BADPASS", "TXT_BADPASS");
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == expected),
                    "TXT_BADPASS did not show"
                );
                MoviesCapture::restore_neutral_pointer(state);
                if self.target == WolTarget::LoginRejected {
                    App::handle_wol_mouse_move(state);
                    self.settle();
                    return Ok(());
                }
                let ok = crate::ui::shell::modal::body_ok_layout(
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                )
                .ok;
                welcome_click(state, ok);
                self.event(frame, "0xD0 OK");
                self.phase = WolPhase::WelcomeAgain;
            }
            WolPhase::WelcomeAgain
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.mode())
                        .is_none(),
                    "the login flow did not return mode 0"
                );
                MoviesCapture::restore_neutral_pointer(state);
                App::handle_wol_mouse_move(state);
                self.event(frame, "fresh 0x10E settled");
                self.settle();
            }
            WolPhase::MainReturn
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                ensure!(state.frontend.wol.is_none(), "the WOL visit survived");
                self.event(frame, "new 0xE2 settled");
                self.settle();
            }
            WolPhase::Lobby => self.lobby_entry(state, frame)?,
            WolPhase::Leaving => {
                let requests = self.requests(state, frame)?;
                if requests.is_empty() {
                    return Ok(());
                }
                // 0x00789B60 calls API+0x24 once unconditionally and once
                // more while joined; 0x00788800 only while joined.
                let leaves = if self.target.guest() { 1 } else { 2 };
                ensure!(
                    requests == vec![WolServiceRequest::LeaveChannel; leaves],
                    "Join/Create did not leave the chat lobby first: {requests:?}"
                );
                service_event(state, WolServiceEvent::ChannelLeft);
                self.event(frame, "controlled channel left");
                let requests = self.requests(state, frame)?;
                if self.target.guest() {
                    ensure!(
                        requests
                            == [WolServiceRequest::JoinChannel {
                                channel: games()[0].clone(),
                                key: String::new(),
                            }],
                        "the open game was not joined"
                    );
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "the join did not slide 0x113 out"
                    );
                    self.phase = WolPhase::Joining;
                } else {
                    let [WolServiceRequest::CreateChannel { channel, key }] = requests.as_slice()
                    else {
                        bail!("the game channel was not created: {requests:?}");
                    };
                    ensure!(
                        channel.name == format!("{NICKNAME}'s_game")
                            && channel.kind == GAME_CHANNEL
                            && channel.max_users == 7
                            && key.is_empty(),
                        "unexpected created channel {channel:?}"
                    );
                    self.phase = WolPhase::Creating;
                }
            }
            WolPhase::Joining if state.frontend.shell_exit.is_none() => {
                // The result arrives once 0x113 is hidden.
                let mut channel = games()[0].clone();
                channel.users += 1;
                service_event(
                    state,
                    WolServiceEvent::Joined(Ok(WolJoined {
                        channel,
                        members: vec![
                            member("Alpha", MEMBER_OPERATOR, ""),
                            member(NICKNAME, 0, ""),
                        ],
                    })),
                );
                self.event(frame, "controlled game join accepted");
                self.phase = WolPhase::Setup;
            }
            WolPhase::Creating => {
                service_event(
                    state,
                    WolServiceEvent::Created(Ok(vec![member(NICKNAME, MEMBER_OPERATOR, "")])),
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "the create did not slide 0x113 out"
                );
                self.event(frame, "controlled create accepted");
                self.phase = WolPhase::Setup;
            }
            WolPhase::Setup => {
                let host = !self.target.guest();
                if !(rendered == PresentedShell::NetworkSetup && wol_setup_settled(state, host)) {
                    return Ok(());
                }
                self.event(frame, if host { "C2 settled" } else { "C9 settled" });
                self.setup_settled(state, frame)?;
            }
            WolPhase::Rejoin
                if state.frontend.shell_exit.is_none()
                    && state.frontend.shell_route.wol_lobby() =>
            {
                let requests = self.requests(state, frame)?;
                if requests.is_empty() {
                    return Ok(());
                }
                ensure!(
                    requests
                        == [WolServiceRequest::JoinChannel {
                            channel: lobbies()[0].clone(),
                            key: LOBBY_KEY.into(),
                        }],
                    "setup Back did not rejoin the chat lobby: {requests:?}"
                );
                join_lobby(state);
                self.rejoined = true;
                self.event(frame, "controlled chat lobby rejoined");
                self.phase = WolPhase::LobbyShown;
            }
            WolPhase::LobbyShown
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                self.lobby_settled(state, frame)?;
            }
            WolPhase::Loading
                if self.target.in_match() && state.frontend.screen == GameScreen::InGame =>
            {
                let sim = &state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .context("installed WOL runtime")?
                    .simulation;
                ensure!(
                    state.match_state.session_mode() == SessionMode::Wol
                        && sim.session().started()
                        && sim.session().game_mode_nonzero(),
                    "the WOL runtime was not admitted"
                );
                self.event(frame, "WOL match installed");
                self.abort_input = Some(match self.target {
                    WolTarget::InternetMenu => {
                        abort_return::AbortReturnInput::begin_menu(state, &mut self.route, frame)?
                    }
                    WolTarget::GameControls => abort_return::AbortReturnInput::begin_game_controls(
                        state,
                        &mut self.route,
                        frame,
                    )?,
                    _ => abort_return::AbortReturnInput::begin(state, &mut self.route, frame)?,
                });
                self.phase = WolPhase::AbortInput;
            }
            WolPhase::AbortInput => {
                if self
                    .abort_input
                    .as_mut()
                    .context("WOL abort input")?
                    .after_present(state, &mut self.route, frame)?
                {
                    self.phase = if matches!(
                        self.target,
                        WolTarget::InternetMenu | WolTarget::GameControls
                    ) {
                        ensure!(
                            crate::app::input::pause_menu::current_template(state)
                                == crate::ui::shell::pause_menu::PauseMenuTemplate::Internet,
                            "a WOL match did not open FF"
                        );
                        WolPhase::Settle(2)
                    } else {
                        WolPhase::MatchLeaving
                    };
                }
            }
            WolPhase::MatchLeaving => {
                if state.match_state.scenario_exit.is_some() && !self.exit_consumed {
                    let sim = &state
                        .match_state
                        .sim_runtime
                        .as_ref()
                        .context("EXIT runtime")?
                        .simulation;
                    ensure!(sim.quit_requested(), "the return began before EXIT ran");
                    self.event(frame, "EXIT consumed");
                    self.exit_consumed = true;
                }
                if state.frontend.shell_route.wol_lobby() && state.frontend.shell_exit.is_none() {
                    ensure!(
                        self.exit_consumed
                            && state.match_state.session_mode() == SessionMode::Wol
                            && state.frontend.loading_session.is_none()
                            && state.match_state.scenario_exit.is_none()
                            && state.frontend.multiplayer_setup.is_none()
                            && state.frontend.network_setup_protocol.is_none(),
                        "the WOL match return cleanup is incomplete"
                    );
                    // A fresh runner, not the resumed one: lobby_entry
                    // requires it hidden behind its wait box.
                    self.event(frame, "post-match 0x113 opened");
                    self.rejoined = true;
                    self.phase = WolPhase::Lobby;
                }
            }
            WolPhase::ServiceLogin
                if rendered == PresentedShell::WolLogin
                    && Self::page_settled(state, ShellSlideKind::WolLogin) =>
            {
                let layout = crate::app::shell_wol::login_layout(state);
                login_click(state, layout.nickname_face);
                App::handle_wol_key(state, None, true, Some(NICKNAME));
                login_click(state, layout.password);
                App::handle_wol_key(state, None, true, Some(PASSWORD));
                login_click(state, button(&layout, LOGIN)?);
                self.event(frame, "0x10F Login (test service)");
                self.phase = WolPhase::ServiceLobby;
            }
            WolPhase::ServiceLobby
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                let lobby = Self::lobby(state)?;
                ensure!(
                    lobby.shown
                        && lobby.ui.lobby_rows.len() == 3
                        && lobby.ui.game_rows.len() == 3
                        && lobby.ui.player_rows.iter().any(|row| row.name == NICKNAME),
                    "the test service did not fill 0x113"
                );
                self.event(frame, "0x113 settled from the test service");
                if self.target == WolTarget::TestServiceCreate {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::CREATE)?;
                    let (width, height) = (
                        state.renderer.gpu.config.width as i32,
                        state.renderer.gpu.config.height as i32,
                    );
                    let ok = crate::ui::shell::modal::wol_create_game_layout(width, height).ok;
                    welcome_click(state, ok);
                    self.event(frame, "0xC3 OK (test service)");
                    self.phase = WolPhase::ServiceSetup;
                    return Ok(());
                }
                if self.target == WolTarget::TestServiceJoin {
                    game_row_click(state, 0)?;
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::JOIN)?;
                    self.event(frame, "Join on Tanya's_game (test service)");
                    self.phase = WolPhase::ServiceSetup;
                    return Ok(());
                }
                self.hover_settle(state);
            }
            WolPhase::ServiceSetup
                if rendered == PresentedShell::NetworkSetup
                    && wol_setup_settled(state, self.target != WolTarget::TestServiceJoin) =>
            {
                ensure!(
                    state
                        .match_state
                        .session
                        .network_setup()
                        .is_some_and(|launch| launch.local_player().name == NICKNAME),
                    "C2/C9 did not open for the local user"
                );
                self.event(frame, "C2/C9 settled from the test service");
                MoviesCapture::restore_neutral_pointer(state);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                self.settle();
            }
            WolPhase::OptionsOpening
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                self.options_settled(state, frame)?;
            }
            WolPhase::OptionsReturn
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                self.options_returned(state, frame)?;
            }
            WolPhase::OptionsReopened
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                let checked = Self::options_page(state)?.ui.checked;
                ensure!(
                    checked == [false, true, true, true, true, true, true, true, false],
                    "the reopened 0x2BC does not show the saved values: {checked:?}"
                );
                self.event(frame, json!({"0x2BC reopened": checked}));
                // The capture's isolated profile goes back as it was.
                let (bytes, _) = self.options_before.clone().context("saved RA2MD.INI")?;
                restore_ra2md(state, bytes)?;
                self.hover_settle(state);
            }
            WolPhase::SoundOpening
                if rendered == PresentedShell::WolSoundOptions
                    && Self::page_settled(state, ShellSlideKind::WolSoundOptions) =>
            {
                self.sound_settled(state, frame)?;
            }
            WolPhase::SoundReturn
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                ensure!(
                    Self::options_page(state)?.sound.is_none(),
                    "0xD6 survived its Back"
                );
                self.event(frame, "0x2BC settled after 0xD6 Back");
                self.hover_settle(state);
            }
            WolPhase::Settle(remaining) if remaining > 0 => {
                self.phase = WolPhase::Settle(remaining - 1);
            }
            _ => {}
        }
        Ok(())
    }

    /// C2/C9 shown and settled: drive the target's interaction.
    fn setup_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::multiplayer_setup as setup_ui;
        match self.target {
            WolTarget::SetupBack => {
                setup_button(state, setup_ui::BACK)?;
                self.event(frame, "C2 Back");
                self.phase = WolPhase::Rejoin;
                return Ok(());
            }
            WolTarget::GuestAccept => {
                setup_button(state, setup_ui::ACCEPT)?;
                let requests = setup_requests(state);
                ensure!(
                    requests.contains(&NetworkSetupRequest::Ready(true))
                        && !setup_view(state)?.enabled(setup_ui::ACCEPT),
                    "C9 Accept did not report readiness: {requests:?}"
                );
                self.event(frame, "C9 Accept");
            }
            WolTarget::GuestBack => {
                setup_button(state, setup_ui::BACK)?;
                let requests = setup_requests(state);
                ensure!(
                    requests == [NetworkSetupRequest::Leave] && state.frontend.shell_exit.is_none(),
                    "C9 Back must leave the channel and wait: {requests:?}"
                );
                setup::service_event(state, NetworkSetupEvent::ChannelLeft)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "the channel-left event did not slide C9 out"
                );
                self.event(frame, "C9 Back, then controlled channel left");
                self.phase = WolPhase::Rejoin;
                return Ok(());
            }
            target if target.guest_joined() => {
                self.join_guest(state, frame)?;
                match target {
                    WolTarget::SetupStartNotReady => {
                        setup_button(state, setup_ui::START)?;
                        let requests = setup_requests(state);
                        let line =
                            App::csf_label(state, "GUI:HostGameStartHost", "GUI:HostGameStartHost");
                        ensure!(
                            requests == [NetworkSetupRequest::RequestAcceptance]
                                && setup_chat_has(state, &line)?,
                            "Start with an unready guest: {requests:?}"
                        );
                        self.event(frame, "C2 Start refused: guest not ready");
                    }
                    WolTarget::SetupChat => self.setup_chat(state, frame)?,
                    WolTarget::SetupStart
                    | WolTarget::AbortReturn
                    | WolTarget::InternetMenu
                    | WolTarget::GameControls => {
                        self.start(state, frame)?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(state);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    /// The host's join sink for the controlled guest, its ping and its
    /// map status.
    fn join_guest(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("C2 setup")?;
        let mut slot = launch.local_slot().clone();
        slot.color_index = (slot.color_index + 1) % 8;
        let row = state
            .match_state
            .session
            .network_observer_row()
            .unwrap_or(8);
        let opened = setup_requests(state);
        self.route.push(json!({"frame": frame,
            "c2_open_requests": opened.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        setup::service_event(
            state,
            NetworkSetupEvent::ParticipantJoined(crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(1),
                name: PEER.into(),
                role: Default::default(),
                slot,
            }),
        )?;
        let requests = setup_requests(state);
        ensure!(
            requests
                == [
                    NetworkSetupRequest::ObserverSlot {
                        name: PEER.into(),
                        row
                    },
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::RequestPings,
                    NetworkSetupRequest::Advertise,
                ],
            "the host join sink's requests: {requests:?}"
        );
        setup::service_event(
            state,
            NetworkSetupEvent::Ping {
                name: PEER.into(),
                ping: 120,
            },
        )?;
        setup::service_event(
            state,
            NetworkSetupEvent::MapStatus {
                index: 1,
                status: 1,
            },
        )?;
        let view = setup_view(state)?;
        ensure!(
            view.ping_icons[1] == crate::ui::multiplayer_setup::ping_icon(120)
                && view.ping_icons[1].is_some()
                && !state.match_state.session.network_player_ready(1),
            "the guest's row, ping icon or readiness: {:?}",
            view.ping_icons
        );
        self.event(frame, "controlled guest joined, ping 120, map present");
        Ok(())
    }

    /// C2 chat: a page through the CSF command, then an emote.
    fn setup_chat(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let page = App::csf_label(state, "TXT_PAGE_COMMAND", "TXT_PAGE_COMMAND");
        let layout = crate::app::input::multiplayer_setup::layout(state);
        setup_click(state, layout.chat);
        crate::app::input::multiplayer_setup::key_press(
            state,
            None,
            Some(&format!("{page} {PEER} ready?")),
        );
        crate::app::input::multiplayer_setup::key_press(
            state,
            Some(winit::keyboard::KeyCode::Enter),
            None,
        );
        let requests = setup_requests(state);
        ensure!(
            requests
                == [NetworkSetupRequest::WolChat(WolChatLine::Page {
                    name: PEER.encode_utf16().collect(),
                    text: "ready?".encode_utf16().collect(),
                })],
            "the page line was not sent: {requests:?}"
        );
        ensure!(
            setup_chat_has(state, &format!("[{NICKNAME} > {PEER}] ready?"))?,
            "the page echo did not show"
        );
        crate::app::input::multiplayer_setup::key_press(state, None, Some("waves"));
        setup_click(state, layout.emote.context("C2 emote")?);
        let requests = setup_requests(state);
        ensure!(
            requests
                == [NetworkSetupRequest::WolEmote(
                    "waves".encode_utf16().collect()
                )]
                && setup_chat_has(state, &format!("{NICKNAME} waves"))?
                && state
                    .frontend
                    .multiplayer_setup
                    .as_ref()
                    .is_some_and(|ui| ui.chat.edit.units.is_empty()),
            "the emote was not sent or echoed: {requests:?}"
        );
        self.event(frame, "C2 page and emote");
        Ok(())
    }

    /// The guest accepts, an option edit clears it, it accepts again; Start
    /// then requests StartGame and the server's OnGameStart completes it.
    fn start(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::multiplayer_setup as setup_ui;
        let ready = || NetworkSetupEvent::PlayerReady {
            index: 1,
            accepted: true,
        };
        setup::service_event(state, ready())?;
        let layout = crate::app::input::multiplayer_setup::layout(state);
        setup_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(layout.checkboxes[0], layout.page.scale),
        );
        ensure!(
            !state.match_state.session.network_player_ready(1),
            "a C2 option edit kept the guest's acceptance"
        );
        setup_requests(state);
        setup::service_event(state, ready())?;
        setup_button(state, setup_ui::START)?;
        let requests = setup_requests(state);
        ensure!(
            requests
                == [
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::StartGame {
                        users: vec![NICKNAME.into(), PEER.into()]
                    },
                ],
            "C2 Start did not commit: {requests:?}"
        );
        // Back and option edits are ignored while the start is pending.
        let options = |state: &AppState| {
            state
                .match_state
                .session
                .network_setup()
                .map(|launch| launch.options.clone())
        };
        let before = options(state);
        setup_button(state, setup_ui::BACK)?;
        setup_click(
            state,
            crate::ui::skirmish_shell::checkbox_icon_rect(layout.checkboxes[0], layout.page.scale),
        );
        ensure!(
            setup_requests(state).is_empty()
                && state.frontend.shell_exit.is_none()
                && options(state) == before
                && state.match_state.session.network_player_ready(1),
            "Back or an option edit acted on a pending start"
        );
        let user = |name: &str, flags| WolStartUser {
            name: name.into(),
            flags,
        };
        let mut pings = [-1; 8];
        pings[1] = 120;
        // A server list naming someone else rejects the start; Start again
        // requests it anew.
        setup::service_event(
            state,
            NetworkSetupEvent::GameStarted {
                users: vec![user(NICKNAME, 1), user("Bravo", 0)],
                pings,
            },
        )?;
        ensure!(
            setup_requests(state).is_empty() && state.frontend.shell_exit.is_none(),
            "a mismatched OnGameStart started the game"
        );
        setup_button(state, setup_ui::START)?;
        ensure!(
            setup_requests(state).last()
                == Some(&NetworkSetupRequest::StartGame {
                    users: vec![NICKNAME.into(), PEER.into()]
                }),
            "Start after a rejected OnGameStart did not request again"
        );
        // 0x7A7C80 compares lowercased names.
        setup::service_event(
            state,
            NetworkSetupEvent::GameStarted {
                users: vec![
                    user(&NICKNAME.to_uppercase(), 1),
                    user(&PEER.to_lowercase(), 0),
                ],
                pings,
            },
        )?;
        let requests = setup_requests(state);
        ensure!(
            requests == [NetworkSetupRequest::Leave] && state.frontend.shell_exit.is_some(),
            "OnGameStart did not leave the channel and slide C2 out: {requests:?}"
        );
        MoviesCapture::restore_neutral_pointer(state);
        self.event(frame, "C2 Start, then controlled OnGameStart");
        self.phase = WolPhase::Loading;
        Ok(())
    }

    /// The runner behind its wait box: the chat-lobby list, the picked
    /// lobby's join, then both lists for the shown dialog.
    fn lobby_entry(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        // The mode-loop pass (0x0077B402) applies the preferences first;
        // after a match the entry apply (0x0077B31D) already has WOLAPI.
        let requests = self.requests(state, frame)?;
        let [apply @ .., list] = requests.as_slice() else {
            bail!("the runner sent nothing");
        };
        let passes = if self.rejoined { 2 } else { 1 };
        ensure!(
            apply.len() == 2 * passes
                && apply.chunks(2).all(|pair| pair == Self::applied(state))
                && *list == WolServiceRequest::ListChannels { kind: 0, flags: 0 },
            "the runner did not apply and request the chat lobbies: {requests:?}"
        );
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        ensure!(
            visit.motd().is_none()
                && visit
                    .lobby()
                    .is_some_and(|lobby| !lobby.shown && lobby.wait.is_some()),
            "0x113 must start hidden behind the wait box with the MOTD consumed"
        );
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: 0,
                channels: lobbies(),
            },
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::JoinChannel {
                    channel: lobbies()[0].clone(),
                    key: LOBBY_KEY.into(),
                }],
            "the picker did not join Lob_41_0: {requests:?}"
        );
        join_lobby(state);
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [
                    WolServiceRequest::ListChannels { kind: 0, flags: 0 },
                    WolServiceRequest::ListChannels {
                        kind: GAME_CHANNEL,
                        flags: 1
                    },
                ],
            "the shown lobby did not request both lists: {requests:?}"
        );
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: 0,
                channels: lobbies(),
            },
        );
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: GAME_CHANNEL,
                channels: games(),
            },
        );
        self.event(frame, "controlled lobbies, join and games");
        self.phase = WolPhase::LobbyShown;
        Ok(())
    }

    fn lobby(state: &AppState) -> Result<&crate::app::shell_wol::lobby::WolLobby> {
        state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .context("the lobby runner")
    }

    fn chat_has(state: &AppState, text: &str) -> Result<bool> {
        Ok(Self::lobby(state)?
            .ui
            .chat
            .lines
            .iter()
            .any(|line| String::from_utf16_lossy(&line.text) == text))
    }

    /// `0x113` shown and settled: drive the target's interaction.
    fn lobby_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let layout = crate::app::shell_wol::lobby::layout(state);
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let joined = App::csf_label(state, "TXT_JOINED_S", "TXT_JOINED_S");
        let base = App::csf_label(state, "TXT_LOB_1", "TXT_LOB_1");
        ensure!(
            Self::chat_has(
                state,
                &crate::assets::csf_file::format_csf(
                    &joined,
                    &[crate::assets::csf_file::CsfArg::Str(&base)]
                )
            )?,
            "TXT_JOINED_S was not printed"
        );
        let lobby = Self::lobby(state)?;
        ensure!(
            lobby.ui.game_rows.len() == 4
                && lobby.ui.player_rows.len() == 4
                && lobby.ui.lobby_rows.len() == 3
                && lobby.ui.lobby_list.selected == Some(0),
            "the lists were not filled"
        );
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        ensure!(
            visit.authenticated() && state.frontend.shell_route.wol_lobby(),
            "the accepted login did not enter the custom lobby"
        );
        ensure!(
            state
                .frontend
                .multiplayer_shell_runtime
                .local_participant()
                .name
                == NICKNAME,
            "the nickname did not become the MultiPlayer handle"
        );
        if self.rejoined {
            self.event(frame, "0x113 settled again");
            self.hover_settle(state);
            return Ok(());
        }
        self.event(frame, "0x113 settled");
        if self.target.options() {
            self.options_before = Some((read_ra2md(state)?, state.frontend.wol_preferences));
            lobby_button(state, &layout, lobby_ui::OPTIONS)?;
            ensure!(
                state.frontend.shell_exit.is_some(),
                "Options did not slide 0x113 out"
            );
            self.event(frame, "0x113 Options");
            self.phase = WolPhase::OptionsOpening;
            return Ok(());
        }
        match self.target {
            WolTarget::LoginAccepted => {
                ensure!(
                    Self::chat_has(state, "Controlled message of the day")?,
                    "the MOTD was not printed"
                );
            }
            WolTarget::LobbySort => {
                let rect = layout
                    .control_rect(lobby_ui::SORT_PING)
                    .context("ping sort")?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.wol_lobby_globals.game_sort != 0
                        && Self::lobby(state)?.ui.game_rows[0].name == "Alpha's_game",
                    "the ping sort did not re-sort"
                );
                self.event(frame, "ping sort");
            }
            WolTarget::LobbyChat => {
                App::handle_wol_key(state, None, true, Some("hello"));
                App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [WolServiceRequest::Chat {
                            line: WolChatLine::Say("hello".encode_utf16().collect()),
                            to: Vec::new(),
                        }],
                    "the chat line was not sent: {requests:?}"
                );
                service_event(
                    state,
                    WolServiceEvent::Page {
                        from: "Golf".into(),
                        text: "are you there?".encode_utf16().collect(),
                    },
                );
                ensure!(
                    Self::chat_has(state, &format!("[{NICKNAME}] hello"))?
                        && Self::chat_has(state, "(Golf) are you there?")?
                        && crate::app::shell_wol::last_pager(state) == Some("Golf"),
                    "the echo or the page did not show"
                );
                service_event(state, WolServiceEvent::UserJoined(member("India", 0, "")));
                service_event(state, WolServiceEvent::UserLeft("Foxtrot".into()));
                let names: Vec<String> = Self::lobby(state)?
                    .ui
                    .player_rows
                    .iter()
                    .map(|row| row.name.clone())
                    .collect();
                ensure!(
                    names == ["Hotel", "Echo", "India", NICKNAME],
                    "the membership did not follow join and part: {names:?}"
                );
                self.event(frame, "chat line and received page");
            }
            WolTarget::JoinAdmission => {
                // Rows: Alpha, Charlie (version 24), Delta, Bravo (full).
                game_row_click(state, 1)?;
                game_row_click(state, 3)?;
                let mismatch = App::csf_label(state, "TXT_MISMATCH", "TXT_MISMATCH");
                let full = App::csf_label(state, "WOL:GameFull", "WOL:GameFull");
                ensure!(
                    Self::chat_has(state, &mismatch)? && Self::chat_has(state, &full)?,
                    "the admission refusals were not printed"
                );
                ensure!(
                    !Self::lobby(state)?.ui.enabled(lobby_ui::JOIN),
                    "a refused game enabled Join"
                );
                self.event(frame, "mismatched and full games refused");
            }
            WolTarget::JoinPassword => {
                game_row_click(state, 2)?;
                lobby_button(state, &layout, lobby_ui::JOIN)?;
                ensure!(
                    Self::lobby(state)?
                        .ui
                        .popup
                        .as_ref()
                        .map(|popup| popup.dialog())
                        == Some(lobby_ui::PASSWORD_DIALOG),
                    "Join on a keyed game did not open 0xE1"
                );
                self.event(frame, "0xE1 opened");
            }
            WolTarget::JoinAccepted | WolTarget::GuestAccept | WolTarget::GuestBack => {
                game_row_click(state, 0)?;
                lobby_button(state, &layout, lobby_ui::JOIN)?;
                self.event(frame, "Join on Alpha's_game");
                self.phase = WolPhase::Leaving;
                return Ok(());
            }
            target
                if matches!(
                    target,
                    WolTarget::CreatePopup | WolTarget::CreateAccepted | WolTarget::SetupBack
                ) || target.guest_joined() =>
            {
                lobby_button(state, &layout, lobby_ui::CREATE)?;
                ensure!(
                    Self::lobby(state)?
                        .ui
                        .popup
                        .as_ref()
                        .map(|popup| popup.dialog())
                        == Some(lobby_ui::CREATE_DIALOG),
                    "Create did not open 0xC3"
                );
                self.event(frame, "0xC3 opened");
                if self.target != WolTarget::CreatePopup {
                    let ok = crate::ui::shell::modal::wol_create_game_layout(width, height).ok;
                    welcome_click(state, ok);
                    self.event(frame, "0xC3 OK");
                    self.phase = WolPhase::Leaving;
                    return Ok(());
                }
            }
            WolTarget::LobbyBack => {
                lobby_button(state, &layout, lobby_ui::BACK)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Back did not slide 0x113 out"
                );
                self.event(frame, "0x113 Back");
                self.phase = WolPhase::WelcomeAgain;
                return Ok(());
            }
            _ => {}
        }
        self.hover_settle(state);
        Ok(())
    }

    fn options_page(state: &AppState) -> Result<&crate::app::shell_wol::options::WolOptionsPage> {
        match &state.frontend.wol.as_ref().context("WOL visit")?.page {
            crate::app::shell_wol::WolPage::Options(page) => Ok(page),
            _ => bail!("0x2BC is not open"),
        }
    }

    /// The preference requests `0x0077E110` sends with the WOLAPI object.
    fn applied(state: &AppState) -> [WolServiceRequest; 2] {
        let live = state.frontend.wol_preferences;
        [
            WolServiceRequest::SetLangFilter(live.lang_filter),
            WolServiceRequest::SetFindPage {
                find: live.allow_find,
                page: live.allow_page,
            },
        ]
    }

    /// `0x2BC` shown and settled over the hidden lobby.
    fn options_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::wol_options::{
            BUDDIES, CANCEL, MUSIC, OK, PAGES, SOUND_OPTIONS, WolOptionsState,
        };
        let live = state.frontend.wol_preferences;
        let page = Self::options_page(state)?;
        ensure!(
            page.ui.checked == WolOptionsState::open(&live).checked && page.sound.is_none(),
            "0x2BC init does not show the live values"
        );
        ensure!(!Self::lobby(state)?.shown, "0x113 must hide under 0x2BC");
        self.event(frame, json!({"0x2BC settled": page.ui.checked}));
        let checked = |state: &AppState, id| {
            Self::options_page(state).is_ok_and(|page| page.ui.is_checked(id))
        };
        match self.target {
            WolTarget::OptionsSteady => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated RA2MD.INI holds [WOnline] values: {live:?}"
                );
                let rect = crate::app::shell_wol::options::layout(state)
                    .checkbox_rect(PAGES)
                    .context("Pages")?;
                set_window_pointer_to_rect_center(state, rect);
                App::handle_wol_mouse_move(state);
                ensure!(
                    crate::app::shell_wol::options::status_key(state) == Some("STT:WOLOptionsPage"),
                    "Pages has no help"
                );
                self.settle();
            }
            WolTarget::OptionsMusicOff => {
                options_checkbox(state, MUSIC)?;
                ensure!(!checked(state, MUSIC), "Play Music stayed checked");
                ensure!(
                    state.audio.theme.saved_slots()[1] == crate::audio::theme::THEME_HOLD,
                    "the preview did not queue -3"
                );
                ensure!(
                    state.frontend.wol_preferences == live
                        && read_ra2md(state)? == self.options_before.clone().context("before")?.0,
                    "the preview changed a preference"
                );
                self.event(frame, "Play Music unchecked: Queue(-3)");
                self.hover_settle(state);
            }
            WolTarget::OptionsCancel | WolTarget::OptionsOk => {
                ensure!(
                    live == WolPreferences::default(),
                    "the isolated RA2MD.INI holds [WOnline] values: {live:?}"
                );
                options_checkbox(state, PAGES)?;
                options_checkbox(state, BUDDIES)?;
                ensure!(
                    !checked(state, PAGES) && checked(state, BUDDIES),
                    "Pages/Buddies did not toggle"
                );
                let id = if self.target == WolTarget::OptionsOk {
                    OK
                } else {
                    CANCEL
                };
                options_button(state, id)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "0x2BC did not slide out"
                );
                self.event(frame, format!("Pages off, Buddies on, button {id:#x}"));
                self.phase = WolPhase::OptionsReturn;
            }
            WolTarget::SoundOptions | WolTarget::SoundOptionsBack => {
                options_button(state, SOUND_OPTIONS)?;
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "0x2BC did not slide out for Sound Options"
                );
                self.event(frame, "0x2BC Sound Options");
                self.phase = WolPhase::SoundOpening;
            }
            _ => bail!("not an Options target"),
        }
        Ok(())
    }

    /// The lobby shown again after `0x2BC` closed.
    fn options_returned(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let lobby = Self::lobby(state)?;
        ensure!(
            lobby.shown
                && matches!(
                    state.frontend.wol.as_ref().map(|visit| &visit.page),
                    Some(crate::app::shell_wol::WolPage::Lobby)
                ),
            "0x113 did not come back"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests == Self::applied(state),
            "0x0077E110 did not send the preferences: {requests:?}"
        );
        let (bytes, before) = self.options_before.clone().context("before")?;
        let live = state.frontend.wol_preferences;
        if self.target == WolTarget::OptionsCancel {
            ensure!(
                live == before
                    && state.persistence.options_profile.wol_preferences == before
                    && read_ra2md(state)? == bytes,
                "Cancel changed the preferences"
            );
            self.event(frame, "0x113 again: Cancel applied the unchanged values");
            self.hover_settle(state);
            return Ok(());
        }
        let expected = WolPreferences {
            allow_page: 0,
            display_buddies: true,
            ..WolPreferences::default()
        };
        ensure!(
            live == expected && state.persistence.options_profile.wol_preferences == expected,
            "OK did not commit the staged values: {live:?}"
        );
        let saved = read_ra2md(state)?.context("RA2MD.INI after OK")?;
        let text = String::from_utf8_lossy(&saved).into_owned();
        let ini = crate::rules::ini_parser::IniFile::from_bytes(&saved)
            .context("parse RA2MD.INI after OK")?;
        let wonline = ini.section("WOnline").context("saved [WOnline]")?;
        let keys = [
            ("AllowPage", "0"),
            ("AllowFind", "1"),
            ("LangFilter", "1"),
            ("LobMusic", "1"),
            ("ShowAll", "1"),
            ("DisplayAsian", "1"),
            ("DisplayLatin", "1"),
            ("DisplayBuddies", "yes"),
            ("DisplayClan", "no"),
        ];
        for (key, value) in keys {
            ensure!(
                wonline.read_string(key, "", 0x40) == value,
                "saved WOnline.{key} is not {value}"
            );
        }
        let at: Vec<usize> = keys
            .iter()
            .map(|(key, _)| text.find(&format!("\n{key}=")).unwrap_or(usize::MAX))
            .collect();
        ensure!(
            at.windows(2).all(|pair| pair[0] < pair[1]),
            "[WOnline] is not in the writer's order"
        );
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        ensure!(
            ini.section_or_empty("MultiPlayer")
                .is_present("ObserverMode")
                && ini
                    .section_or_empty("MultiPlayer")
                    .read_int("ObserverMode", 0)
                    == i32::from(observer),
            "ObserverMode was not written with the preferences"
        );
        self.event(
            frame,
            json!({"0x113 again after OK": text
            .lines()
            .skip_while(|line| line.trim() != "[WOnline]")
            .take(12)
            .collect::<Vec<_>>()}),
        );
        let layout = crate::app::shell_wol::lobby::layout(state);
        lobby_button(state, &layout, lobby_ui::OPTIONS)?;
        self.phase = WolPhase::OptionsReopened;
        Ok(())
    }

    /// `0xD6` shown and settled over the hidden `0x2BC`.
    fn sound_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let positions = crate::app::input::sound::volume_positions(state);
        let dialog = Self::options_page(state)?
            .sound
            .as_ref()
            .context("0xD6 state")?;
        ensure!(dialog.positions == positions, "0xD6 init positions differ");
        // A press on the music rail near its left end jumps once.
        let rail = crate::app::shell_wol::options::sound_layout(state).sliders[0];
        welcome_click(state, RectPx::new(rail.x + 8, rail.y, 4, rail.h));
        let moved = Self::options_page(state)?
            .sound
            .as_ref()
            .context("0xD6 state")?
            .positions;
        ensure!(
            moved[0] < positions[0] && moved[1..] == positions[1..],
            "the music rail press did not move it: {positions:?} -> {moved:?}"
        );
        self.event(
            frame,
            json!({"0xD6 settled": positions, "after rail press": moved}),
        );
        if self.target == WolTarget::SoundOptionsBack {
            let back = crate::app::shell_wol::options::sound_layout(state).back();
            welcome_click(state, back);
            ensure!(
                state.frontend.shell_exit.is_some(),
                "0xD6 Back did not slide out"
            );
            self.event(frame, "0xD6 Back");
            self.phase = WolPhase::SoundReturn;
            return Ok(());
        }
        self.hover_settle(state);
        Ok(())
    }

    fn hover_settle(&mut self, state: &mut AppState) {
        MoviesCapture::restore_neutral_pointer(state);
        App::handle_wol_mouse_move(state);
        self.settle();
    }

    /// Once the local loader reports 100, the controlled guest finishes too.
    fn complete_peer_loading(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::match_runtime::session::NetworkLoadingRequest;
        let requests = state.match_state.session.take_network_loading_requests();
        if !requests
            .iter()
            .any(|request| matches!(request, NetworkLoadingRequest::ReportProgress(100)))
        {
            return Ok(());
        }
        ensure!(
            crate::app::loading::pump::accept_network_loading_progress(state, 1, 100),
            "controlled guest completion was refused"
        );
        self.route.push(json!({"frame": frame,
            "action": "controlled guest loading completion after the local report",
            "requests": requests.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        Ok(())
    }

    pub(super) fn owns_simulation_steps(&self, state: &AppState) -> bool {
        self.target.in_match()
            && self.phase == WolPhase::Loading
            && state.frontend.screen == GameScreen::InGame
    }

    pub(super) fn ready(&self, state: &AppState) -> Result<bool> {
        if self.phase == WolPhase::Loading {
            if self.target.in_match() {
                return Ok(false);
            }
            ensure!(
                state.match_state.session_mode() == SessionMode::Wol,
                "the WOL handoff became offline"
            );
            return Ok(state.frontend.screen == GameScreen::Loading
                && crate::app::loading::pump::next_native_loading_frame(state)
                    == Some(crate::app::loading::pump::NextLoadingFrame::First));
        }
        // Behind a box `0x10E`/`0x10F` is gone, and with it the status line;
        // a hidden or covered `0x113` paints none either, nor does C2/C9's
        // family line belong to this check.
        let backdrop = state.frontend.wol.as_ref().is_some_and(|visit| {
            matches!(
                visit.page,
                crate::app::shell_wol::WolPage::Backdrop { .. }
                    | crate::app::shell_wol::WolPage::Message { .. }
            ) || visit
                .lobby()
                .is_some_and(|lobby| !lobby.shown || lobby.covered || lobby.ui.popup.is_some())
        }) || state.frontend.shell_route.network_setup();
        if let Some(menu) = match self.target {
            WolTarget::InternetMenu => Some(crate::ui::pause_menu::InGameMenuState::Menu),
            WolTarget::GameControls => Some(crate::ui::pause_menu::InGameMenuState::Options),
            _ => None,
        } {
            return Ok(self.phase == WolPhase::Settle(0)
                && state.frontend.screen == GameScreen::InGame
                && state.match_state.match_presentation.in_game_menu == menu);
        }
        Ok(self.phase == WolPhase::Settle(0)
            && state.frontend.shell_exit.is_none()
            && (backdrop || state.frontend.shell_status_line.is_terminal())
            && state.frontend.shell_first_paint_slide.is_none())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Value {
        let visit = state.frontend.wol.as_ref();
        json!({
            "target": format!("{:?}", self.target),
            "route": self.route,
            "service_boundary": "controlled decoded WOL results; no WOLAPI, transport or backend claim",
            "shell_route": format!("{:?}", state.frontend.shell_route),
            "mode": visit.and_then(|visit| visit.mode()).map(|mode| mode.native()),
            "authenticated": visit.is_some_and(|visit| visit.authenticated()),
            "login": visit.and_then(|visit| visit.login()).map(|login| json!({
                "nickname": login.nickname.display_text(),
                "password_units": login.password.units.len(),
                "login_enabled": login.login_enabled,
                "forget_enabled": login.forget_enabled,
                "remember_enabled": login.remember_enabled,
                "remember_checked": login.remember_checked,
                "focused": login.focused_control,
            })),
        })
    }
}

fn member(name: &str, flags: u32, tag: &str) -> WolMember {
    WolMember {
        flags,
        name: name.into(),
        clan_name: if tag.is_empty() {
            String::new()
        } else {
            "Westwood Warriors".into()
        },
        clan_tag: tag.into(),
    }
}

fn lobbies() -> Vec<WolChannel> {
    [(0, 3), (1, 12), (9, 0)]
        .into_iter()
        .map(|(number, users)| WolChannel {
            kind: 0,
            max_users: 100,
            users,
            name: format!("Lob_41_{number}"),
            ping: -1,
            ..Default::default()
        })
        .collect()
}

/// An open game with a free observer slot, a full one, one of another
/// exInfo version and a keyed one limited to 800x600.
fn games() -> Vec<WolChannel> {
    let game = |name: &str, users, max_users, ex_info: &str, ping| WolChannel {
        kind: GAME_CHANNEL,
        max_users,
        users,
        ping,
        name: name.into(),
        ex_info: ex_info.into(),
        ..Default::default()
    };
    let mut keyed = game("Delta's_game", 2, 6, "25,0,0,0,0,d.map", 80);
    keyed.flags = CHANNEL_KEYED;
    keyed.game_flags = GAME_LIMIT_800;
    vec![
        game("Alpha's_game", 2, 8, "25,0,1,0,1,a.map", 45),
        game("Bravo's_game", 3, 4, "25,0,1,0,0,b.map", 120),
        game("Charlie's_game", 1, 8, "24,0,0,0,0,c.map", -1),
        keyed,
    ]
}

/// The chat sink's OnJoin for `Lob_41_0` with its users.
fn join_lobby(state: &mut AppState) {
    service_event(
        state,
        WolServiceEvent::Joined(Ok(WolJoined {
            channel: lobbies()[0].clone(),
            members: vec![
                member("Hotel", MEMBER_OPERATOR, ""),
                member(NICKNAME, 0, ""),
                member("Echo", 0, "WW"),
                member("Foxtrot", MEMBER_VOICE, ""),
            ],
        })),
    );
}

fn wol_setup_settled(state: &AppState, host: bool) -> bool {
    MoviesCapture::slide_settled(
        state,
        ShellSlideKind::NetworkSetup(crate::ui::multiplayer_setup::SetupDialog {
            host,
            flavor: crate::ui::multiplayer_setup::SetupFlavor::Wol,
        }),
    ) && state
        .frontend
        .multiplayer_setup
        .as_ref()
        .is_some_and(|ui| ui.statics.is_terminal())
}

/// A press on one `0x2BC` checkbox's 18x18 icon.
fn options_checkbox(state: &mut AppState, id: u16) -> Result<()> {
    let layout = crate::app::shell_wol::options::layout(state);
    let rect = layout.checkbox_rect(id).context("0x2BC checkbox")?;
    welcome_click(
        state,
        crate::ui::skirmish_shell::checkbox_icon_rect(rect, layout.page.scale),
    );
    Ok(())
}

fn options_button(state: &mut AppState, id: u16) -> Result<()> {
    let layout = crate::app::shell_wol::options::layout(state);
    welcome_click(state, layout.page.button_rect(id).context("0x2BC button")?);
    Ok(())
}

fn ra2md_path(state: &AppState) -> Result<std::path::PathBuf> {
    Ok(state
        .platform
        .game_config
        .as_ref()
        .context("capture game config")?
        .paths
        .ra2_dir
        .join("RA2MD.INI"))
}

fn read_ra2md(state: &AppState) -> Result<Option<Vec<u8>>> {
    let path = ra2md_path(state)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn restore_ra2md(state: &AppState, bytes: Option<Vec<u8>>) -> Result<()> {
    let path = ra2md_path(state)?;
    match bytes {
        Some(bytes) => std::fs::write(&path, bytes),
        None => std::fs::remove_file(&path),
    }
    .with_context(|| format!("restore {}", path.display()))
}

fn lobby_button(state: &mut AppState, layout: &lobby_ui::WolLobbyLayout, id: u16) -> Result<()> {
    welcome_click(state, layout.page.button_rect(id).context("0x113 button")?);
    Ok(())
}

/// A click on one visible game row.
fn game_row_click(state: &mut AppState, row: usize) -> Result<()> {
    let layout = crate::app::shell_wol::lobby::layout(state);
    let lobby = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.lobby())
        .context("the lobby runner")?;
    let geometry =
        lobby
            .ui
            .game_list
            .geometry(layout.games, lobby.ui.game_rows.len(), layout.page.scale);
    let rect = geometry.row(row - lobby.ui.game_list.top_index);
    welcome_click(state, rect);
    Ok(())
}

/// Three IRC records; the preferred-server draw left record 1 selected.
fn server_list() -> WolServerList {
    WolServerList {
        servers: ["Pacific", "Westwood", "Europe"]
            .into_iter()
            .map(|name| WolServer {
                name: format!("irc.westwood.com:{name}"),
            })
            .collect(),
        selected: 1,
    }
}

fn button(layout: &crate::ui::wol_login::WolLoginLayout, id: u16) -> Result<RectPx> {
    layout.page.button_rect(id).context("0x10F button")
}

/// A press and release through the window handlers' WOL routing; like the
/// native helper, the pointer then returns to the neutral point.
fn welcome_click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    App::handle_wol_mouse_move(state);
    App::handle_wol_mouse_down(state);
    App::handle_wol_mouse_up(state);
    MoviesCapture::restore_neutral_pointer(state);
}

fn login_click(state: &mut AppState, rect: RectPx) {
    // The combo face's left part is its NewEdit, clear of the arrow.
    let rect = RectPx::new(rect.x, rect.y, rect.w / 2, rect.h);
    welcome_click(state, rect);
}

fn setup_view(state: &AppState) -> Result<crate::ui::multiplayer_setup::SetupView> {
    setup::view(state).context("C2/C9 view")
}

fn setup_requests(state: &mut AppState) -> Vec<NetworkSetupRequest> {
    state
        .frontend
        .network_setup_protocol
        .as_mut()
        .map(|protocol| protocol.take_service_requests())
        .unwrap_or_default()
}

fn setup_chat_has(state: &AppState, text: &str) -> Result<bool> {
    Ok(state
        .frontend
        .multiplayer_setup
        .as_ref()
        .context("C2/C9")?
        .chat
        .lines
        .iter()
        .any(|line| String::from_utf16_lossy(&line.text) == text))
}

fn setup_click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    crate::app::input::multiplayer_setup::cursor_moved(state);
    crate::app::input::multiplayer_setup::mouse(state, winit::event::MouseButton::Left, true);
    crate::app::input::multiplayer_setup::mouse(state, winit::event::MouseButton::Left, false);
}

fn setup_button(state: &mut AppState, id: u16) -> Result<()> {
    let rect = crate::app::input::multiplayer_setup::layout(state)
        .page
        .buttons
        .into_iter()
        .find(|button| button.id == id)
        .context("setup button")?
        .rect;
    setup_click(state, rect);
    Ok(())
}
