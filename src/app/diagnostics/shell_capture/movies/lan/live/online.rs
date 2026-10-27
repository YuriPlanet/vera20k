//! The online entry and rematch route for the existing live-network observer.
//! Only ordinary keyboard/mouse adapters produce inputs. The HTTPS service,
//! relay, loading barrier and frame boundary run exactly as in an interactive app.

use super::*;
use crate::app::diagnostics::shell_capture::LiveOnlineConfig;
use crate::ui::wol_lobby;
use std::io::Read;

mod exit_reentry;
use crate::app::diagnostics::shell_capture::loading_host_loss::LoadingHostLoss;
use exit_reentry::ExitReentry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoundPhase {
    LanEntry,
    Main,
    Welcome,
    Login,
    Lobby,
    Create,
    Discover,
    HostSetup,
    HostAi,
    HostReady,
    GuestSetup,
    Loading,
    Running,
    HoldFailureGuest,
    EndRound,
    Results,
    Returning,
    ExitFault,
    ExitWelcome,
    MainReturn,
    Finished,
}

pub(super) struct LiveRoundFlow {
    config: Option<LiveOnlineConfig>,
    random_map_input: Option<RandomMapInput>,
    phase: RoundPhase,
    rounds: Vec<Value>,
    current_round: Option<Value>,
    menu_return: Option<Value>,
    result_rows: Vec<[String; 5]>,
    abort: Option<super::super::super::abort_return::AbortReturnInput>,
    route: Vec<Value>,
    last_refresh: Instant,
    last_heartbeat: Instant,
    failure_prefix_written: bool,
    network_checkpoint: Option<Value>,
    network_checkpoint_started: Option<Instant>,
    exit_reentry: Option<ExitReentry>,
    loading_host_loss: Option<LoadingHostLoss>,
    loading_cancel_trigger: Option<Value>,
    loading_cancellation: Option<Value>,
    profile: Option<super::profile::ProfileCheck>,
    ai_input: super::profile::SetupComboInput,
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
        let loading_host_loss = config
            .as_ref()
            .and_then(|config| config.loading_host_loss.as_ref())
            .map(|_| LoadingHostLoss::default());
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
            menu_return: None,
            result_rows: Vec::new(),
            abort: None,
            route: Vec::new(),
            last_refresh: Instant::now(),
            last_heartbeat: Instant::now(),
            failure_prefix_written: false,
            network_checkpoint: None,
            network_checkpoint_started: None,
            exit_reentry: None,
            loading_host_loss,
            loading_cancel_trigger: None,
            loading_cancellation: None,
            profile,
            ai_input: Default::default(),
        }
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
        if let Some(input) = self.random_map_input.as_mut() {
            if !input.after_present(state, frame, &mut self.route)? {
                return Ok(false);
            }
            self.random_map_input = None;
            core.prepare_host_setup(
                state,
                Some("RandMap.Sed"),
                self.round_players(),
                mode_id,
                ai,
            )?;
            self.record(
                frame,
                "Generated seed, preview and start positions published",
            );
            return Ok(true);
        }
        let settled = if self.config.is_some() {
            online_setup_settled(state, true)
        } else {
            setup_settled(state, true)
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
                restored.mode.class.lists_in_skirmish(),
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
        core.prepare_host_setup(state, selected_map, self.round_players(), mode_id, ai)?;
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
    fn round_players(&self) -> usize {
        if self.loading_host_loss.is_some() && !self.rounds.is_empty() {
            2
        } else {
            self.players()
        }
    }
    fn input_host(&self, core: &LiveLanCapture) -> bool {
        if let Some(loss) = self
            .loading_host_loss
            .as_ref()
            .filter(|_| !self.rounds.is_empty())
        {
            loss.rematch_host() == Some(self.online_config().account.as_str())
        } else {
            core.host()
        }
    }
    fn discovery_host(&self) -> &str {
        self.loading_host_loss
            .as_ref()
            .filter(|_| !self.rounds.is_empty())
            .and_then(LoadingHostLoss::rematch_host)
            .unwrap_or(&self.online_config().host)
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

    fn record(&mut self, frame: u32, action: &str) {
        log::info!("Network capture round {}: {action}", self.rounds.len() + 1);
        self.route
            .push(json!({"render_frame":frame,"action":action,"round":self.rounds.len()+1}));
    }

    fn write_prefix(
        &mut self,
        core: &LiveLanCapture,
        state: &AppState,
        frame: u32,
        network: bool,
    ) -> Result<()> {
        if if network {
            self.network_checkpoint.is_some()
        } else {
            self.failure_prefix_written
        } {
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
                if network {
                    "vera20k.online-network-prefix.v1"
                } else {
                    "vera20k.online-failure-prefix.v1"
                },
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
        let nonce = self
            .config
            .as_ref()
            .and_then(|config| config.network_checkpoint.as_ref());
        let prefix_frame = state
            .match_state
            .sim_runtime
            .as_ref()
            .context("prefix world")?
            .simulation
            .session()
            .binary_frame();
        let primary_port = state
            .match_state
            .live_network
            .as_ref()
            .context("prefix transport")?
            .primary_addr()?
            .port();
        let prefix = json!({"schema":schema,"account":account,"host":host,
            "checkpoint_id":nonce,"prefix_frame":prefix_frame,"primary_port":primary_port,
            "external_fault_pending":true,"runtime":core.observed_receipt_through(state,network.then_some(REQUIRED_FRAMES))?});
        write_new_file(path, &serde_json::to_vec_pretty(&prefix)?)?;
        if network {
            self.network_checkpoint = Some(
                json!({"checkpoint_id":nonce,"prefix_frame":prefix_frame,"primary_port":primary_port,"ack":null}),
            );
            self.network_checkpoint_started = Some(Instant::now());
        } else {
            self.failure_prefix_written = true;
        }
        self.record(frame, "Real300-frame command/checksum prefix saved; external fault may begin after both receipts agree");
        Ok(())
    }

    /// The game keeps ticking. Only this movie's later Observe click waits
    /// for the external helper's proven tuple change; no game timer is touched.
    fn network_checkpoint_ready(&mut self, state: &AppState) -> Result<bool> {
        let Some(checkpoint) = self.network_checkpoint.as_mut() else {
            return Ok(true);
        };
        if !checkpoint["ack"].is_null() {
            return Ok(true);
        }
        ensure!(
            self.network_checkpoint_started
                .context("checkpoint start")?
                .elapsed()
                < std::time::Duration::from_secs(120),
            "external network checkpoint deadline expired"
        );
        let path = self
            .config
            .as_ref()
            .and_then(|config| config.failure_prefix_output.as_ref())
            .context("checkpoint path")?
            .with_extension("applied.json");
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            file.metadata()?.len() <= 1024,
            "network checkpoint control exceeds its bound"
        );
        let applied: Value = serde_json::from_reader(file.take(1025))?;
        ensure!(
            applied.as_object().is_some_and(|value| value.len() == 3)
                && applied["checkpoint_id"] == checkpoint["checkpoint_id"]
                && applied["prefix_frame"] == checkpoint["prefix_frame"]
                && applied["applied"] == true,
            "network checkpoint control belongs to another prefix"
        );
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("checkpoint world")?
            .simulation;
        let port = state
            .match_state
            .live_network
            .as_ref()
            .context("checkpoint live transport")?
            .primary_addr()?
            .port();
        ensure!(
            state.frontend.screen == GameScreen::InGame
                && sim.terminal_score_snapshot().is_none()
                && sim.session().binary_frame()
                    >= checkpoint["prefix_frame"]
                        .as_u64()
                        .context("checkpoint frame")? as u32
                && Some(u64::from(port)) == checkpoint["primary_port"].as_u64(),
            "network checkpoint ended or replaced its live socket before acknowledgement"
        );
        checkpoint["ack"] = json!({"native_frame":sim.session().binary_frame(),"tick":sim.session().tick(),"primary_port":port});
        Ok(true)
    }

    fn heartbeat(&mut self, state: &AppState, rendered: PresentedShell) {
        if self.last_heartbeat.elapsed() < std::time::Duration::from_secs(10) {
            return;
        }
        self.last_heartbeat = Instant::now();
        let (page, pending) = state.frontend.wol.as_ref().map_or(("none", 0), |visit| {
            use crate::app::shell_wol::WolPage;
            let page = match &visit.page {
                WolPage::Welcome => "welcome",
                WolPage::Backdrop { .. } => "backdrop",
                WolPage::Login(_) => "login",
                WolPage::Message { .. } => "message",
                WolPage::Lobby => "lobby",
                WolPage::Options(_) => "options",
                WolPage::AutoLoginQuery(_) => "auto_login",
                WolPage::Locale(_) => "locale",
                WolPage::MyInformation(_) => "my_information",
                WolPage::Buddy(_) => "buddy",
                WolPage::QuickMatch(_) => "quick_match",
                WolPage::AddBuddyQuery(_) => "add_buddy",
            };
            (page, visit.pending_service_request_count())
        });
        let (lobby_shown, lobby_wait, lobby_message) = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .map_or((false, false, false), |lobby| {
                (lobby.shown, lobby.wait.is_some(), lobby.message().is_some())
            });
        let session = &state.match_state.session;
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
        let accept_enabled = setup::view(state).map(|view| view.accept_enabled);
        let desired_start = self.config.as_ref().and_then(|config| config.start);
        let sim = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation);
        let native_frame = sim.map(|sim| sim.session().binary_frame());
        let fps = sim
            .and_then(|sim| sim.session().network_command_timing())
            .map(|timing| timing.frames_per_second());
        let max_ahead = sim.and_then(|sim| sim.session().network_max_ahead_frames());
        let local_commands = sim.map(|sim| sim.pending_local_command_count());
        let live_wait = state
            .match_state
            .live_network
            .as_ref()
            .and_then(|live| live.wait_info())
            .map(|wait| wait.kind);
        let live_phase = state
            .match_state
            .live_network
            .as_ref()
            .and_then(|live| live.phase());
        log::info!(
            "Network capture heartbeat: phase={:?}, page={page}, presented={rendered:?}, shell_exit={}, pending_requests={pending}, native_frame={native_frame:?}, timing_fps={fps:?}, max_ahead={max_ahead:?}, local_commands={local_commands:?}, live_wait={live_wait:?}, live_phase={live_phase:?}, setup_humans={setup_humans:?}, bound_rows={bound_rows:?}, seed_present={seed_present}, map_capacity={map_capacity:?}, setup_capacity={setup_capacity:?}, selected_map_present={selected_map_present}, local_row={local_row:?}, local_start={local_start:?}, accept_enabled={accept_enabled:?}, desired_start={desired_start:?}, lobby_shown={lobby_shown}, lobby_wait={lobby_wait}, lobby_message={lobby_message}, slide_first_paint={}, active_slide={:?}",
            self.phase,
            state.frontend.shell_exit.is_some(),
            state.frontend.shell_first_paint_slide.is_some(),
            state.frontend.shell_slide_active_shell,
        );
        if let (Some(sim), Some(live)) = (sim, state.match_state.live_network.as_ref()) {
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
                if MoviesCapture::slide_settled(state, ShellSlideKind::WolLocale) =>
            {
                let layout = persona::layout(state).context("online location dialog")?;
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
                let geometry = persona::list_geometry(&layout, page);
                ensure!(
                    row >= page.top && row < page.top + geometry.visible_rows,
                    "online location choice is outside the visible list"
                );
                let row_rect = geometry.row(row - page.top);
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
                if MoviesCapture::slide_settled(state, ShellSlideKind::WolAutoLoginQuery) =>
            {
                let layout = persona::layout(state).context("online auto-login dialog")?;
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
        self.heartbeat(state, rendered);
        if let Some(config) = self.config.as_ref() {
            if config.observe_loading && state.frontend.screen == GameScreen::Loading {
                crate::app::diagnostics::shell_capture::loading_runtime::observe_progress(state);
            }
            let settled = online_lobby_settled(state);
            match crate::app::diagnostics::shell_capture::loading_runtime::cancellation_step(
                config,
                state,
                rendered,
                settled,
                frame,
                &mut self.loading_cancel_trigger,
            )? {
                crate::app::diagnostics::shell_capture::loading_runtime::CancelStep::Ordinary => {}
                crate::app::diagnostics::shell_capture::loading_runtime::CancelStep::Waiting => {
                    return Ok(());
                }
                crate::app::diagnostics::shell_capture::loading_runtime::CancelStep::Recovered(
                    proof,
                ) => {
                    self.loading_cancellation = Some(proof);
                    reset_round(core);
                    self.phase = RoundPhase::Lobby;
                    self.record(frame,"Actual preinstall cancellation acknowledged finish/leave and returned assets; fresh normal rooms follow in this same process");
                    return Ok(());
                }
            }
        }
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
                            lobby_button(state, crate::ui::lan_shell::CREATE)?;
                        }
                        self.record(frame,"Host profile Back returned to lobby; ordinary Create repeats the same profile");
                        return Ok(());
                    }
                    super::profile::ProfileStep::Exit => {
                        if online {
                            click(
                                state,
                                crate::app::shell_wol::lobby::layout(state)
                                    .page
                                    .button_rect(wol_lobby::BACK)
                                    .context("profile lobby Back")?,
                            );
                            self.phase = RoundPhase::ExitWelcome;
                        } else {
                            lobby_button(state, crate::ui::lan_shell::BACK)?;
                            MoviesCapture::restore_neutral_pointer(state);
                            self.phase = RoundPhase::MainReturn;
                        }
                        self.record(frame,"Extended and native profile rows retained after actual Back/Create; return to Menu");
                        return Ok(());
                    }
                }
            }
        }
        if let Some(exit) = &self.exit_reentry {
            ensure!(
                exit.started.elapsed() < Duration::from_secs(180),
                "online exit/reentry checkpoint expired"
            );
        }
        if self.login_questions(state, rendered, frame)? {
            return Ok(());
        }
        if self.config.is_some()
            && !self.input_host(core)
            && matches!(self.phase, RoundPhase::GuestSetup | RoundPhase::Loading)
            && lobby_ready(state, rendered)
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
        match self.phase {
            RoundPhase::Main
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready_for(
                        MainMenuCaptureSnapshot::from_state(state),
                        self.config.as_ref().is_some_and(|config| config.latency),
                    )? =>
            {
                LanCapture::isolated_root(state)?;
                ensure!(
                    !state.persistence.options_profile.wol_test_service,
                    "online capture requires the offline WOL test service to be disabled"
                );
                click_main_menu(
                    state,
                    crate::ui::main_menu_shell::MainMenuControlId::WwOnline0x684,
                )?;
                self.record(frame, "Multiplayer Internet selected");
                self.phase = RoundPhase::Welcome;
            }
            RoundPhase::Welcome
                if rendered == PresentedShell::WolWelcome
                    && MoviesCapture::slide_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let rect = App::wol_welcome_layout(state)
                    .page
                    .button_rect(crate::ui::wol_shell::CUSTOM_MATCH)
                    .context("Custom Match button")?;
                click(state, rect);
                self.record(frame, "Custom Match selected");
                self.phase = RoundPhase::Login;
            }
            RoundPhase::Login
                if rendered == PresentedShell::WolLogin
                    && MoviesCapture::slide_settled(state, ShellSlideKind::WolLogin) =>
            {
                let layout = crate::app::shell_wol::login_layout(state);
                click(
                    state,
                    RectPx::new(
                        layout.nickname_face.x,
                        layout.nickname_face.y,
                        layout.nickname_face.w / 2,
                        layout.nickname_face.h,
                    ),
                );
                App::handle_wol_key(state, Some(KeyCode::Home), true, None);
                for _ in 0..32 {
                    App::handle_wol_key(state, Some(KeyCode::Delete), true, None);
                }
                App::handle_wol_key(state, None, true, Some(&self.online_config().account));
                let control = crate::ui::wol_login::LOGIN;
                ensure!(
                    state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.login())
                        .is_some_and(|login| login.button_enabled(control)),
                    "online credentials did not enable login"
                );
                click(
                    state,
                    layout
                        .page
                        .button_rect(control)
                        .context("online authentication button")?,
                );
                self.record(frame, "Username submitted through OK");
                self.phase = RoundPhase::Lobby;
            }
            RoundPhase::Lobby if lobby_ready(state, rendered) => {
                if self.reentering() {
                    self.observe_reentry_lobby(state, frame)?;
                    return Ok(());
                }
                if self.input_host(core) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
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
                    );
                    click(state, layout.ok);
                    self.record(frame, "Create Game confirmed");
                    self.phase = RoundPhase::HostSetup;
                }
            }
            RoundPhase::Discover if lobby_ready(state, rendered) => {
                let expected = format!("{}'s_game", self.discovery_host());
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
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    let lobby = state
                        .frontend
                        .wol
                        .as_ref()
                        .and_then(|visit| visit.lobby())
                        .context("online lobby")?;
                    let geometry = lobby.ui.game_list.geometry(
                        layout.games,
                        lobby.ui.game_rows.len(),
                        layout.page.scale,
                    );
                    if row < lobby.ui.game_list.top_index
                        || row >= lobby.ui.game_list.top_index + geometry.visible_rows
                    {
                        let rect = if let Some(bar) = geometry.scrollbar {
                            let y = if row < lobby.ui.game_list.top_index {
                                bar.y + 1
                            } else {
                                bar.y + bar.h - 2 - geometry.metrics.arrow_h
                            };
                            RectPx::new(bar.x + 1, y, bar.w - 2, geometry.metrics.arrow_h)
                        } else {
                            // replace_games preserves the old top row by name.
                            // With a newly shorter list its ordinary mouse input
                            // reconciles that stale viewport before any row click.
                            ensure!(
                                lobby.ui.game_list.top_index > geometry.max_top,
                                "offscreen online game has no ordinary scroll input"
                            );
                            RectPx::new(layout.games.x, layout.games.y, layout.games.w, 1)
                        };
                        click(state, rect);
                        self.record(
                            frame,
                            "Game list scrolled toward the configured discovered room",
                        );
                        return Ok(()); // Re-read the row on its next presented frame.
                    }
                    let rect = geometry.row(row - lobby.ui.game_list.top_index);
                    click(state, rect);
                    click(
                        state,
                        layout
                            .page
                            .button_rect(wol_lobby::JOIN)
                            .context("Join button")?,
                    );
                    self.record(frame, "Configured discovered game joined");
                    self.phase = RoundPhase::GuestSetup;
                } else if self.last_refresh.elapsed() >= Duration::from_secs(2) {
                    let layout = crate::app::shell_wol::lobby::layout(state);
                    click(
                        state,
                        layout
                            .control_rect(wol_lobby::REFRESH)
                            .context("Refresh image button")?,
                    );
                    self.last_refresh = Instant::now();
                }
            }
            RoundPhase::HostSetup => {
                let selected = self
                    .loading_host_loss
                    .as_ref()
                    .filter(|_| !self.rounds.is_empty())
                    .and_then(LoadingHostLoss::rematch_map)
                    .map(str::to_owned)
                    .or_else(|| self.online_config().map.clone());
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
                    && online_setup_settled(state, true) =>
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
                let view = setup::view(state).context("AI Player dropdown view")?;
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
            RoundPhase::HostReady if online_setup_settled(state, true) => {
                if core.select_start(state, self.online_config().start)? {
                    self.record(
                        frame,
                        "Configured authored start selected through the local Start combo",
                    );
                    return Ok(());
                }
                let session = &state.match_state.session;
                let ready = session.network_setup().is_some_and(|launch| {
                    launch.humans.len() == self.round_players()
                        && launch.humans.iter().enumerate().all(|(i, human)| {
                            human.raw_slot_index.is_some()
                                && session.network_player_ready(i)
                                && session.network_map_status(i) == Some(1)
                        })
                });
                let starting = state
                    .frontend
                    .network_setup_protocol
                    .as_ref()
                    .is_some_and(|protocol| protocol.start_in_progress());
                if ready && !starting && core.start_matches(state, self.online_config().start) {
                    setup_button(state, crate::ui::multiplayer_setup::START)?;
                    self.record(
                        frame,
                        "All real readiness acknowledgements present; Start clicked",
                    );
                }
            }
            RoundPhase::GuestSetup if online_setup_settled(state, false) => {
                if self.online_config().start.is_some() && !core.start_selection_admitted(state) {
                    return Ok(());
                }
                let country_was_selected = core.country_selected;
                core.select_country(state)?;
                if !country_was_selected && core.country_selected {
                    // Let the real host options echo this normal control edit
                    // before any Accept can acknowledge it.
                    return Ok(());
                }
                if core.select_start(state, self.online_config().start)? {
                    self.record(
                        frame,
                        "Configured authored start selected through the local Start combo",
                    );
                    return Ok(());
                }
                if state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| {
                        launch.humans.len() == self.round_players()
                            && launch
                                .humans
                                .iter()
                                .all(|human| human.raw_slot_index.is_some())
                    })
                    && state.match_state.session.network_start_seed().is_some()
                    && core.start_matches(state, self.online_config().start)
                    && setup::view(state).is_some_and(|view| view.accept_enabled)
                {
                    setup_button(state, crate::ui::multiplayer_setup::ACCEPT)?;
                    self.record(frame, "Received map and options; Accept clicked");
                }
            }
            RoundPhase::Loading | RoundPhase::HostReady | RoundPhase::GuestSetup
                if state.frontend.screen == GameScreen::InGame =>
            {
                ensure!(
                    state.match_state.live_network.is_some(),
                    "online match has no production command transport"
                );
                core.startup = Some(startup_receipt(state)?);
                if let Some(ai) = core.ai.as_mut() {
                    ai.observe(state)?;
                }
                if let Some(loss) = self.loading_host_loss.as_mut() {
                    loss.begin_round(
                        self.config.as_ref().context("loading-loss config")?,
                        state,
                        self.rounds.len() + 1,
                    )?;
                }
                core.phase = Phase::Running;
                self.record(
                    frame,
                    "Authenticated relay and synchronized loading reached gameplay",
                );
                self.phase = RoundPhase::Running;
            }
            RoundPhase::Running => {
                let startup_failure = core
                    .failure
                    .as_ref()
                    .is_some_and(|failure| failure.is_startup());
                if startup_failure {
                    let failure = core.failure.as_mut().expect("startup observer");
                    if !failure.arm_startup(state, frame)? {
                        return Ok(());
                    }
                }
                let active_players = if let Some(loss) = self.loading_host_loss.as_mut() {
                    loss.collect(state, self.rounds.len() + 1, core.required_frames())?;
                    2
                } else {
                    self.players()
                };
                core.observe_run(state, frame, active_players)?;
                if self.rounds.is_empty()
                    && self
                        .config
                        .as_ref()
                        .is_some_and(|config| config.network_checkpoint.is_some())
                {
                    if core.checksums.len() >= REQUIRED_FRAMES as usize {
                        self.write_prefix(core, state, frame, true)?;
                    }
                    if !self.network_checkpoint_ready(state)? {
                        return Ok(());
                    }
                }
                if !startup_failure && core.failure.as_ref().is_some_and(|failure| failure.armed())
                {
                    self.write_prefix(core, state, frame, false)?;
                }
                if core.phase == Phase::Finished {
                    if core.target == LanTarget::OnlineFailureGuest {
                        self.write_prefix(core, state, frame, false)?;
                        self.record(frame, "Guest remains live for external termination; no Observe or fabricated departure");
                        self.phase = RoundPhase::HoldFailureGuest;
                        return Ok(());
                    }
                    if let Some(loss) = self.loading_host_loss.as_mut() {
                        loss.finish_round(state, self.rounds.len() + 1, core.required_frames())?;
                    }
                    self.current_round = Some(if startup_failure {
                        json!({"startup_failure":core.failure.as_ref().expect("startup observer").receipt()})
                    } else {
                        json!({"runtime":core.round_receipt(state)?})
                    });
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
                    } else if self.loading_host_loss.is_some() {
                        // The initial host was externally killed. Exactly one
                        // live survivor chooses Observe; the next room reverses
                        // that normal input role without changing game state.
                        (self
                            .loading_host_loss
                            .as_ref()
                            .and_then(LoadingHostLoss::rematch_host)
                            == Some(self.online_config().account.as_str()))
                            == (self.rounds.len() == 1)
                    } else if self.players() > 2 {
                        !core.host()
                    } else {
                        core.host() == (self.rounds.len() == 1)
                    };
                    if observe {
                        let solo_ai = self
                            .config
                            .as_ref()
                            .is_some_and(|config| config.ai.is_some());
                        self.abort = Some(if solo_ai {
                            // Native B6 hides Observe with fewer than two live
                            // humans (4F1938). Use its ordinary Leave button.
                            super::super::super::abort_return::AbortReturnInput::begin(
                                state,
                                &mut self.route,
                                frame,
                            )?
                        } else {
                            super::super::super::abort_return::AbortReturnInput::begin_network_observe(
                                state, &mut self.route, frame)?
                        });
                        self.record(
                            frame,
                            if solo_ai {
                                "Leave requested through in-game Options"
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
                // gameplay and any real wait/result reached by the external
                // fault; the orchestrator alone terminates this process.
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
                        } else {
                            "Observe selected through B6; real DESTRUCT queued"
                        },
                    );
                    self.phase = RoundPhase::Results;
                }
            }
            RoundPhase::Results
                if rendered == PresentedShell::Score
                    && MoviesCapture::slide_settled(state, ShellSlideKind::Score)
                    && state
                        .frontend
                        .score_page
                        .as_ref()
                        .is_some_and(|page| page.reveals_terminal()) =>
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
                    let rect = crate::app::frontend::shell_layout::score_layout(state)
                        .paging_button(crate::ui::score_shell::NEXT_PAGE)
                        .context("score Next")?;
                    set_window_pointer_to_rect_center(state, rect);
                    App::handle_score_shell_mouse_move(state);
                    App::handle_score_shell_mouse_down(state);
                    App::handle_score_shell_mouse_up(state);
                    MoviesCapture::restore_neutral_pointer(state);
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
                if self.loading_host_loss.is_some() {
                    let startup =
                        &self.current_round.as_ref().context("round")?["runtime"]["startup"];
                    let names = startup["sources"]
                        .as_array()
                        .context("result startup sources")?;
                    ensure!(
                        self.result_rows.len() == self.round_players()
                            && names.len() == self.result_rows.len()
                            && names.iter().all(|source| self
                                .result_rows
                                .iter()
                                .any(|row| source["name"] == row[0])),
                        "results did not display the complete admitted round roster"
                    );
                }
                self.current_round.as_mut().context("completed round")?["result"] = score;
                self.current_round.as_mut().context("completed round")?["result"]["displayed_table_rows"] =
                    json!(self.result_rows);
                self.current_round.as_mut().context("completed round")?["terminal_simulation"] =
                    terminal_simulation_receipt(state)?;
                let rect = crate::app::frontend::shell_layout::score_layout(state)
                    .page
                    .button_rect(crate::ui::score_shell::CONTINUE_BUTTON)
                    .context("score Continue")?;
                set_window_pointer_to_rect_center(state, rect);
                App::handle_score_shell_mouse_move(state);
                App::handle_score_shell_mouse_down(state);
                App::handle_score_shell_mouse_up(state);
                MoviesCapture::restore_neutral_pointer(state);
                self.record(frame, "Real multiplayer results acknowledged");
                self.phase = RoundPhase::Returning;
            }
            RoundPhase::Results
                if self
                    .config
                    .as_ref()
                    .is_some_and(|config| config.ai.is_some())
                    && lobby_ready(state, rendered) =>
            {
                self.current_round.as_mut().context("completed AI round")?["result"] = json!({"shown":false,"completion":"ordinary B6 Leave returned directly to lobby"});
                self.record(
                    frame,
                    "Singleton Leave returned to the lobby without a results page",
                );
                self.phase = RoundPhase::Returning;
            }
            RoundPhase::Returning
                if if self.config.is_some() {
                    lobby_ready(state, rendered)
                } else {
                    rendered == PresentedShell::LanLobby && lobby_settled(state)
                } =>
            {
                ensure!(
                    state.match_state.live_network.is_none(),
                    "finished round retained live command owner"
                );
                ensure!(
                    state.match_state.session.network_setup().is_none(),
                    "finished round retained setup"
                );
                // The exit fault targets logout only after the final match's
                // existing finish/leave owner has actually acknowledged cleanup.
                if self.rounds.len() == 1
                    && self
                        .config
                        .as_ref()
                        .is_some_and(|config| config.exit_checkpoint.is_some())
                    && !crate::app::shell_wol::online_exit_ready(state)
                {
                    return Ok(());
                }
                let startup_failure = core
                    .failure
                    .as_ref()
                    .is_some_and(|failure| failure.is_startup());
                if startup_failure {
                    let failure = core.failure.as_mut().expect("startup observer");
                    failure.record_startup_lobby(state, frame)?;
                    self.current_round
                        .as_mut()
                        .context("startup failure round")?["startup_failure"] = failure.receipt();
                }
                let mut round = self
                    .current_round
                    .take()
                    .context("completed round result")?;
                round["returned_to_lobby"] = json!(true);
                self.rounds.push(round);
                self.record(
                    frame,
                    if core
                        .failure
                        .as_ref()
                        .is_some_and(|failure| failure.returned())
                    {
                        if startup_failure {
                            "Actual startup ConnectionFailed OK returned to the presented authenticated online lobby"
                        } else if core.target.desync_failure() {
                            "Actual checksum CE Stop returned to the settled LAN lobby"
                        } else {
                            "Expected connection Quit returned to the authenticated online lobby"
                        }
                    } else {
                        "Returned to service lobby through normal result continuation"
                    },
                );
                if startup_failure {
                    let rect = crate::app::shell_wol::lobby::layout(state)
                        .page
                        .button_rect(wol_lobby::BACK)
                        .context("WOL lobby Back button")?;
                    click(state, rect);
                    self.record(frame, "Presented authenticated lobby reached after startup failure; normal Back clicked");
                    self.phase = RoundPhase::ExitWelcome;
                } else if core.failure.is_some() {
                    self.phase = RoundPhase::Finished;
                } else if self.rounds.len() == 2 {
                    if self
                        .config
                        .as_ref()
                        .is_some_and(|config| config.exit_checkpoint.is_some())
                    {
                        self.begin_exit_checkpoint(state, frame)?;
                        self.phase = RoundPhase::ExitFault;
                        return Ok(());
                    }
                    if self.config.is_some() {
                        let rect = crate::app::shell_wol::lobby::layout(state)
                            .page
                            .button_rect(wol_lobby::BACK)
                            .context("WOL lobby Back button")?;
                        click(state, rect);
                        self.phase = RoundPhase::ExitWelcome;
                    } else {
                        lobby_button(state, crate::ui::lan_shell::BACK)?;
                        MoviesCapture::restore_neutral_pointer(state);
                        self.phase = RoundPhase::MainReturn;
                    }
                    self.record(
                        frame,
                        "Final settled lobby Back clicked through ordinary input",
                    );
                } else {
                    reset_round(core);
                    self.phase = if self.config.is_some() {
                        RoundPhase::Lobby
                    } else {
                        core.phase = Phase::Lobby;
                        core.kick_observed = true;
                        RoundPhase::LanEntry
                    };
                }
            }
            RoundPhase::ExitFault => {
                if self.exit_block_applied(state, rendered, frame)? {
                    let rect = crate::app::shell_wol::lobby::layout(state)
                        .page
                        .button_rect(wol_lobby::BACK)
                        .context("WOL lobby Back button")?;
                    click(state, rect);
                    self.record(
                        frame,
                        "Scoped HTTPS block acknowledged; actual lobby Back clicked",
                    );
                    self.phase = RoundPhase::ExitWelcome;
                }
            }
            RoundPhase::ExitWelcome
                if rendered == PresentedShell::WolWelcome
                    && MoviesCapture::slide_settled(state, ShellSlideKind::WolWelcome) =>
            {
                let rect = App::wol_welcome_layout(state)
                    .page
                    .button_rect(crate::ui::wol_shell::MAIN_MENU)
                    .context("WOL welcome Main Menu button")?;
                click(state, rect);
                self.record(
                    frame,
                    "WOL welcome Main Menu clicked through ordinary input",
                );
                self.phase = RoundPhase::MainReturn;
            }
            RoundPhase::MainReturn
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready_for(
                        MainMenuCaptureSnapshot::from_state(state),
                        self.config.as_ref().is_some_and(|config| config.latency),
                    )? =>
            {
                if !self.observe_exit_cleanup(state, frame)? {
                    return Ok(());
                }
                if crate::app::shell_wol::online_cleanup_pending(state) {
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
                    .as_ref()
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
                    "online_cleanup_pending":crate::app::shell_wol::online_cleanup_pending(state),
                    "online_logout_acknowledged":acknowledged}));
                self.record(
                    frame,
                    "Settled Main Menu and acknowledged quiescent cleanup observed",
                );
                self.phase = if self.observe_exit_menu(state, frame)? {
                    RoundPhase::Finished
                } else {
                    RoundPhase::Main
                };
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        let startup_failure = self
            .config
            .as_ref()
            .is_some_and(|config| config.startup_host_loss.is_some());
        ensure!(
            self.ready()
                && self.rounds.len()
                    == if self.profile.is_some() {
                        0
                    } else if self.failure_prefix_written || startup_failure {
                        1
                    } else {
                        2
                    },
            "network outcome capture is incomplete"
        );
        ensure!(
            state.match_state.live_network.is_none(),
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
            "network_checkpoint":self.network_checkpoint,
            "exit_reentry":self.exit_reentry.as_ref().map(ExitReentry::receipt),
            "loading_cancellation":self.loading_cancellation,
            "profile_check":self.profile.as_ref().map(|profile|profile.receipt()).transpose()?,
            "loading_host_loss":self.loading_host_loss.as_ref().map(|loss|loss.receipt(self.online_config())).transpose()?,
            "scope":if self.profile.is_some() { "host-only actual map/page/combo/Back/Create profile reconstruction followed by settled Menu; no match or30-human acceptance claim" } else if self.config.as_ref().is_some_and(|config|config.ai.is_some()) { "two one-human/one-CPU games with ordinary AI dropdown input, actual CPU House/activity, local command/checksum progress, actual B6 Leave, lobby rematch and acknowledged Main Menu return; results are recorded only when shown, with no cross-client determinism claim" } else if startup_failure { "aborted uncertified full-three-player startup after ordinary100, external owned original-host kill, real Initial EA votes, ConnectionFailed OK, actual authenticated lobby and acknowledged Menu/logout; no gameplay prefix, result or rematch is claimed" } else if self.failure_prefix_written && self.config.is_none() { "real300-frame CRC and strong-hash prefix, external scoped FRAMEINFO corruption, actual checksum CE Stop and settled LAN lobby cleanup" } else if self.failure_prefix_written { "real300-frame command/checksum prefix, externally terminated peer, expected ordinary connection timeout/removal or Quit, and authenticated lobby return" } else { "two complete games with tactical input, shared simulation checksums, Observe defeat, real results, lobby rematch and settled Main Menu return; unit scale is measured from each runtime snapshot" },
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
    core.actions.clear();
    core.actor = None;
    core.actor_last_seen = None;
    core.origin = None;
    core.observed_movement = false;
    core.stop_observation = super::StopObservation::default();
    core.stopped = false;
    core.last_wait = None;
    core.kick_observed = false;
    core.entered_setup = false;
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

fn lobby_ready(state: &AppState, rendered: PresentedShell) -> bool {
    rendered == PresentedShell::WolLobby && online_lobby_settled(state)
}

pub(super) fn online_lobby_settled(state: &AppState) -> bool {
    MoviesCapture::slide_settled(state, ShellSlideKind::WolLobby)
        && state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| visit.lobby())
            .is_some_and(|lobby| lobby.interactive())
}

pub(super) fn online_setup_settled(state: &AppState, host: bool) -> bool {
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

fn click(state: &mut AppState, rect: RectPx) {
    set_window_pointer_to_rect_center(state, rect);
    App::handle_wol_mouse_move(state);
    App::handle_wol_mouse_down(state);
    App::handle_wol_mouse_up(state);
    MoviesCapture::restore_neutral_pointer(state);
}
