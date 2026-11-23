//! The WOL capture's per-frame driver: `after_present` advances the
//! checkpoint's phase each presented frame, answering the visit's service
//! requests and dispatching to the page-specific steps in sibling modules.

use super::*;

impl WolCapture {
    pub(in crate::app::diagnostics::shell_capture::movies) fn after_present(
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
                    && steady_main_menu_capture_ready_for(
                        MainMenuCaptureSnapshot::from_state(state).scaled_as_reference(),
                        // The narrow summary capture uses the 640 shell movie.
                        matches!(self.target, WolTarget::ModernLobbySummary { .. }),
                    )? =>
            {
                if self.target == WolTarget::BuddyFromMyInfo {
                    state.frontend.wol_test_credentials = vec![WolCredential {
                        slot: 1,
                        nickname: NICKNAME.into(),
                        password: PASSWORD.into(),
                    }];
                }
                if !self.target.online_login() {
                    write_wolinfo(state, self.target)?;
                }
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellSlideKind::MainMenu,
                    move |state| {
                        crate::app::App::handle_main_menu_shell_action(
                            state,
                            crate::ui::main_menu_shell::MainMenuShellAction::WwOnline,
                        )
                    },
                );
                self.event(frame, "0xE2 Internet");
                // Internet opens VERA's guest/account choice directly.
                self.phase = match self.target {
                    target if target.online_login() => WolPhase::Login,
                    WolTarget::ModernLobby
                    | WolTarget::ModernLobbySummary { .. }
                    | WolTarget::ModernCreate
                    | WolTarget::ModernPlayerCard
                    | WolTarget::ModernChangeCountry
                    | WolTarget::ModernRoomOptions
                    | WolTarget::LobbyVisit
                    | WolTarget::ModernTeams
                    | WolTarget::ModernSpectators
                    | WolTarget::ModernOptions
                    | WolTarget::ModernOptionsTranslation
                    | WolTarget::ModernOptionsTranslationList => WolPhase::ServiceLogin,
                    _ => WolPhase::Welcome,
                };
            }
            WolPhase::Welcome
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                if self.target == WolTarget::WelcomeProfile {
                    let texts = &App::wol_welcome_texts(state);
                    let text = |id: u16| {
                        texts
                            .iter()
                            .find(|(text_id, _)| *text_id == id)
                            .map(|(_, text)| text.as_str())
                    };
                    ensure!(
                        text(0x06EF).is_some_and(|heading| heading.contains(NICKNAME))
                            && [0x06F4, 0x06F5, 0x06F0, 0x06F1, 0x0701].map(text)
                                == [Some("17"), Some("1234"), Some("21"), Some("9"), Some("1")]
                            && text(0x0797).is_some_and(|line| line.contains("10/19/26")),
                        "LastValid's persona did not fill 0x10E: {texts:?}"
                    );
                    self.event(frame, json!({"0x10E profile": texts}));
                    self.hover_settle(state);
                    return Ok(());
                }
                if self.target.my_info() {
                    let rect = crate::app::frontend::shell_layout::wol_welcome_layout(state)
                        .page
                        .button_rect(crate::ui::wol_shell::MY_INFORMATION)
                        .context("My Information")?;
                    welcome_click(state, rect);
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "My Information did not tear 0x10E down"
                    );
                    self.event(frame, "0x10E My Information (controlled service)");
                    self.phase = WolPhase::MyInfoOpening;
                    return Ok(());
                }
                if self.target.buddy() && self.target != WolTarget::BuddyFromLobby {
                    let (id, next) = if self.target == WolTarget::BuddyFromMyInfo {
                        (crate::ui::wol_shell::MY_INFORMATION, WolPhase::BuddyMyInfo)
                    } else {
                        (crate::ui::wol_shell::PLAY_BUDDY, WolPhase::ServiceLogin)
                    };
                    let rect = crate::app::frontend::shell_layout::wol_welcome_layout(state)
                        .page
                        .button_rect(id)
                        .context("welcome button")?;
                    welcome_click(state, rect);
                    self.event(frame, json!({"0x10E button (test service)": id}));
                    self.phase = next;
                    return Ok(());
                }
                let (mode_button, mode_name) = if self.target.quick_coop() {
                    (crate::ui::wol_shell::QUICK_COOP, "Quick Co-op")
                } else if self.target.quick_match() {
                    (crate::ui::wol_shell::QUICK_MATCH, "Quick Match")
                } else {
                    (crate::ui::wol_shell::CUSTOM_MATCH, "Custom Match")
                };
                if self.target.test_service() {
                    let rect = crate::app::frontend::shell_layout::wol_welcome_layout(state)
                        .page
                        .button_rect(mode_button)
                        .context("mode button")?;
                    welcome_click(state, rect);
                    self.event(frame, format!("0x10E {mode_name} (test service)"));
                    self.phase = WolPhase::ServiceLogin;
                    return Ok(());
                }
                let rect = crate::app::frontend::shell_layout::wol_welcome_layout(state)
                    .page
                    .button_rect(mode_button)
                    .context("mode button")?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "{mode_name} did not tear 0x10E down"
                );
                self.event(frame, format!("0x10E {mode_name} (controlled service)"));
                self.phase = WolPhase::Custom;
            }
            WolPhase::Custom if state.frontend.shell_exit.is_none() => {
                let requests = self.requests(state, frame)?;
                // After a match WOL_Main applies [WOnline] twice (0x0077B31D,
                // 0x0077B402) before the runner logs in again.
                let applies = if self.exit_consumed { 4 } else { 0 };
                ensure!(
                    requests.len() == applies + 1
                        && requests[..applies].iter().all(|request| matches!(
                            request,
                            WolServiceRequest::SetLangFilter(_)
                                | WolServiceRequest::SetFindPage { .. }
                        ))
                        && requests[applies] == WolServiceRequest::EnsureApi,
                    "the flow did not start with the WOLAPI object: {requests:?}"
                );
                let mode = if self.target.quick_coop() {
                    crate::ui::wol_shell::WolMode::QuickCoop
                } else if self.target.quick_match() {
                    crate::ui::wol_shell::WolMode::QuickMatch
                } else {
                    crate::ui::wol_shell::WolMode::CustomMatch
                };
                ensure!(
                    state.frontend.wol.as_ref().and_then(|visit| visit.mode()) == Some(mode),
                    "the welcome did not write mode {}",
                    mode.native()
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
                    target if target.auto_login() => {
                        service_event(
                            state,
                            WolServiceEvent::Credentials(vec![WolCredential {
                                slot: 1,
                                nickname: NICKNAME.into(),
                                password: PASSWORD.into(),
                            }]),
                        );
                        service_event(state, WolServiceEvent::NetStatus(SERVER_STATUS_READY));
                        service_event(state, WolServiceEvent::ServerList(Ok(server_list())));
                        let requests = self.requests(state, frame)?;
                        ensure!(
                            state
                                .frontend
                                .wol
                                .as_ref()
                                .is_some_and(|visit| visit.login().is_none())
                                && requests
                                    == [WolServiceRequest::Connect {
                                        server: server_list().servers[1].clone(),
                                        nickname: NICKNAME.into(),
                                        password: PASSWORD.into(),
                                        changed: false,
                                        remember: false,
                                        generated_guest: false,
                                    }],
                            "AutoLogin did not connect the stored account to the persona's server: {requests:?}"
                        );
                        self.event(frame, "AutoLogin: stored account, persona server Westwood");
                        if target == WolTarget::AutoLoginSilent {
                            service_event(
                                state,
                                WolServiceEvent::Login(Ok(WolLoginAccepted {
                                    motd: "Controlled message of the day".into(),
                                    lobby_visits: true,
                                    campaign_rooms: false,
                                })),
                            );
                            self.event(frame, "controlled login accepted");
                            self.after_login(state, frame)?;
                        } else {
                            service_event(state, WolServiceEvent::Login(Err(BAD_PASSWORD)));
                            self.event(frame, "controlled login rejected 0x80040065");
                            self.phase = WolPhase::AutoLoginBox;
                        }
                    }
                    _ => {
                        service_event(state, WolServiceEvent::Credentials(Vec::new()));
                        service_event(state, WolServiceEvent::NetStatus(SERVER_STATUS_READY));
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
                let layout = crate::ui::shell::modal::template_on(
                    crate::ui::shell::modal::WAIT_TEMPLATE,
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                    crate::ui::shell::geom::RETAIL_SCALE,
                );
                welcome_click(state, layout.button(0));
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
                    target if target.online_login() => {
                        // Captures read nothing: the page shows fixed counts.
                        state.frontend.login_info.set(Some(
                            crate::app::shell_wol::login_info::LobbyStats {
                                online: 12,
                                games_24h: 30,
                            },
                        ));
                        ensure!(
                            login_mut(state)?.0.choosing_entry(),
                            "the online entry chooser did not open"
                        );
                        let choice = layout.entry_choice().context("the two entry buttons")?;
                        self.event(frame, "the online entry chooser");
                        if target == WolTarget::ModernLoginPage {
                            MoviesCapture::restore_neutral_pointer(state);
                            self.settle();
                            return Ok(());
                        }
                        // These are visual-only captures. Play as Guest joins
                        // the live lobby immediately, so they use the account form.
                        welcome_click(state, choice.account);
                        let layout = crate::app::shell_wol::login_layout(state);
                        if matches!(target, WolTarget::ModernUsername | WolTarget::ModernLogin) {
                            let remember = layout
                                .rect_of(crate::ui::wol_account::FORM_REMEMBER)
                                .context("the online sign-in form")?;
                            let typed: &[&str] = if target == WolTarget::ModernLogin {
                                &["Player", "s3cret-pw"]
                            } else {
                                &["Player"]
                            };
                            account_type(state, typed);
                            if target == WolTarget::ModernLogin {
                                welcome_click(
                                    state,
                                    crate::ui::skirmish_shell::checkbox_icon_rect(
                                        remember,
                                        layout.page.scale,
                                    ),
                                );
                            }
                            let (login, ui) = login_mut(state)?;
                            ensure!(
                                login.online_name(ui) == "Player"
                                    && login.button_enabled(ui, crate::ui::wol_login::PLAY)
                                        == (target == WolTarget::ModernLogin)
                                    && login.form.as_ref().and_then(|form| form.remember(ui))
                                        == Some(target == WolTarget::ModernLogin),
                                "the sign-in form has the wrong submit or remember state"
                            );
                            self.event(frame, "the sign-in form filled in");
                        } else {
                            self.account_popup(state, frame, target, &layout)?;
                        }
                        MoviesCapture::restore_neutral_pointer(state);
                        self.settle();
                        return Ok(());
                    }
                    WolTarget::LoginSteady => {
                        MoviesCapture::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.settle();
                        return Ok(());
                    }
                    // The post-match re-login's 0x10F.
                    WolTarget::AbortRelogin if self.exit_consumed => {
                        MoviesCapture::restore_neutral_pointer(state);
                        App::handle_wol_mouse_move(state);
                        self.settle();
                        return Ok(());
                    }
                    WolTarget::LoginLastValid | WolTarget::AutoLoginFailed => {
                        login_mut(state)?;
                        let ui = &state.frontend.shell_controller;
                        ensure!(
                            ui.text(LOGIN_DIALOG, crate::ui::wol_login::NICKNAME) == NICKNAME
                                && info_file(state)?.auto_login().is_empty(),
                            "0x10F did not take the persona nickname with AutoLogin cleared"
                        );
                        self.event(frame, "0x10F settled with the persona nickname");
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
                login_click(
                    state,
                    layout
                        .rect_of(crate::ui::wol_login::NICKNAME_DROP)
                        .unwrap_or_default(),
                );
                App::handle_wol_key(state, None, true, Some(NICKNAME));
                login_click(
                    state,
                    layout
                        .rect_of(crate::ui::wol_login::PASSWORD)
                        .unwrap_or_default(),
                );
                App::handle_wol_key(state, None, true, Some(PASSWORD));
                login_mut(state)?;
                let enabled = |id| {
                    state
                        .frontend
                        .shell_controller
                        .flag(LOGIN_DIALOG, id, Prop::Enabled, true)
                };
                ensure!(
                    enabled(LOGIN)
                        && enabled(crate::ui::wol_login::REMEMBER)
                        && !enabled(crate::ui::wol_login::FORGET),
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
                            remember: false,
                            generated_guest: false,
                        }],
                    "Login did not connect to the selected record"
                );
                if self.target.lobby() {
                    service_event(
                        state,
                        WolServiceEvent::Login(Ok(WolLoginAccepted {
                            motd: "Controlled message of the day".into(),
                            lobby_visits: true,
                            campaign_rooms: false,
                        })),
                    );
                    self.event(frame, "controlled login accepted");
                    self.after_login(state, frame)?;
                } else {
                    service_event(state, WolServiceEvent::Login(Err(BAD_PASSWORD)));
                    self.event(frame, "controlled login rejected 0x80040065");
                    self.phase = WolPhase::Rejected;
                }
            }
            WolPhase::Rejected => {
                let expected = state.csf_label("TXT_BADPASS", "TXT_BADPASS");
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
                let ok = crate::ui::shell::modal::menu_message_box(
                    1,
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                )
                .button(0);
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
                    && steady_main_menu_capture_ready(
                        MainMenuCaptureSnapshot::from_state(state).scaled_as_reference(),
                    )? =>
            {
                ensure!(state.frontend.wol.is_none(), "the WOL visit survived");
                self.event(frame, "new 0xE2 settled");
                self.settle();
            }
            WolPhase::AutoLoginBox => {
                let expected = state.csf_label("TXT_BADPASS", "TXT_BADPASS");
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == expected),
                    "the silent connect's failure box did not show TXT_BADPASS"
                );
                let ok = crate::ui::shell::modal::menu_message_box(
                    1,
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                )
                .button(0);
                welcome_click(state, ok);
                self.event(frame, "0xD0 OK");
                self.phase = WolPhase::Login;
            }
            WolPhase::PersonaPage
                if (rendered == PresentedShell::WolAutoLoginQuery
                    && Self::page_settled(state, ShellSlideKind::WolAutoLoginQuery))
                    || (rendered == PresentedShell::WolLocale
                        && Self::page_settled(state, ShellSlideKind::WolLocale)) =>
            {
                self.persona_page(state, frame)?;
            }
            WolPhase::PersonaClosing if state.frontend.shell_exit.is_none() => {
                let file = info_file(state)?;
                if let Some(locale) = self.chosen_locale {
                    let persona = file.load_persona(NICKNAME, &|key| state.csf_label(key, key));
                    ensure!(
                        (persona.locale, persona.locale_verified) == (locale, false),
                        "0xFE OK did not save the persona's locale: {persona:?}"
                    );
                } else {
                    ensure!(
                        !file.ask_auto_login(),
                        "Don't Ask and No did not write [Ask] AutoLogin=no"
                    );
                }
                self.event(frame, "persona page closed; wolinfoMD.ini written");
                self.phase = WolPhase::Lobby;
                self.lobby_entry(state, frame)?;
            }
            WolPhase::PageAnswers => self.page_answers(state, frame)?,
            WolPhase::PageWelcome
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                // A current client's page on the welcome: no 0x53F anywhere.
                service_event(
                    state,
                    WolServiceEvent::Page {
                        from: member("Tanya", 0, ""),
                        text: "Are you coming back?".encode_utf16().collect(),
                        ansi: false,
                    },
                );
                let received = crate::app::shell_wol::page::page_received(state)
                    .context("0x11E did not open")?;
                ensure!(
                    received.reply_to == "Tanya" && received.lines.len() == 1,
                    "0x11E did not take the page: {:?}",
                    received.reply_to
                );
                self.event(frame, "page on 0x10E: 0x11E opened");
                let layout = crate::app::shell_wol::page::received_layout(state);
                match self.target {
                    WolTarget::PageReceivedReply => {
                        welcome_click(state, layout.rect_of(0x540).unwrap_or_default());
                        App::handle_wol_key(state, None, true, Some("On my way"));
                        App::handle_wol_key(
                            state,
                            Some(winit::keyboard::KeyCode::Enter),
                            true,
                            None,
                        );
                        self.event(frame, "0x11E reply typed, Enter");
                        self.phase = WolPhase::PageReceivedAnswers;
                    }
                    WolTarget::PageReceivedBack => {
                        welcome_click(state, layout.rect_of(0x686).unwrap_or_default());
                        ensure!(
                            crate::app::shell_wol::page::page_received(state).is_none(),
                            "Back did not close 0x11E"
                        );
                        self.event(frame, "0x11E Back");
                        self.hover_settle(state);
                    }
                    _ => self.hover_settle(state),
                }
            }
            WolPhase::PageReceivedAnswers => {
                let received =
                    crate::app::shell_wol::page::page_received(state).context("0x11E closed")?;
                let texts: Vec<String> = received
                    .lines
                    .iter()
                    .map(|line| String::from_utf16_lossy(&line.text))
                    .collect();
                if texts.contains(&test_page_back("Tanya")) {
                    ensure!(
                        texts.contains(&format!("[{NICKNAME}] On my way"))
                            && received.lines.iter().any(|line| line.color == 0xF0F0),
                        "0x11E did not echo the reply: {texts:?}"
                    );
                    self.event(frame, json!({"0x11E lines": texts}));
                    self.hover_settle(state);
                }
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
                                channel: local_games(state)[0].clone(),
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
                    let [WolServiceRequest::CreateChannel { channel, key, .. }] =
                        requests.as_slice()
                    else {
                        bail!("the game channel was not created: {requests:?}");
                    };
                    ensure!(
                        channel.name == format!("{NICKNAME}'s_game")
                            && channel.kind == GAME_CHANNEL
                            && channel.max_users
                                == crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32
                            && key.is_empty(),
                        "unexpected created channel {channel:?}"
                    );
                    // 0x0077E7A0 at Create: the local hash and the slots
                    // WOL entry left (0x005E8B60): no AI, no observer slot.
                    let hash = crate::app::shell_wol::lobby::local_ini_hash(state);
                    ensure!(
                        hash.is_some()
                            && channel
                                .ex_info
                                .starts_with(&format!("25,{},0,0,0,", hash.unwrap_or(0))),
                        "unexpected created exInfo {:?} for hash {hash:?}",
                        channel.ex_info
                    );
                    self.phase = WolPhase::Creating;
                }
            }
            WolPhase::Joining if state.frontend.shell_exit.is_none() => {
                // The result arrives once 0x113 is hidden.
                let mut channel = local_games(state)[0].clone();
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
                // The joined channel's user list asks the unranked users'
                // ladder and high scores (0x007A7110, 0x007A77CB).
                let listed: Vec<String> = vec!["alpha".into(), NICKNAME.to_ascii_lowercase()];
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [
                            WolServiceRequest::LadderRecords(listed.clone()),
                            WolServiceRequest::HighScores(listed),
                        ],
                    "the joined channel's users were not looked up: {requests:?}"
                );
                self.event(frame, "controlled game join accepted");
                self.phase = WolPhase::Setup;
            }
            WolPhase::Creating => {
                // A clan game's echo lists the guest of the other squad too,
                // so the channel users carry both squads.
                let members = if self.target.clan_match() {
                    vec![
                        squad_member(NICKNAME, MEMBER_OPERATOR, CLAN_SQUAD, "VERA"),
                        squad_member(PEER, 0, RIVAL_SQUAD, "RIVL"),
                    ]
                } else {
                    vec![member(NICKNAME, MEMBER_OPERATOR, "")]
                };
                // The created channel's user list (sink 0x007A7110) asks the
                // ladder and high scores of every unranked user, the host
                // included (0x007A77CB).
                let listed: Vec<String> = members
                    .iter()
                    .map(|member| member.name.to_ascii_lowercase())
                    .collect();
                service_event(state, WolServiceEvent::Created(Ok(members)));
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [
                            WolServiceRequest::LadderRecords(listed.clone()),
                            WolServiceRequest::HighScores(listed),
                        ],
                    "the created channel's users were not looked up: {requests:?}"
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
                    && state.frontend.shell_route == ShellRoute::WolLobby =>
            {
                let mut requests = self.requests(state, frame)?;
                if requests.is_empty() {
                    return Ok(());
                }
                if !self.target.guest() {
                    // C2 Back's own leave (0x7900B8), then the runner's join
                    // (0x788A30) leaves again first (0x788800); the OnPart
                    // says TXT_YOULEFT.
                    ensure!(
                        requests
                            == [
                                WolServiceRequest::LeaveChannel,
                                WolServiceRequest::LeaveChannel
                            ],
                        "C2 Back did not leave the game channel first: {requests:?}"
                    );
                    let game = crate::app::shell_wol::lobby::current_channel(state)
                        .context("the game channel stays until its leave")?;
                    service_event(state, WolServiceEvent::ChannelLeft);
                    let left = crate::assets::csf_file::format_csf(
                        &state.csf_label("TXT_YOULEFT", "TXT_YOULEFT"),
                        &[crate::assets::csf_file::CsfArg::Str(&game.name)],
                    );
                    let shown = Self::lobby(state)?
                        .ui
                        .chat
                        .lines
                        .iter()
                        .any(|line| String::from_utf16_lossy(&line.text) == left);
                    ensure!(shown, "C2 Back's leave did not say {left:?} in 0x113");
                    self.event(frame, json!({"C2 Back left": left}));
                    requests = self.requests(state, frame)?;
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
                    .runtime()
                    .context("installed WOL runtime")?
                    .simulation;
                ensure!(
                    state.match_state.session_mode() == SessionMode::Wol
                        && sim.session().started()
                        && sim.session().game_mode_nonzero(),
                    "the WOL runtime was not admitted"
                );
                // [0xB77DC1]: a line received during the match shows nowhere.
                let lines = |state: &AppState| {
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.lobby())
                        .map(|lobby| lobby.ui.chat.lines.len())
                };
                let before = lines(state);
                service_event(
                    state,
                    WolServiceEvent::Message {
                        from: member("Hotel", MEMBER_OPERATOR, ""),
                        text: "mid-match".encode_utf16().collect(),
                        localized: false,
                        warning: false,
                        private: false,
                        action: false,
                        ansi: false,
                        options: None,
                        sent_at: None,
                    },
                );
                ensure!(
                    lines(state) == before,
                    "a line received during the match was shown"
                );
                self.event(frame, "WOL match installed");
                if self.target.clan_match() {
                    return self.clan_match(state, frame);
                }
                if matches!(
                    self.target,
                    WolTarget::IngamePage | WolTarget::IngamePageCancel
                ) {
                    self.ingame_page(state, frame)?;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::IngameConnectionLost | WolTarget::AbortRelogin
                ) {
                    // NetStatus 0x4012F twice: [0xB77DC2] lets one row through.
                    let rows = |state: &AppState| {
                        state
                            .match_state
                            .match_presentation
                            .message_list
                            .messages()
                            .len()
                    };
                    let before = rows(state);
                    for _ in 0..2 {
                        service_event(
                            state,
                            WolServiceEvent::NetStatus(crate::app::shell_wol::NET_STATUS_LOST),
                        );
                    }
                    let lost = state.csf_label("TXT_WOL_CONNLOST", "TXT_WOL_CONNLOST");
                    let messages = state.match_state.match_presentation.message_list.messages();
                    ensure!(
                        rows(state) == before + 1
                            && messages.last().is_some_and(|row| row.text.contains(&lost)),
                        "the lost connection did not post one TXT_WOL_CONNLOST row"
                    );
                    self.event(frame, "NetStatus 0x4012F twice in the match");
                    if self.target == WolTarget::IngameConnectionLost {
                        self.phase = WolPhase::Settle(2);
                        return Ok(());
                    }
                }
                self.abort_input = Some(match self.target {
                    WolTarget::InternetMenu => {
                        abort_return::AbortReturnInput::begin_menu(state, &mut self.route, frame)?
                    }
                    // A Quick Match game is a tournament game (+0x14 = 1):
                    // Abort opens the Surrender box, whose OK leaves.
                    WolTarget::QmPostmatch | WolTarget::QmPostmatchYes => {
                        abort_return::AbortReturnInput::begin_surrender(
                            state,
                            &mut self.route,
                            frame,
                            true,
                        )?
                    }
                    _ => abort_return::AbortReturnInput::begin(state, &mut self.route, frame)?,
                });
                self.phase = WolPhase::AbortInput;
            }
            WolPhase::ClanLeaving { tick } => {
                let sim = &state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .context("the clan peer's EXIT lost the runtime")?
                    .simulation;
                let peer = clan_peer_house(state)?;
                // Flag_To_Die's zero allow-win timer (0x004FC980) blows the
                // House up on its next update; the defeat follows once its
                // objects are gone.
                let house = &sim.houses()[&peer];
                let waited = sim.session().tick().saturating_sub(tick);
                if !house.left_game() || (!house.is_defeated() && waited < 300) {
                    return Ok(());
                }
                let left = state.csf_label("TXT_LEFT_GAME", "TXT_LEFT_GAME");
                let posted = state
                    .match_state
                    .match_presentation
                    .message_list
                    .messages()
                    .iter()
                    .any(|row| row.text.contains(left.trim_start_matches("%s").trim()));
                let killed = house.is_defeated();
                ensure!(
                    killed && posted,
                    "a tournament game's leaver was not killed ({killed}) or its line \
                     ({posted}) is missing: won {} lost {} tournament {} rows {:?}",
                    house.has_won(),
                    house.has_lost(),
                    sim.session().wol_tournament_game(),
                    state
                        .match_state
                        .match_presentation
                        .message_list
                        .messages()
                        .iter()
                        .map(|row| row.text.clone())
                        .collect::<Vec<_>>()
                );
                self.event(frame, "the clan leaver's House is killed");
                self.phase = WolPhase::Settle(2);
            }
            WolPhase::AbortInput => {
                if self
                    .abort_input
                    .as_mut()
                    .context("WOL abort input")?
                    .after_present(state, &mut self.route, frame)?
                {
                    self.phase = if self.target == WolTarget::ClanSurrender {
                        WolPhase::Settle(2)
                    } else if matches!(self.target, WolTarget::InternetMenu) {
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
                        .runtime()
                        .context("EXIT runtime")?
                        .simulation;
                    ensure!(sim.quit_requested(), "the return began before EXIT ran");
                    self.event(frame, "EXIT consumed");
                    self.exit_consumed = true;
                }
                if self.target.quick_match() {
                    if self.exit_consumed {
                        self.phase = WolPhase::Qm(quick_match::QmPhase::AddBuddyQuery);
                    }
                    return Ok(());
                }
                if self.target == WolTarget::AbortRelogin
                    && state.frontend.shell_route == ShellRoute::Wol
                    && state.frontend.shell_exit.is_none()
                {
                    ensure!(
                        self.exit_consumed
                            && state
                                .frontend
                                .wol
                                .as_ref()
                                .is_some_and(|visit| visit.lobby().is_none()),
                        "the post-match runner did not log in again first"
                    );
                    self.event(frame, "post-match runner logs in again");
                    self.phase = WolPhase::Custom;
                    return Ok(());
                }
                if state.frontend.shell_route == ShellRoute::WolLobby
                    && state.frontend.shell_exit.is_none()
                {
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
                // A stored account selected by LastValid is ready as it is.
                let ready = login_mut(state).is_ok()
                    && state.frontend.shell_controller.flag(
                        LOGIN_DIALOG,
                        LOGIN,
                        Prop::Enabled,
                        true,
                    );
                if !ready {
                    login_click(
                        state,
                        layout
                            .rect_of(crate::ui::wol_login::NICKNAME_DROP)
                            .unwrap_or_default(),
                    );
                    App::handle_wol_key(state, None, true, Some(NICKNAME));
                    login_click(
                        state,
                        layout
                            .rect_of(crate::ui::wol_login::PASSWORD)
                            .unwrap_or_default(),
                    );
                    App::handle_wol_key(state, None, true, Some(PASSWORD));
                }
                login_click(state, button(&layout, LOGIN)?);
                self.event(frame, "0x10F Login (test service)");
                self.phase = if self.target.buddy() && self.target != WolTarget::BuddyFromLobby {
                    WolPhase::BuddyShown
                } else if self.target.quick_match() {
                    // The test service runs the whole match: lobby, request,
                    // Start, the guest's join and accept, OnGameStart.
                    WolPhase::Mismatch
                } else {
                    WolPhase::ServiceLobby
                };
            }
            WolPhase::ServiceLobby
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                let lobby = Self::lobby(state)?;
                ensure!(
                    lobby.shown
                        && lobby.ui.lobby_rows.len()
                            == if matches!(
                                self.target,
                                WolTarget::ModernLobby
                                    | WolTarget::ModernLobbySummary { .. }
                                    | WolTarget::ModernCreate
                                    | WolTarget::ModernPlayerCard
                                    | WolTarget::ModernChangeCountry
                                    | WolTarget::ModernRoomOptions
                                    | WolTarget::LobbyVisit
                                    | WolTarget::ModernTeams
                                    | WolTarget::ModernSpectators
                                    | WolTarget::ModernOptions
                                    | WolTarget::ModernOptionsTranslation
                                    | WolTarget::ModernOptionsTranslationList
                            ) {
                                1
                            } else {
                                3
                            }
                        && lobby.ui.game_rows.len() == 4
                        && lobby.ui.player_rows.iter().any(|row| row.name == NICKNAME),
                    "the test service did not fill 0x113"
                );
                self.event(frame, "0x113 settled from the test service");
                if lobby.ui.is_modern() {
                    // The one chat lobby is named nowhere: no list, no join line
                    // (rows start with their time).
                    let joined = crate::assets::csf_file::format_csf(
                        &state.csf_label("TXT_JOINED_S", "TXT_JOINED_S"),
                        &[crate::assets::csf_file::CsfArg::Str("")],
                    );
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    ensure!(
                        layout.control_rect(lobby_ui::LOBBIES).is_none()
                            && !lobby.ui.enabled(lobby_ui::LOBBIES)
                            && !lobby.ui.chat.lines.iter().any(|line| {
                                String::from_utf16_lossy(&line.text).contains(&joined)
                            }),
                        "the modern 0x113 names its chat lobby"
                    );
                }
                if self.target == WolTarget::ModernCreate {
                    let layout = crate::app::shell_wol::lobby::layout(state);
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
                    // VERA's Create Game offers its room's limits, from 30
                    // people with spectators.
                    ensure!(
                        Self::lobby(state)?.ui.room_limits()
                            == Some(crate::ui::room_limits::RoomLimits::default()),
                        "the modern 0xC3 has no Max players or Allow spectators"
                    );
                    self.event(frame, "0xC3 opened (modern)");
                    let create = crate::app::shell_wol::lobby::create_layout(state);
                    set_window_pointer_to_rect_center(state, create.setting_label(create.limit));
                    App::handle_wol_mouse_move(state);
                    ensure!(
                        Self::lobby(state)?
                            .ui
                            .hovered()
                            .map(|id| id & !lobby_ui::LABEL)
                            == Some(lobby_ui::CREATE_LIMIT),
                        "Limit Screen Resolution has no help"
                    );
                    // Ranked game says when the lobby ranks the room's games.
                    set_window_pointer_to_rect_center(state, create.ranked.context("Ranked game")?);
                    App::handle_wol_mouse_move(state);
                    let help = crate::app::shell_wol::lobby::status_help(state);
                    ensure!(
                        help == state.csf_label("VERA:RankedGameHelp", "VERA:RankedGameHelp"),
                        "Ranked game's help read {help:?}"
                    );
                    self.event(frame, json!({"Ranked game help": help}));
                    self.settle();
                    return Ok(());
                }
                if let WolTarget::ModernLobbySummary { watch_teams } = self.target {
                    self.lobby_summary(state, frame, watch_teams)?;
                    return Ok(());
                }
                if self.target == WolTarget::ModernLobby {
                    // Tanya's row names her badge, rank and OS. The players
                    // show in the chat view.
                    show_lobby_chat(state)?;
                    let row = player_row_rect(state, "Tanya")?;
                    set_window_pointer_to_rect_center(state, row);
                    App::handle_wol_mouse_move(state);
                    let help = crate::app::shell_wol::lobby::status_help(state);
                    let label = |key: &str| state.csf_label(key, key);
                    let expected = crate::i18n::fill(
                        &label("VERA:PlayerHelp"),
                        &[
                            ("name", &"Tanya"),
                            ("badge", &label("VERA:LobbyBadgeCommanderInChief")),
                            ("rank", &1),
                            ("os", &"48.51"),
                        ],
                    );
                    ensure!(help == expected, "Tanya's row help read {help:?}");
                    self.event(frame, json!({"hovered player help": help}));
                    self.settle();
                    return Ok(());
                }
                if self.target == WolTarget::ModernPlayerCard {
                    // The test service has sent Tanya's badge, country and
                    // season record.
                    player_row_click(state, "Tanya")?;
                    ensure!(
                        Self::lobby(state)?
                            .ui
                            .card
                            .as_ref()
                            .is_some_and(|card| card.name == "Tanya"),
                        "a click on Tanya did not open her card"
                    );
                    self.event(frame, "player card opened");
                    // Message's help in the box's status line.
                    let card = crate::app::shell_wol::player_card::layout(state);
                    set_window_pointer_to_rect_center(state, card.message);
                    App::handle_wol_mouse_move(state);
                    let help = crate::app::shell_wol::lobby::status_help(state);
                    let expected = crate::i18n::fill(
                        &state.csf_label("VERA:MessageHelp", "VERA:MessageHelp"),
                        &[("name", &"Tanya")],
                    );
                    ensure!(help == expected, "Message's help read {help:?}");
                    // The season row: her OS to the hundredth, and what OS
                    // is under the pointer.
                    let label = |key: &str| state.csf_label(key, key);
                    let rows = crate::app::shell_wol::player_card::view(state)
                        .context("Tanya's card")?
                        .rows;
                    let season = rows
                        .iter()
                        .position(|(caption, _)| *caption == label("VERA:Season"))
                        .context("the season row")?;
                    let shown = crate::i18n::fill(
                        &label("VERA:SeasonRank"),
                        &[("rank", &1), ("os", &"48.51")],
                    );
                    ensure!(rows[season].1 == shown, "the card shows {rows:?}");
                    let explained = label("VERA:SeasonHelp");
                    set_window_pointer_to_rect_center(state, card.rows[season].1);
                    App::handle_wol_mouse_move(state);
                    let help = crate::app::shell_wol::lobby::status_help(state);
                    ensure!(help == explained, "the season row's help read {help:?}");
                    self.event(frame, json!({"season row help": help}));
                    self.settle();
                    return Ok(());
                }
                if self.target == WolTarget::ModernChangeCountry {
                    self.change_country(state, frame)?;
                    self.settle();
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::ModernOptions
                        | WolTarget::ModernOptionsTranslation
                        | WolTarget::ModernOptionsTranslationList
                ) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::OPTIONS)?;
                    ensure!(
                        state.frontend.shell_exit.is_some(),
                        "Options did not slide 0x113 out"
                    );
                    self.event(frame, "0x113 Options");
                    self.phase = WolPhase::OptionsOpening;
                    return Ok(());
                }
                if self.target == WolTarget::BuddyFromLobby && !self.rejoined {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::BUDDY)?;
                    self.event(frame, "0x113 Buddy List");
                    self.phase = WolPhase::BuddyShown;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::TestServiceCreate
                        | WolTarget::ModernRoomOptions
                        | WolTarget::LobbyVisit
                        | WolTarget::ModernTeams
                        | WolTarget::ModernSpectators
                        | WolTarget::SetupPage
                        | WolTarget::GuestChoices
                        | WolTarget::GuestObserver
                        | WolTarget::GuestKick
                        | WolTarget::ScenariosMismatch
                        | WolTarget::MismatchOk
                        | WolTarget::ClanLockout
                        | WolTarget::ClanNoStart
                        | WolTarget::ClanChooseMap
                ) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::CREATE)?;
                    // VERA's online box puts OK a row lower, and grows with
                    // the lobby in a scaled window.
                    let create = crate::app::shell_wol::lobby::create_layout(state);
                    if self.target.clan() {
                        // 0x78DF35: the clan box is enabled by the squad id.
                        ensure!(
                            crate::app::shell_wol::lobby::squad_id(state) == 7,
                            "the stand-in squad did not reach [0xB77DE4]"
                        );
                        welcome_click(
                            state,
                            crate::ui::skirmish_shell::checkbox_icon_rect(
                                create.clan.context("0xC3 clan box")?,
                                crate::ui::shell::geom::RETAIL_SCALE,
                            ),
                        );
                        self.event(frame, "0xC3 clan box (squad 7)");
                    }
                    welcome_click(state, create.ok);
                    self.event(frame, "0xC3 OK (test service)");
                    self.phase = WolPhase::ServiceSetup;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::PageDialog | WolTarget::PageFindStatus | WolTarget::PageUserGone
                ) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::PAGE_BUTTON)?;
                    self.page_input(state, frame, false)?;
                    return Ok(());
                }
                if self.target.page_received() {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::BACK)?;
                    self.event(frame, "0x113 Back");
                    self.phase = WolPhase::PageWelcome;
                    return Ok(());
                }
                if self.target == WolTarget::TestServiceJoin || self.target.guest_service() {
                    if self.rejoined {
                        self.event(frame, "0x113 settled after C9 closed");
                        let lobby = Self::lobby(state)?;
                        let lines: Vec<String> = lobby
                            .ui
                            .chat
                            .lines
                            .iter()
                            .map(|line| String::from_utf16_lossy(&line.text))
                            .collect();
                        if self.target == WolTarget::GuestKicked {
                            let kicked = state.csf_label("TXT_YOURE_KICKED", "TXT_YOURE_KICKED");
                            let expected = crate::assets::csf_file::format_csf(
                                &kicked,
                                &[
                                    crate::assets::csf_file::CsfArg::Str("Tanya's_game"),
                                    crate::assets::csf_file::CsfArg::Str("Tanya"),
                                ],
                            );
                            ensure!(
                                lines.contains(&expected),
                                "the kick line did not reach 0x113: {lines:?}"
                            );
                        }
                        ensure!(lobby.message().is_none(), "C9's close showed a box");
                        self.event(frame, json!({"0x113 transcript": lines}));
                        self.hover_settle(state);
                        return Ok(());
                    }
                    let host = self.target.guest_host();
                    let row = Self::lobby(state)?
                        .ui
                        .game_rows
                        .iter()
                        .position(|row| row.name == format!("{host}'s_game"))
                        .context("the stand-in host's game")?;
                    game_row_click(state, row)?;
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    lobby_button(state, &layout, lobby_ui::JOIN)?;
                    self.event(frame, format!("Join on {host}'s_game (test service)"));
                    self.phase = WolPhase::ServiceSetup;
                    return Ok(());
                }
                self.hover_settle(state);
            }
            WolPhase::ServiceSetup
                if rendered == PresentedShell::NetworkSetup
                    && wol_setup_settled(
                        state,
                        self.target != WolTarget::TestServiceJoin && !self.target.guest_service(),
                    ) =>
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
                if self.target.guest_service() {
                    return self.guest_options(state, frame);
                }
                if self.target == WolTarget::SetupPage {
                    setup_button(state, crate::ui::multiplayer_setup::PAGE)?;
                    self.page_input(state, frame, true)?;
                    return Ok(());
                }
                if matches!(
                    self.target,
                    WolTarget::GuestChoices
                        | WolTarget::GuestObserver
                        | WolTarget::GuestKick
                        | WolTarget::ModernSpectators
                        | WolTarget::ScenariosMismatch
                        | WolTarget::MismatchOk
                        | WolTarget::ClanLockout
                        | WolTarget::ClanNoStart
                        | WolTarget::ClanChooseMap
                ) {
                    return self.service_host(state, frame);
                }
                if self.target == WolTarget::LobbyVisit {
                    self.lobby_visit(state, frame)?;
                }
                if self.target == WolTarget::ModernRoomOptions {
                    use crate::ui::multiplayer_setup::GAME_OPTIONS;
                    setup_button(state, GAME_OPTIONS)?;
                    let crates = setup_view(state)?.checks[2];
                    ensure!(
                        state
                            .frontend
                            .multiplayer_setup
                            .as_ref()
                            .is_some_and(|ui| ui.game_options),
                        "Game Options did not open"
                    );
                    let layout = crate::app::input::multiplayer_setup::layout(state);
                    setup_click(
                        state,
                        crate::ui::skirmish_shell::checkbox_icon_rect(
                            layout.checkboxes[2],
                            layout.page.scale,
                        ),
                    );
                    ensure!(
                        setup_view(state)?.checks[2] != crates,
                        "Crates did not toggle in Game Options"
                    );
                    self.event(frame, "C2 Game Options: Crates toggled");
                    setup_click(
                        state,
                        layout
                            .control_rect(crate::ui::multiplayer_setup::GAME_OPTIONS_PAGE)
                            .context("Game Options page button")?,
                    );
                    ensure!(
                        state
                            .frontend
                            .multiplayer_setup
                            .as_ref()
                            .and_then(|ui| ui.game_options.then_some(ui.game_options_page))
                            == Some(crate::ui::multiplayer_setup::RoomBox::MoreOptions),
                        "Game Options did not turn to VERA's page"
                    );
                    let no_france = crate::ui::multiplayer_setup::ROOM_CHECKBOXES
                        .iter()
                        .position(|id| *id == crate::ui::multiplayer_setup::NO_FRANCE)
                        .expect("No France is a room checkbox");
                    setup_click(
                        state,
                        crate::ui::skirmish_shell::checkbox_icon_rect(
                            layout.room_checkboxes[no_france],
                            layout.page.scale,
                        ),
                    );
                    ensure!(
                        setup_view(state)?.room_checks[no_france],
                        "No France did not toggle in Game Options"
                    );
                    let french = state
                        .rules()
                        .and_then(|rules| rules.trigger_house_type_index("French"))
                        .map(|index| i32::from(index.0));
                    let view = setup_view(state)?;
                    ensure!(
                        row_combo(&view, crate::ui::multiplayer_setup::SetupColumn::Country, 0)
                            .is_some_and(|(_, combo)| {
                                !combo.choices.is_empty()
                                    && combo.choices.iter().all(|item| Some(item.value) != french)
                            }),
                        "France stayed in the host's Side list"
                    );
                    self.event(frame, "C2 Game Options: No France toggled");
                    // Back to the first page, with Tech Level and the
                    // session checkboxes.
                    setup_click(
                        state,
                        layout
                            .control_rect(crate::ui::multiplayer_setup::GAME_OPTIONS_PAGE)
                            .context("Game Options page button")?,
                    );
                    ensure!(
                        state
                            .frontend
                            .multiplayer_setup
                            .as_ref()
                            .and_then(|ui| ui.game_options.then_some(ui.game_options_page))
                            == Some(crate::ui::multiplayer_setup::RoomBox::GameOptions),
                        "Game Options did not turn back to its first page"
                    );
                }
                if self.target == WolTarget::ModernTeams {
                    use crate::app::input::multiplayer_setup as room_input;
                    use crate::ui::multiplayer_setup::{
                        SetupAction, SetupColumn, TEAM_COUNT, TEAM_MENU,
                    };
                    use crate::ui::team_tools::TeamTool;
                    // The host's pick from its Quick Team Setup list.
                    let team_tool = |state: &mut AppState, tool: TeamTool| {
                        crate::app::shell_network_setup::apply_action(
                            state,
                            SetupAction::Select {
                                control: TEAM_MENU,
                                value: tool.code(),
                            },
                        );
                    };
                    // With Teams Off the room is free-for-all and seats the
                    // guest as it comes in; the guest then picks Team B.
                    let natasha = state
                        .match_state
                        .session
                        .network_setup()
                        .and_then(|launch| {
                            launch.humans.iter().find(|human| human.name == "Natasha")
                        })
                        .cloned()
                        .context("Natasha")?;
                    ensure!(!natasha.is_observer(), "Natasha came in watching");
                    let mut slot = natasha.slot;
                    slot.team = crate::skirmish_launch::LaunchTeam::Team(1);
                    crate::app::shell_wol::service_event(
                        state,
                        WolServiceEvent::PlayerChoices {
                            from: "Natasha".into(),
                            slot,
                            observer: false,
                        },
                    );
                    crate::app::shell_network_setup::apply_action(
                        state,
                        SetupAction::Trackbar {
                            control: TEAM_COUNT,
                            value: 2,
                        },
                    );
                    let session = &state.match_state.session;
                    ensure!(
                        session.network_teams() == Some(2)
                            && (0..2).all(|team| session.network_team_players(team) == 1),
                        "Teams did not seat the two players on two teams"
                    );
                    // Online, Teams offers Balance and leaves the Team lists
                    // open: nothing balances the room by itself.
                    let view = setup_view(state)?;
                    let team_list = row_combo(&view, SetupColumn::Team, 0)
                        .is_some_and(|(_, combo)| combo.enabled);
                    let balance = view.combo(TEAM_MENU).is_some_and(|menu| {
                        menu.choices
                            .iter()
                            .any(|item| item.value == TeamTool::Balance.code() && !item.greyed)
                    });
                    ensure!(
                        balance && team_list,
                        "setting Teams online closed the Team lists or Balance"
                    );
                    let captions: Vec<&str> = view
                        .headers
                        .iter()
                        .flatten()
                        .map(|header| header.caption.as_str())
                        .collect();
                    // Team Colors gives the host on Team A YR's Orange and
                    // the guest on Team B its Blue: warm against cool.
                    team_tool(state, TeamTool::Colors);
                    let colors: Vec<(String, Option<u8>)> = state
                        .match_state
                        .session
                        .network_setup()
                        .context("room setup")?
                        .humans
                        .iter()
                        .map(|human| {
                            let random = human.slot.color_random;
                            let color = (!random).then_some(human.slot.color_index);
                            (human.name.clone(), color)
                        })
                        .collect();
                    ensure!(
                        colors == [("Tester".into(), Some(4)), ("Natasha".into(), Some(2))],
                        "Team Colors gave {colors:?}"
                    );
                    // The room told everyone the guest came in, and its host
                    // sets the guest's side as the guest's own choices would.
                    let joined =
                        crate::i18n::tr_fill("VERA:PlayerJoinedRoom", &[("name", &"Natasha")]);
                    ensure!(
                        state.frontend.multiplayer_setup.as_ref().is_some_and(|ui| {
                            ui.chat
                                .lines
                                .iter()
                                .any(|line| String::from_utf16_lossy(&line.text) == joined)
                        }),
                        "the room did not tell {joined:?}"
                    );
                    let guest_row = |state: &AppState| {
                        state
                            .match_state
                            .session
                            .network_setup()
                            .and_then(|launch| {
                                launch
                                    .humans
                                    .iter()
                                    .find(|human| human.name == "Natasha")
                                    .cloned()
                            })
                    };
                    let natasha = guest_row(state).context("Natasha")?;
                    let view = setup_view(state)?;
                    let (_, side) = row_combo(
                        &view,
                        SetupColumn::Country,
                        natasha.raw_slot_index.context("Natasha's row")?,
                    )
                    .context("Natasha's Side")?;
                    let current = side.selected.map(|index| side.choices[index].value);
                    let value = side
                        .choices
                        .iter()
                        .map(|item| item.value)
                        .filter(|value| *value >= 0 && Some(*value) != current)
                        .last()
                        .context("another side")?;
                    ensure!(side.enabled, "the host cannot set the guest's side");
                    crate::app::shell_network_setup::apply_action(
                        state,
                        SetupAction::Select {
                            control: side.id,
                            value,
                        },
                    );
                    let slot = guest_row(state).context("Natasha")?.slot;
                    ensure!(
                        !slot.country_random
                            && slot.country
                                == crate::skirmish_launch::LaunchCountry::from_country_index(
                                    value as u32
                                ),
                        "the host's side {value} for the guest gave {slot:?}"
                    );
                    // The host drags the guest's name onto Team A's box: the
                    // list the press opened closes, and the guest moves.
                    let view = setup_view(state)?;
                    let layout = crate::app::input::multiplayer_setup::layout(state);
                    let (_, name) = row_combo(
                        &view,
                        SetupColumn::Player,
                        natasha.raw_slot_index.context("Natasha's row")?,
                    )
                    .context("Natasha's name")?;
                    let from = layout
                        .control_rect(name.id)
                        .context("Natasha's name cell")?;
                    let team_a = view
                        .headers
                        .iter()
                        .position(|header| {
                            header.as_ref().is_some_and(|header| {
                                header.group == crate::ui::multiplayer_setup::SetupGroup::Team(0)
                            })
                        })
                        .and_then(|row| layout.group_boxes[row])
                        .context("Team A's box")?;
                    set_window_pointer_to_rect_center(state, from);
                    crate::app::input::multiplayer_setup::cursor_moved(state);
                    crate::app::input::multiplayer_setup::mouse(
                        state,
                        winit::event::MouseButton::Left,
                        true,
                    );
                    set_window_pointer_to_rect_center(state, team_a);
                    crate::app::input::multiplayer_setup::cursor_moved(state);
                    crate::app::input::multiplayer_setup::mouse(
                        state,
                        winit::event::MouseButton::Left,
                        false,
                    );
                    let moved = guest_row(state).context("Natasha")?;
                    ensure!(
                        !moved.is_observer()
                            && moved.slot.team == crate::skirmish_launch::LaunchTeam::Team(0)
                            && crate::app::input::multiplayer_setup::open_list(state).is_none()
                            && state.frontend.multiplayer_setup.is_some()
                            && state.frontend.shell_controller.gui().drag().is_none(),
                        "dragging Natasha onto Team A gave {:?}",
                        moved.slot.team
                    );
                    // The room tells who left too; the guest then comes back.
                    crate::app::shell_network_setup::user_left(state, "Natasha", false)?;
                    let left = crate::i18n::tr_fill("VERA:PlayerLeftRoom", &[("name", &"Natasha")]);
                    ensure!(
                        guest_row(state).is_none()
                            && state.frontend.multiplayer_setup.as_ref().is_some_and(|ui| {
                                ui.chat.lines.last().is_some_and(|line| {
                                    String::from_utf16_lossy(&line.text) == left
                                })
                            }),
                        "the room did not tell {left:?}"
                    );
                    crate::app::shell_network_setup::user_joined(state, "Natasha".into())?;
                    ensure!(
                        guest_row(state).is_some_and(|human| human.is_observer()),
                        "Natasha did not come back watching"
                    );
                    // Teams goes up to 15: the Team list then runs from A
                    // to O, and the headers name the teams past D too, down
                    // the roster's scroll, each box five lines high.
                    crate::app::shell_network_setup::apply_action(
                        state,
                        SetupAction::Trackbar {
                            control: TEAM_COUNT,
                            value: 15,
                        },
                    );
                    let wide = setup_view(state)?;
                    let letters: Vec<&str> = row_combo(&wide, SetupColumn::Team, 0)
                        .map(|(_, combo)| {
                            combo
                                .choices
                                .iter()
                                .map(|item| item.text.as_str())
                                .collect()
                        })
                        .unwrap_or_default();
                    let layout = crate::app::input::multiplayer_setup::layout(state);
                    let roster = layout.roster_scroll(&wide).context("the roster's scroll")?;
                    // Release the roster thumb, then use the host's colour list.
                    setup_click(state, roster.thumb.context("roster thumb")?);
                    let (_, color) =
                        row_combo(&wide, SetupColumn::Color, 0).context("host colour")?;
                    let face = layout.control_rect(color.id).context("colour cell")?;
                    setup_click(state, face);
                    let popup = room_input::open_list(state)
                        .context("open colour list after releasing the roster thumb")?
                        .1
                        .popup;
                    let thumb = popup.thumb().context("colour list thumb")?;
                    set_window_pointer_to_rect_center(state, thumb);
                    room_input::cursor_moved(state);
                    room_input::mouse(state, winit::event::MouseButton::Left, true);
                    set_window_pointer_to_rect_center(state, popup.content());
                    room_input::cursor_moved(state);
                    room_input::mouse(state, winit::event::MouseButton::Left, false);
                    ensure!(
                        room_input::open_list(state)
                            .is_some_and(|(id, list)| id == color.id && list.popup.first > 0)
                            && setup_view(state)?.first_row == wide.first_row,
                        "the colour thumb drag failed to scroll just the list"
                    );
                    // Reach the end, then scroll farther with a fractional trackpad delta.
                    ensure!(
                        room_input::wheel(state, -30.0) && room_input::wheel(state, -0.25),
                        "the colour list did not take the wheel at its limit"
                    );
                    ensure!(
                        room_input::open_list(state).is_some_and(|(id, list)| {
                            id == color.id && list.popup.first == popup.total - popup.visible_rows
                        }) && setup_view(state)?.first_row == wide.first_row,
                        "wheeling to the end of the colour list moved the roster or closed the list"
                    );
                    setup_click(state, face);
                    // A notch moves at most 30 places: four reach the foot.
                    for _ in 0..4 {
                        room_input::wheel(state, -100.0);
                    }
                    let foot = setup_view(state)?;
                    for _ in 0..4 {
                        room_input::wheel(state, 100.0);
                    }
                    let wide_captions: Vec<&str> = wide
                        .headers
                        .iter()
                        .chain(&foot.headers)
                        .flatten()
                        .map(|header| header.caption.as_str())
                        .collect();
                    ensure!(
                        state.match_state.session.network_teams() == Some(15)
                            && letters.len() == 15
                            && letters.first() == Some(&"A")
                            && letters.last() == Some(&"O")
                            && wide_captions.first() == Some(&"Team A")
                            && wide_captions.contains(&"Team O"),
                        "Teams 15 gave the Team list {letters:?} and headers {wide_captions:?}"
                    );
                    self.event(
                        frame,
                        json!({
                            "C2 Teams": captions,
                            "Team Colors": colors,
                            "Teams 15": wide_captions,
                        }),
                    );
                    // With Teams set the map does not limit who plays: on a
                    // two-player map six people play on two teams, and Team
                    // Starts gives each team one start its people share.
                    crate::app::shell_network_setup::apply_action(
                        state,
                        SetupAction::Trackbar {
                            control: TEAM_COUNT,
                            value: 2,
                        },
                    );
                    let mode_id = state
                        .match_state
                        .session
                        .network_setup()
                        .context("room setup")?
                        .mode
                        .id;
                    let mode =
                        crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, mode_id)
                            .context("the room's mode")?;
                    let two = state
                        .frontend
                        .scenario_catalog
                        .records()
                        .iter()
                        .position(|map| {
                            map.kind
                                == crate::map::skirmish_scenarios::SkirmishScenarioKind::ConcreteMap
                                && map.player_capacity == 2
                                && crate::map::skirmish_scenarios::is_valid_map(map, mode, false)
                        })
                        .context("a two-player map")?;
                    crate::app::shell_network_setup::commit_map_selection(
                        state, mode_id, two, None,
                    )?;
                    for name in ["Boris", "Ivan", "Sofia", "Lena"] {
                        crate::app::shell_wol::service_event(
                            state,
                            WolServiceEvent::UserJoined(member(name, 0, "")),
                        );
                    }
                    let picks = [
                        ("Natasha", 1, None),
                        ("Boris", 0, Some(2)),
                        ("Ivan", 1, Some(1)),
                        ("Sofia", 0, None),
                        ("Lena", 1, None),
                    ];
                    for (name, team, color) in picks {
                        let human = state
                            .match_state
                            .session
                            .network_setup()
                            .and_then(|launch| {
                                launch.humans.iter().find(|human| human.name == name)
                            })
                            .context("a guest")?;
                        let mut slot = human.slot.clone();
                        slot.team = crate::skirmish_launch::LaunchTeam::Team(team);
                        if let Some(color) = color {
                            slot.color_random = false;
                            slot.color_index = color;
                        }
                        crate::app::shell_wol::service_event(
                            state,
                            WolServiceEvent::PlayerChoices {
                                from: name.into(),
                                slot,
                                observer: false,
                            },
                        );
                    }
                    team_tool(state, TeamTool::Starts);
                    let session = &state.match_state.session;
                    let players: Vec<(String, u8, String)> = session
                        .network_setup()
                        .context("room setup")?
                        .humans
                        .iter()
                        .filter(|human| !human.is_observer())
                        .map(|human| {
                            let team = match human.slot.team {
                                crate::skirmish_launch::LaunchTeam::Team(team) => team,
                                crate::skirmish_launch::LaunchTeam::None => u8::MAX,
                            };
                            let start = format!("{:?}", human.slot.start_position);
                            (human.name.clone(), team, start)
                        })
                        .collect();
                    let start_of = |team: u8| {
                        let starts: std::collections::BTreeSet<&String> = players
                            .iter()
                            .filter(|(_, on, _)| *on == team)
                            .map(|(.., start)| start)
                            .collect();
                        (starts.len() == 1)
                            .then(|| starts.into_iter().next())
                            .flatten()
                    };
                    ensure!(
                        session.network_setup_capacity() == Some(2)
                            && players.len() == 6
                            && (0..2).all(|team| session.network_team_players(team) == 3)
                            && start_of(0).is_some()
                            && start_of(1).is_some()
                            && start_of(0) != start_of(1)
                            && !start_of(0).is_some_and(|start| start.contains("Auto")),
                        "six players on a two-player map gave {players:?}; everyone: {:?}",
                        session.network_setup().map(|launch| launch
                            .humans
                            .iter()
                            .map(|human| (
                                human.name.clone(),
                                human.is_observer(),
                                human.raw_slot_index
                            ))
                            .collect::<Vec<_>>())
                    );
                    self.event(frame, json!({"Two-player map": players}));
                    // The host adds an AI to Team B past the map's seats too,
                    // and Team Starts starts it on Team B's start, beside
                    // its people.
                    let view = setup_view(state)?;
                    let team_b = view
                        .headers
                        .iter()
                        .position(|header| {
                            header.as_ref().is_some_and(|header| {
                                header.group == crate::ui::multiplayer_setup::SetupGroup::Team(1)
                            })
                        })
                        .context("Team B's header")?;
                    crate::app::shell_network_setup::apply_action(
                        state,
                        SetupAction::Select {
                            control: crate::ui::multiplayer_setup::team_add_ai(team_b),
                            value: 4,
                        },
                    );
                    team_tool(state, TeamTool::Starts);
                    // People may still watch with an AI playing: the
                    // Spectators box stays, with its open seat.
                    let watched = setup_view(state)?;
                    ensure!(
                        watched.headers.iter().flatten().any(|header| {
                            header.group == crate::ui::multiplayer_setup::SetupGroup::Spectators
                        }) && state.match_state.session.network_free_row(true).is_some(),
                        "the AI closed the room to spectators"
                    );
                    let launch = state
                        .match_state
                        .session
                        .network_setup()
                        .context("room setup")?;
                    let ai = launch.opponents.first().context("Team B's AI")?;
                    let lena = launch
                        .humans
                        .iter()
                        .find(|human| human.name == "Lena")
                        .context("Lena")?;
                    ensure!(
                        launch.opponents.len() == 1
                            && ai.raw_slot_index >= 2
                            && ai.team == crate::skirmish_launch::LaunchTeam::Team(1)
                            && ai.start_position == lena.slot.start_position
                            && matches!(
                                ai.start_position,
                                crate::skirmish_launch::LaunchStartPosition::Position(_)
                            ),
                        "the AI added to Team B gave row {} {:?} {:?}",
                        ai.raw_slot_index,
                        ai.team,
                        ai.start_position
                    );
                    self.event(
                        frame,
                        json!({"AI on Team B": format!("row {} {:?}", ai.raw_slot_index, ai.start_position)}),
                    );
                    // The host watches with the Spectators' Watch, plays
                    // again with Team A's Join, and watches again.
                    let host = |state: &AppState| {
                        state.match_state.session.network_setup().map(|launch| {
                            (
                                launch.local_player().is_observer(),
                                launch.local_player().slot.team,
                            )
                        })
                    };
                    press_header(state, crate::ui::multiplayer_setup::SetupGroup::Spectators)?;
                    ensure!(
                        host(state).is_some_and(|(watching, _)| watching),
                        "the host's Watch gave {:?}",
                        host(state)
                    );
                    press_header(state, crate::ui::multiplayer_setup::SetupGroup::Team(0))?;
                    ensure!(
                        host(state) == Some((false, crate::skirmish_launch::LaunchTeam::Team(0))),
                        "the host's Join gave {:?}",
                        host(state)
                    );
                    press_header(state, crate::ui::multiplayer_setup::SetupGroup::Spectators)?;
                    ensure!(
                        host(state).is_some_and(|(watching, _)| watching),
                        "the host's second Watch gave {:?}",
                        host(state)
                    );
                    self.event(frame, json!({"Host": "watches, plays and watches again"}));
                    // The watching host's orange and the random seats no
                    // longer vote. Boris supplies A's blue, Ivan B's red.
                    let view = setup_view(state)?;
                    for (team, color) in [(0, 2), (1, 1)] {
                        ensure!(
                            view.headers.iter().flatten().any(|header| header.group
                                == crate::ui::multiplayer_setup::SetupGroup::Team(team)
                                && header.member_colors == [color]),
                            "the team caption did not follow its seated players' colours"
                        );
                    }
                }
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
            WolPhase::TranslationReturn
                if rendered == PresentedShell::WolLobby
                    && Self::page_settled(state, ShellSlideKind::WolLobby) =>
            {
                self.translation_saved(state, frame)?;
            }
            WolPhase::TranslationReopened
                if rendered == PresentedShell::WolOptions
                    && Self::page_settled(state, ShellSlideKind::WolOptions) =>
            {
                self.translation_reopened(state, frame)?;
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
                let checked = crate::app::shell_wol::options::shown(state)
                    .map(|(preferences, _)| preferences.checks());
                ensure!(
                    checked == Some([false, true, true, true, true, true, true, true, false]),
                    "the reopened 0x2BC does not show the saved values: {checked:?}"
                );
                self.event(frame, json!({"0x2BC reopened": checked}));
                // The capture's isolated profile goes back as it was.
                let (bytes, _) = self.options_before.clone().context("saved VERA.INI")?;
                restore_settings(state, bytes)?;
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
            WolPhase::BuddyMyInfo
                if rendered == PresentedShell::WolMyInformation
                    && Self::page_settled(state, ShellSlideKind::WolMyInformation) =>
            {
                self.buddy_my_info(state, frame)?;
            }
            WolPhase::BuddyShown
                if rendered == PresentedShell::WolBuddyList
                    && Self::page_settled(state, ShellSlideKind::WolBuddyList) =>
            {
                self.buddy_shown(state, frame)?;
            }
            WolPhase::Mismatch => {
                let Some(verify) = crate::app::shell_network_setup::verify(state) else {
                    return Ok(());
                };
                let Some(message) = &verify.mismatch else {
                    return Ok(());
                };
                let expected =
                    state.csf_label("TXT_SCENARIOS_DO_NOT_MATCH", "TXT_SCENARIOS_DO_NOT_MATCH");
                ensure!(
                    message.body == expected
                        && verify.names == [NICKNAME.to_owned(), SERVICE_GUEST.to_owned()]
                        && verify.checked()[..2] == [true, true],
                    "the mismatch box over 0x130: {verify:?}"
                );
                self.event(frame, json!({"0x130": verify.names, "box": message.body}));
                if self.target.quick_match() {
                    ensure!(
                        crate::app::shell_wol::quick_match::dialog(state).is_none()
                            && state
                                .match_state
                                .session
                                .network_setup()
                                .is_some_and(|launch| launch.humans.len() == 2
                                    && launch.local_player_index == 0),
                        "the Quick Match start did not close 0x117 with the user hosting"
                    );
                }
                if matches!(
                    self.target,
                    WolTarget::MismatchOk | WolTarget::QmTestServiceOk
                ) {
                    let (width, height) = (
                        state.renderer.gpu.config.width as i32,
                        state.renderer.gpu.config.height as i32,
                    );
                    let ok = crate::ui::shell::modal::menu_message_box(1, width, height).button(0);
                    if self.target.quick_match() {
                        // No setup dialog: 0x130 takes the WOL input routes.
                        welcome_click(state, ok);
                    } else {
                        setup_click(state, ok);
                    }
                    self.event(frame, "mismatch box OK");
                    self.phase = WolPhase::MismatchClosed;
                    return Ok(());
                }
                if self.target.quick_match() {
                    self.hover_settle(state);
                    return Ok(());
                }
                MoviesCapture::restore_neutral_pointer(state);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                self.settle();
            }
            WolPhase::MismatchClosed
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let visit = state.frontend.wol.as_ref().context("WOL visit")?;
                ensure!(
                    !visit.authenticated()
                        && visit.lobby().is_none()
                        && state.match_state.session.network_setup().is_none(),
                    "the abort did not log out and close the runner"
                );
                self.event(frame, "0x10E settled after the abort, logged out");
                self.hover_settle(state);
            }
            WolPhase::GuestKicked => {
                let gone = state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| {
                        !launch
                            .humans
                            .iter()
                            .any(|human| human.name == SERVICE_GUEST)
                    });
                if !gone {
                    return Ok(());
                }
                let channel = crate::app::shell_wol::lobby::current_channel(state)
                    .context("C2's game channel")?;
                let line = crate::assets::csf_file::format_csf(
                    &state.csf_label("TXT_USER_KICKED", "TXT_USER_KICKED"),
                    &[
                        crate::assets::csf_file::CsfArg::Str(SERVICE_GUEST),
                        crate::assets::csf_file::CsfArg::Str(&channel.name),
                        crate::assets::csf_file::CsfArg::Str(NICKNAME),
                    ],
                );
                ensure!(
                    setup_chat_has(state, &line)?,
                    "the kick echo did not say {line:?} in C2"
                );
                self.event(frame, json!({"kick echo": line}));
                MoviesCapture::restore_neutral_pointer(state);
                crate::app::input::multiplayer_setup::cursor_moved(state);
                self.settle();
            }
            WolPhase::BuddyStored => {
                let listed = |name: &str| {
                    state
                        .frontend
                        .wol_test_buddies
                        .iter()
                        .any(|known| known == name)
                };
                let done = if self.target == WolTarget::BuddyAdded {
                    listed("Natasha") && state.frontend.wol_buddies.status("Natasha") == Some(4)
                } else {
                    !listed("Ghost")
                };
                if done {
                    let rows = Self::buddy_rows(state)?;
                    self.event(frame, json!({"service buddy list": state.frontend.wol_test_buddies, "rows": rows}));
                    self.hover_settle(state);
                }
            }
            WolPhase::BuddyCreatePopup
                if state.frontend.shell_exit.is_none()
                    && Self::lobby(state).is_ok_and(|lobby| lobby.ui.popup.is_some()) =>
            {
                let (width, height) = (
                    state.renderer.gpu.config.width as i32,
                    state.renderer.gpu.config.height as i32,
                );
                let ok = crate::ui::shell::modal::wol_create_game_layout(width, height, false).ok;
                welcome_click(state, ok);
                self.event(frame, "0xC3 OK over the backdrop");
                self.phase = WolPhase::BuddySetup;
            }
            WolPhase::BuddySetup
                if rendered == PresentedShell::NetworkSetup
                    && wol_setup_settled(state, self.buddy_hosts()) =>
            {
                self.buddy_setup(state, frame)?;
            }
            WolPhase::BuddyReturn if self.buddy_returned(state, rendered) => {
                self.buddy_return(state, frame)?;
            }
            WolPhase::MyInfoOpening if state.frontend.shell_exit.is_none() => {
                self.my_info_open(state, frame)?;
            }
            WolPhase::MyInfoShown
                if rendered == PresentedShell::WolMyInformation
                    && Self::page_settled(state, ShellSlideKind::WolMyInformation) =>
            {
                self.my_info_act(state, frame)?;
            }
            WolPhase::MyInfoBack
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let texts = &App::wol_welcome_texts(state);
                ensure!(
                    texts
                        .iter()
                        .any(|(id, text)| *id == 0x06EF && text.contains("Alpha")),
                    "0x10E did not show the nickname 0x114 selected: {texts:?}"
                );
                self.event(frame, json!({"0x10E profile": texts}));
                self.hover_settle(state);
            }
            WolPhase::Qm(phase) => self.qm_after_present(state, rendered, frame, phase)?,
            WolPhase::Settle(remaining) if remaining > 0 => {
                self.phase = WolPhase::Settle(remaining - 1);
            }
            _ => {}
        }
        Ok(())
    }
}
