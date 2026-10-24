//! Two independent App processes, using the ordinary LAN service and frame
//! boundary. Only user actions are automated. No peer result, loading report,
//! synchronized record, checksum or simulation step is injected here.

use super::*;

/// Navigate with the normal buttons before resolving a resource control. Its
/// visible slot is never the participant's absolute row on an extended roster.
fn setup_control_at_row(
    state: &mut AppState,
    column: SetupColumn,
    row: usize,
) -> Result<Option<u16>> {
    use crate::ui::multiplayer_setup::{NEXT_ROWS, PREVIOUS_ROWS, VISIBLE_SETUP_ROWS};
    for _ in 0..=crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT / VISIBLE_SETUP_ROWS {
        let view = setup::view(state).context("setup view")?;
        if row >= view.row_count {
            return Ok(None);
        }
        if row < view.first_row {
            setup_button(state, PREVIOUS_ROWS)?;
        } else if row >= view.first_row + VISIBLE_SETUP_ROWS {
            setup_button(state, NEXT_ROWS)?;
        } else {
            return Ok(view
                .combos
                .iter()
                .find(|combo| {
                    combo.row as usize == row
                        && combo.visible
                        && SetupColumn::decode(combo.id)
                            .is_some_and(|(candidate, _)| candidate == column)
                })
                .map(|combo| combo.id));
        }
    }
    anyhow::bail!("setup page navigation did not reach the participant")
}
use crate::app::diagnostics::shell_capture::{LiveLanConfig, LiveLanTransport};
use crate::net::udp::UdpBindConfig;
use crate::ui::multiplayer_setup::{SetupAction, SetupColumn};

mod battle;
mod failure;
mod mass;
mod online;
mod profile;
mod setup_crash;

const HOST_NAME: &str = "LiveHost";
const GUEST_NAME: &str = "LiveGuest";
const REQUIRED_FRAMES: u32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Main,
    Lobby,
    Discover,
    HostSetup,
    HostReady,
    HostKick,
    GuestSetup,
    Loading,
    Running,
    Finished,
}

pub(super) struct LiveLanCapture {
    target: LanTarget,
    config: Option<LiveLanConfig>,
    rounds: Option<online::LiveRoundFlow>,
    frame_budget: Option<u32>,
    expected_players: usize,
    phase: Phase,
    route: Vec<Value>,
    /// Observations of the existing checksum ring, indexed by native frame.
    checksums: Vec<u32>,
    /// The existing replay owner's shared-world hashes for the observed prefix.
    /// Retained across teardown because a connection Quit flushes that log.
    simulation_hashes: Vec<(u64, u64)>,
    startup: Option<Value>,
    /// Read-only proof retained before the setup protocol is dropped for loading.
    map_admission: Option<Value>,
    actions: Vec<Value>,
    actor: Option<u64>,
    origin: Option<(u16, u16)>,
    observed_movement: bool,
    stopped_position: Option<Value>,
    stopped: bool,
    last_wait: Option<String>,
    kick_observed: bool,
    entered_setup: bool,
    country_selected: bool,
    battle: Option<battle::BattleOpening>,
    mass: Option<mass::MassInput>,
    failure: Option<failure::FailureCapture>,
    setup_crash: Option<setup_crash::SetupCrashCapture>,
}

impl LiveLanCapture {
    pub(super) fn new(target: LanTarget, config: LiveLanConfig) -> Self {
        let flow = matches!(
            target,
            LanTarget::LiveHost
                | LanTarget::LiveGuest
                | LanTarget::LiveDesyncHost
                | LanTarget::LiveDesyncGuest
        )
        .then(online::LiveRoundFlow::new_lan);
        Self::new_config(target, Some(config), flow)
    }

    pub(super) fn new_online(
        target: LanTarget,
        config: crate::app::diagnostics::shell_capture::LiveOnlineConfig,
    ) -> Self {
        let startup_failure = config
            .startup_host_loss
            .as_ref()
            .map(|_| failure::FailureCapture::startup(config.clone()));
        let frames = config.frames;
        let players = config.players;
        let simple = config.simple_input;
        let mass = config.mass_input;
        let mut capture = Self::new_config(target, None, Some(online::LiveRoundFlow::new(config)));
        if let Some(failure) = startup_failure {
            capture.failure = Some(failure);
        }
        capture.frame_budget = Some(frames);
        capture.expected_players = players;
        if simple {
            capture.battle = None;
        }
        if mass {
            capture.mass = Some(mass::MassInput::default());
        }
        capture
    }

    fn new_config(
        target: LanTarget,
        config: Option<LiveLanConfig>,
        rounds: Option<online::LiveRoundFlow>,
    ) -> Self {
        let setup_crash = config
            .as_ref()
            .and_then(|config| config.setup_crash)
            .map(setup_crash::SetupCrashCapture::new);
        Self {
            target,
            config,
            rounds,
            setup_crash,
            frame_budget: None,
            expected_players: 2,
            phase: Phase::Main,
            route: Vec::new(),
            checksums: Vec::new(),
            simulation_hashes: Vec::new(),
            startup: None,
            map_admission: None,
            actions: Vec::new(),
            actor: None,
            origin: None,
            observed_movement: false,
            stopped_position: None,
            stopped: false,
            last_wait: None,
            kick_observed: false,
            entered_setup: false,
            country_selected: false,
            battle: matches!(
                target,
                LanTarget::BattleHost
                    | LanTarget::BattleGuest
                    | LanTarget::OnlineHost
                    | LanTarget::OnlineGuest
            )
            .then(|| battle::BattleOpening::new(!target.online())),
            mass: None,
            failure: match target {
                LanTarget::LiveTimeoutHost | LanTarget::OnlineTimeoutHost => {
                    Some(failure::FailureCapture::new(failure::Mode::Timeout))
                }
                LanTarget::LiveQuitHost | LanTarget::OnlineQuitHost => {
                    Some(failure::FailureCapture::new(failure::Mode::Quit))
                }
                LanTarget::LiveDesyncHost | LanTarget::LiveDesyncGuest => {
                    Some(failure::FailureCapture::new(failure::Mode::Desync))
                }
                _ => None,
            },
        }
    }

    fn host(&self) -> bool {
        matches!(
            self.target,
            LanTarget::OnlineHost
                | LanTarget::OnlineTimeoutHost
                | LanTarget::OnlineQuitHost
                | LanTarget::LiveHost
                | LanTarget::LiveTimeoutHost
                | LanTarget::LiveQuitHost
                | LanTarget::LiveDesyncHost
                | LanTarget::BattleHost
        )
    }

    fn required_frames(&self) -> u32 {
        if let Some(frames) = self.frame_budget {
            return frames;
        }
        if self.battle.is_some() {
            // Includes paid GI rally/combat, building repair and sale with an
            // unfinished Factory queue, natural crew and cleanup. Every frame
            // still belongs to normal production pacing/pathing/combat.
            7200
        } else {
            REQUIRED_FRAMES
        }
    }

    fn select_country(&mut self, state: &mut AppState) -> Result<()> {
        if self.battle.is_none() || self.country_selected {
            return Ok(());
        }
        let country = state
            .process_assets
            .multiplayer_countries()
            .position(|(id, _)| id.eq_ignore_ascii_case("Americans"))
            .context("retail Americans country")?;
        let row = state
            .match_state
            .session
            .network_setup()
            .map(|launch| launch.local_player())
            .and_then(|human| human.raw_slot_index)
            .context("local setup row")?;
        let control = setup_control_at_row(state, SetupColumn::Country, usize::from(row))?
            .context("visible local country row")?;
        setup::apply_action(
            state,
            SetupAction::Select {
                control,
                value: i32::try_from(country)?,
            },
        );
        self.country_selected = true;
        Ok(())
    }

    /// Initial joined setup has neither host metadata nor a canonical roster.
    /// Wait for that admission, including the intended player capacity, before
    /// treating the local combo's available waypoints as authoritative.
    fn start_selection_admitted(&self, state: &AppState) -> bool {
        let session = &state.match_state.session;
        (session.host() || session.network_start_seed().is_some())
            && session
                .network_map_metadata()
                .is_some_and(|metadata| metadata.player_capacity >= self.expected_players as i32)
            && session.network_setup().is_some_and(|launch| {
                launch.selected_map_file.is_some()
                    && launch.humans.len() == self.expected_players
                    && launch
                        .humans
                        .iter()
                        .all(|human| human.raw_slot_index.is_some())
            })
    }

    fn start_matches(&self, state: &AppState, start: Option<u8>) -> bool {
        start.is_none_or(|start| {
            state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| {
                    launch.local_player().slot.start_position
                        == crate::skirmish_launch::LaunchStartPosition::Position(start)
                })
        })
    }

    /// Select an authored waypoint through the local human's visible Start
    /// combo. The configured waypoint is never used as a source/raw-row index.
    /// Read the real choice each time: later map selection can reset it to Auto.
    fn select_start(&mut self, state: &mut AppState, start: Option<u8>) -> Result<bool> {
        let Some(start) = start else {
            return Ok(false);
        };
        if self.start_matches(state, Some(start)) || !self.start_selection_admitted(state) {
            return Ok(false);
        }
        let row = state
            .match_state
            .session
            .network_setup()
            .and_then(|launch| launch.local_player().raw_slot_index)
            .context("local start-choice row")?;
        let control = setup_control_at_row(state, SetupColumn::Start, usize::from(row))?
            .context("visible local Start combo")?;
        let view = setup::view(state).context("start-choice view")?;
        let combo = view.combo(control).context("local Start combo")?;
        ensure!(
            view.enabled(control)
                && combo
                    .choices
                    .iter()
                    .any(|choice| choice.value == i32::from(start)),
            "configured start is not available through the normal local combo"
        );
        setup::apply_action(
            state,
            SetupAction::Select {
                control,
                value: i32::from(start),
            },
        );
        ensure!(
            state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| launch.local_player().slot.start_position
                    == crate::skirmish_launch::LaunchStartPosition::Position(start)),
            "normal Start control did not select the configured waypoint"
        );
        Ok(true)
    }

    fn event(&mut self, frame: u32, action: &str) {
        log::debug!("live LAN {:?} render {frame}: {action}", self.target);
        self.route
            .push(json!({"render_frame":frame,"action":action}));
    }

    pub(super) fn ready(&self) -> bool {
        if let Some(crash) = &self.setup_crash {
            return crash.ready();
        }
        self.rounds
            .as_ref()
            .map_or(self.phase == Phase::Finished, online::LiveRoundFlow::ready)
    }

    pub(super) fn owns_simulation_steps(&self) -> bool {
        // Keep the final observed frame stable during GPU readback. Ordinary
        // App frame pacing and NetworkCommandBoundary own every earlier step.
        self.ready()
    }

    pub(super) fn after_present(
        &mut self,
        state: &mut AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<()> {
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "live LAN capture reached shell fallback"
        );
        // Only the explicitly armed external desync run accepts checksum CE.
        // The disappearance runs still require the ordinary connection EA.
        let expected_checksum = self
            .failure
            .as_ref()
            .is_some_and(|failure| failure.expects_checksum());
        let expected_startup = self
            .failure
            .as_ref()
            .is_some_and(|failure| failure.is_startup() && failure.armed());
        ensure!(
            state.match_state.network_failure.is_none() || expected_checksum || expected_startup,
            "unexpected network failure modal during live capture"
        );
        if let Some(runtime) = &state.match_state.sim_runtime {
            ensure!(
                runtime.simulation.pending_network_failure().is_none() || expected_checksum,
                "live LAN checksum failure: {:?}",
                runtime.simulation.pending_network_failure()
            );
            ensure!(
                runtime.simulation.pending_network_interruption().is_none()
                    || (expected_startup
                        && runtime.simulation.pending_network_interruption()
                            == Some(crate::sim::world::NetworkInterruption::ConnectionFailed)),
                "live LAN interrupted: {:?}",
                runtime.simulation.pending_network_interruption()
            );
        }
        if rendered == PresentedShell::NetworkSetup {
            self.observe_map_admission(state)?;
        }
        if let Some(mut rounds) = self.rounds.take() {
            let controls = rounds.controls(self);
            let result = if controls {
                rounds.after_present(self, state, rendered, frame)
            } else {
                Ok(())
            };
            self.rounds = Some(rounds);
            if controls {
                return result;
            }
        }
        if let Some(mut crash) = self.setup_crash.take() {
            // This pre-GO checkpoint holds ordinary Accept/Start/kick inputs,
            // while the same LAN poll/map/lease owners continue normally. In
            // particular, its wait cannot hit the generic1000-frame join guard.
            let result = crash.after_present(self, state, rendered, frame);
            self.setup_crash = Some(crash);
            if result? {
                return Ok(());
            }
        }
        self.entered_setup |= state.match_state.session.network_setup().is_some();
        if !self.host()
            && self.entered_setup
            && !self.kick_observed
            && matches!(self.phase, Phase::GuestSetup | Phase::Loading)
            && rendered == PresentedShell::LanLobby
            && lobby_settled(state)
        {
            ensure!(
                state.match_state.session.network_setup().is_none(),
                "kicked guest retained admitted setup"
            );
            self.kick_observed = true;
            self.country_selected = false;
            self.phase = Phase::Discover;
            self.event(
                frame,
                "host kick returned guest to BB; rediscover and rejoin",
            );
        }
        if frame % 120 == 0 {
            log::debug!(
                "live LAN {:?} render {frame}: phase={:?}, screen={:?}, setup={}",
                self.target,
                self.phase,
                state.frontend.screen,
                state
                    .match_state
                    .session
                    .network_setup()
                    .map_or(0, |launch| launch.humans.len())
            );
        }
        let last_action_frame = self
            .route
            .last()
            .and_then(|event| event["render_frame"].as_u64())
            .unwrap_or(0) as u32;
        if frame.saturating_sub(last_action_frame) >= 1000
            && matches!(self.phase, Phase::GuestSetup | Phase::HostReady)
            && state
                .match_state
                .session
                .network_setup()
                .is_none_or(|launch| launch.humans.len() < 2)
        {
            let lobby = state.frontend.lan.as_ref();
            anyhow::bail!(
                "live LAN admission stalled: phase={:?}, pending={:?}, modal={:?}, lobby={:?}",
                self.phase,
                lobby.and_then(|lobby| lobby.pending_game()),
                lobby.and_then(|lobby| lobby.modal),
                lobby.map(|lobby| &lobby.lobby)
            );
        }
        match self.phase {
            Phase::Main
                if rendered == PresentedShell::MainMenu
                    && steady_main_menu_capture_ready(MainMenuCaptureSnapshot::from_state(
                        state,
                    ))? =>
            {
                // Hidden-window transcript captures suppress physical LAN.
                // Live captures install the real owner before normal Network
                // input opens the shell; it retains sockets through gameplay.
                LanCapture::isolated_root(state)?;
                ensure!(
                    state.match_state.lan_service.is_none(),
                    "LAN transport was already installed"
                );
                let config = self.config.as_ref().expect("LAN transport configuration");
                let mut service = match config.transport {
                    LiveLanTransport::ProductionBroadcast => {
                        crate::app::lan_service::LanService::open(
                            crate::app::match_runtime::frame_pacer::wall_clock_bucket(),
                        )?
                    }
                    LiveLanTransport::Explicit { bind, peer } => {
                        let alternate = std::net::SocketAddrV4::new(*bind.ip(), bind.port() + 2);
                        crate::app::lan_service::LanService::open_on(
                            UdpBindConfig {
                                primary: bind,
                                alternate,
                            },
                            &[peer],
                        )?
                    }
                };
                service.start_io_pump()?;
                state.match_state.lan_service = Some(service);
                click_main_menu(
                    state,
                    crate::ui::main_menu_shell::MainMenuControlId::Network0x578,
                )?;
                self.event(
                    frame,
                    "real UDP service installed; MainMenu Network clicked",
                );
                self.phase = Phase::Lobby;
            }
            Phase::Lobby if rendered == PresentedShell::LanLobby && lobby_settled(state) => {
                let name = if self.host() { HOST_NAME } else { GUEST_NAME };
                lobby_click(state, lobby::layout(state).name);
                lobby::key_press(state, Some(KeyCode::Home), None);
                for _ in 0..32 {
                    lobby::key_press(state, Some(KeyCode::Delete), None);
                }
                lobby::key_press(state, None, Some(name));
                if state.frontend.lan.as_ref().context("BB")?.lobby.observer {
                    lobby_click(state, lobby::layout(state).observer_toggle);
                }
                self.event(frame, "BB name entered through normal input");
                if self.host() {
                    lobby_button(state, crate::ui::lan_shell::CREATE)?;
                    self.phase = Phase::HostSetup;
                } else {
                    self.phase = Phase::Discover;
                }
            }
            Phase::Discover if rendered == PresentedShell::LanLobby && lobby_settled(state) => {
                let found = state
                    .frontend
                    .lan
                    .as_ref()
                    .context("BB")?
                    .lobby
                    .games
                    .iter()
                    .position(|game| game.name == HOST_NAME && game.open);
                if let Some(index) = found {
                    // Index0 is BB's synthetic no-game row. Drive its normal
                    // selection handler, which emits the real query packet.
                    let layout = lobby::layout(state);
                    let lobby_state = &state.frontend.lan.as_ref().context("BB")?.lobby;
                    let geometry = lobby_state.game_list.geometry(
                        layout.games,
                        lobby_state.game_count(),
                        layout.page.scale,
                    );
                    let (x, y) =
                        list_row_point(geometry, index + 1, lobby_state.game_list.top_index);
                    lobby_click(state, RectPx::new(x, y, 2, 2));
                    lobby_button(state, crate::ui::lan_shell::JOIN)?;
                    self.event(frame, "discovered host selected; real Join request");
                    self.phase = Phase::GuestSetup;
                }
            }
            Phase::HostSetup => {
                let selected = self.config.as_ref().and_then(|config| config.map.clone());
                let prepared = if let Some(mut rounds) = self.rounds.take() {
                    let result =
                        rounds.prepare_host_map(self, state, rendered, frame, selected.as_deref());
                    self.rounds = Some(rounds);
                    result?
                } else if rendered == PresentedShell::NetworkSetup && setup_settled(state, true) {
                    self.prepare_host_setup(state, selected.as_deref(), self.expected_players)?;
                    true
                } else {
                    false
                };
                if prepared {
                    self.event(
                        frame,
                        "host selected Battle map and open human rows through normal setup",
                    );
                    self.phase = Phase::HostReady;
                }
            }
            Phase::HostReady
                if rendered == PresentedShell::NetworkSetup && setup_settled(state, true) =>
            {
                let session = &state.match_state.session;
                if !self.kick_observed
                    && let Some(row) = session.network_setup().and_then(|launch| {
                        launch
                            .humans
                            .iter()
                            .find(|human| human.name == GUEST_NAME)
                            .and_then(|human| human.raw_slot_index)
                    })
                {
                    let control =
                        setup_control_at_row(state, SetupColumn::Player, usize::from(row))?
                            .context("visible guest row")?;
                    setup::apply_action(state, SetupAction::Select { control, value: 2 });
                    self.event(
                        frame,
                        "replace guest row through normal UI; real Type8 kick",
                    );
                    self.phase = Phase::HostKick;
                    return Ok(());
                }
                if let Some(launch) = session.network_setup()
                    && launch.humans.len() == 2
                    && launch
                        .humans
                        .iter()
                        .all(|human| human.raw_slot_index.is_some())
                    && launch.humans.iter().enumerate().all(|(index, _)| {
                        session.network_player_ready(index)
                            && session.network_map_status(index) == Some(1)
                    })
                {
                    setup_button(state, crate::ui::multiplayer_setup::START)?;
                    self.event(frame, "all real acknowledgements present; Start clicked");
                    self.phase = Phase::Loading;
                }
            }
            Phase::HostKick
                if rendered == PresentedShell::NetworkSetup && setup_settled(state, true) =>
            {
                if state
                    .match_state
                    .session
                    .network_setup()
                    .is_some_and(|launch| launch.humans.len() == 1)
                {
                    self.kick_observed = true;
                    self.event(
                        frame,
                        "real guest departure removed host membership and endpoint",
                    );
                    self.phase = Phase::HostReady;
                }
            }
            Phase::GuestSetup
                if rendered == PresentedShell::NetworkSetup && setup_settled(state, false) =>
            {
                self.select_country(state)?;
                let session = &state.match_state.session;
                if session.network_setup().is_some_and(|launch| {
                    launch.humans.len() == 2
                        && launch
                            .humans
                            .iter()
                            .all(|human| human.raw_slot_index.is_some())
                }) && session.network_start_seed().is_some()
                    && session.network_setup().is_some_and(|launch| {
                        !session.network_player_ready(launch.local_player_index)
                    })
                    && setup::view(state).is_some_and(|view| view.accept_enabled)
                {
                    setup_button(state, crate::ui::multiplayer_setup::ACCEPT)?;
                    self.event(frame, "received real options/map status; Accept clicked");
                    // A host may still commit a generated/custom map. Remain
                    // in editable setup so its ordinary Ready invalidation can
                    // be acknowledged again before actual loading begins.
                }
            }
            Phase::Loading | Phase::GuestSetup if state.frontend.screen == GameScreen::InGame => {
                ensure!(
                    self.kick_observed,
                    "live diagnostic skipped kick/rejoin validation"
                );
                ensure!(
                    state.match_state.live_network.is_some(),
                    "loaded LAN match has no real command transport"
                );
                self.startup = Some(startup_receipt(state)?);
                self.event(
                    frame,
                    "real GO/loading finished; ordinary simulation running",
                );
                self.phase = Phase::Running;
            }
            _ => {}
        }
        if self.phase == Phase::Running {
            self.observe_run(state, frame, self.expected_players)?;
        }
        Ok(())
    }

    fn observe_map_admission(&mut self, state: &AppState) -> Result<()> {
        let session = &state.match_state.session;
        let Some(launch) = session.network_setup() else {
            return Ok(());
        };
        let (Some(file), Some(metadata), Some(source), Some(effective)) = (
            launch.selected_map_file.as_deref(),
            session.network_map_metadata(),
            session.network_local_map_source(),
            session.network_local_map_effective_digest(),
        ) else {
            return Ok(());
        };
        if session.network_map_status(launch.local_player_index) != Some(1)
            || source.physical_identity()
                != Some((
                    usize::try_from(metadata.byte_length)?,
                    metadata.source_sha256.as_str(),
                ))
            || effective != metadata.effective_digest
        {
            return Ok(());
        }
        let protocol = state
            .frontend
            .network_setup_protocol
            .as_ref()
            .context("verified setup protocol")?;
        let generated = crate::map::rmg::is_seed_selection(file);
        let staging = if generated {
            if session.network_random_map_preview().is_none() {
                return Ok(());
            }
            let text = state
                .frontend
                .multiplayer_shell_runtime
                .start_staging()
                .to_w_text();
            let checked = crate::app::shell_network_setup::maps::checked_staging(
                &text,
                metadata.player_capacity,
            )?;
            Some(
                json!({"text":text,"digest":crate::app::shell_network_setup::maps::staging_digest(state),
                "count":checked.count,"width":checked.width,"height":checked.height}),
            )
        } else {
            None
        };
        self.map_admission = Some(json!({"file":file,"generated":generated,"source":source,
        "source_sha256":metadata.source_sha256,"byte_length":metadata.byte_length,
        "effective_digest":effective,"capacity":metadata.player_capacity,
        "preview_source":session.network_random_map_preview(),"staging":staging,
        "preview_surface":protocol.received_preview().map(|(_,image)|json!({"width":image.width,"height":image.height})),
        "preview_bounds":protocol.received_preview_bounds().map(|bounds|json!({
            "origin":[bounds.origin_x,bounds.origin_y],"width":bounds.width,"height":bounds.height,
            "starts":bounds.start_points.iter().map(|point|[point.x,point.y]).collect::<Vec<_>>()
        }))}));
        Ok(())
    }
    fn prepare_host_setup(
        &mut self,
        state: &mut AppState,
        selected_map: Option<&str>,
        players: usize,
    ) -> Result<()> {
        let mode = state
            .frontend
            .skirmish_modes
            .iter()
            .find(|mode| mode.class == crate::skirmish_modes::MpModeClass::Battle)
            .context("retail Battle mode")?;
        let mode_id = mode.id;
        let map_index = state
            .frontend
            .scenario_catalog
            .records()
            .iter()
            .position(|map| {
                (map.kind == crate::map::skirmish_scenarios::SkirmishScenarioKind::ConcreteMap
                    || selected_map.is_some_and(|file| file.eq_ignore_ascii_case("RandMap.Sed")))
                    && map.player_capacity >= players as i32
                    && selected_map.is_none_or(|file| map.file_name.eq_ignore_ascii_case(file))
                    && crate::map::skirmish_scenarios::is_valid_map(map, mode, false)
            })
            .context("retail Battle map with two starts")?;
        setup::commit_map_selection(state, mode_id, map_index, None)?;
        self.select_country(state)?;
        // Use the existing action admission, including current control
        // availability. The isolated profile must allow one open peer.
        let row_count = setup::view(state).context("host setup view")?.row_count;
        for row in 1..row_count {
            if state
                .match_state
                .session
                .network_setup()
                .is_some_and(|launch| {
                    launch
                        .humans
                        .iter()
                        .any(|human| human.raw_slot_index == Some(row as u8))
                })
            {
                continue;
            }
            let Some(control) = setup_control_at_row(state, SetupColumn::Player, row)? else {
                continue;
            };
            if setup::view(state)
                .and_then(|view| view.combo(control).cloned())
                .is_some_and(|combo| {
                    combo.visible
                        && combo.enabled
                        && combo.choices.iter().any(|choice| choice.value == 2)
                })
            {
                setup::apply_action(state, SetupAction::Select { control, value: 2 });
            }
        }
        // Keep ordinary host input on its local page after visiting the roster.
        let _ = setup_control_at_row(state, SetupColumn::Player, 0)?;
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("host setup")?;
        ensure!(
            launch.opponents.is_empty(),
            "live diagnostic requires a two-human match without AI"
        );
        Ok(())
    }

    fn observe_run(
        &mut self,
        state: &mut AppState,
        frame: u32,
        active_players: usize,
    ) -> Result<()> {
        if let Some(failure) = self.failure.as_mut().filter(|failure| failure.armed()) {
            if failure.step(state, frame, &mut self.route)? {
                MoviesCapture::restore_neutral_pointer(state);
                self.phase = Phase::Finished;
            }
            return Ok(());
        }
        ensure!(
            state.frontend.screen == GameScreen::InGame,
            "live LAN match left gameplay before completion"
        );
        let simulation = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("live simulation")?
            .simulation;
        let committed = simulation.session().binary_frame();
        if self.battle.is_none()
            && let Some(actor) = self.actor
        {
            let entity = simulation
                .entities()
                .get(actor)
                .context("commanded actor disappeared")?;
            self.observed_movement |= self
                .origin
                .is_some_and(|origin| origin != (entity.position.rx(), entity.position.ry()));
            if committed >= 160 && self.stopped_position.is_none() {
                self.stopped_position = Some(serde_json::to_value(entity.position)?);
            }
        }
        ensure!(
            committed >= self.checksums.len() as u32,
            "native frame moved backwards"
        );
        ensure!(
            committed.min(self.required_frames()) - self.checksums.len() as u32 <= 256,
            "capture missed overwritten checksum history"
        );
        let required_frames = self.required_frames();
        while (self.checksums.len() as u32) < committed.min(required_frames) {
            self.checksums.push(
                state
                    .match_state
                    .session
                    .checksum_history()
                    .get(self.checksums.len() as u32),
            );
        }
        if committed >= required_frames && self.simulation_hashes.is_empty() {
            self.simulation_hashes =
                recorded_simulation_hashes(state, Some(u64::from(required_frames)))?;
        }
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("real live transport")?;
        // A LAN battle ends at its own committed observation prefix. The peer
        // can write that same prefix and exit before our next render; waiting
        // for its final advertisement races that ordinary transport teardown.
        // The pair runner still requires and compares both complete prefixes.
        let lan_battle_prefix =
            self.battle.is_some() && !self.target.online() && committed >= required_frames;
        ensure!(
            lan_battle_prefix || live.peers().len() + 1 == active_players,
            "live diagnostic peer disappeared"
        );
        let wait = format!("{:?}", live.wait_info());
        if self.last_wait.as_deref() != Some(&wait) {
            self.route
                .push(json!({"render_frame":frame,"native_frame":committed,"wait":wait}));
            self.last_wait = Some(wait);
        }
        if let Some(mass) = self.mass.as_mut() {
            let complete = mass.step(state, committed)?;
            if committed >= required_frames {
                ensure!(
                    complete,
                    "mass input did not finish within the configured observation frames"
                );
                if state
                    .match_state
                    .live_network
                    .as_ref()
                    .context("live transport")?
                    .peers()
                    .iter()
                    .all(|peer| peer.frame() >= required_frames as i32)
                {
                    state
                        .match_state
                        .match_diagnostics
                        .end_command_observation();
                    MoviesCapture::restore_neutral_pointer(state);
                    self.phase = Phase::Finished;
                }
            }
            return Ok(());
        }
        if self.battle.is_some() {
            // Wait_For_Players is inside an unfinished native MainTick: Logic
            // may already have repaired a building, but DoList and the frame
            // commit are still pending. Observe only its committed result.
            if simulation.pending_network_wait() {
                return Ok(());
            }
            let host = self.host();
            let battle = self.battle.as_mut().expect("battle target");
            battle.step(state, committed, host, &mut self.actions)?;
            if committed >= required_frames {
                ensure!(
                    battle.finished(state)?,
                    "Battle opening incomplete: {}",
                    battle.phase()
                );
                let live = state
                    .match_state
                    .live_network
                    .as_ref()
                    .context("live transport")?;
                if lan_battle_prefix
                    || live
                        .peers()
                        .iter()
                        .all(|peer| peer.frame() >= required_frames as i32)
                {
                    MoviesCapture::restore_neutral_pointer(state);
                    self.phase = Phase::Finished;
                }
            }
            return Ok(());
        }
        if committed >= 10 && self.actor.is_none() {
            self.issue_move(state, committed)?;
        }
        if committed >= 80 && !self.stopped {
            let actor = self.actor.context("local move actor")?;
            battle::mobile_stop_with_keyboard(state, actor)?;
            self.stopped = true;
            self.actions
                .push(json!({"native_frame":committed,"input":"Stop","actor":actor}));
        }
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("live transport")?;
        if self.checksums.len() >= self.required_frames() as usize
            && self.stopped
            // The survivor must arm from its own committed prefix: the
            // finishing guest may exit at300 before sending frame300's
            // advertisement. The pair receipt compares both300-frame rings.
            && (self.failure.is_some()
                || live
                    .peers()
                    .iter()
                    .all(|peer| peer.frame() >= self.required_frames() as i32))
        {
            ensure!(
                self.observed_movement,
                "Move never changed the actor's cell"
            );
            let entity = state
                .match_state
                .sim_runtime
                .as_ref()
                .context("simulation")?
                .simulation
                .entities()
                .get(self.actor.context("actor")?)
                .context("actor")?;
            ensure!(
                self.stopped_position.as_ref() == Some(&serde_json::to_value(entity.position)?),
                "Stop did not retain a stable position through the final frames"
            );
            MoviesCapture::restore_neutral_pointer(state);
            if let Some(failure) = self.failure.as_mut() {
                failure.arm(state)?;
                self.event(
                    frame,
                    if self.target.desync_failure() {
                        "shared 300-frame prefix complete; awaiting external scoped FRAMEINFO corruption"
                    } else {
                        "shared 300-frame prefix complete; host awaits real peer disappearance"
                    },
                );
            } else {
                self.phase = Phase::Finished;
            }
        }
        Ok(())
    }

    fn issue_move(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let (actor, from, to) = battle::mobile_move_with_mouse(state)?;
        self.actor = Some(actor);
        self.origin = Some(from);
        self.actions.push(json!({"native_frame":frame,"input":"tactical Move","actor":actor,"from":from,"target":to}));
        Ok(())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        if let Some(crash) = &self.setup_crash {
            return crash.receipt();
        }
        if let Some(online) = &self.rounds {
            return online.receipt(state);
        }
        self.round_receipt(state)
    }

    fn round_receipt(&self, state: &AppState) -> Result<Value> {
        ensure!(self.ready(), "live LAN capture has not finished");
        self.observed_receipt(state)
    }

    fn observed_receipt(&self, state: &AppState) -> Result<Value> {
        self.observed_receipt_through(state, None)
    }

    fn observed_receipt_through(&self, state: &AppState, through: Option<u32>) -> Result<Value> {
        let checksums = if let Some(frame) = through {
            self.checksums
                .get(..frame as usize)
                .context("uncommitted checkpoint prefix")?
        } else {
            &self.checksums
        };
        let hashes = if let Some(frame) = through {
            recorded_simulation_hashes(state, Some(u64::from(frame)))?
        } else {
            self.simulation_hashes.clone()
        };
        ensure!(
            !hashes.is_empty(),
            "live capture has no recorded simulation hashes"
        );
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("simulation")?
            .simulation;
        let live = state.match_state.live_network.as_ref();
        ensure!(
            live.is_some()
                || self
                    .failure
                    .as_ref()
                    .is_some_and(|failure| failure.returned()),
            "finished live capture lost its transport"
        );
        Ok(
            json!({"target":format!("{:?}",self.target),"service_boundary":if self.target.online() {"independent App processes; real HTTPS/UDP relay; ordinary frame boundary"} else {"two independent App processes; real UDP/global/private queues; ordinary frame boundary"},
            "scope":if self.target.desync_failure() { "real300-frame CRC and strong-hash prefix followed by external scoped FRAMEINFO corruption, actual checksum CE Stop and ordinary LAN lobby cleanup" } else if self.failure.is_some() { "shared 300-frame prefix followed by real peer disappearance and ordinary wait recovery or Quit; bounded runtime validation, not whole-match/native parity" } else if self.target.online() {"observed production match before real outcome; tactical commands and simulation checksums"} else if self.battle.is_some() { "kick/rejoin, Battle opening, paid GI rally/combat, paid GAPILE repair and sale with an unfinished Infantry queue, natural crew, refunds and cleanup, then former-foundation Move; bounded runtime validation, not whole-match/native parity" } else { "kick/rejoin, 300 native frames and local Move/Stop; bounded runtime validation, not whole-match/native parity" },
            "process_id":std::process::id(),
            "loading_progress":crate::app::diagnostics::shell_capture::loading_runtime::progress_receipt(state)?,
            "participant_count":self.startup.as_ref().and_then(|startup|startup["sources"].as_array()).map_or(self.expected_players,Vec::len),
            "configured_participant_count":self.expected_players,"active_participant_count":live.map(|live|live.peers().len()+1),
            "active_mobile_count":sim.entities().values_sorted().filter(|entity|entity.is_alive() && !entity.in_limbo() && matches!(entity.category(),crate::map::entities::EntityCategory::Infantry | crate::map::entities::EntityCategory::Unit)).count(),
            "required_frames":through.unwrap_or(self.required_frames()),"battle":self.battle.as_ref().map(|battle| battle.receipt(state)).transpose()?,
            "failure":self.failure.as_ref().map(|failure| failure.receipt()),"mass_input":self.mass.as_ref().map(mass::MassInput::receipt),
            "kick_observed":self.kick_observed,
            "production_broadcast":self.config.as_ref().map(|config|matches!(config.transport,LiveLanTransport::ProductionBroadcast)),
            "bind":self.config.as_ref().and_then(|config|config.transport.explicit_endpoints()).map(|(bind,_)|bind.to_string()),"peer":self.config.as_ref().and_then(|config|config.transport.explicit_endpoints()).map(|(_,peer)|peer.to_string()),"startup":self.startup,"map_admission":self.map_admission,
            "native_checksums":checksums,"simulation_hashes":hashes,
            "simulation_hash_interval":crate::sim::replay::PLAY_HASH_INTERVAL.get(),
            "actions":self.actions,"observed_movement":self.observed_movement,"stopped_position":self.stopped_position,"route":self.route,
            "final_native_frame":sim.session().binary_frame(),"checksum_failure":format!("{:?}",sim.pending_network_failure()),
            "sent_commands":live.map(|live| live.sent_commands()),"peers":live.map(|live| live.peers().iter().map(|peer| json!({"house":peer.house_id(),"frame":peer.frame(),"advertised_commands":peer.advertised_commands(),"received_commands":peer.received_commands()})).collect::<Vec<_>>()),
            "mobile_objects":sim.entities().values_sorted().filter(|entity| matches!(entity.category(), crate::map::entities::EntityCategory::Infantry | crate::map::entities::EntityCategory::Unit)).map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),"position":entity.position,"alive":entity.is_alive()})).collect::<Vec<_>>() }),
        )
    }
}

/// Reuse the rendered hitbox and ordinary pointer handlers for both network
/// entry routes; never inject a main-menu result from the capture driver.
fn click_main_menu(
    state: &mut AppState,
    control: crate::ui::main_menu_shell::MainMenuControlId,
) -> Result<()> {
    let layout = crate::app::frontend::shell_layout::main_menu_layout(state);
    let rect = layout
        .buttons
        .iter()
        .find(|button| button.id == control)
        .context("rendered multiplayer main-menu button")?
        .rect;
    set_window_pointer_to_rect_center(state, rect);
    App::handle_main_menu_shell_mouse_move(state);
    App::handle_main_menu_shell_mouse_down(state);
    App::handle_main_menu_shell_mouse_up(state);
    MoviesCapture::restore_neutral_pointer(state);
    Ok(())
}

/// Read committed observations from the one diagnostic replay owner. This
/// capture neither computes another hash nor admits command journals as state.
fn recorded_simulation_hashes(
    state: &AppState,
    through_tick: Option<u64>,
) -> Result<Vec<(u64, u64)>> {
    let replay = state
        .match_state
        .match_diagnostics
        .replay_log
        .as_ref()
        .context("running match diagnostic replay")?;
    let hashes: Vec<_> = replay
        .ticks
        .iter()
        .filter(|tick| through_tick.is_none_or(|limit| tick.tick <= limit))
        .filter_map(|tick| tick.state_hash.map(|hash| (tick.tick, hash)))
        .collect();
    ensure!(!hashes.is_empty(), "diagnostic replay has no state hashes");
    Ok(hashes)
}

fn terminal_simulation_receipt(state: &AppState) -> Result<Value> {
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("terminal simulation")?
        .simulation;
    ensure!(
        sim.terminal_score_snapshot().is_some(),
        "multiplayer result has no terminal score snapshot"
    );
    let hashes = recorded_simulation_hashes(state, None)?;
    let (tick, hash) = hashes.last().copied().context("terminal replay hash")?;
    ensure!(
        tick == sim.session().tick(),
        "result lacks the terminal frame's recorded simulation hash"
    );
    Ok(
        json!({"tick":tick,"native_frame":sim.session().binary_frame(),
        "hash":hash,"recorded_hashes":hashes,"score_finalized":true,
        "state_hash_owners":sim.state_hash_owners()}),
    )
}

fn local_owner(state: &AppState) -> Result<String> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .context("network setup")?;
    crate::sim::scenario_bootstrap::normalized_launch_slots(launch)
        .into_iter()
        .find(|slot| slot.source_human_index == Some(launch.local_player_index))
        .map(|slot| slot.owner_name)
        .context("local normalized House")
}

fn startup_receipt(state: &AppState) -> Result<Value> {
    let launch = state
        .match_state
        .session
        .network_setup()
        .context("network setup")?;
    let sim = &state
        .match_state
        .sim_runtime
        .as_ref()
        .context("simulation")?
        .simulation;
    crate::app::diagnostics::shell_capture::network_startup_receipt(state, launch, Some(sim))
}
