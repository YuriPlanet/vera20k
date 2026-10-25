//! Production WOL Quick Match checkpoints: `0x10E` Quick Match -> login ->
//! the runner `0x0079FC40` with `0x117`, through the real runner, matchbot,
//! game-channel and box handlers. The controlled service answers the
//! visit's own requests with decoded results and plays the matchbot and
//! the opponent; the test-service targets let the production stand-in
//! answer instead.

use super::*;
use crate::app::shell_wol::quick_match::{self as qm, MatchTransport, stage};
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::ui::wol_lobby::EX_INFO_VERSION;

/// The controlled Quick Match lobby: its number reaches the created game's
/// flags (`0x800 + [0xB77DD4]`).
const QM_LOBBY: &str = "Lob_40_3";
/// The Quick Co-op lobby (`Lob_39_`, `0x0079FD9E`).
const QC_LOBBY: &str = "Lob_39_3";
const QM_LOBBY_NUMBER: u32 = 3;
/// Cooperative `+0x20` in a fresh process: stale `[0xA8B254]` 0 is no
/// campaign, so it selects the first Allied map (`0x005C1B1D..0x005C1BA1`),
/// which replaces the matchbot's.
const QC_HOST_MAP: &str = "C1A01MD.MAP";
/// The lobby's operator, the last in its user list.
const MATCHBOT: &str = "matchbot";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QmPhase {
    /// The runner's chat-lobby list is outstanding behind its wait box.
    Runner,
    /// `0x117` opening; its lobby join is answered once it settles.
    Join,
    /// The request is out: waiting for `0x117`'s 2 s dot timer to change
    /// the transcript's `rows` or its last row's `last` units.
    Dots { rows: usize, last: usize },
    /// A runner box naming this CSF key is due.
    Box(&'static str),
    /// The runner ended: waiting for the logged-out `0x10E`.
    Welcome,
    /// The match returned: waiting for `0x122`.
    AddBuddyQuery,
    /// `0x122` Yes pressed: its teardown runs.
    AddBuddyClosing,
}

impl WolCapture {
    pub(super) fn qm_after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
        phase: QmPhase,
    ) -> Result<()> {
        match phase {
            QmPhase::Runner => self.qm_runner(state, frame),
            QmPhase::Join
                if rendered == PresentedShell::WolQuickMatch
                    && Self::page_settled(state, ShellSlideKind::WolQuickMatch) =>
            {
                self.qm_joined(state, frame)
            }
            QmPhase::Dots { rows, last } => {
                let lines = qm_rows(state)?;
                let grown = lines.len() != rows
                    || lines.last().map_or(0, |line| line.encode_utf16().count()) != last;
                if grown {
                    ensure!(
                        lines.last().is_some_and(|line| line.ends_with('.'))
                            && qm::stage(state) == stage::REQUESTED,
                        "0x117's timer did not add a dot at stage 4: {lines:?}"
                    );
                    self.event(frame, json!({"0x117 after a dot": lines}));
                    self.hover_settle(state);
                }
                Ok(())
            }
            QmPhase::Box(key) => {
                if box_body(state).is_some_and(|body| body == csf(state, key)) {
                    self.qm_box_shown(state, frame, key)?;
                }
                Ok(())
            }
            QmPhase::Welcome
                if rendered == PresentedShell::WolWelcome
                    && Self::page_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let visit = state.frontend.wol.as_ref().context("WOL visit")?;
                ensure!(
                    !visit.authenticated()
                        && visit.lobby().is_none()
                        && visit.mode().is_none()
                        && qm::dialog(state).is_none()
                        && state.match_state.session.network_setup().is_none(),
                    "the runner's end did not log out to a fresh 0x10E"
                );
                self.event(frame, "0x10E settled after the runner, logged out");
                self.hover_settle(state);
                Ok(())
            }
            QmPhase::AddBuddyQuery
                if rendered == PresentedShell::WolAddBuddyQuery
                    && Self::page_settled(state, ShellSlideKind::WolAddBuddyQuery) =>
            {
                self.qm_add_buddy(state, frame)
            }
            QmPhase::AddBuddyClosing if state.frontend.shell_exit.is_none() => {
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests.contains(&WolServiceRequest::AddBuddy(PEER.into()))
                        && requests.contains(&WolServiceRequest::Logout),
                    "0x122 Yes did not add the opponent and log out: {requests:?}"
                );
                self.event(frame, "0x122 Yes: AddBuddy, then mode 0 logs out");
                self.phase = WolPhase::Qm(QmPhase::Welcome);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The runner's entry after the login: the preferences applied, then
    /// the chat-lobby list behind `TXT_REQ_CHANLIST`.
    fn qm_runner(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let requests = self.requests(state, frame)?;
        let [apply @ .., list] = requests.as_slice() else {
            bail!("the runner sent nothing");
        };
        ensure!(
            *list == WolServiceRequest::ListChannels { kind: 0, flags: 0 }
                && apply.iter().all(|request| matches!(
                    request,
                    WolServiceRequest::SetLangFilter(_) | WolServiceRequest::SetFindPage { .. }
                )),
            "the runner did not list the chat lobbies: {requests:?}"
        );
        ensure!(
            Self::lobby(state)?.wait.is_some() && qm::dialog(state).is_none(),
            "the lobby list must wait behind 0xC4 without 0x117"
        );
        let channels = match self.target {
            WolTarget::QmNoAgent => Vec::new(),
            WolTarget::QmServerUnavailable => lobbies(),
            _ => {
                let mut channels = lobbies();
                channels.push(qm_lobby(self.target, 2));
                channels
            }
        };
        let names: Vec<String> = channels.iter().map(|c| c.name.clone()).collect();
        service_event(state, WolServiceEvent::Channels { kind: 0, channels });
        self.event(frame, json!({"controlled chat lobbies": names}));
        match self.target {
            WolTarget::QmNoAgent => {
                self.phase = WolPhase::Qm(QmPhase::Box("WOL:NoMatchingAgentFound"));
            }
            WolTarget::QmServerUnavailable => {
                self.phase = WolPhase::Qm(QmPhase::Box("WOL:MatchingServerUnavailable"));
            }
            _ => {
                ensure!(qm::dialog(state).is_some(), "Lob_40_ did not open 0x117");
                // 0x0079FEAC: the buddy mirror restarts, then stage 2 joins.
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests
                        == [
                            WolServiceRequest::ListBuddies,
                            WolServiceRequest::JoinChannel {
                                channel: qm_lobby(self.target, 2),
                                key: LOBBY_KEY.into(),
                            }
                        ],
                    "the runner did not join {QM_LOBBY}: {requests:?}"
                );
                service_event(state, WolServiceEvent::Buddies(Ok(Vec::new())));
                ensure!(
                    qm::stage(state) == stage::IN_LOBBY,
                    "the join did not move to stage 3"
                );
                self.phase = WolPhase::Qm(QmPhase::Join);
            }
        }
        Ok(())
    }

    /// `0x117` settled: the lobby's join with its matchbot sends the
    /// request; then the target's step.
    fn qm_joined(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let from = qm_rows(state)?.len();
        service_event(
            state,
            WolServiceEvent::Joined(Ok(WolJoined {
                channel: qm_lobby(self.target, 3),
                members: vec![
                    member(PEER, 0, ""),
                    member(MATCHBOT, MEMBER_OPERATOR, ""),
                    member(NICKNAME, 0, ""),
                ],
            })),
        );
        let requesting = csf(state, "WOL:RequestingMatch");
        self.expect_rows(state, frame, from, &[requesting])?;
        let requests = self.requests(state, frame)?;
        // The user-list sink's ladder batch (0x007A77CB), then the request.
        let listed: Vec<String> = [PEER, MATCHBOT, NICKNAME]
            .map(str::to_ascii_lowercase)
            .into();
        let [
            WolServiceRequest::LadderRecords(ladder),
            WolServiceRequest::HighScores(scores),
            WolServiceRequest::RequestMatch { matchbot, request },
        ] = requests.as_slice()
        else {
            bail!("the lobby join did not request a match: {requests:?}");
        };
        ensure!(
            *ladder == listed && *scores == listed,
            "the lobby join batched {ladder:?} and {scores:?}"
        );
        let text = request.text(MatchTransport::default());
        let keys: Vec<&str> = text
            .split(", ")
            .map(|field| field.split('=').next().unwrap_or_default())
            .collect();
        ensure!(
            matchbot == MATCHBOT
                && keys
                    == [
                        "Match COU",
                        "COL",
                        "MBR",
                        "LOC",
                        "RAT",
                        "RES",
                        "ADR",
                        "NAT",
                        "PRT"
                    ]
                && request.locale == Some(2)
                && request.width > 0
                && text.contains(", LOC=2, RAT=0, RES=")
                && qm::stage(state) == stage::REQUESTED,
            "the request to {matchbot}: {text:?} at stage {}",
            qm::stage(state)
        );
        self.event(
            frame,
            json!({"RequestMatch": {"to": matchbot, "text": text}}),
        );
        match self.target {
            WolTarget::QmRequesting => {
                let lines = qm_rows(state)?;
                self.phase = WolPhase::Qm(QmPhase::Dots {
                    rows: lines.len(),
                    last: lines.last().map_or(0, |line| line.encode_utf16().count()),
                });
            }
            WolTarget::QmMatchbotLines => self.qm_lines(state, frame)?,
            WolTarget::QmTimeout | WolTarget::QmTimeoutOk => {
                qm::expire_stage(state);
                self.event(frame, "stage 4's 180 s expired");
                self.phase = WolPhase::Qm(QmPhase::Box("WOL:MatchTimeout"));
            }
            WolTarget::QmCancel => {
                let layout = qm::layout(state);
                let cancel = layout
                    .page
                    .button_rect(crate::ui::wol_quick_match::CANCEL)
                    .context("0x117 Cancel")?;
                welcome_click(state, cancel);
                ensure!(qm::dialog(state).is_none(), "Cancel did not close 0x117");
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests == [WolServiceRequest::LeaveChannel],
                    "Cancel did not leave {QM_LOBBY}: {requests:?}"
                );
                service_event(state, WolServiceEvent::ChannelLeft);
                let requests = self.requests(state, frame)?;
                ensure!(
                    requests.contains(&WolServiceRequest::Logout),
                    "the runner's end did not log out: {requests:?}"
                );
                self.event(frame, "0x117 Cancel, controlled channel left");
                self.phase = WolPhase::Qm(QmPhase::Welcome);
            }
            WolTarget::QmExit => {
                let from = qm_rows(state)?.len();
                matchbot_says(state, "Exit", false, None);
                let exit = csf(state, "WOL:MatchFailExit");
                self.expect_rows(state, frame, from, &[exit])?;
                self.phase = WolPhase::Qm(QmPhase::Box("WOL:MatchFailExit"));
            }
            WolTarget::QmGuestJoined | WolTarget::QcGuestJoined => self.qm_guest(state, frame)?,
            _ => self.qm_host(state, frame)?,
        }
        Ok(())
    }

    /// The matchbot's progress, fail and unknown texts, one row each.
    fn qm_lines(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let says = [
            ("Working", true, csf(state, "GUI:MatchingProgress2")),
            ("Info hello", false, "hello".to_owned()),
            (
                "Pool 12",
                false,
                csf_name(state, "WOL:MatchingPlayerPool", "12"),
            ),
            ("Fail", true, csf(state, "WOL:MatchFail")),
            ("PingFail", true, csf(state, "WOL:MatchFailPing")),
            (
                "Status 7",
                true,
                csf(state, "WOL:MatchUnknownServerCommand"),
            ),
        ];
        for (text, private, expected) in says {
            let from = qm_rows(state)?.len();
            matchbot_says(state, text, private, None);
            self.expect_rows(state, frame, from, &[expected])?;
            if text == "Working" {
                ensure!(
                    qm::stage(state) == stage::WORKING,
                    "Working did not move stage 4 to 5"
                );
            }
        }
        ensure!(
            qm::stage(state) == stage::WORKING,
            "a fail line changed the stage"
        );
        self.event(frame, json!({"0x117 rows": qm_rows(state)?}));
        self.hover_settle(state);
        Ok(())
    }

    /// A runner box shown: the leave its path sends, then the target's
    /// settle or OK.
    fn qm_box_shown(&mut self, state: &mut AppState, frame: u32, key: &str) -> Result<()> {
        let requests = self.requests(state, frame)?;
        match self.target {
            WolTarget::QmNoAgent | WolTarget::QmServerUnavailable => ensure!(
                requests.is_empty() && qm::dialog(state).is_none(),
                "the lobby-list box sent {requests:?} or opened 0x117"
            ),
            _ => {
                ensure!(
                    requests == [WolServiceRequest::LeaveChannel],
                    "{key} did not leave {QM_LOBBY}: {requests:?}"
                );
                service_event(state, WolServiceEvent::ChannelLeft);
            }
        }
        self.event(frame, json!({"runner box": key}));
        if self.target != WolTarget::QmTimeoutOk {
            self.hover_settle(state);
            return Ok(());
        }
        let ok = crate::ui::shell::modal::body_ok_layout(
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        )
        .ok;
        welcome_click(state, ok);
        let requests = self.requests(state, frame)?;
        ensure!(
            box_body(state).is_none()
                && qm::dialog(state).is_none()
                && requests.contains(&WolServiceRequest::Logout),
            "the box's OK did not end the runner: {requests:?}"
        );
        self.event(frame, "runner box OK");
        self.phase = WolPhase::Qm(QmPhase::Welcome);
        Ok(())
    }

    /// `Start` naming the user as host: the game channel is created; the
    /// guest then joins and accepts, and the start runs to the loading
    /// screen.
    fn qm_host(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let options = crate::app::shell_wol::test_service_matchbot_options(state, NICKNAME, PEER)?;
        let from = qm_rows(state)?.len();
        matchbot_says(
            state,
            &start_text(NICKNAME, PEER),
            true,
            Some(Box::new(options.clone())),
        );
        let found = [
            csf_name(state, "WOL:MatchFoundWith", PEER),
            csf(state, "WOL:MatchOpponentPingUnknown"),
            csf(state, "WOL:MatchCreatingGame"),
        ];
        self.expect_rows(state, frame, from, &found)?;
        ensure!(
            qm::stage(state) == stage::CREATING,
            "the host's Start did not move to stage 6"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [
                    WolServiceRequest::LeaveChannel,
                    WolServiceRequest::LeaveChannel
                ],
            "the create did not leave {QM_LOBBY} first: {requests:?}"
        );
        service_event(state, WolServiceEvent::ChannelLeft);
        let requests = self.requests(state, frame)?;
        let [WolServiceRequest::CreateChannel { channel, key }] = requests.as_slice() else {
            bail!("the game channel was not created: {requests:?}");
        };
        ensure!(
            channel.name == format!("{NICKNAME}'s_game")
                && channel.kind == GAME_CHANNEL
                && channel.max_users == 2
                && channel.tournament == i32::from(!self.target.quick_coop())
                && channel.game_flags == 0x800 + QM_LOBBY_NUMBER
                && key.is_empty(),
            "unexpected Quick Match game channel {channel:?}"
        );
        self.event(
            frame,
            json!({"CreateChannel": {"name": channel.name, "max_users": channel.max_users,
                "tournament": channel.tournament, "game_flags": format!("{:#X}", channel.game_flags)}}),
        );
        let from = qm_rows(state)?.len();
        service_event(
            state,
            WolServiceEvent::Created(Ok(vec![member(NICKNAME, MEMBER_OPERATOR, "")])),
        );
        let waiting = csf_name(state, "WOL:MatchWaitingForJoin", PEER);
        self.expect_rows(state, frame, from, &[waiting])?;
        ensure!(
            crate::app::shell_network_setup::quick_match(state)
                && state.match_state.session.host()
                && qm::dialog(state).is_some()
                && !state.frontend.shell_route.network_setup(),
            "the create did not run the host's nodes headless under 0x117"
        );
        if self.target == WolTarget::QmHostWaiting {
            self.hover_settle(state);
            return Ok(());
        }
        // The opponent joins: the matchbot's options apply (0x007A6DC2),
        // then the host's join tail without the advertisement.
        let opened = setup_requests(state);
        self.route.push(json!({"frame": frame,
            "setup_open_requests": opened.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        service_event(state, WolServiceEvent::UserJoined(member(PEER, 0, "")));
        let requests = setup_requests(state);
        // The observer row the matchbot's options left.
        let row = state
            .match_state
            .session
            .network_observer_row()
            .unwrap_or(8);
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("the host setup")?;
        // The material-change pass after the options parse may first take
        // the host's own acceptance back (`Ready(false)`); the join tail
        // follows without the advertisement.
        self.route.push(json!({"frame": frame,
            "guest_join_setup_requests": requests.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        let expected_map = if self.target.quick_coop() {
            Some(QC_HOST_MAP.to_owned())
        } else {
            options.launch.selected_map_file.clone()
        };
        ensure!(
            requests.ends_with(&[
                NetworkSetupRequest::ObserverSlot {
                    name: PEER.into(),
                    row,
                },
                NetworkSetupRequest::PublishRoster,
                NetworkSetupRequest::PublishOptions,
                NetworkSetupRequest::RequestPings,
            ]) && !requests.contains(&NetworkSetupRequest::Advertise)
                && launch
                    .selected_map_file
                    .as_deref()
                    .zip(expected_map.as_deref())
                    .is_some_and(|(map, expected)| map.eq_ignore_ascii_case(expected))
                && launch.humans.len() == 2,
            "the guest's join did not apply the matchbot's options: {requests:?}, map {:?} \
             (matchbot {:?}), humans {:?}",
            launch.selected_map_file,
            options.launch.selected_map_file,
            launch
                .humans
                .iter()
                .map(|human| &human.name)
                .collect::<Vec<_>>()
        );
        // `+0x94` (`0x005E2FA0` from `0x007A6E5A`): the campaign with both
        // names, before the slot names and options.
        let campaign = requests.iter().position(|request| {
            matches!(request, NetworkSetupRequest::PublishCooperativeCampaign(_))
        });
        if self.target.quick_coop() {
            let expected = qc_campaign_text(NICKNAME, PEER);
            ensure!(
                campaign.is_some_and(|index| requests[index]
                    == NetworkSetupRequest::PublishCooperativeCampaign(expected.clone())
                    && requests[index + 1..]
                        .iter()
                        .any(|request| matches!(request, NetworkSetupRequest::PublishOptions))),
                "the Quick Co-op join did not publish {expected:?} before the options: \
                 {requests:?}"
            );
            ensure!(
                launch.mode.class == crate::skirmish_modes::MpModeClass::Cooperative,
                "the matchbot's options did not make Cooperative current"
            );
        } else {
            ensure!(
                campaign.is_none(),
                "a Quick Match join published a campaign"
            );
        }
        self.event(
            frame,
            json!({"controlled guest joined; the matchbot's options applied":
                {"map": launch.selected_map_file, "matchbot_map": options.launch.selected_map_file,
                 "countries": countries(state)}}),
        );
        service_event(
            state,
            WolServiceEvent::PlayerChoices {
                from: PEER.into(),
                slot: options.launch.humans[1].slot.clone(),
                observer: false,
            },
        );
        service_event(
            state,
            WolServiceEvent::MapStatus {
                from: PEER.into(),
                status: 1,
            },
        );
        let choices = setup_requests(state);
        self.route.push(json!({"frame": frame,
            "after_guest_choices": choices.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
            "countries": countries(state)}));
        service_event(
            state,
            WolServiceEvent::PlayerReady {
                from: PEER.into(),
                value: 1,
            },
        );
        let requests = setup_requests(state);
        let users: Vec<String> = crate::app::shell_wol::lobby::member_names(state)
            .into_iter()
            .rev()
            .collect();
        ensure!(
            users.len() == 2
                && requests
                    == [NetworkSetupRequest::StartGame {
                        users: users.clone()
                    }]
                && qm::stage(state) == stage::WAITING,
            "the guest's A1 did not request the start for {users:?}: {requests:?}"
        );
        self.event(frame, json!({"guest A1: StartGame": users}));
        let user = |name: &str, flags| WolStartUser {
            name: name.into(),
            flags,
        };
        let mut pings = [-1; 8];
        pings[1] = 90;
        setup::service_event(
            state,
            NetworkSetupEvent::GameStarted {
                users: vec![user(NICKNAME, 1), user(PEER, 0)],
                pings: pings.to_vec(),
            },
        )?;
        let names = setup::verify(state).map(|verify| verify.names.clone());
        ensure!(
            qm::dialog(state).is_none()
                && names.as_deref() == Some(&[NICKNAME.to_owned(), PEER.to_owned()][..]),
            "OnGameStart did not close 0x117 and open 0x130: {names:?}"
        );
        let quick_coop = state
            .match_state
            .session
            .network_wol_start()
            .is_some_and(|terms| terms.quick_coop);
        ensure!(
            quick_coop == self.target.quick_coop(),
            "OnGameStart's terms read Quick Co-op {quick_coop}"
        );
        self.event(frame, "controlled OnGameStart: 0x117 closed, 0x130");
        setup::service_event(state, NetworkSetupEvent::VerifyStarted { pairs: true })?;
        setup::service_event(state, NetworkSetupEvent::PeerConnected { node: 1 })?;
        setup::service_event(state, NetworkSetupEvent::Negotiated { result: 0 })?;
        let requests = self.requests(state, frame)?;
        ensure!(
            requests.contains(&WolServiceRequest::LeaveChannel) && setup::verify(state).is_none(),
            "the negotiation did not close 0x130 and leave the game channel: {requests:?}"
        );
        MoviesCapture::restore_neutral_pointer(state);
        self.event(frame, "negotiated: 0x130 closed, game channel left");
        self.phase = WolPhase::Loading;
        Ok(())
    }

    /// `Start` naming the user as guest: the host's game listed, joined,
    /// then the host's options accepted at once (`A1`).
    fn qm_guest(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let mut options =
            crate::app::shell_wol::test_service_matchbot_options(state, PEER, NICKNAME)?;
        let from = qm_rows(state)?.len();
        matchbot_says(
            state,
            &start_text(PEER, NICKNAME),
            true,
            Some(Box::new(options.clone())),
        );
        let found = [
            csf_name(state, "WOL:MatchFoundWith", PEER),
            csf(state, "WOL:MatchOpponentPingUnknown"),
            csf_name(state, "WOL:MatchAttemptingToFindGame", PEER),
        ];
        self.expect_rows(state, frame, from, &found)?;
        ensure!(
            qm::stage(state) == stage::SEARCHING,
            "the guest's Start did not move to stage 7"
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::ListChannels {
                    kind: GAME_CHANNEL,
                    flags: 0
                }],
            "the search did not list the games (0x29, 0): {requests:?}"
        );
        let game = WolChannel {
            kind: GAME_CHANNEL,
            max_users: 2,
            users: 1,
            tournament: i32::from(!self.target.quick_coop()),
            ping: -1,
            name: format!("{PEER}'s_game"),
            game_flags: 0x800 + QM_LOBBY_NUMBER,
            ex_info: format!(
                "{EX_INFO_VERSION},{},0,0,0,",
                crate::app::shell_wol::lobby::local_ini_hash(state).unwrap_or(0)
            ),
            ..Default::default()
        };
        service_event(
            state,
            WolServiceEvent::Channels {
                kind: GAME_CHANNEL,
                channels: vec![game.clone()],
            },
        );
        let requests = self.requests(state, frame)?;
        ensure!(
            requests == [WolServiceRequest::LeaveChannel],
            "the found game's join did not leave {QM_LOBBY} first: {requests:?}"
        );
        service_event(state, WolServiceEvent::ChannelLeft);
        let requests = self.requests(state, frame)?;
        ensure!(
            requests
                == [WolServiceRequest::JoinChannel {
                    channel: game.clone(),
                    key: String::new(),
                }],
            "the host's game was not joined: {requests:?}"
        );
        let from = qm_rows(state)?.len();
        service_event(
            state,
            WolServiceEvent::Joined(Ok(WolJoined {
                channel: WolChannel { users: 2, ..game },
                members: vec![member(PEER, MEMBER_OPERATOR, ""), member(NICKNAME, 0, "")],
            })),
        );
        let joined = csf(state, "WOL:MatchJoinedGame");
        self.expect_rows(state, frame, from, &[joined])?;
        ensure!(
            qm::stage(state) == stage::WAITING
                && crate::app::shell_network_setup::quick_match(state)
                && !state.match_state.session.host()
                && qm::dialog(state).is_some()
                && !state.frontend.shell_route.network_setup(),
            "the join did not run the guest's nodes headless under 0x117 at stage 9"
        );
        let opened = setup_requests(state);
        self.route.push(json!({"frame": frame,
            "guest_join_requests": opened.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>()}));
        self.event(frame, "controlled game join accepted");
        // The host's options packet as the guest's adapter decodes it: the
        // matchbot's rows (the host on 0, this user on 1) in the guest's own
        // node order, this user first.
        options.launch.humans.swap(0, 1);
        if self.target.quick_coop() {
            // The native host's C0 reaches the guest before its options
            // (`0x007A7F02`), and those carry the map `+0x20` selected.
            let text = qc_campaign_text(PEER, NICKNAME);
            setup::service_event(state, NetworkSetupEvent::CooperativeCampaign(text.clone()))?;
            self.event(frame, json!({"host C0": text}));
            options.launch.selected_map_file = Some(QC_HOST_MAP.to_owned());
            options.map_metadata =
                crate::app::shell_network_setup::local_map_metadata(state, QC_HOST_MAP)?;
        }
        setup::service_event(state, NetworkSetupEvent::HostSetup(options))?;
        let requests = setup_requests(state);
        // The parse's map check sends K1, then the Quick Match sink's A1.
        ensure!(
            requests
                == [
                    NetworkSetupRequest::MapStatus(1),
                    NetworkSetupRequest::Ready(true)
                ],
            "the host's options did not draw the guest's K1 and A1: {requests:?}"
        );
        self.event(frame, "host options: the guest's Ready(true) (A1)");
        self.hover_settle(state);
        Ok(())
    }

    /// `0x122` after the match: its question names the opponent; Yes for
    /// the second target.
    fn qm_add_buddy(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let text = crate::app::shell_wol::persona::add_buddy_text(state);
        let expected = csf_name(state, "WOL:AddBuddyQuery", PEER);
        ensure!(
            text.as_deref() == Some(expected.as_str()),
            "0x122 did not ask about {PEER}: {text:?}"
        );
        self.event(frame, json!({"0x122": text}));
        if self.target != WolTarget::QmPostmatchYes {
            self.hover_settle(state);
            return Ok(());
        }
        let layout = crate::app::shell_wol::persona::layout(state).context("0x122 layout")?;
        let yes = layout
            .page
            .button_rect(crate::ui::wol_persona::ACCEPT)
            .context("0x122 Yes")?;
        welcome_click(state, yes);
        ensure!(
            state.frontend.shell_exit.is_some(),
            "Yes did not tear 0x122 down"
        );
        self.event(frame, "0x122 Yes");
        self.phase = WolPhase::Qm(QmPhase::AddBuddyClosing);
        Ok(())
    }

    /// The rows added since `from` hold `expected` in order; the transcript
    /// wraps long lines, so the comparison skips spaces and line feeds.
    fn expect_rows(
        &mut self,
        state: &AppState,
        frame: u32,
        from: usize,
        expected: &[String],
    ) -> Result<()> {
        let rows = qm_rows(state)?;
        let added = rows.get(from..).unwrap_or_default();
        ensure!(
            squeeze(&added.concat()) == squeeze(&expected.concat()),
            "0x117 rows {added:?} are not {expected:?}"
        );
        self.event(frame, json!({"0x117 rows added": added}));
        Ok(())
    }
}

/// The host's C0 for a fresh Allied record (variant 0 at each of its three
/// stages), names in the host's order.
fn qc_campaign_text(host: &str, guest: &str) -> String {
    use crate::app::frontend::skirmish_session::cooperative::encode_name;
    format!(
        "C0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,-1,{},{}",
        encode_name(host.as_bytes()),
        encode_name(guest.as_bytes())
    )
}

/// Each human's country, `None` while Random.
fn countries(state: &AppState) -> Vec<(String, Option<u16>)> {
    state
        .match_state
        .session
        .network_setup()
        .map(|launch| {
            launch
                .humans
                .iter()
                .map(|human| {
                    let slot = &human.slot;
                    (
                        human.name.clone(),
                        (!slot.country_random).then(|| slot.country.raw_index()),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The controlled Quick Match (or Quick Co-op) lobby with `users` users.
fn qm_lobby(target: WolTarget, users: i32) -> WolChannel {
    WolChannel {
        kind: 0,
        max_users: 100,
        users,
        name: if target.quick_coop() {
            QC_LOBBY
        } else {
            QM_LOBBY
        }
        .into(),
        ping: -1,
        ..Default::default()
    }
}

/// A matchbot `Start`: options, then the players, the host first and the
/// guest tenth.
fn start_text(host: &str, guest: &str) -> String {
    format!("Start 1:{host},0,0,0,0,0,0,0,0,{guest},1,0,0,0,0,0,0,0")
}

/// An ANSI line from the lobby's operator.
fn matchbot_says(
    state: &mut AppState,
    text: &str,
    private: bool,
    options: Option<Box<setup::HostOptions>>,
) {
    service_event(
        state,
        WolServiceEvent::Message {
            from: member(MATCHBOT, MEMBER_OPERATOR, ""),
            text: text.encode_utf16().collect(),
            private,
            action: false,
            ansi: true,
            options,
        },
    );
}

fn qm_rows(state: &AppState) -> Result<Vec<String>> {
    Ok(qm::dialog(state)
        .context("0x117")?
        .ui
        .lines
        .iter()
        .map(|line| String::from_utf16_lossy(&line.text))
        .collect())
}

fn box_body(state: &AppState) -> Option<String> {
    state
        .frontend
        .wol
        .as_ref()?
        .lobby()?
        .message()
        .map(|message| message.body.clone())
}

fn csf(state: &AppState, key: &str) -> String {
    App::csf_label(state, key, key)
}

fn csf_name(state: &AppState, key: &str, name: &str) -> String {
    format_csf(&csf(state, key), &[CsfArg::Str(name)])
}

fn squeeze(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, ' ' | '\n' | '\r'))
        .collect()
}
