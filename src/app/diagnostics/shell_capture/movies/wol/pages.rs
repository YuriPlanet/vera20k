//! The online login flows: the continuation after an accepted login, the
//! persona and in-game pages with their answers and input, the guest's
//! loading completion, and the account popups over the login page.

use super::*;

impl WolCapture {
    /// `0x007864C0`'s continuation after an accepted login: the locale
    /// request answered (2, or unknown for the locale targets), then
    /// `0xFE`, `0x112` or the runner.
    pub(super) fn after_login(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::RequestLocales {
                    names: vec![NICKNAME.into()],
                }],
            "the login did not request the user's locale: {requests:?}"
        );
        let locale = if self.target.locale_unknown() { 0 } else { 2 };
        service_event(
            state,
            WolServiceEvent::UserLocales(Ok(vec![(NICKNAME.into(), locale)])),
        );
        self.event(frame, format!("controlled locale {locale}"));
        self.phase = if self.target.locale_unknown() {
            let requests = self.requests(state, frame)?;
            ensure!(
                requests == [WolServiceRequest::LocaleCount],
                "0xFE did not ask for the locale count: {requests:?}"
            );
            service_event(state, WolServiceEvent::LocaleCount(38));
            WolPhase::PersonaPage
        } else if self.target.asks_auto_login() {
            WolPhase::PersonaPage
        } else if self.target.quick_match() {
            WolPhase::Qm(quick_match::QmPhase::Runner)
        } else {
            WolPhase::Lobby
        };
        Ok(())
    }

    /// `0x112` or `0xFE` settled: drive the target's answer.
    pub(super) fn persona_page(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::WolPage;
        use crate::ui::wol_persona::{ACCEPT, DECLINE};
        let layout = crate::app::shell_wol::persona::layout(state).context("persona layout")?;
        match self.target {
            WolTarget::AutoLoginQuery | WolTarget::LocaleQuery => {
                self.event(frame, "persona page settled");
                self.hover_settle(state);
            }
            WolTarget::AutoLoginDeclined => {
                welcome_click(
                    state,
                    crate::ui::skirmish_shell::checkbox_icon_rect(
                        layout
                            .rect_of(crate::ui::wol_persona::DO_NOT_ASK)
                            .context("0x112 Don't Ask")?,
                        layout.page.scale,
                    ),
                );
                welcome_click(state, layout.page.button_rect(DECLINE).context("0x112 No")?);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Don't Ask and No did not close 0x112"
                );
                self.event(frame, "0x112 Don't Ask, No");
                self.phase = WolPhase::PersonaClosing;
            }
            WolTarget::LocaleChosen => {
                let Some(WolPage::Locale(page)) =
                    state.frontend.wol.as_ref().map(|visit| &visit.page)
                else {
                    bail!("0xFE is not open");
                };
                let (geometry, _) =
                    crate::app::shell_wol::persona::list_view(state).context("0xFE list")?;
                let locale = page.rows.get(2).context("0xFE rows")?.locale;
                welcome_click(state, geometry.row(2));
                welcome_click(state, layout.page.button_rect(ACCEPT).context("0xFE OK")?);
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "a row and OK did not close 0xFE"
                );
                self.event(frame, json!({"0xFE OK": locale}));
                self.chosen_locale = Some(locale);
                self.phase = WolPhase::PersonaClosing;
            }
            target => bail!("{target:?} has no persona page"),
        }
        Ok(())
    }

    /// `0xCF` opened over `0x113` or C2: the target's name and text through
    /// the real input routes.
    /// The in-match page path: an ANSI page posts its two rows and arms the
    /// reply window; PageUser (U) inside it opens `0xCF` with the pager and
    /// the caret in the text; a reply goes out and the stand-in answers.
    pub(super) fn ingame_page(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::page;
        use crate::ui::pause_menu::InGameMenuState;
        let rows = |state: &AppState| -> Vec<String> {
            state
                .match_state
                .match_presentation
                .message_list
                .messages()
                .iter()
                .map(|row| row.text.clone())
                .collect()
        };
        let before = rows(state).len();
        service_event(
            state,
            WolServiceEvent::Page {
                from: member("Tanya", MEMBER_VOICE, ""),
                text: "where are you?".encode_utf16().collect(),
                ansi: true,
            },
        );
        let hint = state.csf_label("TXT_HOW_TO_REPLY", "TXT_HOW_TO_REPLY");
        let after = rows(state);
        ensure!(
            after.len() == before + 2
                && after[before].contains("(Tanya) where are you?")
                && after[before + 1].contains(&hint),
            "the in-match page did not post its two rows: {after:?}"
        );
        self.event(frame, json!({"in-match page rows": &after[before..]}));
        // U as the window handler hands it to the in-game key edge.
        let u = winit::keyboard::Key::Character("u".into());
        crate::app::input::keyboard::in_game_key_edge(
            state,
            crate::app::input::keyboard::InGameKeyEdge {
                physical: winit::keyboard::KeyCode::KeyU,
                logical: &u,
                unmodified: &u,
                location: winit::keyboard::KeyLocation::Standard,
                state: winit::event::ElementState::Pressed,
                repeat: false,
                text: Some("u"),
            },
            false,
        );
        let opened = page::page_dialog(state).is_some()
            && crate::app::shell_wol::leaf::text(
                state,
                crate::ui::wol_page::PAGE_DIALOG,
                crate::ui::wol_page::NAME,
            ) == "Tanya"
            && crate::app::shell_wol::leaf::focused(state) == Some(crate::ui::wol_page::TEXT);
        ensure!(
            state.match_state.match_presentation.in_game_menu == InGameMenuState::Page && opened,
            "PageUser did not open 0xCF with the pager prefilled"
        );
        self.event(frame, "PageUser opened 0xCF, prefilled");
        page::key_press(state, None, Some("on my way"));
        page::key_press(state, Some(winit::keyboard::KeyCode::Enter), None);
        self.page_tick = state
            .match_state
            .sim_runtime
            .runtime()
            .map(|runtime| runtime.simulation.session().tick());
        // The controlled service answers the request it was sent.
        let requests = self.requests(state, frame)?;
        let paged = requests.iter().any(|request| {
            matches!(request, WolServiceRequest::Chat {
                line: WolChatLine::Page { name, text },
                ..
            } if String::from_utf16_lossy(name) == "Tanya"
                && String::from_utf16_lossy(text) == "on my way")
        });
        ensure!(paged, "the in-match 0xCF sent no page: {requests:?}");
        self.event(
            frame,
            json!({"in-match 0xCF paged at tick": self.page_tick}),
        );
        service_event(state, WolServiceEvent::PageResult(Ok(())));
        service_event(
            state,
            WolServiceEvent::Page {
                from: member("Tanya", MEMBER_VOICE, ""),
                text: "Got your page.".encode_utf16().collect(),
                ansi: false,
            },
        );
        self.ingame_page_answers(state, frame)
    }

    /// The stand-in's answers in the in-match `0xCF`: the echo, the result
    /// and its UTF-16 page back, which posts one row with no reply hint.
    fn ingame_page_answers(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::app::shell_wol::page;
        use crate::ui::pause_menu::InGameMenuState;
        let texts: Vec<String> = page::page_dialog(state)
            .context("the in-match 0xCF closed")?
            .lines
            .iter()
            .map(|line| String::from_utf16_lossy(&line.text))
            .collect();
        let sent = state.csf_label("TXT_PAGESENT", "TXT_PAGESENT");
        ensure!(
            texts.contains(&format!("[{NICKNAME}] on my way"))
                && texts.contains(&sent)
                && texts.iter().any(|text| text == "(Tanya) Got your page."),
            "the in-match 0xCF lacks the echo or the result: {texts:?}"
        );
        let rows: Vec<String> = state
            .match_state
            .match_presentation
            .message_list
            .messages()
            .iter()
            .map(|row| row.text.clone())
            .collect();
        let hint = state.csf_label("TXT_HOW_TO_REPLY", "TXT_HOW_TO_REPLY");
        ensure!(
            rows.last()
                .is_some_and(|row| row.contains("(Tanya) Got your page."))
                && rows.iter().filter(|row| row.contains(&hint)).count() == 1,
            "the answer did not post one row without a hint: {rows:?}"
        );
        self.event(frame, json!({"in-match 0xCF lines": texts, "rows": rows}));
        if self.target == WolTarget::IngamePageCancel {
            let cancel = page::page_layout(state)
                .rect_of(crate::ui::shell::modal::control::CANCEL)
                .unwrap_or_default();
            state.match_state.input.cursor_x = (cancel.x + cancel.w / 2) as f32;
            state.match_state.input.cursor_y = (cancel.y + cancel.h / 2) as f32;
            for pressed in [
                winit::event::ElementState::Pressed,
                winit::event::ElementState::Released,
            ] {
                crate::app::input::dispatch::handle_mouse_input(
                    state,
                    winit::event::MouseButton::Left,
                    pressed,
                );
            }
            ensure!(
                state.match_state.match_presentation.in_game_menu == InGameMenuState::Closed
                    && page::page_dialog(state).is_none(),
                "Cancel did not end state 7"
            );
            self.event(frame, "Cancel ended state 7");
        }
        self.phase = WolPhase::Settle(SETTLE_FRAMES);
        Ok(())
    }

    pub(super) fn page_input(
        &mut self,
        state: &mut AppState,
        frame: u32,
        setup: bool,
    ) -> Result<()> {
        use crate::app::shell_wol::page;
        ensure!(page::page_dialog(state).is_some(), "Page did not open 0xCF");
        self.event(frame, "0xCF opened");
        let layout = page::page_layout(state);
        let click = |state: &mut AppState, rect: RectPx| {
            if setup {
                setup_click(state, rect);
            } else {
                welcome_click(state, rect);
            }
        };
        let key =
            |state: &mut AppState, code: Option<winit::keyboard::KeyCode>, text: Option<&str>| {
                if setup {
                    crate::app::input::multiplayer_setup::key_press(state, code, text);
                } else {
                    App::handle_wol_key(state, code, true, text);
                }
            };
        let (name, text) = match self.target {
            WolTarget::SetupPage => ("Boris", Some("Join my game")),
            WolTarget::PageFindStatus => ("Nobody", None),
            WolTarget::PageUserGone => ("Nobody", Some("hello")),
            _ => ("Tanya", Some("hello")),
        };
        click(state, layout.rect_of(0x5C1).unwrap_or_default());
        key(state, None, Some(name));
        match text {
            Some(text) => {
                // Leaving the name for the text locates it.
                click(state, layout.rect_of(0x540).unwrap_or_default());
                key(state, None, Some(text));
                ensure!(
                    crate::app::shell_wol::leaf::enabled(
                        state,
                        crate::ui::wol_page::PAGE_DIALOG,
                        crate::ui::wol_page::SEND
                    ),
                    "0xCF Page stayed disabled"
                );
                key(state, Some(winit::keyboard::KeyCode::Enter), None);
                self.event(frame, json!({"0xCF page": [name, text]}));
            }
            None => {
                key(state, Some(winit::keyboard::KeyCode::Tab), None);
                self.event(frame, json!({"0xCF Tab from": name}));
            }
        }
        self.phase = WolPhase::PageAnswers;
        Ok(())
    }

    /// The test service has answered `0xCF`'s locate and page.
    pub(super) fn page_answers(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let Some(page) = crate::app::shell_wol::page::page_dialog(state) else {
            bail!("0xCF closed");
        };
        let texts: Vec<String> = page
            .lines
            .iter()
            .map(|line| String::from_utf16_lossy(&line.text))
            .collect();
        let csf = |key: &str| state.csf_label(key, key);
        let done = match self.target {
            WolTarget::PageFindStatus => page.status == csf("TXT_FINDPAGE_NOT_ONLINE"),
            WolTarget::PageUserGone => texts.contains(&csf("TXT_PAGEUSERGONE")),
            WolTarget::SetupPage => texts.contains(&test_page_back("Boris")),
            _ => texts.contains(&test_page_back("Tanya")),
        };
        if !done {
            return Ok(());
        }
        if matches!(self.target, WolTarget::PageDialog | WolTarget::SetupPage) {
            let name = if self.target == WolTarget::SetupPage {
                "Boris"
            } else {
                "Tanya"
            };
            let echo = texts
                .iter()
                .any(|text| text.starts_with(&format!("[{NICKNAME}] ")));
            ensure!(
                echo && texts.contains(&csf("TXT_PAGESENT")) && !page.status.is_empty(),
                "0xCF lacks the echo, the result or the status: {texts:?} {:?}",
                page.status
            );
            let line = test_page_back(name);
            let transcript = if self.target == WolTarget::SetupPage {
                setup_chat_has(state, &line)?
            } else {
                Self::chat_has(state, &line)?
            };
            ensure!(
                transcript,
                "the page back did not reach the transcript below"
            );
        }
        self.event(
            frame,
            json!({"0xCF lines": texts, "status": page.status.clone()}),
        );
        MoviesCapture::restore_neutral_pointer(state);
        if self.target == WolTarget::SetupPage {
            crate::app::input::multiplayer_setup::cursor_moved(state);
            self.settle();
        } else {
            self.hover_settle(state);
        }
        Ok(())
    }

    pub(super) fn lobby_summary(
        &mut self,
        state: &mut AppState,
        frame: u32,
        watch_teams: Option<usize>,
    ) -> Result<()> {
        use crate::app::shell_wol::WolMatch;
        use crate::ui::wol_lobby::WolRoomSummary;

        const MATCH_ID: &str = "00000000-0000-4000-8000-000000000042";
        let watch = watch_teams.is_some();
        let many = watch_teams == Some(15);
        let (watch_room, watch_map, watch_format, teams, names) = if many {
            // The lobby's published limits: 80 bytes of room name and 64
            // characters of map name. Fourteen human players and one AI
            // fill fifteen independent teams; the two observers are seated.
            (
                "Friday Night Invitational Championship Finals at the Red Alert World Open Series",
                "Northern Frontier Crossing at the Foot of the Forgotten Mountain",
                "1v1v1v1v1v1v1v1v1v1v1v1v1v1v1",
                vec![1; 15],
                (1..=14).map(|i| format!("Player{i}")).collect(),
            )
        } else {
            (
                "Team Practice",
                "Tour of Egypt",
                "2v2",
                vec![2, 2],
                ["Tanya", "Boris", "Yuri"].map(String::from).into(),
            )
        };
        let summary = |map: &str, teams| WolRoomSummary {
            map: Some(map.into()),
            teams: Some(teams),
            observers: Some(2),
            computers: Some(1),
            ..Default::default()
        };
        let open = WolChannel {
            kind: GAME_CHANNEL,
            name: "Friday Night".into(),
            users: 9,
            max_users: 10,
            ping: 42,
            summary: Some(summary("Heck Freezes Over", vec![4, 4])),
            ..Default::default()
        };
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: GAME_CHANNEL,
                channels: vec![open],
            },
        );
        service_event(
            state,
            WolServiceEvent::Matches(vec![WolMatch {
                match_id: MATCH_ID.into(),
                name: watch_room.into(),
                map: watch_map.into(),
                names,
                watchers: 1,
                max_watchers: 8,
                summary: Some(summary(watch_map, teams)),
            }]),
        );

        let layout = crate::app::shell_wol::lobby::layout(state);
        let ui = &Self::lobby(state)?.ui;
        let rows = &ui.game_rows;
        ensure!(
            rows.len() == 2 && !rows[0].watch && rows[1].watch && rows[1].name == MATCH_ID,
            "unexpected room and match rows"
        );
        let titles = rows.iter().map(|row| row.text.clone()).collect::<Vec<_>>();
        ensure!(
            titles[0] == "Friday Night - 4v4 - Heck Freezes Over"
                && titles[1] == format!("{watch_room} - {watch_format} - {watch_map}"),
            "unexpected lobby titles: {titles:?}"
        );
        let index = usize::from(watch);
        let geometry = ui.shown_geometry(
            lobby_ui::GAMES,
            layout.control_rect(lobby_ui::GAMES).unwrap_or_default(),
            rows.len(),
            layout.page.scale,
        );
        let rect = geometry.row(index - ui.list_top(lobby_ui::GAMES));
        // Hit the far edge of the row as well as its name. The source id
        // remains the click target even where a long visible title is clipped.
        for x in [rect.x + rect.w - 2, rect.x + 3] {
            set_window_pointer_to_rect_center(state, RectPx::new(x, rect.y, 1, rect.h));
            App::handle_wol_mouse_move(state);
            let ui = &Self::lobby(state)?.ui;
            ensure!(
                ui.hovered() == Some(lobby_ui::GAMES)
                    && ui.row_at(lobby_ui::GAMES, x, rect.y + 1) == Some(index),
                "the pointer missed the game row at {x}"
            );
        }
        let help = crate::app::shell_wol::lobby::status_help(state);
        let (size, seats, limit, capacity_key) = if watch {
            (2, 1, 8, "VERA:LobbyWatchCapacity")
        } else {
            (4, 9, 10, "VERA:LobbyRoomCapacity")
        };
        let team =
            |number| crate::i18n::tr_count("VERA:LobbyTeamSeats", size as u8, &[("team", &number)]);
        let mut details = if many {
            vec![format!("{}: 15", crate::i18n::tr("VERA:Teams"))]
        } else {
            vec![team(1), team(2)]
        };
        details.extend([
            crate::i18n::tr_count("VERA:LobbySpectators", 2u32, &[]),
            crate::i18n::tr_fill("VERA:LobbyAiCount", &[("count", &1)]),
            crate::i18n::tr_fill(capacity_key, &[("count", &seats), ("max", &limit)]),
        ]);
        ensure!(
            help == details.join(" - "),
            "incomplete status help for row {index}: {help}"
        );
        self.event(
            frame,
            json!({"room and watch titles": titles, "hovered row": index, "status help": help}),
        );
        self.settle();
        Ok(())
    }

    pub(super) fn hover_settle(&mut self, state: &mut AppState) {
        MoviesCapture::restore_neutral_pointer(state);
        App::handle_wol_mouse_move(state);
        self.settle();
    }

    /// Once the local loader reports 100, the controlled guest finishes too.
    pub(super) fn complete_peer_loading(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let reports = state
            .match_state
            .session
            .take_network_loading_progress_reports();
        if !reports.contains(&100) {
            return Ok(());
        }
        ensure!(
            crate::app::loading::pump::accept_network_loading_progress(state, 1, 100),
            "controlled guest completion was refused"
        );
        self.route.push(json!({"frame": frame,
            "action": "controlled guest loading completion after the local report",
            "reports": reports}));
        Ok(())
    }
}

impl WolCapture {
    /// Create Account, Forgot Password or Delete Account over the online
    /// login page. The lobby's answers come in place, so nothing reaches the
    /// live lobby.
    pub(super) fn account_popup(
        &mut self,
        state: &mut AppState,
        frame: u32,
        target: WolTarget,
        layout: &crate::ui::wol_login::WolLoginLayout,
    ) -> Result<()> {
        use crate::ui::wol_account::{
            AccountFormAction, AccountRequest, AccountSuccess, FORM_OK, FormKind,
        };
        use crate::ui::wol_login::{CREATE_ACCOUNT, DELETE_ACCOUNT, FORGOT_PASSWORD};
        const PASSWORD: &str = "s3cret-pw";
        let control = match target {
            WolTarget::ModernForgotPassword => FORGOT_PASSWORD,
            WolTarget::ModernDeleteAccount => DELETE_ACCOUNT,
            _ => CREATE_ACCOUNT,
        };
        welcome_click(state, button(layout, control)?);
        match target {
            WolTarget::ModernForgotPassword => {
                account_type(state, &["Player"]);
                let (login, ui) = login_mut(state)?;
                let form = login.account.as_mut().context("Forgot Password")?;
                ensure!(
                    matches!(
                        form.activate(ui, FORM_OK),
                        Some(AccountFormAction::Send(AccountRequest::Forgot { .. }))
                    ) && form.answered(ui, Ok(AccountSuccess::Accepted)).is_none()
                        && form.kind == FormKind::Reset,
                    "Forgot Password did not move on to its code"
                );
                account_type(state, &["123456", PASSWORD, PASSWORD]);
                self.event(frame, "Forgot Password: code and new password typed");
            }
            WolTarget::ModernDeleteAccount => {
                account_type(state, &["player+delete@example.com", PASSWORD]);
                self.event(frame, "Delete Account: email and password typed");
            }
            _ => {
                account_type(state, &["Player", "player@example.com", PASSWORD, PASSWORD]);
                self.event(frame, "Create Account filled in");
                if target == WolTarget::ModernAccountCode {
                    let (login, ui) = login_mut(state)?;
                    let form = login.account.as_mut().context("Create Account")?;
                    ensure!(
                        matches!(form.activate(ui, FORM_OK), Some(AccountFormAction::Send(_)))
                            && form.answered(ui, Ok(AccountSuccess::Accepted)).is_none()
                            && form.kind == FormKind::Verify,
                        "Create Account did not move on to its code"
                    );
                    account_type(state, &["123456"]);
                    self.event(frame, "the mailed code typed");
                }
            }
        }
        let (login, ui) = login_mut(state)?;
        let form = login.account.as_ref().context("account popup")?;
        ensure!(
            form.ok_enabled(ui),
            "the filled account popup kept OK disabled"
        );
        Ok(())
    }
}
