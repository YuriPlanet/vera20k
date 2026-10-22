//! Two independent App processes, using the ordinary LAN service and frame
//! boundary. Only user actions are automated. No peer result, loading report,
//! synchronized record, checksum or simulation step is injected here.

use super::*;
use crate::app::diagnostics::shell_capture::LiveLanConfig;
use crate::net::udp::UdpBindConfig;
use crate::ui::multiplayer_setup::{SetupAction, SetupColumn};

mod battle;
mod failure;

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
    config: LiveLanConfig,
    phase: Phase,
    route: Vec<Value>,
    /// Observations of the existing checksum ring, indexed by native frame.
    checksums: Vec<u32>,
    startup: Option<Value>,
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
    failure: Option<failure::FailureCapture>,
}

impl LiveLanCapture {
    pub(super) fn new(target: LanTarget, config: LiveLanConfig) -> Self {
        Self {
            target,
            config,
            phase: Phase::Main,
            route: Vec::new(),
            checksums: Vec::new(),
            startup: None,
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
            battle: matches!(target, LanTarget::BattleHost | LanTarget::BattleGuest)
                .then(battle::BattleOpening::default),
            failure: match target {
                LanTarget::LiveTimeoutHost => {
                    Some(failure::FailureCapture::new(failure::Mode::Timeout))
                }
                LanTarget::LiveQuitHost => Some(failure::FailureCapture::new(failure::Mode::Quit)),
                _ => None,
            },
        }
    }

    fn host(&self) -> bool {
        matches!(
            self.target,
            LanTarget::LiveHost
                | LanTarget::LiveTimeoutHost
                | LanTarget::LiveQuitHost
                | LanTarget::BattleHost
        )
    }

    fn required_frames(&self) -> u32 {
        if self.battle.is_some() {
            // Includes a barracks rally and paid GI walk, ordinary travel across
            // the map, object Attack/Stop and a stable separated tail. Every frame
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
        setup::apply_action(
            state,
            SetupAction::Select {
                control: SetupColumn::Country.control(usize::from(row)),
                value: i32::try_from(country)?,
            },
        );
        self.country_selected = true;
        Ok(())
    }

    fn event(&mut self, frame: u32, action: &str) {
        log::debug!("live LAN {:?} render {frame}: {action}", self.target);
        self.route
            .push(json!({"render_frame":frame,"action":action}));
    }

    pub(super) fn ready(&self) -> bool {
        self.phase == Phase::Finished
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
        ensure!(
            state.match_state.network_failure.is_none(),
            "live LAN entered a network failure modal"
        );
        if let Some(runtime) = &state.match_state.sim_runtime {
            ensure!(
                runtime.simulation.pending_network_failure().is_none(),
                "live LAN checksum failure: {:?}",
                runtime.simulation.pending_network_failure()
            );
            ensure!(
                runtime.simulation.pending_network_interruption().is_none(),
                "live LAN interrupted: {:?}",
                runtime.simulation.pending_network_interruption()
            );
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
                // The normal hidden-window diagnostics suppress physical LAN.
                // This explicit target installs the same service before Network
                // opens; it alone owns the sockets through shell/loading/play.
                LanCapture::isolated_root(state)?;
                ensure!(
                    state.match_state.lan_service.is_none(),
                    "LAN transport was already installed"
                );
                let alternate = std::net::SocketAddrV4::new(
                    *self.config.bind.ip(),
                    self.config.bind.port() + 2,
                );
                state.match_state.lan_service = Some(crate::app::lan_service::LanService::open_on(
                    UdpBindConfig {
                        primary: self.config.bind,
                        alternate,
                    },
                    &[self.config.peer],
                )?);
                App::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::MainMenu(
                        crate::ui::main_menu_shell::MainMenuShellAction::Network,
                    ),
                );
                self.event(frame, "real UDP service installed; MainMenu Network");
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
            Phase::HostSetup
                if rendered == PresentedShell::NetworkSetup && setup_settled(state, true) =>
            {
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
                        map.kind
                            == crate::map::skirmish_scenarios::SkirmishScenarioKind::ConcreteMap
                            && map.player_capacity >= 2
                            && crate::map::skirmish_scenarios::is_valid_map(map, mode, true)
                    })
                    .context("retail Battle map with two starts")?;
                setup::commit_map_selection(state, mode_id, map_index, None)?;
                self.select_country(state)?;
                // Use the existing action admission, including current control
                // availability. The isolated profile must allow one open peer.
                for row in 1..8usize {
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
                    let control = SetupColumn::Player.control(row);
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
                let launch = state
                    .match_state
                    .session
                    .network_setup()
                    .context("host setup")?;
                ensure!(
                    launch.opponents.is_empty(),
                    "live diagnostic requires a two-human match without AI"
                );
                self.event(frame, "host selected retail Battle map and open human rows");
                self.phase = Phase::HostReady;
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
                    setup::apply_action(
                        state,
                        SetupAction::Select {
                            control: SetupColumn::Player.control(usize::from(row)),
                            value: 2,
                        },
                    );
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
                    && setup::view(state).is_some_and(|view| view.accept_enabled)
                {
                    setup_button(state, crate::ui::multiplayer_setup::ACCEPT)?;
                    self.event(frame, "received real options/map status; Accept clicked");
                    self.phase = Phase::Loading;
                }
            }
            Phase::Loading if state.frontend.screen == GameScreen::InGame => {
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
            self.observe_run(state, frame)?;
        }
        Ok(())
    }

    fn observe_run(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
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
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("real live transport")?;
        ensure!(live.peers().len() == 1, "live diagnostic peer disappeared");
        let wait = format!("{:?}", live.wait_info());
        if self.last_wait.as_deref() != Some(&wait) {
            self.route
                .push(json!({"render_frame":frame,"native_frame":committed,"wait":wait}));
            self.last_wait = Some(wait);
        }
        if self.battle.is_some() {
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
                if live
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
            let owner = local_owner(state)?;
            let sim = &mut state
                .match_state
                .sim_runtime
                .as_mut()
                .context("simulation")?
                .simulation;
            ensure!(
                crate::app::input::commands::schedule_command_in_sim(
                    sim,
                    &owner,
                    crate::sim::command::Command::Stop { entity_id: actor }
                )
                .is_some(),
                "normal Stop producer rejected local input"
            );
            self.stopped = true;
            self.actions
                .push(json!({"native_frame":committed,"input":"Stop","actor":actor}));
        }
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("live transport")?;
        if self.checksums.len() >= REQUIRED_FRAMES as usize
            && self.stopped
            // The survivor must arm from its own committed prefix: the
            // finishing guest may exit at300 before sending frame300's
            // advertisement. The pair receipt compares both300-frame rings.
            && (self.failure.is_some()
                || live
                    .peers()
                    .iter()
                    .all(|peer| peer.frame() >= REQUIRED_FRAMES as i32))
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
                    "shared 300-frame prefix complete; host awaits real peer disappearance",
                );
            } else {
                self.phase = Phase::Finished;
            }
        }
        Ok(())
    }

    fn issue_move(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        let owner = local_owner(state)?;
        let (actor, from, to) = {
            let sim = &state
                .match_state
                .sim_runtime
                .as_ref()
                .context("simulation")?
                .simulation;
            let owner_id = sim.interner().get(&owner).context("local House")?;
            let actor = sim
                .entities()
                .values_sorted()
                .filter(|entity| {
                    entity.owner() == owner_id
                        && entity.is_alive()
                        && !entity.in_limbo()
                        && matches!(
                            entity.category(),
                            crate::map::entities::EntityCategory::Infantry
                                | crate::map::entities::EntityCategory::Unit
                        )
                        && entity.locomotor.is_some()
                })
                .min_by_key(|entity| {
                    (
                        entity.category() != crate::map::entities::EntityCategory::Infantry,
                        entity.stable_id(),
                    )
                })
                .context("local mobile object")?;
            let from = (actor.position.rx(), actor.position.ry());
            let rules = state.rules().context("loaded rules")?;
            let to = [(18, 0), (-18, 0), (0, 18), (0, -18)]
                .into_iter()
                .find_map(|(x, y)| {
                    let candidate = (i32::from(from.0) + x, i32::from(from.1) + y);
                    if candidate.0 < 0
                        || candidate.1 < 0
                        || candidate.0 > i32::from(u16::MAX)
                        || candidate.1 > i32::from(u16::MAX)
                    {
                        return None;
                    }
                    crate::app::input::commands::ordinary_cell_move_goal(
                        sim,
                        rules,
                        state.overlay_registry(),
                        owner_id,
                        actor.stable_id(),
                        (candidate.0 as u16, candidate.1 as u16),
                        false,
                    )
                    .filter(|cell| *cell != from)
                })
                .context("reachable local Move target")?;
            (actor.stable_id(), from, to)
        };
        let sim = &mut state
            .match_state
            .sim_runtime
            .as_mut()
            .context("simulation")?
            .simulation;
        ensure!(
            crate::app::input::commands::schedule_command_in_sim(
                sim,
                &owner,
                crate::sim::command::Command::Move {
                    entity_id: actor,
                    target_rx: to.0,
                    target_ry: to.1
                }
            )
            .is_some(),
            "normal Move producer rejected local input"
        );
        self.actor = Some(actor);
        self.origin = Some(from);
        self.actions.push(
            json!({"native_frame":frame,"input":"Move","actor":actor,"from":from,"target":to}),
        );
        Ok(())
    }

    pub(super) fn receipt(&self, state: &AppState) -> Result<Value> {
        ensure!(self.ready(), "live LAN capture has not finished");
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
            json!({"target":format!("{:?}",self.target),"service_boundary":"two independent App processes; real UDP/global/private queues; ordinary frame boundary",
            "scope":if self.failure.is_some() { "shared 300-frame prefix followed by real peer disappearance and ordinary wait recovery or Quit; bounded runtime validation, not whole-match/native parity" } else if self.battle.is_some() { "kick/rejoin, Battle opening, tactical barracks rally through raw ARCHIVE, two automatic paid GI deliveries and natural rally walks per Human, tactical GI Move/object Attack, post-command rifle damage and raw Stop, then separation; bounded runtime validation, not whole-match/native parity" } else { "kick/rejoin, 300 native frames and local Move/Stop; bounded runtime validation, not whole-match/native parity" },
            "required_frames":self.required_frames(),"battle":self.battle.as_ref().map(|battle| battle.receipt(state)).transpose()?,
            "failure":self.failure.as_ref().map(|failure| failure.receipt()),
            "kick_observed":self.kick_observed,
            "bind":self.config.bind.to_string(),"peer":self.config.peer.to_string(),"startup":self.startup,
            "native_checksums":self.checksums,"actions":self.actions,"observed_movement":self.observed_movement,"stopped_position":self.stopped_position,"route":self.route,
            "final_native_frame":sim.session().binary_frame(),"checksum_failure":format!("{:?}",sim.pending_network_failure()),
            "sent_commands":live.map(|live| live.sent_commands()),"peers":live.map(|live| live.peers().iter().map(|peer| json!({"house":peer.house_id(),"frame":peer.frame(),"advertised_commands":peer.advertised_commands(),"received_commands":peer.received_commands()})).collect::<Vec<_>>()),
            "mobile_objects":sim.entities().values_sorted().filter(|entity| matches!(entity.category(), crate::map::entities::EntityCategory::Infantry | crate::map::entities::EntityCategory::Unit)).map(|entity| json!({"id":entity.stable_id(),"owner":sim.interner().resolve(entity.owner()),"position":entity.position,"alive":entity.is_alive()})).collect::<Vec<_>>() }),
        )
    }
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
    Ok(
        json!({"seed":state.match_state.session.network_start_seed(),"map":launch.selected_map_file,
        "max_ahead":state.match_state.session.network_max_ahead_frames(),"local_source":launch.local_player_index,
        "sources":launch.humans.iter().enumerate().map(|(source,human)| json!({"source":source,"name":human.name,"raw_row":human.raw_slot_index,"slot":human.slot,"house":crate::app::match_runtime::network_match::source_house(&state.match_state.session,sim,source),"endpoint":state.match_state.session.network_endpoint(source).map(|address|address.to_string())})).collect::<Vec<_>>() }),
    )
}
