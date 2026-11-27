//! The online entry and rematch route for the existing live-network observer.
//! Only ordinary keyboard/mouse adapters produce inputs. The HTTPS service,
//! relay, loading barrier and frame boundary run exactly as in an interactive app.

use super::*;
use crate::app::diagnostics::shell_capture::LiveOnlineConfig;
use crate::ui::wol_lobby;

mod watch;
use watch::WatchRun;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoundPhase {
    LanEntry,
    Main,
    Login,
    Lobby,
    /// Quick Match pressed: the runner, the matchbot's pairing and the
    /// automatic room run until gameplay.
    QuickMatch,
    /// `online-live-watch` from the settled lobby: the watch list, the
    /// watched match and Escape back to the lobby.
    Watch,
    Create,
    Discover,
    HostSetup,
    HostAi,
    HostReady,
    GuestSetup,
    Running,
    HoldFailureGuest,
    EndRound,
    Results,
    Returning,
    LeaveRoom,
    LeaveLobby,
    ExitSignIn,
    MainReturn,
    Finished,
}

pub(super) struct LiveRoundFlow {
    config: Option<LiveOnlineConfig>,
    random_map_input: Option<RandomMapInput>,
    phase: RoundPhase,
    rounds: Vec<Value>,
    current_round: Option<Value>,
    // The first round whose latency measurement was incomplete. Its receipt is
    // still written; the capture outcome then fails.
    latency_failure: Option<String>,
    menu_return: Option<Value>,
    result_rows: Vec<[String; 5]>,
    abort: Option<super::super::super::abort_return::AbortReturnInput>,
    route: Vec<Value>,
    last_refresh: Instant,
    last_heartbeat: Instant,
    failure_prefix_written: bool,
    profile: Option<super::profile::ProfileCheck>,
    ai_input: super::profile::SetupComboInput,
    /// The host saw its guest back in the reopened room after this match.
    guest_back: bool,
    /// Quick Match guest: when this lobby settled, and whether and when the
    /// host was seen in it and then left.
    quick_match_wait: Option<(Instant, bool, Option<Instant>)>,
    /// Quick Match: the paired room, recorded once per round.
    quick_match_room: Option<String>,
    watch: Option<WatchRun>,
}

impl LiveRoundFlow {
    pub(super) fn new(config: LiveOnlineConfig) -> Self {
        Self::with_config(Some(config))
    }

    pub(super) fn new_lan() -> Self {
        Self::with_config(None)
    }

    fn with_config(config: Option<LiveOnlineConfig>) -> Self {
        let phase = if config.is_some() {
            RoundPhase::Main
        } else {
            RoundPhase::LanEntry
        };
        let profile = config
            .as_ref()
            .filter(|config| config.profile_check)
            .map(|_| Default::default());
        Self {
            config,
            random_map_input: None,
            phase,
            rounds: Vec::new(),
            current_round: None,
            latency_failure: None,
            menu_return: None,
            result_rows: Vec::new(),
            abort: None,
            route: Vec::new(),
            last_refresh: Instant::now(),
            last_heartbeat: Instant::now(),
            failure_prefix_written: false,
            profile,
            ai_input: Default::default(),
            guest_back: false,
            quick_match_wait: None,
            quick_match_room: None,
            watch: None,
        }
    }

    /// The matchbot makes the earlier waiter the host, so the configured
    /// guest queues once the host has left this lobby for Quick Match (3 s
    /// later, for its list, join and request), or 30 s after the lobby
    /// settled without the host in it.
    fn quick_match_turn(&mut self, core: &LiveLanCapture, wol: &Option<WolVisit>) -> bool {
        if core.host() {
            return true;
        }
        let host = self.online_config().host.clone();
        let present = crate::app::shell_wol::lobby::member_names(wol.as_ref())
            .any(|name| name.eq_ignore_ascii_case(&host));
        let (since, seen, left) =
            self.quick_match_wait
                .get_or_insert((Instant::now(), false, None));
        if present {
            *seen = true;
            *left = None;
            return false;
        }
        if *seen {
            return left.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(3);
        }
        since.elapsed() >= Duration::from_secs(30)
    }

    /// LAN and online use the same normal chooser/generator input owner.
    /// Return false while its actual dialog/generation is still in progress.
    pub(super) fn prepare_host_map(
        &mut self,
        core: &mut LiveLanCapture,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
        selected_map: Option<&str>,
    ) -> Result<bool> {
        let mode_id = self.config.as_ref().and_then(|config| config.mode_id);
        let ai = self.config.as_ref().and_then(|config| config.ai);
        let prior_map = self
            .rounds
            .is_empty()
            .then(|| {
                core.config
                    .as_ref()
                    .and_then(|config| config.prior_map.clone())
            })
            .flatten();
        let selected_map = prior_map.as_deref().or(selected_map);
        if let Some(input) = self.random_map_input.as_mut() {
            if !input.after_present(state, frame, &mut self.route)? {
                return Ok(false);
            }
            self.random_map_input = None;
            core.prepare_host_setup(state, Some("RandMap.Sed"), self.players(), mode_id, ai)?;
            self.record(
                frame,
                "Generated seed, preview and start positions published",
            );
            return Ok(true);
        }
        let settled = if self.config.is_some() {
            online_setup_settled(&state.frontend, true)
        } else {
            setup_settled(&state.frontend, true)
        };
        if rendered != PresentedShell::NetworkSetup || !settled {
            return Ok(false);
        }
        if selected_map.is_some_and(|file| file.eq_ignore_ascii_case("RandMap.Sed"))
            && self.rounds.is_empty()
        {
            ensure!(
                self.players() + usize::from(ai.is_some()) <= 8,
                "random maps support two to eight players"
            );
            setup_button(state, crate::ui::multiplayer_setup::CHOOSE_MAP)?;
            self.random_map_input = Some(RandomMapInput::new(Some(
                self.players() + usize::from(ai.is_some()),
            )));
            self.record(frame, "Choose Map opened for normal random-map generation");
            return Ok(false);
        }
        if ai.is_some() && self.rounds.is_empty() {
            let restored = state
                .match_state
                .session
                .network_setup()
                .context("restored online setup")?;
            ensure!(
                restored.mode.class.policy().lists_in_skirmish,
                "ordinary online setup did not offer computer opponents"
            );
            self.record(
                frame,
                &format!(
                    "Restored computer-capable online mode {} before diagnostic map selection",
                    restored.mode.id
                ),
            );
        }
        core.prepare_host_setup(state, selected_map, self.players(), mode_id, ai)?;
        self.record(
            frame,
            "Requested retail mode, map and human slots configured",
        );
        Ok(true)
    }

    fn online_config(&self) -> &LiveOnlineConfig {
        self.config.as_ref().expect("online entry phase")
    }
    fn players(&self) -> usize {
        self.config.as_ref().map_or(2, |config| config.players)
    }
    pub(super) fn controls(&self, core: &LiveLanCapture) -> bool {
        self.config.is_some()
            || self.phase != RoundPhase::LanEntry
            || core.phase == Phase::Running
            || (core
                .config
                .as_ref()
                .is_some_and(|config| config.profile_check)
                && (core.phase == Phase::HostReady
                    || self
                        .profile
                        .as_ref()
                        .is_some_and(|profile| profile.started())))
    }

    pub(super) fn ready(&self) -> bool {
        self.phase == RoundPhase::Finished
    }

    /// The online part of a co-op capture's receipt.
    pub(super) fn cooperative_receipt(&self) -> Value {
        json!({"account":self.config.as_ref().map(|config| &config.account),
            "host":self.config.as_ref().map(|config| &config.host),"route":self.route})
    }

    fn record(&mut self, frame: u32, action: &str) {
        log::info!("Network capture round {}: {action}", self.rounds.len() + 1);
        self.route
            .push(json!({"render_frame":frame,"action":action,"round":self.rounds.len()+1}));
    }

    fn write_prefix(
        &mut self,
        core: &LiveLanCapture,
        frontend: &FrontendState,
        match_state: &MatchState,
        frame: u32,
    ) -> Result<()> {
        if self.failure_prefix_written {
            return Ok(());
        }
        ensure!(
            core.checksums.len() >= REQUIRED_FRAMES as usize,
            "failure prefix did not retain every real checksum"
        );
        let (path, schema, account, host) = if let Some(config) = &self.config {
            (
                config
                    .failure_prefix_output
                    .as_ref()
                    .context("failure prefix path")?,
                "vera20k.online-failure-prefix.v1",
                config.account.as_str(),
                config.host.as_str(),
            )
        } else {
            (
                core.config
                    .as_ref()
                    .and_then(|config| config.failure_prefix_output.as_ref())
                    .context("LAN desync prefix path")?,
                "vera20k.lan-desync-prefix.v1",
                if core.host() { HOST_NAME } else { GUEST_NAME },
                HOST_NAME,
            )
        };
        let prefix_frame = match_state
            .sim_runtime
            .runtime()
            .context("prefix world")?
            .simulation
            .session()
            .binary_frame();
        let primary_port = match_state
            .live_network
            .as_ref()
            .context("prefix transport")?
            .primary_addr()?
            .port();
        let prefix = json!({"schema":schema,"account":account,"host":host,
            "prefix_frame":prefix_frame,"primary_port":primary_port,
            "external_fault_pending":true,"runtime":core.observed_receipt(frontend, match_state)?});
        write_new_file(path, &serde_json::to_vec_pretty(&prefix)?)?;
        self.failure_prefix_written = true;
        self.record(frame, "Real300-frame command/checksum prefix saved; external fault may begin after both receipts agree");
        Ok(())
    }

    fn heartbeat(
        &mut self,
        renderer: &RendererState,
        frontend: &FrontendState,
        match_state: &MatchState,
        process_assets: &ProcessAssets,
        rendered: PresentedShell,
    ) {
        if self.last_heartbeat.elapsed() < std::time::Duration::from_secs(10) {
            return;
        }
        self.last_heartbeat = Instant::now();
        let (page, pending) = frontend.wol.as_ref().map_or(("none", 0), |visit| {
            use crate::app::shell_wol::WolPage;
            let page = match &visit.page {
                WolPage::Welcome => "welcome",
                WolPage::Backdrop { .. } => "backdrop",
                WolPage::Login(_) => "login",
                WolPage::Message { .. } => "message",
                WolPage::Lobby => "lobby",
                WolPage::Options(_) => "options",
                WolPage::AutoLoginQuery => "auto_login",
                WolPage::Locale(_) => "locale",
                WolPage::MyInformation(_) => "my_information",
                WolPage::Buddy(_) => "buddy",
                WolPage::QuickMatch(_) => "quick_match",
                WolPage::AddBuddyQuery(_) => "add_buddy",
            };
            (page, visit.pending_service_request_count())
        });
        let (lobby_shown, lobby_wait, lobby_message) = frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .map_or((false, false, false), |lobby| {
                (lobby.shown, lobby.wait.is_some(), lobby.message().is_some())
            });
        let session = &match_state.session;
        let setup = session.network_setup();
        let setup_humans = setup.map(|launch| launch.humans.len());
        let bound_rows = setup.map(|launch| {
            launch
                .humans
                .iter()
                .filter(|human| human.raw_slot_index.is_some())
                .count()
        });
        let seed_present = session.network_start_seed().is_some();
        let map_capacity = session
            .network_map_metadata()
            .map(|metadata| metadata.player_capacity);
        let setup_capacity = session.network_setup_capacity();
        let selected_map_present = setup.is_some_and(|launch| launch.selected_map_file.is_some());
        let local = setup.and_then(|launch| launch.humans.get(launch.local_player_index));
        let local_row = local.and_then(|human| human.raw_slot_index);
        let local_start = local.map(|human| human.slot.start_position);
        let accept_enabled = setup::view(renderer, frontend, match_state, process_assets)
            .map(|view| view.accept_enabled);
        let desired_start = self.config.as_ref().and_then(|config| config.start);
        let sim = match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.simulation);
        let native_frame = sim.map(|sim| sim.session().binary_frame());
        let fps = sim
            .and_then(|sim| sim.session().network_command_timing())
            .map(|timing| timing.frames_per_second());
        let max_ahead = sim.and_then(|sim| sim.session().network_max_ahead_frames());
        let local_commands = sim.map(|sim| sim.pending_local_command_count());
        let live_wait = match_state
            .live_network
            .as_ref()
            .and_then(|live| live.wait_info())
            .map(|wait| wait.kind);
        let live_phase = match_state
            .live_network
            .as_ref()
            .and_then(|live| live.phase());
        log::info!(
            "Network capture heartbeat: phase={:?}, page={page}, presented={rendered:?}, shell_exit={}, pending_requests={pending}, native_frame={native_frame:?}, timing_fps={fps:?}, max_ahead={max_ahead:?}, local_commands={local_commands:?}, live_wait={live_wait:?}, live_phase={live_phase:?}, setup_humans={setup_humans:?}, bound_rows={bound_rows:?}, seed_present={seed_present}, map_capacity={map_capacity:?}, setup_capacity={setup_capacity:?}, selected_map_present={selected_map_present}, local_row={local_row:?}, local_start={local_start:?}, accept_enabled={accept_enabled:?}, desired_start={desired_start:?}, lobby_shown={lobby_shown}, lobby_wait={lobby_wait}, lobby_message={lobby_message}, slide_first_paint={}, active_slide={:?}",
            self.phase,
            frontend.shell_exit.is_some(),
            frontend.shell_first_paint_slide.is_some(),
            frontend.shell_slide_active_shell,
        );
        if let (Some(sim), Some(live)) = (sim, match_state.live_network.as_ref()) {
            crate::app::match_runtime::network_match::log_live_progress(
                session,
                sim,
                live,
                "heartbeat",
            );
        }
    }

    /// First-time accounts pass through the same location and auto-login
    /// questions as interactive sign-in. Wait for their ordinary close waves.
    fn login_questions(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<bool> {
        if self.phase != RoundPhase::Lobby
            || state.frontend.shell_exit.is_some()
            || self.config.is_none()
        {
            return Ok(false);
        }
        use crate::app::shell_wol::{WolPage, persona};
        use crate::ui::wol_persona::{ACCEPT, DECLINE};
        match rendered {
            PresentedShell::WolLocale
                if MoviesCapture::slide_settled(&state.frontend, ShellSlideKind::WolLocale) =>
            {
                let layout = persona::layout(state.frontend.wol.as_ref(), &state.renderer.gpu)
                    .context("online location dialog")?;
                let Some(WolPage::Locale(page)) =
                    state.frontend.wol.as_ref().map(|visit| &visit.page)
                else {
                    bail!("online location dialog lost its model");
                };
                if page.rows.is_empty() {
                    return Ok(true);
                }
                let row = page
                    .rows
                    .iter()
                    .position(|row| row.locale != 0)
                    .unwrap_or(0);
                let (geometry, top) = persona::list_view(&state.frontend.shell_controller)
                    .context("online location list")?;
                ensure!(
                    row >= top && row < top + geometry.visible_rows,
                    "online location choice is outside the visible list"
                );
                let row_rect = geometry.row(row - top);
                click(state, row_rect);
                click(
                    state,
                    layout
                        .page
                        .button_rect(ACCEPT)
                        .context("online location OK")?,
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "online location did not close"
                );
                self.record(frame, "Location selected");
                Ok(true)
            }
            PresentedShell::WolAutoLoginQuery
                if MoviesCapture::slide_settled(
                    &state.frontend,
                    ShellSlideKind::WolAutoLoginQuery,
                ) =>
            {
                let layout = persona::layout(state.frontend.wol.as_ref(), &state.renderer.gpu)
                    .context("online auto-login dialog")?;
                click(
                    state,
                    layout.page.button_rect(DECLINE).context("auto-login No")?,
                );
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "auto-login question did not close"
                );
                self.record(frame, "Auto-login declined");
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn after_present(
        &mut self,
        core: &mut LiveLanCapture,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        self.heartbeat(
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.process_assets,
            rendered,
        );
        if self.profile.is_none()
            && core
                .config
                .as_ref()
                .is_some_and(|config| config.profile_check)
        {
            self.profile = Some(Default::default());
        }
        if let Some(profile) = self.profile.as_mut() {
            let online = self.config.is_some();
            let start = if online {
                self.phase == RoundPhase::HostReady
            } else {
                core.phase == Phase::HostReady
            };
            if start || profile.started() {
                match profile.step(state, rendered, online, frame)? {
                    super::profile::ProfileStep::Ordinary => {}
                    super::profile::ProfileStep::Waiting => return Ok(()),
                    super::profile::ProfileStep::Recreate => {
                        if online {
                            self.phase = RoundPhase::Lobby;
                        } else {
                            create_lan_game(state)?;
                        }
                        self.record(frame,"Host profile Back returned to lobby; ordinary Create repeats the same profile");
                        return Ok(());
                    }
                    super::profile::ProfileStep::Exit => {
                        if online {
                            click(
                                state,
                                crate::app::shell_wol::lobby::layout(
                                    state.frontend.wol.as_ref(),
                                    &state.renderer.gpu,
                                )
                                .page
                                .button_rect(wol_lobby::BACK)
                                .context("profile lobby Back")?,
                            );
                            self.phase = RoundPhase::ExitSignIn;
                        } else {
                            lobby_button(state, crate::ui::lan_shell::BACK)?;
                            MoviesCapture::restore_neutral_pointer(
                                &state.renderer.gpu,
                                &mut state.match_state.input,
                            );
                            self.phase = RoundPhase::MainReturn;
                        }
                        self.record(frame,"Extended and native profile rows retained after actual Back/Create; return to Menu");
                        return Ok(());
                    }
                }
            }
        }
        if self.login_questions(state, rendered, frame)? {
            return Ok(());
        }
        if self.config.is_some()
            && !core.host()
            && self.phase == RoundPhase::GuestSetup
            && lobby_ready(&state.frontend, rendered)
        {
            ensure!(
                state.match_state.session.network_setup().is_none(),
                "returned guest retained an admitted setup"
            );
            core.country_selected = false;
            self.phase = RoundPhase::Discover;
            self.last_refresh = Instant::now();
            self.record(
                frame,
                "Actual setup departure returned to the lobby; rediscover and rejoin normally",
            );
            return Ok(());
        }
        if self.phase == RoundPhase::LanEntry && core.phase == Phase::Running {
            self.record(frame, "Real LAN loading reached synchronized gameplay");
            self.phase = RoundPhase::Running;
        }
        let online = self.config.is_some();
        // A kept online room can promote a surviving guest to host.
        let host = if online {
            state.match_state.session.host()
        } else {
            core.host()
        };
        let online_room = online
            && rendered == PresentedShell::NetworkSetup
            && online_setup_settled(&state.frontend, host);
        // On LAN the guest rejoins the host after the room reopens. Note
        // the visit even while the room still slides in.
        if !online
            && host
            && matches!(self.phase, RoundPhase::Returning | RoundPhase::LeaveRoom)
            && rendered == PresentedShell::NetworkSetup
            && state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| launch.humans.len() == self.players())
        {
            self.guest_back = true;
        }
        match self.phase {
            RoundPhase::Main
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready_for(
                        MainMenuCaptureSnapshot::from_state(
                            &state.platform,
                            &state.renderer,
                            &state.frontend,
                            &state.match_state,
                            &state.persistence,
                        ),
                        self.config.as_ref().is_some_and(|config| config.latency),
                    )? =>
            {
                LanCapture::isolated_root(&state.process_assets)?;
                click_main_menu(
                    state,
                    crate::ui::main_menu_shell::MainMenuControlId::WwOnline0x684,
                )?;
                self.record(frame, "Multiplayer Internet selected");
                self.phase = RoundPhase::Login;
            }
            RoundPhase::Login
                if rendered == PresentedShell::WolLogin
                    && MoviesCapture::slide_settled(&state.frontend, ShellSlideKind::WolLogin) =>
            {
                let chooser = crate::app::shell_wol::login_layout(
                    state.frontend.wol.as_ref(),
                    &state.renderer.gpu,
                )
                .entry_choice()
                .context("online entry chooser")?;
                click(state, chooser.guest);
                // The live clients locate each other's games by their agreed
                // command-line names. Set that fixture identity before the
                // normal slide-out commits the guest's Connect request.
                ensure!(
                    state.frontend.shell_exit.is_some(),
                    "Play as Guest did not start the login slide-out"
                );
                let identity = &mut state.frontend.wol_account;
                identity.nickname.clone_from(&self.online_config().account);
                identity.generated_guest = false;
                self.record(
                    frame,
                    "Play as Guest selected; using coordinated capture nickname",
                );
                self.phase = RoundPhase::Lobby;
            }
            RoundPhase::Lobby if lobby_ready(&state.frontend, rendered) => {
                if core.target == LanTarget::OnlineWatch {
                    self.watch_from_lobby(frame);
                    return Ok(());
                }
                if self.online_config().quick_match {
                    if !self.quick_match_turn(core, &state.frontend.wol) {
                        return Ok(());
                    }
                    let layout = crate::app::shell_wol::lobby::layout(
                        state.frontend.wol.as_ref(),
                        &state.renderer.gpu,
                    );
                    click(
                        state,
                        layout
                            .page
                            .button_rect(wol_lobby::QUICK_MATCH)
                            .context("Quick Match button")?,
                    );
                    self.quick_match_wait = None;
                    self.quick_match_room = None;
                    self.record(frame, "Quick Match selected");
                    self.phase = RoundPhase::QuickMatch;
                    return Ok(());
                }
                if core.host() {
                    let layout = crate::app::shell_wol::lobby::layout(
                        state.frontend.wol.as_ref(),
                        &state.renderer.gpu,
                    );
                    click(
                        state,
                        layout
                            .page
                            .button_rect(wol_lobby::CREATE)
                            .context("Create Game button")?,
                    );
                    self.record(frame, "Create Game selected");
                    self.phase = RoundPhase::Create;
                } else {
                    self.phase = RoundPhase::Discover;
                }
            }
            RoundPhase::Create => {
                let open = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.lobby())
                    .is_some_and(|lobby| lobby.ui.popup.is_some());
                if open {
                    let size = &state.renderer.gpu.config;
                    let layout = crate::ui::shell::modal::wol_create_game_layout(
                        size.width as i32,
                        size.height as i32,
                        true,
                    );
                    click(state, layout.ok);
                    self.record(frame, "Create Game confirmed");
                    self.phase = RoundPhase::HostSetup;
                }
            }
            RoundPhase::Discover if lobby_ready(&state.frontend, rendered) => {
                let expected = format!("{}'s_game", self.online_config().host);
                let selected = state
                    .frontend
                    .wol
                    .as_ref()
                    .and_then(|visit| visit.lobby())
                    .and_then(|lobby| {
                        lobby
                            .ui
                            .game_rows
                            .iter()
                            .position(|row| row.name.eq_ignore_ascii_case(&expected))
                    });
                if let Some(row) = selected {
                    if !join_game_row(state, row)? {
                        self.record(
                            frame,
                            "Game list scrolled toward the configured discovered room",
                        );
                        return Ok(()); // Re-read the row on its next presented frame.
                    }
                    self.record(frame, "Configured discovered game joined");
                    self.phase = RoundPhase::GuestSetup;
                } else if self.last_refresh.elapsed() >= Duration::from_secs(2) {
                    let layout = crate::app::shell_wol::lobby::layout(
                        state.frontend.wol.as_ref(),
                        &state.renderer.gpu,
                    );
                    click(
                        state,
                        layout
                            .control_rect(wol_lobby::REFRESH)
                            .context("Refresh image button")?,
                    );
                    self.last_refresh = Instant::now();
                }
            }
            RoundPhase::HostSetup if core.cooperative.is_some() => {
                let run = core.cooperative.as_mut().context("co-op run")?;
                if run.prepare_host(state, rendered, &mut self.route, frame)? {
                    core.open_human_rows(state, None)?;
                    self.record(frame, "Cooperative campaign chosen through Choose Map");
                    self.phase = RoundPhase::HostReady;
                }
            }
            RoundPhase::HostSetup => {
                let selected = self.online_config().map.clone();
                if self.prepare_host_map(core, state, rendered, frame, selected.as_deref())? {
                    self.phase = if self.online_config().ai.is_some() {
                        RoundPhase::HostAi
                    } else {
                        RoundPhase::HostReady
                    };
                }
            }
            RoundPhase::HostAi
                if rendered == PresentedShell::NetworkSetup
                    && online_setup_settled(&state.frontend, true) =>
            {
                let difficulty = self.online_config().ai.context("explicit AI difficulty")?;
                let launch = state
                    .match_state
                    .session
                    .network_setup()
                    .context("AI host setup")?;
                let retained_row = launch.opponents.first().map(|ai| ai.raw_slot_index);
                let value = match difficulty {
                    crate::skirmish_launch::AiDifficulty::Easy => 4,
                    crate::skirmish_launch::AiDifficulty::Normal => 5,
                    crate::skirmish_launch::AiDifficulty::Hard => 6,
                };
                let view = setup::view(
                    &state.renderer,
                    &state.frontend,
                    &state.match_state,
                    &state.process_assets,
                )
                .context("AI Player dropdown view")?;
                let row = view
                    .combos
                    .iter()
                    .find(|combo| {
                        SetupColumn::decode(combo.id)
                            .is_some_and(|(column, _)| column == SetupColumn::Player)
                            && combo.visible
                            && combo.enabled
                            && retained_row.map_or(
                                state.match_state.session.network_vacancy(combo.row)
                                    == Some(
                                        crate::app::match_runtime::session::NetworkVacancy::Open,
                                    ),
                                |row| combo.row == row,
                            )
                            && combo.choices.iter().any(|choice| choice.value == value)
                    })
                    .map(|combo| usize::from(combo.row))
                    .context("ordinary Open row offering the requested AI difficulty")?;
                if self
                    .ai_input
                    .select(state, SetupColumn::Player, row, value)?
                {
                    let launch = state
                        .match_state
                        .session
                        .network_setup()
                        .context("selected AI setup")?;
                    ensure!(
                        launch.humans.len() == 1
                            && launch.opponents.len() == 1
                            && launch.opponents[0].raw_slot_index == row as u8
                            && launch.opponents[0].difficulty == difficulty,
                        "Player dropdown did not publish exactly one canonical AI row"
                    );
                    self.record(
                        frame,
                        "Explicit AI selected through the presented Player dropdown",
                    );
                    self.phase = RoundPhase::HostReady;
                }
            }
            RoundPhase::HostReady if online_setup_settled(&state.frontend, true) => {
                if core.select_start(state, self.online_config().start)? {
                    self.record(
                        frame,
                        "Configured authored start selected through the local Start combo",
                    );
                    return Ok(());
                }
                let session = &state.match_state.session;
                let ready = session.network_setup().is_some_and(|launch| {
                    launch.humans.len() == self.players()
                        && launch.humans.iter().enumerate().all(|(i, human)| {
                            human.raw_slot_index.is_some()
                                && session.network_player_ready(i)
                                && session.network_map_status(i) == Some(1)
                        })
                });
                let starting = crate::app::shell_network_setup::start_phase(
                    state.frontend.network_setup_protocol.as_ref(),
                )
                .in_progress();
                let start = self.online_config().start;
                if ready
                    && !starting
                    && core.start_matches(&state.match_state.session, start)
                    && core.choices_made(&state.match_state.session, start.is_some())
                {
                    setup_button(state, crate::ui::multiplayer_setup::START)?;
                    self.record(
                        frame,
                        "All real readiness acknowledgements present; Start clicked",
                    );
                }
            }
            RoundPhase::GuestSetup if online_setup_settled(&state.frontend, false) => {
                if self.online_config().start.is_some()
                    && !core.start_selection_admitted(&state.match_state.session)
                {
                    return Ok(());
                }
                // The guest's game accepts the received map and options by
                // itself, and again after each echoed choice, so the run only
                // makes its choices and stays in editable setup until Start.
                let country_was_selected = core.country_selected;
                core.select_country(state)?;
                if !country_was_selected && core.country_selected {
                    // One normal control edit per frame.
                    return Ok(());
                }
                if core.select_start(state, self.online_config().start)? {
                    self.record(
                        frame,
                        "Configured authored start selected through the local Start combo",
                    );
                }
            }
            RoundPhase::HostReady | RoundPhase::GuestSetup | RoundPhase::QuickMatch
                if state.frontend.screen == GameScreen::InGame =>
            {
                ensure!(
                    state.match_state.live_network.is_some(),
                    "online match has no production command transport"
                );
                ensure!(
                    self.phase != RoundPhase::QuickMatch || self.quick_match_room.is_some(),
                    "Quick Match reached gameplay before its room was checked"
                );
                core.startup = Some(startup_receipt(&state.frontend.wol, &state.match_state)?);
                if let Some(run) = core.cooperative.as_mut() {
                    run.match_loaded(&state.match_state.session);
                }
                if let Some(ai) = core.ai.as_mut() {
                    ai.observe(&state.match_state)?;
                }
                core.phase = Phase::Running;
                self.record(
                    frame,
                    "Authenticated relay and synchronized loading reached gameplay",
                );
                self.phase = RoundPhase::Running;
            }
            // The paired room is the host's `<host>'s_game`: it names the
            // matchbot's host, which must be the configured one. A second
            // pairing means the first room never started.
            RoundPhase::QuickMatch => {
                let Some(room) =
                    crate::app::shell_wol::lobby::current_channel(state.frontend.wol.as_ref())
                        .filter(|channel| channel.kind == wol_lobby::GAME_CHANNEL)
                else {
                    return Ok(());
                };
                if self.quick_match_room.is_some() {
                    ensure!(
                        self.quick_match_room.as_deref() == Some(room.name.as_str()),
                        "the matchbot paired again in {} after {:?} never started",
                        room.name,
                        self.quick_match_room
                    );
                    return Ok(());
                }
                let expected = format!("{}'s_game", self.online_config().host);
                ensure!(
                    room.name.eq_ignore_ascii_case(&expected),
                    "the matchbot paired in {} instead of {expected}",
                    room.name
                );
                self.record(frame, "Matchbot pairing joined the host's room");
                self.quick_match_room = Some(room.name);
            }
            RoundPhase::Running if core.cooperative.is_some() => {
                let host = core.host();
                let run = core.cooperative.as_mut().context("co-op run")?;
                if rendered == PresentedShell::NetworkSetup
                    && run.room_returned(
                        &state.frontend,
                        &state.match_state.session,
                        &state.process_assets,
                        host,
                        &mut self.route,
                        frame,
                    )?
                {
                    // The next stage starts from the room as a first start does.
                    self.phase = if host {
                        RoundPhase::HostReady
                    } else {
                        RoundPhase::GuestSetup
                    };
                    return Ok(());
                }
                run.step(state, &mut self.route, frame)?;
                if run.done() {
                    core.phase = Phase::Finished;
                    self.record(frame, "The kept room's next stage ran");
                    self.phase = RoundPhase::Finished;
                }
            }
            RoundPhase::Running => {
                core.observe_run(state, frame, self.players())?;
                if core.failure.as_ref().is_some_and(|failure| failure.armed()) {
                    self.write_prefix(core, &state.frontend, &state.match_state, frame)?;
                }
                if core.phase == Phase::Finished {
                    if core.target == LanTarget::OnlineFailureGuest {
                        self.write_prefix(core, &state.frontend, &state.match_state, frame)?;
                        self.record(frame, "Guest remains live for external termination; no Observe or fabricated departure");
                        self.phase = RoundPhase::HoldFailureGuest;
                        return Ok(());
                    }
                    self.current_round = Some(
                        json!({"runtime":core.round_receipt(&state.frontend, &state.match_state)?}),
                    );
                    if self.latency_failure.is_none()
                        && let Some(reason) = core
                            .latency
                            .as_ref()
                            .and_then(super::latency::LatencyCapture::incomplete)
                    {
                        self.latency_failure =
                            Some(format!("round {}: {reason}", self.rounds.len() + 1));
                    }
                    self.result_rows.clear();
                    if let Some(failure) = &core.failure {
                        if failure.returned() {
                            // Logical cleanup may finish during the final
                            // transition render, still presented as Other.
                            // The existing return owner waits for the actual
                            // settled lobby before committing this receipt.
                            self.phase = RoundPhase::Returning;
                        } else {
                            self.record(
                                frame,
                                "Expected timeout removed the peer; awaiting ordinary result page",
                            );
                            self.phase = RoundPhase::Results;
                        }
                        return Ok(());
                    }
                    // Alternate the player choosing Observe so both ends exercise
                    // the local defeat path as well as the surviving player's result.
                    let observe = if self
                        .config
                        .as_ref()
                        .is_some_and(|config| config.ai.is_some())
                    {
                        // No remote human can end this round. The sole local
                        // source takes the real Leave path each time.
                        true
                    } else if self.players() > 2 {
                        !core.host()
                    } else {
                        core.host() == (self.rounds.len() == 1)
                    };
                    if observe {
                        use super::super::super::abort_return::AbortGoal;
                        use crate::ui::shell::abort::AbortButton;
                        let solo_ai = self
                            .config
                            .as_ref()
                            .is_some_and(|config| config.ai.is_some());
                        let quick_match = self
                            .config
                            .as_ref()
                            .is_some_and(|config| config.quick_match);
                        self.abort = Some(if solo_ai {
                            // Native B6 hides Observe with fewer than two live
                            // humans (4F1938). Use its ordinary Leave button.
                            super::super::super::abort_return::AbortReturnInput::begin(
                                state,
                                &mut self.route,
                                frame,
                                AbortGoal::Choose(Some(AbortButton::Leave)),
                                true,
                            )?
                        } else if quick_match {
                            // A Quick Match game is a tournament game: Abort
                            // opens the Surrender box instead of B6.
                            super::super::super::abort_return::AbortReturnInput::begin(
                                state,
                                &mut self.route,
                                frame,
                                AbortGoal::Surrender { press_ok: true },
                                true,
                            )?
                        } else {
                            super::super::super::abort_return::AbortReturnInput::begin(
                                state,
                                &mut self.route,
                                frame,
                                AbortGoal::Choose(Some(AbortButton::Secondary)),
                                false,
                            )?
                        });
                        self.record(
                            frame,
                            if solo_ai {
                                "Leave requested through in-game Options"
                            } else if quick_match {
                                "Surrender requested through in-game Options"
                            } else {
                                "Observe requested through in-game Options"
                            },
                        );
                        self.phase = RoundPhase::EndRound;
                    } else {
                        self.phase = RoundPhase::Results;
                    }
                }
            }
            RoundPhase::HoldFailureGuest => {
                // No input or completion action. The normal app keeps serving
                // gameplay; only killing this process ends it.
            }
            RoundPhase::EndRound => {
                if self
                    .abort
                    .as_mut()
                    .context("round exit input")?
                    .after_present(state, &mut self.route, frame)?
                {
                    self.record(
                        frame,
                        if self
                            .config
                            .as_ref()
                            .is_some_and(|config| config.ai.is_some())
                        {
                            "Leave selected through B6; real EXIT queued"
                        } else if self
                            .config
                            .as_ref()
                            .is_some_and(|config| config.quick_match)
                        {
                            "Surrender OK; real DESTRUCT and EXIT queued"
                        } else {
                            "Observe selected through B6; real DESTRUCT queued"
                        },
                    );
                    self.phase = RoundPhase::Results;
                }
            }
            RoundPhase::Results
                if rendered == PresentedShell::Score
                    && MoviesCapture::slide_settled(&state.frontend, ShellSlideKind::Score)
                    && state
                        .frontend
                        .score_page
                        .as_ref()
                        .is_some_and(|page| page.statics.is_terminal()) =>
            {
                let page = state.frontend.score_page.as_ref().context("score page")?;
                let visible = page.visible_row_texts();
                let range = page.visible_range();
                let expected: Vec<[String; 5]> = page.model.rows[range.clone()]
                    .iter()
                    .map(|row| {
                        [
                            row.name.clone(),
                            row.kills.to_string(),
                            row.losses.to_string(),
                            row.built.to_string(),
                            row.score.to_string(),
                        ]
                    })
                    .collect();
                ensure!(
                    visible == expected,
                    "score page did not display every expected row and statistic"
                );
                ensure!(
                    self.result_rows.len() == range.start,
                    "score pages skipped or repeated rows"
                );
                self.result_rows.extend(visible);
                if page.page_index() + 1 < page.page_count() {
                    let rect =
                        crate::app::frontend::shell_layout::score_layout(&state.renderer.gpu)
                            .paging_button(crate::ui::score_shell::NEXT_PAGE)
                            .context("score Next")?;
                    set_window_pointer_to_rect_center(
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state.input,
                        rect,
                    );
                    App::handle_score_shell_mouse(state, None);
                    App::handle_score_shell_mouse(state, Some(true));
                    App::handle_score_shell_mouse(state, Some(false));
                    MoviesCapture::restore_neutral_pointer(
                        &state.renderer.gpu,
                        &mut state.match_state.input,
                    );
                    self.record(frame, "Next results page displayed through normal button");
                    return Ok(());
                }
                ensure!(
                    self.result_rows.len() == page.model.rows.len(),
                    "results capture omitted players"
                );
                let model = &state
                    .frontend
                    .score_page
                    .as_ref()
                    .context("score page")?
                    .model;
                let score = json!({"game_number":model.game_number,"elapsed_seconds":model.elapsed_seconds,
                    "rows":model.rows.iter().map(|row|json!({"name":row.name,"kills":row.kills,
                        "losses":row.losses,"built":row.built,"score":row.score})).collect::<Vec<_>>()});
                self.current_round.as_mut().context("completed round")?["result"] = score;
                self.current_round.as_mut().context("completed round")?["result"]["displayed_table_rows"] =
                    json!(self.result_rows);
                self.current_round.as_mut().context("completed round")?["terminal_simulation"] =
                    terminal_simulation_receipt(&state.match_state)?;
                let rect = crate::app::frontend::shell_layout::score_layout(&state.renderer.gpu)
                    .page
                    .button_rect(crate::ui::score_shell::CONTINUE_BUTTON)
                    .context("score Continue")?;
                set_window_pointer_to_rect_center(
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state.input,
                    rect,
                );
                App::handle_score_shell_mouse(state, None);
                App::handle_score_shell_mouse(state, Some(true));
                App::handle_score_shell_mouse(state, Some(false));
                MoviesCapture::restore_neutral_pointer(
                    &state.renderer.gpu,
                    &mut state.match_state.input,
                );
                self.record(frame, "Real multiplayer results acknowledged");
                self.phase = RoundPhase::Returning;
            }
            RoundPhase::Results
                if self
                    .config
                    .as_ref()
                    .is_some_and(|config| config.ai.is_some())
                    && (online_room || lobby_ready(&state.frontend, rendered)) =>
            {
                self.current_round.as_mut().context("completed AI round")?["result"] =
                    json!({"shown":false,"completion":"ordinary B6 Leave"});
                self.record(frame, "Singleton Leave returned without a results page");
                self.phase = RoundPhase::Returning;
            }
            // Surrender's EXIT ends the match for its player at once.
            RoundPhase::Results
                if self
                    .config
                    .as_ref()
                    .is_some_and(|config| config.quick_match)
                    && self.abort.is_some()
                    && lobby_ready(&state.frontend, rendered) =>
            {
                self.current_round
                    .as_mut()
                    .context("completed Quick Match round")?["result"] =
                    json!({"shown":false,"completion":"Surrender returned directly to lobby"});
                self.record(
                    frame,
                    "Surrender returned to the lobby without a results page",
                );
                self.phase = RoundPhase::Returning;
            }
            RoundPhase::Returning
                if if online {
                    lobby_ready(&state.frontend, rendered)
                        || (core.failure.is_none() && online_room)
                } else if returns_to_room(core, &state.frontend) {
                    room_returned(core, &state.frontend, &state.match_state.session, rendered)
                } else {
                    rendered == PresentedShell::LanLobby && lobby_settled(&state.frontend)
                } =>
            {
                let room = rendered == PresentedShell::NetworkSetup;
                ensure!(
                    state.match_state.live_network.is_none(),
                    "finished round retained live command owner"
                );
                ensure!(
                    room || state.match_state.session.network_setup().is_none(),
                    "finished round retained setup"
                );
                let mut round = self
                    .current_round
                    .take()
                    .context("completed round result")?;
                round[if room {
                    "returned_to_room"
                } else {
                    "returned_to_lobby"
                }] = json!(true);
                self.rounds.push(round);
                self.abort = None;
                self.record(
                    frame,
                    if core
                        .failure
                        .as_ref()
                        .is_some_and(|failure| failure.returned())
                    {
                        if core.target.desync_failure() {
                            "Actual checksum CE Stop returned to the settled LAN lobby"
                        } else {
                            "Expected connection Quit returned to the authenticated online lobby"
                        }
                    } else if room {
                        "Returned to VERA's reopened room through normal result continuation"
                    } else {
                        "Returned to service lobby through normal result continuation"
                    },
                );
                if core.failure.is_some() {
                    self.phase = RoundPhase::Finished;
                } else if self.rounds.len() == 2 {
                    self.phase = if room {
                        RoundPhase::LeaveRoom
                    } else {
                        RoundPhase::LeaveLobby
                    };
                } else {
                    reset_round(core);
                    self.guest_back = false;
                    self.phase = if online && room {
                        if host {
                            RoundPhase::HostSetup
                        } else {
                            RoundPhase::GuestSetup
                        }
                    } else if online {
                        RoundPhase::Lobby
                    } else if room {
                        // Already in the room: the host sets up the next
                        // match again and the guest waits for its Start.
                        core.phase = if host {
                            Phase::HostSetup
                        } else {
                            Phase::GuestSetup
                        };
                        core.event(frame, "next match set up in VERA's reopened room");
                        RoundPhase::LanEntry
                    } else {
                        core.phase = Phase::Lobby;
                        RoundPhase::LanEntry
                    };
                }
            }
            RoundPhase::LeaveRoom
                if (online_room || (!online && rendered == PresentedShell::NetworkSetup && setup_settled(&state.frontend, host)))
                    // Online the service reopens with everyone seated. A guest
                    // can leave before the host draws that first roster.
                    && (!host
                        || (online || self.guest_back)
                            && state
                                .match_state
                                .session
                                .network_setup()
                                .is_some_and(|launch| launch.humans.len() == 1)) =>
            {
                setup_button(state, crate::ui::multiplayer_setup::BACK)?;
                MoviesCapture::restore_neutral_pointer(
                    &state.renderer.gpu,
                    &mut state.match_state.input,
                );
                self.record(
                    frame,
                    if host {
                        "Host left the reopened room through Back after the other seats cleared"
                    } else {
                        "Guest left the reopened room through Back"
                    },
                );
                self.phase = RoundPhase::LeaveLobby;
            }
            RoundPhase::LeaveLobby
                if if online {
                    lobby_ready(&state.frontend, rendered)
                } else {
                    rendered == PresentedShell::LanLobby && lobby_settled(&state.frontend)
                } =>
            {
                ensure!(
                    state.match_state.session.network_setup().is_none(),
                    "Back from the reopened room retained its setup"
                );
                if online {
                    let rect = crate::app::shell_wol::lobby::layout(
                        state.frontend.wol.as_ref(),
                        &state.renderer.gpu,
                    )
                    .page
                    .button_rect(wol_lobby::BACK)
                    .context("WOL lobby Back button")?;
                    click(state, rect);
                    self.phase = RoundPhase::ExitSignIn;
                } else {
                    lobby_button(state, crate::ui::lan_shell::BACK)?;
                    MoviesCapture::restore_neutral_pointer(
                        &state.renderer.gpu,
                        &mut state.match_state.input,
                    );
                    self.phase = RoundPhase::MainReturn;
                }
                self.record(
                    frame,
                    "Final settled lobby Back clicked through ordinary input",
                );
            }
            RoundPhase::Watch => self.watch_step(state, rendered, frame)?,
            RoundPhase::ExitSignIn
                if rendered == PresentedShell::WolLogin
                    && MoviesCapture::slide_settled(&state.frontend, ShellSlideKind::WolLogin) =>
            {
                let layout = crate::app::shell_wol::login_layout(
                    state.frontend.wol.as_ref(),
                    &state.renderer.gpu,
                );
                let rect = layout
                    .page
                    .button_rect(crate::ui::wol_login::BACK)
                    .context("WOL sign-in Back button")?;
                click(state, rect);
                if layout.entry_choice().is_some() {
                    self.record(
                        frame,
                        "Online entry chooser Back clicked through ordinary input",
                    );
                    self.phase = RoundPhase::MainReturn;
                } else {
                    ensure!(
                        crate::app::shell_wol::login_layout(
                            state.frontend.wol.as_ref(),
                            &state.renderer.gpu
                        )
                        .entry_choice()
                        .is_some(),
                        "Back did not return to the online entry chooser"
                    );
                }
            }
            RoundPhase::MainReturn
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready_for(
                        MainMenuCaptureSnapshot::from_state(
                            &state.platform,
                            &state.renderer,
                            &state.frontend,
                            &state.match_state,
                            &state.persistence,
                        ),
                        self.config.as_ref().is_some_and(|config| config.latency),
                    )? =>
            {
                if crate::app::shell_wol::online_cleanup_pending(
                    &state.frontend,
                    &state.match_state,
                ) {
                    return Ok(());
                }
                let visits_retired = state.frontend.lan.is_none() && state.frontend.wol.is_none();
                let setup_retired = state.frontend.network_setup_protocol.is_none()
                    && state.frontend.multiplayer_setup.is_none()
                    && state.match_state.session.network_setup().is_none();
                let transport_retired = state.match_state.live_network.is_none()
                    && state.match_state.lan_service.is_none();
                let session_retired = state.match_state.session.network_return_context().is_none();
                // Main_Game48CEBF retires ScenarioStarted. The runtime and
                // presentation intentionally remain through shell handoff.
                let scenario_retired = state
                    .match_state
                    .sim_runtime
                    .runtime()
                    .is_none_or(|runtime| !runtime.simulation.session().started());
                ensure!(
                    visits_retired
                        && setup_retired
                        && transport_retired
                        && session_retired
                        && scenario_retired,
                    "settled Main Menu retained an old network owner"
                );
                let acknowledged = state.frontend.online_cleanup.logout_acknowledged();
                ensure!(
                    self.config.is_none() || acknowledged,
                    "online cleanup stopped without an acknowledged logout"
                );
                self.menu_return = Some(json!({"render_frame":frame,"settled":true,
                    "visits_retired":visits_retired,"setup_retired":setup_retired,
                    "transport_retired":transport_retired,"session_retired":session_retired,
                    "scenario_retired":scenario_retired,
                    "online_cleanup_pending":crate::app::shell_wol::online_cleanup_pending(&state.frontend, &state.match_state),
                    "online_logout_acknowledged":acknowledged}));
                self.record(
                    frame,
                    "Settled Main Menu and acknowledged quiescent cleanup observed",
                );
                self.phase = RoundPhase::Finished;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn latency_failure(&self) -> Option<&str> {
        self.latency_failure.as_deref()
    }

    pub(super) fn receipt(&self, live_network: &Option<LiveCommandSession>) -> Result<Value> {
        if let Some(watch) = &self.watch {
            return self.watch_receipt(live_network, watch);
        }
        ensure!(
            self.ready()
                && self.rounds.len()
                    == if self.profile.is_some() {
                        0
                    } else if self.failure_prefix_written {
                        1
                    } else {
                        2
                    },
            "network outcome capture is incomplete"
        );
        ensure!(
            live_network.is_none(),
            "finished network capture retained command transport"
        );
        ensure!(
            self.failure_prefix_written || self.menu_return.is_some(),
            "successful rematch capture did not return to the settled Main Menu"
        );
        Ok(
            json!({"target":if self.config.is_some() {"online-live"} else {"lan-live"},
            "account":self.config.as_ref().map(|config| &config.account),"host":self.config.as_ref().map(|config| &config.host),
            "service_boundary":if self.config.is_some() {"normal WOL UI, deployed HTTPS lobby, authenticated UDP relay, production loading and command boundary"} else {"normal LAN UI, real UDP, production loading and command boundary"},
            "fixed_start":self.config.as_ref().and_then(|config|config.start),
            "requested_mode_id":self.config.as_ref().and_then(|config|config.mode_id),"requested_ai":self.config.as_ref().and_then(|config|config.ai).map(|difficulty|format!("{difficulty:?}")),
            "players":self.players(),"required_frames":self.config.as_ref().map_or(300, |config|config.frames),"simple_input":self.config.as_ref().is_none_or(|config|config.simple_input),
            "failure_prefix":self.config.as_ref().and_then(|config|config.failure_prefix_output.as_ref()),
            "profile_check":self.profile.as_ref().map(|profile|profile.receipt()).transpose()?,
            "scope":if self.profile.is_some() { "host-only actual map/page/combo/Back/Create profile reconstruction followed by settled Menu; no match or30-human acceptance claim" } else if self.config.as_ref().is_some_and(|config|config.ai.is_some()) { "two one-human/one-CPU games with ordinary AI dropdown input, actual CPU House/activity, local command/checksum progress, actual B6 Leave, lobby rematch and acknowledged Main Menu return; results are recorded only when shown, with no cross-client determinism claim" } else if self.failure_prefix_written && self.config.is_none() { "real300-frame CRC and strong-hash prefix, external scoped FRAMEINFO corruption, actual checksum CE Stop and settled LAN lobby cleanup" } else if self.failure_prefix_written { "real300-frame command/checksum prefix, externally terminated peer, expected ordinary connection timeout/removal or Quit, and authenticated lobby return" } else { "two complete games with tactical input, shared simulation checksums, Observe defeat, real results, lobby rematch and settled Main Menu return; unit scale is measured from each runtime snapshot" },
            "returned_to_menu":self.menu_return.is_some(),"menu_return":self.menu_return,
            "rounds":self.rounds,"route":self.route}),
        )
    }
}

fn reset_round(core: &mut LiveLanCapture) {
    core.phase = Phase::Main;
    core.route.clear();
    core.checksums.clear();
    core.simulation_hashes.clear();
    core.startup = None;
    core.map_admission = None;
    core.setup_catalog = None;
    core.actions.clear();
    core.actor = None;
    core.actor_last_seen = None;
    core.origin = None;
    core.observed_movement = false;
    core.stop_observation = super::StopObservation::default();
    core.stopped = false;
    core.last_wait = None;
    core.country_selected = false;
    if core.latency.is_some() {
        core.latency = Some(super::latency::LatencyCapture::default());
    }
    if core.mass.is_some() {
        core.mass = Some(mass::MassInput::default());
    }
    if core.battle.is_some() {
        core.battle = Some(battle::BattleOpening::default());
    }
    if core.ai.is_some() {
        core.ai = Some(AiActivity::default());
    }
}

/// VERA's LAN room comes back after a match (`return_after_match`): its host
/// opens it again at once and the guest joins it again once it is open. The
/// original-style room returns to BB, and the desync route keeps its own
/// return check.
fn returns_to_room(core: &LiveLanCapture, frontend: &FrontendState) -> bool {
    core.failure.is_none() && !frontend.native_shell_ui
}

fn room_returned(
    core: &LiveLanCapture,
    frontend: &FrontendState,
    session: &MatchSession,
    rendered: PresentedShell,
) -> bool {
    rendered == PresentedShell::NetworkSetup
        && setup_settled(frontend, core.host())
        && session
            .network_setup()
            .is_some_and(|launch| core.host() || launch.humans.len() == 2)
}

fn lobby_ready(frontend: &FrontendState, rendered: PresentedShell) -> bool {
    rendered == PresentedShell::WolLobby && online_lobby_settled(frontend)
}

pub(super) fn online_lobby_settled(frontend: &FrontendState) -> bool {
    MoviesCapture::slide_settled(frontend, ShellSlideKind::WolLobby)
        && frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .is_some_and(|lobby| lobby.interactive())
}

pub(super) fn online_setup_settled(frontend: &FrontendState, host: bool) -> bool {
    MoviesCapture::slide_settled(
        frontend,
        ShellSlideKind::NetworkSetup(crate::app::shell_network_setup::setup_dialog(
            frontend,
            host,
            crate::ui::multiplayer_setup::SetupFlavor::Wol,
        )),
    ) && frontend
        .multiplayer_setup
        .as_ref()
        .is_some_and(|ui| ui.statics.is_terminal())
}

/// Select a game list row and press Join, through the ordinary mouse
/// adapter. False after a click that scrolls toward a row outside the list's
/// view instead.
fn join_game_row(state: &mut AppState, row: usize) -> Result<bool> {
    let layout =
        crate::app::shell_wol::lobby::layout(state.frontend.wol.as_ref(), &state.renderer.gpu);
    let lobby = state
        .frontend
        .wol
        .as_ref()
        .and_then(|visit| visit.lobby())
        .context("online lobby")?;
    let geometry = lobby.ui.shown_geometry(
        crate::ui::wol_lobby::GAMES,
        layout
            .control_rect(crate::ui::wol_lobby::GAMES)
            .unwrap_or_default(),
        lobby.ui.game_rows.len(),
        layout.page.scale,
    );
    let top = lobby.ui.list_top(crate::ui::wol_lobby::GAMES);
    if row < top || row >= top + geometry.visible_rows {
        let rect = if let Some(bar) = geometry.scrollbar {
            let y = if row < top {
                bar.y + 1
            } else {
                bar.y + bar.h - 2 - geometry.metrics.arrow_h
            };
            RectPx::new(bar.x + 1, y, bar.w - 2, geometry.metrics.arrow_h)
        } else {
            // replace_games preserves the old top row by name. With a newly
            // shorter list its ordinary mouse input reconciles that stale
            // viewport before any row click.
            ensure!(
                top > geometry.max_top,
                "offscreen online game has no ordinary scroll input"
            );
            let games = layout
                .control_rect(crate::ui::wol_lobby::GAMES)
                .unwrap_or_default();
            RectPx::new(games.x, games.y, games.w, 1)
        };
        click(state, rect);
        return Ok(false);
    }
    let rect = geometry.row(row - top);
    click(state, rect);
    click(
        state,
        layout
            .page
            .button_rect(wol_lobby::JOIN)
            .context("Join button")?,
    );
    Ok(true)
}

fn click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(
        &state.renderer,
        &state.frontend.screen,
        &mut state.match_state.input,
        rect,
    );
    App::handle_wol_mouse_move(state);
    App::handle_wol_mouse_down(state);
    App::handle_wol_mouse_up(state);
    MoviesCapture::restore_neutral_pointer(&state.renderer.gpu, &mut state.match_state.input);
}
