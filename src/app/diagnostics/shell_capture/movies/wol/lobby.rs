//! The chat lobby and the custom-match setup page: the lobby's entry,
//! visit and settled checks, the setup page's settle, clan match, guest
//! join, chat and start, through the production lobby and setup handlers.

use super::*;

impl WolCapture {
    /// The host's Lobby and Back to Room (docs/lobby-protocol.md "Visiting
    /// the lobby"); this plays the lobby's answers and the room's events.
    pub(super) fn lobby_visit(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        crate::app::shell_lan::append_chat(
            &state.renderer,
            &mut state.frontend,
            &state.match_state,
            &state.process_assets,
            crate::ui::lan_shell::LanChatLine {
                text: "before the lobby".encode_utf16().collect(),
                color: u32::MAX,
            },
        );
        let room = crate::app::shell_wol::lobby::current_channel(state.frontend.wol.as_ref())
            .context("the room's channel")?;
        let names = |state: &AppState| -> Vec<String> {
            state
                .match_state
                .session
                .network_setup()
                .map(|launch| {
                    launch
                        .humans
                        .iter()
                        .map(|human| human.name.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        let lines = |chat: &crate::ui::lan_shell::LanChatState| -> Vec<String> {
            chat.lines
                .iter()
                .map(|line| String::from_utf16_lossy(&line.text))
                .collect()
        };
        let room_chat = |state: &AppState| -> Vec<String> {
            state
                .frontend
                .multiplayer_setup
                .as_ref()
                .map(|ui| lines(&ui.chat))
                .unwrap_or_default()
        };
        let lobby_chat = |state: &AppState| -> Vec<String> {
            state
                .frontend
                .wol
                .as_ref()
                .and_then(|visit| visit.lobby())
                .map(|lobby| lines(&lobby.ui.chat))
                .unwrap_or_default()
        };
        let seated = names(state);
        ensure!(
            seated.iter().any(|name| name == "Natasha"),
            "the room had no Natasha to let go: {seated:?}"
        );
        setup_requests(&mut state.frontend.network_setup_protocol);
        self.requests(&mut state.frontend.wol, frame)?;
        setup_button(state, crate::ui::multiplayer_setup::LOBBY)?;
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests.contains(&WolServiceRequest::VisitLobby),
            "C2's Lobby did not visit the lobby: {requests:?}"
        );
        let visit_joined = || {
            WolServiceEvent::VisitJoined(WolJoined {
                channel: lobbies()[0].clone(),
                members: vec![
                    member("Hotel", MEMBER_OPERATOR, ""),
                    member(NICKNAME, 0, ""),
                ],
            })
        };
        service_event(state, visit_joined());
        let visiting = crate::i18n::tr_fill("VERA:VisitingLobby", &[("room", &room.name)]);
        let lobby = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .context("the lobby runner")?;
        let players: Vec<&str> = lobby
            .ui
            .player_rows
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        let shows = lobby.shown
            && lobby.ui.enabled(lobby_ui::BACK_TO_ROOM)
            && players.contains(&"Hotel")
            // The modern lobby shows each line's time first.
            && lobby_chat(state).iter().any(|line| line.ends_with(&visiting));
        ensure!(
            shows
                && state.frontend.shell_route != ShellRoute::NetworkSetup
                && crate::app::shell_wol::lobby::visiting(state.frontend.wol.as_ref())
                && names(state) == seated,
            "the lobby page did not show over the room: shown {shows}, route {:?}, players {players:?}",
            state.frontend.shell_route
        );
        self.event(frame, "Lobby: the public lobby over the hidden room");
        // The destination can fill between listing and joining. The current
        // host and their guests must still be together after the refusal.
        let destination =
            local_games(&state.frontend.scenario_catalog, &state.process_assets).remove(0);
        let switch = |movie: &mut Self, state: &mut AppState| -> Result<()> {
            service_event(
                state,
                WolServiceEvent::Channels {
                    kind: GAME_CHANNEL,
                    channels: vec![destination.clone()],
                },
            );
            game_row_click(state, 0)?;
            let layout = crate::app::shell_wol::lobby::layout(
                state.frontend.wol.as_ref(),
                &state.renderer.gpu,
            );
            lobby_button(state, &layout, lobby_ui::JOIN)?;
            let requests = movie.requests(&mut state.frontend.wol, frame)?;
            ensure!(
                requests.iter().any(|request| matches!(request,
                WolServiceRequest::JoinChannel { channel, .. } if channel == &destination))
                    && !requests.contains(&WolServiceRequest::LeaveChannel),
                "switching rooms must ask to join before giving up the host's seat: {requests:?}"
            );
            Ok(())
        };
        switch(self, state)?;
        service_event(state, WolServiceEvent::Joined(Err(0x8004_006E)));
        ensure!(
            Self::lobby(&state.frontend.wol)?.message().is_some(),
            "no room-full message"
        );
        let screen = &state.renderer.gpu.config;
        let ok =
            crate::ui::shell::modal::menu_message_box(1, screen.width as i32, screen.height as i32)
                .button(0);
        welcome_click(state, ok);
        ensure!(
            Self::lobby(&state.frontend.wol)?.message().is_none()
                && crate::app::shell_wol::lobby::visiting(state.frontend.wol.as_ref())
                && state.match_state.session.host()
                && names(state) == seated,
            "refused switch lost the original host or their guests"
        );
        // The room goes on: Golf comes in and is seated, Natasha talks in the
        // room and leaves, and Hotel talks in the lobby.
        service_event(state, WolServiceEvent::UserJoined(member("Golf", 0, "")));
        let said = |from: &str, text: &str, visit: bool| {
            let from = member(from, 0, "");
            let text: Vec<u16> = text.encode_utf16().collect();
            if visit {
                WolServiceEvent::VisitMessage {
                    from,
                    text,
                    localized: false,
                    private: false,
                    action: false,
                    sent_at: None,
                }
            } else {
                WolServiceEvent::Message {
                    from,
                    text,
                    localized: false,
                    warning: false,
                    private: false,
                    action: false,
                    ansi: false,
                    options: None,
                    sent_at: None,
                }
            }
        };
        service_event(state, said("Natasha", "room line", false));
        service_event(state, said("Hotel", "lobby line", true));
        service_event(state, WolServiceEvent::UserLeft("Natasha".into()));
        let joined = crate::i18n::tr_fill("VERA:PlayerJoinedRoom", &[("name", &"Golf")]);
        let back: Vec<String> = seated
            .iter()
            .filter(|name| *name != "Natasha")
            .cloned()
            .chain(["Golf".to_owned()])
            .collect();
        let (room_lines, lobby_lines) = (room_chat(state), lobby_chat(state));
        let has = |lines: &[String], text: &str| lines.iter().any(|line| line.contains(text));
        ensure!(
            names(state) == back
                && has(&room_lines, &joined)
                && has(&room_lines, "room line")
                && has(&lobby_lines, "lobby line")
                && !has(&lobby_lines, "room line")
                && !has(&room_lines, "lobby line"),
            "the hidden room did not go on: humans {:?}, room chat {room_lines:?}, lobby chat {lobby_lines:?}",
            names(state)
        );
        self.event(frame, "the hidden room seats Golf and lets Natasha go");
        let layout =
            crate::app::shell_wol::lobby::layout(state.frontend.wol.as_ref(), &state.renderer.gpu);
        lobby_button(state, &layout, lobby_ui::BACK_TO_ROOM)?;
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests.contains(&WolServiceRequest::EndVisit)
                && state.frontend.shell_route == ShellRoute::NetworkSetup
                && !crate::app::shell_wol::lobby::visiting(state.frontend.wol.as_ref())
                && names(state) == back
                && setup_chat_has(&state.frontend.multiplayer_setup, "before the lobby")?,
            "Back to Room did not show the room as it is now: {requests:?}, route {:?}",
            state.frontend.shell_route
        );
        self.event(frame, "Back to Room: the room as it is now");
        // On success the server emits channel_left followed by joined. Start
        // the new room as a guest before applying its host's options.
        setup_button(state, crate::ui::multiplayer_setup::LOBBY)?;
        self.requests(&mut state.frontend.wol, frame)?;
        service_event(state, visit_joined());
        switch(self, state)?;
        service_event(state, WolServiceEvent::ChannelLeft);
        service_event(
            state,
            WolServiceEvent::Joined(Ok(WolJoined {
                channel: destination.clone(),
                members: vec![
                    member("Alpha", MEMBER_OPERATOR, ""),
                    member(NICKNAME, 0, ""),
                ],
            })),
        );
        ensure!(
            !crate::app::shell_wol::lobby::visiting(state.frontend.wol.as_ref())
                && !state.match_state.session.host()
                && state.frontend.shell_route == ShellRoute::NetworkSetup
                && names(state).contains(&"Alpha".to_owned())
                && !names(state).contains(&"Golf".to_owned()),
            "successful room switch did not install the destination roster"
        );
        // A visit from the new room outlives that room when it closes.
        setup_button(state, crate::ui::multiplayer_setup::LOBBY)?;
        self.requests(&mut state.frontend.wol, frame)?;
        service_event(state, visit_joined());
        service_event(state, WolServiceEvent::ChannelLeft);
        let closed = crate::i18n::tr_fill("VERA:VisitedRoomClosed", &[("room", &destination.name)]);
        let channel = crate::app::shell_wol::lobby::current_channel(state.frontend.wol.as_ref())
            .map(|room| room.name);
        let shown = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .is_some_and(|lobby| lobby.shown);
        ensure!(
            !crate::app::shell_wol::lobby::visiting(state.frontend.wol.as_ref())
                && state.frontend.multiplayer_setup.is_none()
                && state.frontend.shell_route != ShellRoute::NetworkSetup
                && shown
                && channel.as_deref() == Some(lobbies()[0].name.as_str())
                && lobby_chat(state)
                    .last()
                    .is_some_and(|line| line.ends_with(&closed)),
            "the visit did not outlive its room: channel {channel:?}, shown {shown}, route {:?}, lobby chat {:?}",
            state.frontend.shell_route,
            lobby_chat(state)
        );
        self.event(frame, "the room closed: the visited lobby stays");
        Ok(())
    }

    /// C2/C9 shown and settled: drive the target's interaction.
    pub(super) fn setup_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        use crate::ui::multiplayer_setup as setup_ui;
        match self.target {
            WolTarget::PersonaPicks => {
                let slot = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C2 setup")?
                    .local_slot()
                    .clone();
                ensure!(
                    slot.country == crate::skirmish_launch::LaunchCountry::from_country_index(3)
                        && !slot.country_random
                        && slot.color_index == 2
                        && !slot.color_random,
                    "the host row did not take the persona's Country 3 and Color 2: {slot:?}"
                );
                self.event(frame, "C2 host row: persona Country 3, Color 2");
            }
            WolTarget::SetupBack => {
                setup_button(state, setup_ui::BACK)?;
                self.event(frame, "C2 Back");
                self.phase = WolPhase::Rejoin;
                return Ok(());
            }
            WolTarget::JoinAccepted => {
                // Before the host's 'L' (0x5E8EA0) the user-list sink's nodes
                // (0x7A7110) hold no row, so 0x5EA3E0 marks no host.
                let launch = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C9 setup")?;
                let rows: Vec<(String, Option<u8>)> = launch
                    .humans
                    .iter()
                    .map(|human| (human.name.clone(), human.raw_slot_index))
                    .collect();
                let view = setup_view(
                    &state.renderer,
                    &state.frontend,
                    &state.match_state,
                    &state.process_assets,
                )?;
                let marked = view.row_icons.iter().flatten().any(|icon| icon.host);
                ensure!(
                    rows.contains(&(PEER.to_owned(), None)) && !marked,
                    "C9 before options must hold an unbound host and no host icon: \
                     {rows:?} host icon {marked}"
                );
                self.event(frame, json!({"C9 before options": rows}));
            }
            WolTarget::GuestAccept => {
                setup_button(state, setup_ui::ACCEPT)?;
                let requests = setup_requests(&mut state.frontend.network_setup_protocol);
                ensure!(
                    requests.contains(&NetworkSetupRequest::Ready(true))
                        && !setup_view(
                            &state.renderer,
                            &state.frontend,
                            &state.match_state,
                            &state.process_assets,
                        )?
                        .enabled(setup_ui::ACCEPT),
                    "C9 Accept did not report readiness: {requests:?}"
                );
                self.event(frame, "C9 Accept");
            }
            WolTarget::HostHandover => {
                // VERA's lobby gives the room to the player in it longest when
                // its host leaves: `host_changed`, then the old host's
                // `member_left`. This guest's game hosts it on, chat and all.
                crate::app::shell_lan::append_chat(
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    crate::ui::lan_shell::LanChatLine {
                        text: "before the host left".encode_utf16().collect(),
                        color: u32::MAX,
                    },
                );
                setup_requests(&mut state.frontend.network_setup_protocol);
                service_event(
                    state,
                    WolServiceEvent::HostChanged {
                        name: NICKNAME.into(),
                        channel: None,
                    },
                );
                service_event(state, WolServiceEvent::UserLeft(PEER.into()));
                let names: Vec<String> = state
                    .match_state
                    .session
                    .network_setup()
                    .context("C2 setup")?
                    .humans
                    .iter()
                    .map(|human| human.name.clone())
                    .collect();
                let host = state.match_state.session.host();
                let requests = setup_requests(&mut state.frontend.network_setup_protocol);
                ensure!(
                    host && names == [NICKNAME]
                        && state.frontend.shell_route == ShellRoute::NetworkSetup
                        && requests.contains(&NetworkSetupRequest::PublishOptions)
                        && requests.contains(&NetworkSetupRequest::Advertise),
                    "the guest did not take the room over: host {host}, humans {names:?}, \
                     requests {requests:?}"
                );
                let left = crate::i18n::tr_fill("VERA:PlayerLeftRoom", &[("name", &PEER)]);
                let now_host = crate::i18n::tr("VERA:NowHost");
                for line in ["before the host left", left.as_str(), now_host.as_str()] {
                    ensure!(
                        setup_chat_has(&state.frontend.multiplayer_setup, line)?,
                        "the room's chat lacks {line:?}"
                    );
                }
                self.event(frame, "C9's host left: the guest hosts the room");
            }
            WolTarget::GuestBack => {
                setup_button(state, setup_ui::BACK)?;
                let requests = setup_requests(&mut state.frontend.network_setup_protocol);
                // The user-list sink's first R (0x7A7A5D) precedes Back's leave.
                ensure!(
                    matches!(
                        requests.as_slice(),
                        [
                            NetworkSetupRequest::PlayerChoices { row: 0, .. },
                            NetworkSetupRequest::Leave
                        ]
                    ) && state.frontend.shell_exit.is_none(),
                    "C9 Back must leave the channel and wait: {requests:?}"
                );
                service_event(state, WolServiceEvent::ChannelLeft);
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
                        let requests = setup_requests(&mut state.frontend.network_setup_protocol);
                        let line = state
                            .process_assets
                            .csf_label("GUI:HostGameStartHost", "GUI:HostGameStartHost");
                        ensure!(
                            requests == [NetworkSetupRequest::RequestAcceptance]
                                && setup_chat_has(&state.frontend.multiplayer_setup, &line)?,
                            "Start with an unready guest: {requests:?}"
                        );
                        self.event(frame, "C2 Start refused: guest not ready");
                    }
                    WolTarget::SetupChat => self.setup_chat(state, frame)?,
                    WolTarget::SetupStart
                    | WolTarget::AbortReturn
                    | WolTarget::InternetMenu
                    | WolTarget::IngameConnectionLost
                    | WolTarget::IngamePage
                    | WolTarget::AbortRelogin
                    | WolTarget::ClanLeaver
                    | WolTarget::ClanSurrender
                    | WolTarget::ClanSurrenderOk => {
                        self.start(state, frame)?;
                        return Ok(());
                    }
                    WolTarget::VerifyDialog => return self.start(state, frame),
                    _ => {}
                }
            }
            _ => {}
        }
        MoviesCapture::restore_neutral_pointer(&state.renderer.gpu, &mut state.match_state.input);
        crate::app::input::multiplayer_setup::cursor_moved(state);
        self.settle();
        Ok(())
    }

    /// The clan game's match: the WOL start terms in the sim, the refused
    /// Ally, then the target's own step.
    pub(super) fn clan_match(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let peer = clan_peer_house(&state.match_state)?;
        let (terms, pending) = {
            let sim = &state
                .match_state
                .sim_runtime
                .runtime()
                .context("the clan match runtime")?
                .simulation;
            let session = sim.session();
            ensure!(
                session.wol_tournament_game() && session.fixed_alliance(),
                "a clan game's start terms did not reach the sim"
            );
            let names: Vec<String> = sim
                .houses()
                .values()
                .filter(|house| house.is_human())
                .filter_map(|house| house.native_scenario_name().map(str::to_owned))
                .collect();
            ensure!(
                names.iter().any(|name| name == NICKNAME) && names.iter().any(|name| name == PEER),
                "WOL human Houses must carry their nodes' names: {names:?}"
            );
            (names, sim.pending_command_snapshot().len())
        };
        // 0x731850: FixedAlliance refuses the Ally request.
        crate::app::input::diplomacy::ally(&mut state.match_state.sim_runtime, peer);
        let after = state
            .match_state
            .sim_runtime
            .runtime()
            .context("the clan match runtime")?
            .simulation
            .pending_command_snapshot()
            .len();
        ensure!(
            after == pending,
            "FixedAlliance did not refuse the Ally request"
        );
        self.event(
            frame,
            json!({"clan match": {"tournament": 2, "house names": terms,
                "fixed alliance": true, "ally refused": true}}),
        );
        match self.target {
            WolTarget::ClanSurrender | WolTarget::ClanSurrenderOk => {
                self.abort_input = Some(abort_return::AbortReturnInput::begin(
                    state,
                    &mut self.route,
                    frame,
                    abort_return::AbortGoal::Surrender {
                        press_ok: self.target == WolTarget::ClanSurrenderOk,
                    },
                    true,
                )?);
                self.phase = WolPhase::AbortInput;
            }
            WolTarget::ClanLeaver => {
                queue_peer_command(
                    &mut state.match_state,
                    1,
                    crate::sim::command::Command::ExitMatch,
                )?;
                let tick = state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .context("the clan match runtime")?
                    .simulation
                    .session()
                    .tick();
                self.event(frame, "controlled clan peer EXIT");
                self.phase = WolPhase::ClanLeaving { tick };
            }
            _ => unreachable!("clan match targets"),
        }
        Ok(())
    }

    /// The host's join sink for the controlled guest and its map status.
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
        let opened = setup_requests(&mut state.frontend.network_setup_protocol);
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
        let requests = setup_requests(&mut state.frontend.network_setup_protocol);
        ensure!(
            requests
                == [
                    NetworkSetupRequest::ObserverSlot {
                        name: PEER.into(),
                        row
                    },
                    NetworkSetupRequest::PublishRoster,
                    NetworkSetupRequest::PublishOptions,
                    NetworkSetupRequest::Advertise,
                ],
            "the host join sink's requests: {requests:?}"
        );
        setup::service_event(
            state,
            NetworkSetupEvent::MapStatus {
                index: 1,
                status: 1,
            },
        )?;
        ensure!(
            !state.match_state.session.network_player_ready(1),
            "the joined guest starts ready"
        );
        self.event(frame, "controlled guest joined, map present");
        Ok(())
    }

    /// C2 chat: a page through the CSF command, then an emote.
    fn setup_chat(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let page = state
            .process_assets
            .csf_label("TXT_PAGE_COMMAND", "TXT_PAGE_COMMAND");
        let layout = crate::app::input::multiplayer_setup::layout(
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.process_assets,
        );
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
        let requests = setup_requests(&mut state.frontend.network_setup_protocol);
        ensure!(
            requests
                == [NetworkSetupRequest::WolChat(WolChatLine::Page {
                    name: PEER.encode_utf16().collect(),
                    text: "ready?".encode_utf16().collect(),
                })],
            "the page line was not sent: {requests:?}"
        );
        ensure!(
            setup_chat_has(
                &state.frontend.multiplayer_setup,
                &format!("[{NICKNAME} > {PEER}] ready?")
            )?,
            "the page echo did not show"
        );
        crate::app::input::multiplayer_setup::key_press(state, None, Some("waves"));
        setup_click(state, layout.emote.context("C2 emote")?);
        let requests = setup_requests(&mut state.frontend.network_setup_protocol);
        ensure!(
            requests
                == [NetworkSetupRequest::WolEmote(
                    "waves".encode_utf16().collect()
                )]
                && setup_chat_has(
                    &state.frontend.multiplayer_setup,
                    &format!("{NICKNAME} waves")
                )?
                && state.frontend.multiplayer_setup.is_some()
                && crate::app::input::multiplayer_setup::chat_text(
                    &state.frontend,
                    &state.match_state
                )
                .is_empty(),
            "the emote was not sent or echoed: {requests:?}"
        );
        // C2 in the stack passes the language gate: with both display
        // filters off, the guest's line still shows.
        state.frontend.wol_preferences.display_latin = 0;
        state.frontend.wol_preferences.display_asian = 0;
        service_event(
            state,
            WolServiceEvent::Message {
                from: member(PEER, 0, ""),
                text: "\u{4f60}\u{597d}".encode_utf16().collect(),
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
            setup_chat_has(
                &state.frontend.multiplayer_setup,
                &format!("[{PEER}] : \u{4f60}\u{597d}")
            )?,
            "the guest's line did not show in C2"
        );
        self.event(frame, "C2 page, emote and a received line");
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
        let layout = crate::app::input::multiplayer_setup::layout(
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.process_assets,
        );
        // A clan room's Short Game is locked (0x78F85E), so the edit is
        // made only elsewhere.
        if !self.target.clan_match() {
            setup_click(
                state,
                crate::ui::skirmish_shell::checkbox_icon_rect(
                    layout.checkboxes[0],
                    layout.page.scale,
                ),
            );
            ensure!(
                !state.match_state.session.network_player_ready(1),
                "a C2 option edit kept the guest's acceptance"
            );
            setup_requests(&mut state.frontend.network_setup_protocol);
            setup::service_event(state, ready())?;
        }
        setup_button(state, setup_ui::START)?;
        let requests = setup_requests(&mut state.frontend.network_setup_protocol);
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
            setup_requests(&mut state.frontend.network_setup_protocol).is_empty()
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
                pings: pings.to_vec(),
            },
        )?;
        ensure!(
            setup_requests(&mut state.frontend.network_setup_protocol).is_empty()
                && state.frontend.shell_exit.is_none(),
            "a mismatched OnGameStart started the game"
        );
        setup_button(state, setup_ui::START)?;
        ensure!(
            setup_requests(&mut state.frontend.network_setup_protocol).last()
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
                pings: pings.to_vec(),
            },
        )?;
        // Start Game Now (0x794BA0) keeps C2 up under its verify 0x130.
        let names = |state: &AppState| {
            setup::verify(state.frontend.network_setup_protocol.as_ref())
                .map(|verify| (verify.names.clone(), verify.checked()))
        };
        ensure!(
            setup_requests(&mut state.frontend.network_setup_protocol).is_empty()
                && state.frontend.shell_exit.is_none()
                && names(state).is_some_and(|(names, checked)| {
                    names == [NICKNAME.to_owned(), PEER.to_owned()] && checked == [false; 2]
                }),
            "OnGameStart did not open 0x130 over C2: {:?}",
            names(state)
        );
        self.event(frame, "C2 Start, then controlled OnGameStart: 0x130");
        // 0x7A7E03 holds [0xB77E28] = 2: a guest's A0 and K0 are dropped
        // under 0x130 (0x5E3E73).
        setup::player_ready(state, PEER, 0)?;
        setup::map_status(state, PEER, 0)?;
        let session = &state.match_state.session;
        ensure!(
            session.network_player_ready(1)
                && session.network_map_status(1) == Some(1)
                && setup_requests(&mut state.frontend.network_setup_protocol).is_empty(),
            "a guest packet changed the setup under 0x130"
        );
        setup::service_event(state, NetworkSetupEvent::VerifyStarted { pairs: true })?;
        setup::service_event(state, NetworkSetupEvent::PeerConnected { node: 1 })?;
        // A click holds node 0's box and clears a peer's (0x7932D7).
        let layout = setup::verify_layout(&state.renderer.gpu, &state.frontend, &state.match_state);
        for node in [0, 1] {
            setup_click(
                state,
                crate::ui::skirmish_shell::checkbox_icon_rect(
                    layout.boxes[node],
                    crate::ui::shell::geom::RETAIL_SCALE,
                ),
            );
        }
        let checked = names(state).context("0x130")?.1;
        ensure!(
            checked[..2] == [true, false],
            "0x130 clicks: node 0 held, the peer cleared: {checked:?}"
        );
        self.event(frame, json!({"0x130 after clicks": checked[..2]}));
        if self.target == WolTarget::VerifyDialog {
            MoviesCapture::restore_neutral_pointer(
                &state.renderer.gpu,
                &mut state.match_state.input,
            );
            crate::app::input::multiplayer_setup::cursor_moved(state);
            self.settle();
            return Ok(());
        }
        setup::service_event(state, NetworkSetupEvent::Negotiated { result: 0 })?;
        let requests = setup_requests(&mut state.frontend.network_setup_protocol);
        ensure!(
            requests == [NetworkSetupRequest::Leave]
                && state.frontend.shell_exit.is_some()
                && setup::verify(state.frontend.network_setup_protocol.as_ref()).is_none(),
            "the negotiation did not close 0x130, leave and slide C2 out: {requests:?}"
        );
        MoviesCapture::restore_neutral_pointer(&state.renderer.gpu, &mut state.match_state.input);
        self.event(frame, "negotiated: 0x130 closed, channel left");
        self.phase = WolPhase::Loading;
        Ok(())
    }

    /// The runner behind its wait box: the chat-lobby list, the picked
    /// lobby's join, then both lists for the shown dialog.
    pub(super) fn lobby_entry(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        // The mode-loop pass (0x0077B402) applies the preferences first;
        // after a match the entry apply (0x0077B31D) already has WOLAPI.
        let mut requests = self.requests(&mut state.frontend.wol, frame)?;
        if let Some(locale) = self.chosen_locale {
            let sent = [
                WolServiceRequest::SetLocale(locale),
                WolServiceRequest::RequestLocales {
                    names: vec![NICKNAME.into()],
                },
            ];
            ensure!(
                requests.starts_with(&sent),
                "0xFE OK did not send the locale: {requests:?}"
            );
            requests.drain(..sent.len());
        }
        // 0x00787B33 asks for the user's squad, then 0x00787BC8 restarts
        // the buddy mirror before TXT_REQ_CHANLIST: the buddy list, then the
        // game list with automatic pings.
        let [apply @ .., squad, buddies, game_list, list] = requests.as_slice() else {
            bail!("the runner sent too little: {requests:?}");
        };
        let passes = if self.rejoined { 2 } else { 1 };
        ensure!(
            apply.len() == 2 * passes
                && apply
                    .chunks(2)
                    .all(|pair| pair == Self::applied(&state.frontend.wol_preferences))
                && *squad == WolServiceRequest::SquadInfo { id: 0 }
                && *buddies == WolServiceRequest::ListBuddies
                && *game_list
                    == WolServiceRequest::ListChannels {
                        kind: GAME_CHANNEL,
                        flags: 1,
                    }
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
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::JoinChannel {
                    channel: lobbies()[0].clone(),
                    key: LOBBY_KEY.into(),
                }],
            "the picker did not join Lob_41_0: {requests:?}"
        );
        join_lobby(state);
        let requests = self.requests(&mut state.frontend.wol, frame)?;
        // The user-list sink's batch of every unranked user (0x007A77CB),
        // then the lists.
        let listed: Vec<String> = ["Hotel", NICKNAME, "Echo", "Foxtrot"]
            .map(str::to_ascii_lowercase)
            .into();
        ensure!(
            requests
                == [
                    WolServiceRequest::LadderRecords(listed.clone()),
                    WolServiceRequest::HighScores(listed),
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
                channels: local_games(&state.frontend.scenario_catalog, &state.process_assets),
            },
        );
        self.event(frame, "controlled lobbies, join and games");
        self.phase = WolPhase::LobbyShown;
        Ok(())
    }

    pub(super) fn lobby(wol: &Option<WolVisit>) -> Result<&crate::app::shell_wol::lobby::WolLobby> {
        wol.as_ref()
            .and_then(|visit| visit.lobby())
            .context("the lobby runner")
    }

    pub(super) fn chat_has(wol: &Option<WolVisit>, text: &str) -> Result<bool> {
        Ok(Self::chat_color(wol, text)?.is_some())
    }

    fn chat_color(wol: &Option<WolVisit>, text: &str) -> Result<Option<u32>> {
        Ok(Self::lobby(wol)?
            .ui
            .chat
            .lines
            .iter()
            .find(|line| String::from_utf16_lossy(&line.text) == text)
            .map(|line| line.color))
    }

    /// A line typed into the lobby's chat edit and sent with Enter.
    fn type_line(state: &mut AppState, text: &str) {
        App::handle_wol_key(state, None, true, Some(text));
        App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
    }

    /// `0x113` shown and settled: drive the target's interaction.
    pub(super) fn lobby_settled(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let layout =
            crate::app::shell_wol::lobby::layout(state.frontend.wol.as_ref(), &state.renderer.gpu);
        let (width, height) = (
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        let joined = state
            .process_assets
            .csf_label("TXT_JOINED_S", "TXT_JOINED_S");
        let base = state.process_assets.csf_label("TXT_LOB_1", "TXT_LOB_1");
        ensure!(
            Self::chat_has(
                &state.frontend.wol,
                &crate::assets::csf_file::format_csf(
                    &joined,
                    &[crate::assets::csf_file::CsfArg::Str(&base)]
                )
            )?,
            "TXT_JOINED_S was not printed"
        );
        let lobby = Self::lobby(&state.frontend.wol)?;
        ensure!(
            lobby.ui.game_rows.len() == 4
                && lobby.ui.player_rows.len() == 4
                && lobby.ui.lobby_rows.len() == 3
                && lobby.ui.selected_row(crate::ui::wol_lobby::LOBBIES) == Some(0),
            "the lists were not filled"
        );
        let visit = state.frontend.wol.as_ref().context("WOL visit")?;
        ensure!(
            visit.authenticated() && state.frontend.shell_route == ShellRoute::WolLobby,
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
            self.options_before = Some((
                read_settings(&state.platform.game_config)?,
                state.frontend.wol_preferences,
            ));
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
                    Self::chat_has(&state.frontend.wol, "Controlled message of the day")?,
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
                        && Self::lobby(&state.frontend.wol)?.ui.game_rows[0].name == "Alpha's_game",
                    "the ping sort did not re-sort"
                );
                self.event(frame, "ping sort");
            }
            WolTarget::LobbyChat => {
                App::handle_wol_key(state, None, true, Some("hello"));
                App::handle_wol_key(state, Some(winit::keyboard::KeyCode::Enter), true, None);
                let requests = self.requests(&mut state.frontend.wol, frame)?;
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
                        from: member("Golf", 0, ""),
                        text: "are you there?".encode_utf16().collect(),
                        // An older client's ANSI page: the /reply target.
                        ansi: true,
                    },
                );
                ensure!(
                    Self::chat_has(&state.frontend.wol, &format!("[{NICKNAME}] hello"))?
                        && Self::chat_has(&state.frontend.wol, "(Golf) are you there?")?
                        && crate::app::shell_wol::last_pager(state.frontend.wol.as_ref())
                            == Some("Golf"),
                    "the echo or the page did not show"
                );
                service_event(state, WolServiceEvent::UserJoined(member("India", 0, "")));
                service_event(state, WolServiceEvent::UserLeft("Foxtrot".into()));
                let names: Vec<String> = Self::lobby(&state.frontend.wol)?
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
            WolTarget::LobbyIncomingChat => {
                use crate::ui::wol_chat as chat;
                let lines = [
                    ("Hotel", MEMBER_OPERATOR, false, false, "welcome all"),
                    ("Echo", 0, false, false, "anyone for a game?"),
                    ("Foxtrot", MEMBER_VOICE, true, false, "psst"),
                    ("Hotel", MEMBER_OPERATOR, true, false, "behave"),
                    ("Echo", 0, false, true, "waves"),
                    ("Foxtrot", MEMBER_VOICE, true, true, "nods"),
                    (NICKNAME, 0, false, false, "my own line"),
                ];
                for (name, flags, private, action, text) in lines {
                    service_event(
                        state,
                        WolServiceEvent::Message {
                            from: member(name, flags, ""),
                            text: text.encode_utf16().collect(),
                            localized: false,
                            warning: false,
                            private,
                            action,
                            ansi: false,
                            options: None,
                            sent_at: None,
                        },
                    );
                }
                let expected = [
                    ("[Hotel] : welcome all", chat::OPERATOR_COLOR),
                    ("[Echo] : anyone for a game?", chat::PUBLIC_COLOR),
                    ("Foxtrot : psst", chat::PRIVATE_COLOR),
                    ("Hotel : behave", chat::OPERATOR_COLOR),
                    ("Echo waves", chat::EMOTE_COLOR),
                    ("Foxtrot nods", chat::PRIVATE_EMOTE_COLOR),
                ];
                for (text, color) in expected {
                    ensure!(
                        Self::chat_color(&state.frontend.wol, text)? == Some(color),
                        "{text:?} did not show in {color:#X}"
                    );
                }
                ensure!(
                    !Self::chat_has(&state.frontend.wol, &format!("[{NICKNAME}] : my own line"))?,
                    "the user's own line was shown"
                );
                self.event(frame, "received lines in their sink colors");
            }
            WolTarget::LobbyFilteredChat => {
                let say = |state: &mut AppState, name, tag, text: &str| {
                    let flags = if name == "Hotel" { MEMBER_OPERATOR } else { 0 };
                    service_event(
                        state,
                        WolServiceEvent::Message {
                            from: member(name, flags, tag),
                            text: text.encode_utf16().collect(),
                            localized: false,
                            warning: false,
                            private: false,
                            action: false,
                            ansi: false,
                            options: None,
                            sent_at: None,
                        },
                    );
                };
                // DisplayAsian off: non-Latin-1 text shows only from an
                // operator.
                state.frontend.wol_preferences.display_asian = 0;
                say(state, "Echo", "WW", "\u{4f60}\u{597d}");
                say(state, "Hotel", "", "\u{4f60}\u{597d}");
                say(state, "Foxtrot", "", "caf\u{e9}");
                ensure!(
                    !Self::chat_has(&state.frontend.wol, "[Echo] : \u{4f60}\u{597d}")?
                        && Self::chat_has(&state.frontend.wol, "[Hotel] : \u{4f60}\u{597d}")?
                        && Self::chat_has(&state.frontend.wol, "[Foxtrot] : caf\u{e9}")?,
                    "the language gate did not hold"
                );
                // /clan: the local user has no clan, so only clanless users
                // show; Echo's clan hides it.
                let clan = state
                    .process_assets
                    .csf_label("TXT_CLAN_FILTER", "TXT_CLAN_FILTER");
                Self::type_line(state, &clan);
                ensure!(
                    state.frontend.wol_preferences.display_clan,
                    "/clan did not turn DisplayClan on"
                );
                say(state, "Echo", "WW", "clan one");
                say(state, "Foxtrot", "", "clan two");
                ensure!(
                    !Self::chat_has(&state.frontend.wol, "[Echo] : clan one")?
                        && Self::chat_has(&state.frontend.wol, "[Foxtrot] : clan two")?,
                    "the clan gate did not hold"
                );
                // /clan off and /buddy on: no buddies, every sender hidden.
                Self::type_line(state, &clan);
                let buddy = state
                    .process_assets
                    .csf_label("TXT_BUDDY_FILTER", "TXT_BUDDY_FILTER");
                Self::type_line(state, &buddy);
                say(state, "Foxtrot", "", "buddy one");
                say(state, "Hotel", "", "buddy two");
                ensure!(
                    !Self::chat_has(&state.frontend.wol, "[Foxtrot] : buddy one")?
                        && !Self::chat_has(&state.frontend.wol, "[Hotel] : buddy two")?,
                    "the buddy gate let a sender through"
                );
                self.event(frame, "language, clan and buddy gates");
            }
            WolTarget::LobbyFind => {
                let online = state
                    .process_assets
                    .csf_label("TXT_ONLINE_COMMAND", "TXT_ONLINE_COMMAND");
                let room = state.process_assets.csf_label("TXT_LOB_1", "TXT_LOB_1");
                use crate::assets::csf_file::{CsfArg, format_csf};
                let cases: [(&str, Result<WolChannel, u32>, String); 3] = [
                    (
                        "Hotel",
                        Ok(lobbies()[0].clone()),
                        format_csf(
                            &state
                                .process_assets
                                .csf_label("TXT_USER_FOUND", "TXT_USER_FOUND"),
                            &[CsfArg::Str("Hotel"), CsfArg::Str(&room)],
                        ),
                    ),
                    (
                        "Zulu",
                        Err(0x0004_0130),
                        format_csf(
                            &state
                                .process_assets
                                .csf_label("TXT_USER_NOT_ONLINE", "TXT_USER_NOT_ONLINE"),
                            &[CsfArg::Str("Zulu")],
                        ),
                    ),
                    (
                        "Kilo",
                        Err(0x0004_0132),
                        format_csf(
                            &state
                                .process_assets
                                .csf_label("TXT_USER_FIND_DISABLED", "TXT_USER_FIND_DISABLED"),
                            &[CsfArg::Str("Kilo")],
                        ),
                    ),
                ];
                for (name, result, expected) in cases {
                    Self::type_line(state, &format!("{online} {name}"));
                    let requests = self.requests(&mut state.frontend.wol, frame)?;
                    ensure!(
                        requests
                            == [WolServiceRequest::Chat {
                                line: WolChatLine::Locate {
                                    name: name.encode_utf16().collect(),
                                },
                                to: Vec::new(),
                            }],
                        "the locate was not sent: {requests:?}"
                    );
                    service_event(state, WolServiceEvent::Located(result));
                    ensure!(
                        Self::chat_color(&state.frontend.wol, &expected)? == Some(u32::MAX),
                        "{expected:?} did not show"
                    );
                }
                self.event(frame, "found, not online and find disabled");
            }
            WolTarget::LobbyUserTooltip => {
                service_event(
                    state,
                    WolServiceEvent::Ladder(vec![crate::app::shell_wol::WolLadderRecord {
                        name: "ECHO".into(),
                        rank: 42,
                        ..Default::default()
                    }]),
                );
                service_event(
                    state,
                    WolServiceEvent::UserLocales(Ok(vec![("Echo".into(), 2)])),
                );
                // Player rows: Hotel, Echo, Foxtrot, the user.
                let lobby = Self::lobby(&state.frontend.wol)?;
                ensure!(
                    lobby.ui.player_rows.get(1).map(|row| row.name.as_str()) == Some("Echo"),
                    "Echo is not the second player row"
                );
                let geometry = lobby.ui.shown_geometry(
                    crate::ui::wol_lobby::PLAYERS,
                    layout
                        .control_rect(crate::ui::wol_lobby::PLAYERS)
                        .unwrap_or_default(),
                    lobby.ui.player_rows.len(),
                    layout.page.scale,
                );
                let rect = geometry.row(1 - lobby.ui.list_top(crate::ui::wol_lobby::PLAYERS));
                set_window_pointer_to_rect_center(
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state.input,
                    rect,
                );
                App::handle_wol_mouse_move(state);
                let expected = format!(
                    "Echo : {} 42: Westwood Warriors : {}",
                    state.process_assets.csf_label("TXT_RANK", "TXT_RANK"),
                    state
                        .process_assets
                        .csf_label("WOL:Locale02", "WOL:Locale02")
                );
                let help = crate::app::shell_wol::lobby::status_help(
                    &state.frontend,
                    &state.match_state.input,
                    &state.process_assets,
                );
                ensure!(help == expected, "the user tooltip read {help:?}");
                self.event(frame, json!({"hovered user help": help}));
                self.settle();
                return Ok(());
            }
            WolTarget::LobbyPlayerColumns => {
                use crate::app::shell_wol::{WolHighScoreRecord, WolLadderRecord};
                let ladder = |name: &str, rank| WolLadderRecord {
                    name: name.into(),
                    rank,
                    ..Default::default()
                };
                let score = |name: &str, points, rank, coop| WolHighScoreRecord {
                    name: name.into(),
                    points,
                    rank,
                    coop,
                };
                service_event(
                    state,
                    WolServiceEvent::HighScores(Ok(vec![
                        score("Hotel", 90, 123, 0x7),
                        score("ECHO", 1520, 7, 0xF),
                    ])),
                );
                // The high-score sink refreshes nothing; the ladder sink
                // refills the rows.
                service_event(
                    state,
                    WolServiceEvent::Ladder(vec![ladder("Hotel", 3), ladder("Echo", 42)]),
                );
                let rows: Vec<_> = Self::lobby(&state.frontend.wol)?
                    .ui
                    .player_rows
                    .iter()
                    .map(|row| (row.name.clone(), row.rank.clone(), row.score_icons.clone()))
                    .collect();
                let expected = vec![
                    (
                        "Hotel".to_string(),
                        Some("3".to_string()),
                        vec![
                            ("number1.pcx", 0x12),
                            ("number2.pcx", 0x1A),
                            ("number3.pcx", 0x22),
                            ("lieutena.pcx", 0x3D),
                        ],
                    ),
                    (
                        "Echo".into(),
                        Some("42".into()),
                        vec![
                            ("number7.pcx", 0x22),
                            ("cooperat.pcx", 0x2C),
                            ("stargen.pcx", 0x3D),
                        ],
                    ),
                    ("Foxtrot".into(), None, Vec::new()),
                    (NICKNAME.into(), None, Vec::new()),
                ];
                ensure!(rows == expected, "the player columns read {rows:?}");
                self.event(frame, "ladder and high-score columns");
                self.settle();
                return Ok(());
            }
            WolTarget::LobbyRankSort => {
                let rect = layout
                    .control_rect(lobby_ui::PLAYER_SORT_RANK)
                    .context("rank sort")?;
                welcome_click(state, rect);
                ensure!(
                    state.frontend.wol_lobby_globals.player_sort == 2,
                    "the rank sort was not chosen"
                );
                service_event(
                    state,
                    WolServiceEvent::Ladder(vec![crate::app::shell_wol::WolLadderRecord {
                        name: "foxtrot".into(),
                        rank: 1,
                        ..Default::default()
                    }]),
                );
                service_event(
                    state,
                    WolServiceEvent::Ladder(vec![crate::app::shell_wol::WolLadderRecord {
                        name: "Echo".into(),
                        rank: 42,
                        ..Default::default()
                    }]),
                );
                let names: Vec<String> = Self::lobby(&state.frontend.wol)?
                    .ui
                    .player_rows
                    .iter()
                    .map(|row| row.name.clone())
                    .collect();
                ensure!(
                    names == ["Hotel", "Foxtrot", "Echo", NICKNAME],
                    "the rank sort read {names:?}"
                );
                self.event(frame, json!({"rank sort": names}));
                self.settle();
                return Ok(());
            }
            WolTarget::LobbyGameTooltip => {
                // Rows: Alpha (the retail map), Charlie, Delta, Bravo (the
                // random map, full).
                let (_, map) = first_record(&state.frontend.scenario_catalog);
                let random = state
                    .process_assets
                    .csf_label("TXT_RANDOM_MAP_DESCRIPTION", "TXT_RANDOM_MAP_DESCRIPTION");
                let mut helps = Vec::new();
                for (row, name) in [(0, map), (3, random)] {
                    let lobby = Self::lobby(&state.frontend.wol)?;
                    let geometry = lobby.ui.shown_geometry(
                        crate::ui::wol_lobby::GAMES,
                        layout
                            .control_rect(crate::ui::wol_lobby::GAMES)
                            .unwrap_or_default(),
                        lobby.ui.game_rows.len(),
                        layout.page.scale,
                    );
                    let rect = geometry.row(row - lobby.ui.list_top(crate::ui::wol_lobby::GAMES));
                    set_window_pointer_to_rect_center(
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state.input,
                        rect,
                    );
                    App::handle_wol_mouse_move(state);
                    let help = crate::app::shell_wol::lobby::status_help(
                        &state.frontend,
                        &state.match_state.input,
                        &state.process_assets,
                    );
                    ensure!(
                        !name.is_empty() && help.ends_with(&format!(", {name}")),
                        "game row {row}'s help read {help:?}, not ending with {name:?}"
                    );
                    helps.push(help);
                }
                self.event(frame, json!({"hovered game helps": helps}));
                self.settle();
                return Ok(());
            }
            WolTarget::LobbyDisconnected | WolTarget::LobbyDisconnectReturn => {
                service_event(
                    state,
                    WolServiceEvent::NetStatus(crate::app::shell_wol::NET_STATUS_LOST),
                );
                let expected = state
                    .process_assets
                    .csf_label("TXT_YOURE_DISCON", "TXT_YOURE_DISCON");
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == expected),
                    "TXT_YOURE_DISCON did not show"
                );
                self.event(frame, "NetStatus 0x4012F in 0x113");
                if self.target == WolTarget::LobbyDisconnectReturn {
                    let ok = crate::ui::shell::modal::menu_message_box(1, width, height).button(0);
                    welcome_click(state, ok);
                    ensure!(
                        state
                            .frontend
                            .wol
                            .as_ref()
                            .is_some_and(|visit| visit.lobby().is_none()),
                        "OK did not end the runner"
                    );
                    self.event(frame, "0xD0 OK");
                    self.phase = WolPhase::WelcomeAgain;
                    return Ok(());
                }
            }
            WolTarget::LobbyKicked => {
                use crate::assets::csf_file::{CsfArg, format_csf};
                let channel = lobbies()[0].clone();
                service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel: channel.clone(),
                        user: member("Foxtrot", MEMBER_VOICE, ""),
                        by: member("Hotel", MEMBER_OPERATOR, ""),
                    },
                );
                service_event(
                    state,
                    WolServiceEvent::ChannelBan {
                        name: "Foxtrot".into(),
                        banned: true,
                    },
                );
                let other = format_csf(
                    &state
                        .process_assets
                        .csf_label("TXT_USER_KICKED", "TXT_USER_KICKED"),
                    &[
                        CsfArg::Str("Foxtrot"),
                        CsfArg::Str(&channel.name),
                        CsfArg::Str("Hotel"),
                    ],
                );
                let ban = format_csf(
                    &state
                        .process_assets
                        .csf_label("TXT_CHAN_BAN", "TXT_CHAN_BAN"),
                    &[CsfArg::Str("Foxtrot")],
                );
                ensure!(
                    Self::chat_has(&state.frontend.wol, &other)?
                        && Self::chat_has(&state.frontend.wol, &ban)?
                        && !Self::lobby(&state.frontend.wol)?
                            .ui
                            .player_rows
                            .iter()
                            .any(|row| row.name == "Foxtrot"),
                    "another user's kick or the ban did not show"
                );
                let number = state.frontend.wol_lobby_globals.lobby_number;
                service_event(
                    state,
                    WolServiceEvent::Kicked {
                        channel: channel.clone(),
                        user: member(NICKNAME, lobby_ui::MEMBER_LOCAL, ""),
                        by: member("Hotel", MEMBER_OPERATOR, ""),
                    },
                );
                let kicked = format_csf(
                    &state
                        .process_assets
                        .csf_label("TXT_YOURE_KICKED", "TXT_YOURE_KICKED"),
                    &[CsfArg::Str(&channel.name), CsfArg::Str("Hotel")],
                );
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.message())
                        .is_some_and(|message| message.body == kicked),
                    "TXT_YOURE_KICKED did not show"
                );
                let ok = crate::ui::shell::modal::menu_message_box(1, width, height).button(0);
                welcome_click(state, ok);
                let left = format_csf(
                    &state.process_assets.csf_label("TXT_YOULEFT", "TXT_YOULEFT"),
                    &[CsfArg::Str(
                        &state.process_assets.csf_label("TXT_LOB_1", "TXT_LOB_1"),
                    )],
                );
                ensure!(
                    Self::chat_has(&state.frontend.wol, &left)?
                        && Self::lobby(&state.frontend.wol)?.ui.player_rows.is_empty()
                        && state.frontend.wol_lobby_globals.lobby_number == number + 1
                        && !crate::app::shell_wol::lobby::in_channel(state.frontend.wol.as_ref()),
                    "the kick's box did not leave the channel"
                );
                self.event(frame, "kicked, then OK: TXT_YOULEFT and no channel");
            }
            WolTarget::JoinAdmission => {
                // Rows: Alpha, Charlie (other rules), Delta, Bravo (full).
                game_row_click(state, 1)?;
                game_row_click(state, 3)?;
                let mismatch = state
                    .process_assets
                    .csf_label("TXT_MISMATCH", "TXT_MISMATCH");
                let full = state
                    .process_assets
                    .csf_label("WOL:GameFull", "WOL:GameFull");
                ensure!(
                    Self::chat_has(&state.frontend.wol, &mismatch)?
                        && Self::chat_has(&state.frontend.wol, &full)?,
                    "the admission refusals were not printed"
                );
                ensure!(
                    !Self::lobby(&state.frontend.wol)?.ui.enabled(lobby_ui::JOIN),
                    "a refused game enabled Join"
                );
                self.event(frame, "mismatched and full games refused");
            }
            WolTarget::JoinPassword => {
                game_row_click(state, 2)?;
                lobby_button(state, &layout, lobby_ui::JOIN)?;
                ensure!(
                    Self::lobby(&state.frontend.wol)?
                        .ui
                        .popup
                        .as_ref()
                        .map(|popup| popup.dialog())
                        == Some(lobby_ui::PASSWORD_DIALOG),
                    "Join on a keyed game did not open 0xE1"
                );
                self.event(frame, "0xE1 opened");
            }
            WolTarget::JoinAccepted
            | WolTarget::GuestAccept
            | WolTarget::GuestBack
            | WolTarget::HostHandover => {
                game_row_click(state, 0)?;
                lobby_button(state, &layout, lobby_ui::JOIN)?;
                self.event(frame, "Join on Alpha's_game");
                self.phase = WolPhase::Leaving;
                return Ok(());
            }
            target
                if matches!(
                    target,
                    WolTarget::CreatePopup
                        | WolTarget::CreateAccepted
                        | WolTarget::SetupBack
                        | WolTarget::PersonaPicks
                ) || target.guest_joined() =>
            {
                if target.clan_match() {
                    // The own squad request's answer (0x7A4EC0) enables the
                    // clan box.
                    service_event(
                        state,
                        WolServiceEvent::Squad {
                            id: CLAN_SQUAD,
                            squad: Ok(crate::ui::wol_lobby::WolSquad {
                                name: "VERA Testers".into(),
                                tag: "VERA".into(),
                            }),
                        },
                    );
                }
                lobby_button(state, &layout, lobby_ui::CREATE)?;
                ensure!(
                    Self::lobby(&state.frontend.wol)?
                        .ui
                        .popup
                        .as_ref()
                        .map(|popup| popup.dialog())
                        == Some(lobby_ui::CREATE_DIALOG),
                    "Create did not open 0xC3"
                );
                self.event(frame, "0xC3 opened");
                if self.target != WolTarget::CreatePopup {
                    let create =
                        crate::ui::shell::modal::wol_create_game_layout(width, height, false);
                    if self.target.clan_match() {
                        welcome_click(
                            state,
                            crate::ui::skirmish_shell::checkbox_icon_rect(
                                create.clan.context("0xC3 clan box")?,
                                crate::ui::shell::geom::RETAIL_SCALE,
                            ),
                        );
                        self.event(frame, "0xC3 clan box");
                    }
                    let ok = create.ok;
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
}
