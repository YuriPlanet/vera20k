//! CnCNet/spawn.ini admission. This adapter never constructs gameplay state:
//! normalized requests enter the same campaign/skirmish owners as the shell,
//! and relay multiplayer enters the online lobby's start owners
//! (docs/spawn.md).

use anyhow::{Context, Result, ensure};

use crate::util::time::Instant;

use super::{App, AppState};
use crate::app::frontend::cncnet_spawn::{SpawnGame, SpawnRequest, SpawnTransport};
use crate::app::frontend::startup_options::RetailStartupOptions;
use crate::app::match_bootstrap::{MatchSeedClock, MatchSeedSource, OrdinaryMatchSeedClock};

/// How an external launch ended for the local player (docs/spawn.md). The
/// first decided value wins: a defeat followed by EXIT stays `Lost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalMatchResult {
    Won,
    Lost,
    /// Reserved by the contract; no current end-of-match owner decides it.
    #[allow(dead_code)]
    Draw,
    Disconnected,
    Quit,
}

impl ExternalMatchResult {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Won => "won",
            Self::Lost => "lost",
            Self::Draw => "draw",
            Self::Disconnected => "disconnected",
            Self::Quit => "quit",
        }
    }

    /// The desktop process exit status for this result (docs/spawn.md).
    /// `Quit` keeps the 0 every completed spawn used to return.
    pub(crate) const fn exit_code(self) -> i32 {
        match self {
            Self::Quit => 0,
            Self::Won => 10,
            Self::Lost => 11,
            Self::Draw => 12,
            Self::Disconnected => 13,
        }
    }
}

/// The exit status of a launch that failed, as before (anyhow's error exit).
pub(crate) const FAILED_LAUNCH_EXIT_CODE: i32 = 1;

/// The result file the desktop writes beside spawn.ini (docs/spawn.md).
pub(crate) const SPAWN_RESULT_FILE: &str = "spawn_result.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ExternalLaunchMode {
    #[default]
    Single,
    Relay,
    Direct,
}

/// What an external launch observed while its match ran; reset per launch.
#[derive(Debug, Default)]
pub(crate) struct ExternalLaunchProgress {
    pub(crate) mode: ExternalLaunchMode,
    pub(crate) match_id: Option<String>,
    pub(crate) source_index: Option<usize>,
    /// First and latest frame service with the match on screen.
    pub(crate) match_started: Option<Instant>,
    pub(crate) match_last: Option<Instant>,
    /// The last observed simulation frame (`binary_frame`).
    pub(crate) frames: u32,
    /// Every opposing playing participant had left or disconnected while
    /// the local player was still in the match (see [`opponents_left`]).
    pub(crate) opponents_left: bool,
    /// Scripted actions (docs/spawn.md "Scripted leave and resign") waiting
    /// to run, and which desktop test switches already fired.
    pub(crate) pending_actions: Vec<SpawnAction>,
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), allow(dead_code))]
    pub(crate) actions_fired: SpawnActionsFired,
}

/// A scripted in-match action of an external multiplayer launch
/// (docs/spawn.md "Scripted leave and resign"): the abort-box press a player
/// would make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnAction {
    /// Abort box Leave (with Observe offered, native DESTRUCT and EXIT); on
    /// the score screen, Continue.
    Leave,
    /// Abort box Observe: DESTRUCT the local House and keep watching.
    Resign,
}

/// Which desktop test switches already fired in this launch.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnActionsFired {
    leave: bool,
    resign: bool,
}

/// The desktop test switches `VERA20K_SPAWN_LEAVE_AFTER_SECONDS` and
/// `VERA20K_SPAWN_RESIGN_AFTER_SECONDS`: seconds after the match appears.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnActionTimers {
    leave: Option<std::time::Duration>,
    resign: Option<std::time::Duration>,
}

#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), allow(dead_code))]
impl SpawnActionTimers {
    /// Unset or unparsable values leave a switch off.
    pub(crate) fn from_values(leave: Option<&str>, resign: Option<&str>) -> Self {
        let seconds = |value: Option<&str>| {
            value
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(std::time::Duration::from_secs)
        };
        Self {
            leave: seconds(leave),
            resign: seconds(resign),
        }
    }

    /// The switches due at `elapsed` that have not fired yet, resign first.
    pub(crate) fn due(
        &self,
        elapsed: std::time::Duration,
        fired: &mut SpawnActionsFired,
    ) -> Vec<SpawnAction> {
        let mut due = Vec::new();
        if !fired.resign && self.resign.is_some_and(|after| elapsed >= after) {
            fired.resign = true;
            due.push(SpawnAction::Resign);
        }
        if !fired.leave && self.leave.is_some_and(|after| elapsed >= after) {
            fired.leave = true;
            due.push(SpawnAction::Leave);
        }
        due
    }
}

/// `spawn_result.json` / the `spawn:finished:` payload (docs/spawn.md).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct ExternalLaunchReport {
    /// `won`, `lost`, `draw`, `disconnected`, `quit` or `failed`.
    pub(crate) result: &'static str,
    pub(crate) exit_code: i32,
    pub(crate) mode: ExternalLaunchMode,
    /// Relay launches only.
    pub(crate) match_id: Option<String>,
    /// Multiplayer launches only: the local SourceIndex.
    pub(crate) source_index: Option<usize>,
    pub(crate) frames: u32,
    /// Wall time the match was on screen, in seconds (millisecond precision).
    pub(crate) duration_seconds: f64,
    pub(crate) opponents_left: bool,
    /// The failure message of a `failed` launch.
    pub(crate) reason: Option<String>,
}

impl ExternalLaunchReport {
    pub(crate) fn new(
        result: Option<ExternalMatchResult>,
        progress: Option<&ExternalLaunchProgress>,
        failure: Option<&str>,
    ) -> Self {
        let (result, exit_code) = match failure {
            Some(_) => ("failed", FAILED_LAUNCH_EXIT_CODE),
            None => {
                let result = result.unwrap_or(ExternalMatchResult::Quit);
                (result.as_str(), result.exit_code())
            }
        };
        let empty = ExternalLaunchProgress::default();
        let progress = progress.unwrap_or(&empty);
        let duration = progress
            .match_started
            .zip(progress.match_last)
            .map_or(0.0, |(start, last)| {
                (last.saturating_duration_since(start).as_millis() as f64) / 1000.0
            });
        Self {
            result,
            exit_code,
            mode: progress.mode,
            match_id: progress.match_id.clone(),
            source_index: progress.source_index,
            frames: progress.frames,
            duration_seconds: duration,
            opponents_left: progress.opponents_left,
            reason: failure.map(str::to_owned),
        }
    }

    pub(crate) fn json(&self) -> String {
        serde_json::to_string(self).expect("report serializes")
    }
}

/// docs/spawn.md "Opponent departure": true when the local player plays,
/// at least one opposing human exists, no opposing AI exists, and every
/// opposing human is no longer connected. Opposing means not on the local
/// player's team (no team opposes everyone). Like native LAN/online, a
/// departure decides no outcome by itself; this only reports it.
pub(crate) fn opponents_left(
    launch: &crate::skirmish_launch::SkirmishLaunchSession,
    connected: &[usize],
) -> bool {
    use crate::skirmish_launch::LaunchTeam;
    let local_source = launch.local_player_index;
    let Some(local) = launch.humans.get(local_source) else {
        return false;
    };
    // Only a frozen match lists sources, always including the local one.
    if local.is_observer() || !connected.contains(&local_source) {
        return false;
    }
    let opposing = |team: LaunchTeam| match (local.slot.team, team) {
        (LaunchTeam::Team(local), LaunchTeam::Team(other)) => local != other,
        _ => true,
    };
    if launch.opponents.iter().any(|ai| opposing(ai.team)) {
        return false;
    }
    let mut any = false;
    for (source, human) in launch.humans.iter().enumerate() {
        if source == local_source || human.is_observer() || !opposing(human.slot.team) {
            continue;
        }
        if connected.contains(&source) {
            return false;
        }
        any = true;
    }
    any
}

/// Replace `path` atomically: a launcher never reads a partial result.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn write_spawn_result(
    path: &std::path::Path,
    report: &ExternalLaunchReport,
) -> Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = std::path::PathBuf::from(temporary);
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    std::fs::write(&temporary, bytes).with_context(|| format!("write {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

/// The browser page's launch state machine (`web.rs`; docs/spawn.md,
/// "Repeat launches"). `start_game` keeps its one-shot contract; an engine
/// started by `start_spawn` accepts another launch, and new launch files,
/// only after the previous launch reported `spawn:finished`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")),
    allow(dead_code)
)]
pub(crate) struct BrowserSpawnGate {
    started: bool,
    spawn_host: bool,
    idle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserSpawnStart {
    /// Create the engine for this launch.
    StartEngine,
    /// Queue the launch for the running, idle engine.
    QueueNext,
}

#[cfg_attr(
    not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")),
    allow(dead_code)
)]
impl BrowserSpawnGate {
    pub(crate) const fn started(self) -> bool {
        self.started
    }

    /// A `start_spawn` launch is running (not yet `spawn:finished`).
    pub(crate) const fn spawn_running(self) -> bool {
        self.spawn_host && !self.idle
    }

    /// Retail files and saves are fixed once the engine runs.
    pub(crate) const fn may_replace_resources(self) -> bool {
        !self.started
    }

    /// Launch files may change before the first launch and between launches.
    pub(crate) const fn may_replace_launch_files(self) -> bool {
        !self.started || (self.spawn_host && self.idle)
    }

    pub(crate) fn start_game(&mut self) -> Result<(), &'static str> {
        if self.started {
            return Err("Game already started; reload to restart");
        }
        self.started = true;
        Ok(())
    }

    /// What `start_spawn` may do now; nothing changes until [`Self::commit`].
    pub(crate) fn start_spawn(self) -> Result<BrowserSpawnStart, &'static str> {
        match (self.started, self.spawn_host, self.idle) {
            (false, ..) => Ok(BrowserSpawnStart::StartEngine),
            (true, true, true) => Ok(BrowserSpawnStart::QueueNext),
            (true, true, false) => Err("A launch is running; wait for spawn:finished"),
            (true, false, _) => Err("Game already started; reload to restart"),
        }
    }

    /// `start_spawn` succeeded with the admitted [`BrowserSpawnStart`].
    pub(crate) fn commit(&mut self, _start: BrowserSpawnStart) {
        self.started = true;
        self.spawn_host = true;
        self.idle = false;
    }

    /// The launch reported `spawn:finished`.
    pub(crate) fn finished(&mut self) {
        if self.spawn_host {
            self.idle = true;
        }
    }
}

struct SpawnSeedClock {
    seed: Option<u32>,
}

impl MatchSeedClock for SpawnSeedClock {
    fn low_u32(&mut self) -> u32 {
        self.seed
            .unwrap_or_else(|| OrdinaryMatchSeedClock.low_u32())
    }

    fn source(&self) -> MatchSeedSource {
        if self.seed.is_some() {
            MatchSeedSource::Controlled
        } else {
            OrdinaryMatchSeedClock.source()
        }
    }

    fn seed_authority_certifying(&self) -> bool {
        self.seed.is_none() && OrdinaryMatchSeedClock.seed_authority_certifying()
    }
}

impl App {
    /// CnCNet runs the executable with its game directory as cwd and writes
    /// spawn.ini immediately before launch. Read once, before window creation.
    pub fn new_spawn(options: RetailStartupOptions) -> Result<Self> {
        let result_path = std::path::absolute(SPAWN_RESULT_FILE)?;
        let app = (|| {
            ensure!(
                std::env::var_os("RA2_QUICKPLAY").is_none(),
                "-SPAWN cannot be combined with RA2_QUICKPLAY"
            );
            Self::new_spawn_at(std::path::Path::new("spawn.ini"), options)
        })();
        match app {
            Ok(mut app) => {
                app.spawn_result_path = Some(result_path);
                Ok(app)
            }
            Err(error) => {
                // A rejected spawn.ini is a failed launch too.
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                if let Err(write) = write_spawn_result(
                    &result_path,
                    &ExternalLaunchReport::new(None, None, Some(&format!("{error:#}"))),
                ) {
                    log::error!("Could not write {}: {write:#}", result_path.display());
                }
                Err(error)
            }
        }
    }

    /// Admit a launch file whose client map lies beside it. The browser host
    /// passes its in-memory spawn directory here.
    pub(crate) fn new_spawn_at(
        path: &std::path::Path,
        options: RetailStartupOptions,
    ) -> Result<Self> {
        let request = SpawnRequest::read(path)?;
        let mut app = Self::new(options);
        app.spawn_request = Some(request);
        Ok(app)
    }

    /// The browser host reads its launch file itself (`web::start_spawn`).
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub(crate) fn set_spawn_request(&mut self, request: SpawnRequest) {
        self.spawn_request = Some(request);
    }

    pub(super) fn start_spawn_session(state: &mut AppState, request: SpawnRequest) -> Result<()> {
        ensure!(
            state.frontend.main_menu_shell_error.is_none(),
            "retail assets are unavailable: {}",
            state
                .frontend
                .main_menu_shell_error
                .as_deref()
                .unwrap_or_default()
        );
        if let SpawnGame::Network(network) = &request.game {
            match &network.transport {
                // Browsers cannot open the UDP sockets direct play needs.
                #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                SpawnTransport::Direct(_) => anyhow::bail!(
                    "direct multiplayer spawn launches need the desktop client; use a [Relay] launch in the browser"
                ),
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                SpawnTransport::Direct(_) => {}
                // The single-player browser package has no relay transport.
                #[cfg(all(
                    feature = "wasm",
                    not(feature = "wasm-net"),
                    target_arch = "wasm32",
                    target_os = "unknown"
                ))]
                SpawnTransport::Relay(_) => anyhow::bail!(
                    "relay multiplayer spawn launches need the browser package built with --network"
                ),
                #[cfg(not(all(
                    feature = "wasm",
                    not(feature = "wasm-net"),
                    target_arch = "wasm32",
                    target_os = "unknown"
                )))]
                SpawnTransport::Relay(_) => {}
            }
        }
        Self::check_spawn_expectations(state, &request)?;
        state.frontend.external_launch = true;
        state.frontend.external_progress = ExternalLaunchProgress::default();
        if let SpawnGame::Network(network) = &request.game {
            let progress = &mut state.frontend.external_progress;
            progress.source_index = Some(network.session.local_player_index);
            match &network.transport {
                SpawnTransport::Relay(relay) => {
                    progress.mode = ExternalLaunchMode::Relay;
                    progress.match_id = Some(relay.match_id.clone());
                }
                SpawnTransport::Direct(_) => progress.mode = ExternalLaunchMode::Direct,
            }
        }
        state.persistence.options_profile.game_speed = request.game_speed;
        Self::destroy_current_shell_dialog(state);
        state.frontend.shell_first_paint_slide = None;
        let mut clock = SpawnSeedClock { seed: request.seed };
        match request.game {
            SpawnGame::Skirmish(session) => {
                #[cfg(feature = "ares")]
                crate::app::frontend::skirmish_session::prepare_external_skirmish_countries(
                    &mut state.process_assets,
                    &session,
                );
                log::info!(
                    "CnCNet offline skirmish: scenario={:?}, players={}, seed={:?}",
                    session.selected_map_file,
                    1 + session.opponents.len(),
                    request.seed
                );
                Self::start_skirmish_session(state, *session, &mut clock)?;
            }
            SpawnGame::Campaign(campaign) => {
                state.process_assets.refresh_empty_campaigns_if_available();
                let catalog = state
                    .process_assets
                    .campaigns()
                    .context("retail campaign catalog unavailable")?;
                let side = campaign.side;
                let campaign_id = campaign.campaign_id;
                let startup = campaign.prepare_startup(catalog, request.game_speed, &mut clock)?;
                state.persistence.options_profile.difficulty = startup.difficulty();
                state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
                log::info!(
                    "CnCNet campaign: scenario={}, side={}, difficulty={}, client_campaign_id={}",
                    startup.scenario(),
                    side,
                    startup.difficulty(),
                    campaign_id
                );
                Self::start_campaign_scenario(
                    state,
                    crate::app::loading::pump::LoadingRequest::campaign(startup),
                );
            }
            SpawnGame::Network(network) => {
                let seed = request.seed.context("multiplayer launch has no Seed")?;
                let relay = match network.transport {
                    SpawnTransport::Relay(relay) => relay,
                    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                    SpawnTransport::Direct(direct) => {
                        log::info!(
                            "Direct spawn: source={}, humans={}, ai={}, port={}, speed={}",
                            network.session.local_player_index,
                            network.session.humans.len(),
                            network.session.opponents.len(),
                            direct.local_port,
                            request.game_speed
                        );
                        return crate::app::lan_service::begin_direct_spawn(
                            state,
                            network.session,
                            direct,
                            seed,
                        );
                    }
                    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                    SpawnTransport::Direct(_) => {
                        unreachable!("browser direct launches were rejected above")
                    }
                };
                log::info!(
                    "Relay spawn: match={}, source={}, humans={}, ai={}, speed={}",
                    relay.match_id,
                    network.session.local_player_index,
                    network.session.humans.len(),
                    network.session.opponents.len(),
                    request.game_speed
                );
                crate::app::shell_wol::begin_spawned_match(state, network.session, relay, seed)?;
            }
        }
        Ok(())
    }

    /// `[Settings] Expected*` (docs/spawn.md): reject the launch before it
    /// starts when this build, its rules or the client map differ from what
    /// the launcher probed.
    fn check_spawn_expectations(state: &AppState, request: &SpawnRequest) -> Result<()> {
        let map = match &request.game {
            SpawnGame::Skirmish(session) => session.selected_map_file.as_deref(),
            SpawnGame::Network(network) => network.session.selected_map_file.as_deref(),
            SpawnGame::Campaign(_) => None,
        };
        request.expected.mismatch(
            &crate::app::shell_network_setup::agreement::version(),
            || {
                state
                    .process_assets
                    .multiplayer_rules_digest(
                        crate::app::frontend::cncnet_spawn::BATTLE_MODE_OVERRIDE_FILE,
                    )
                    .map_err(anyhow::Error::msg)
            },
            || {
                let map = map.context("launch has no map")?;
                let root = &state
                    .platform
                    .game_config
                    .as_ref()
                    .context("no asset directory")?
                    .paths
                    .ra2_dir;
                let assets = state
                    .process_assets
                    .manager()
                    .context("retail assets are unavailable")?;
                Ok(
                    crate::app::spawn_probe::spawn_map_identity(root, map, assets)?
                        .map
                        .source_sha256,
                )
            },
        )
    }

    /// Sample the running match for the launch report (docs/spawn.md):
    /// frames, time on screen and opponent departure. External launches only.
    pub(super) fn poll_external_launch_progress(state: &mut AppState) {
        if !state.frontend.external_launch
            || state.frontend.screen != crate::ui::game_screen::GameScreen::InGame
        {
            return;
        }
        let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
            return;
        };
        let frame = runtime.simulation.session().binary_frame();
        let left = state
            .match_state
            .session
            .network_setup()
            .is_some_and(|launch| {
                opponents_left(
                    launch,
                    state.match_state.session.network_connected_source_indices(),
                )
            });
        let progress = &mut state.frontend.external_progress;
        let now = Instant::now();
        if progress.match_started.is_none() {
            log::info!("External launch: match on screen at frame {frame}");
            progress.match_started = Some(now);
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            crate::web::status("spawn:match_started");
        }
        progress.match_last = Some(now);
        progress.frames = progress.frames.max(frame);
        if left && !progress.opponents_left {
            log::info!("External launch: every opposing participant has left the match");
            progress.opponents_left = true;
        }
    }

    /// This launch's report from its final state.
    pub(crate) fn external_launch_report(
        state: Option<&AppState>,
        failure: Option<&str>,
    ) -> ExternalLaunchReport {
        let failure = failure
            .or_else(|| state.and_then(|state| state.frontend.external_launch_error.as_deref()));
        ExternalLaunchReport::new(
            state.and_then(Self::external_match_result),
            state.map(|state| &state.frontend.external_progress),
            failure,
        )
    }

    /// Desktop: write `spawn_result.json` beside spawn.ini and return the
    /// process exit status; `None` for a launch that is not `-SPAWN`.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    pub fn finish_spawn_launch(&self) -> Option<i32> {
        let path = self.spawn_result_path.as_deref()?;
        let report =
            Self::external_launch_report(self.state.as_ref(), self.startup_failure.as_deref());
        log::info!("External launch result: {}", report.json());
        if let Err(error) = write_spawn_result(path, &report) {
            log::error!("Could not write {}: {error:#}", path.display());
        }
        Some(report.exit_code)
    }

    /// Browser: the external launch ended (its quit cascade finished, or the
    /// start failed). Report it through `spawn:finished:<json>`, release
    /// its network owners as a return after a match does, and wait on the
    /// menu backdrop for the page's next `start_spawn` (docs/spawn.md).
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub(super) fn finish_browser_external_launch(state: &mut AppState, failure: Option<&str>) {
        let report = Self::external_launch_report(Some(state), failure);
        crate::app::shell_wol::discard_spawned_visit(state);
        if state.match_state.lan_service.is_some() {
            crate::app::lan_service::close(state);
        }
        if state.match_state.session.network_return_context().is_some() {
            state.match_state.session.leave_network();
        }
        state.frontend.network_setup_protocol = None;
        Self::destroy_current_shell_dialog(state);
        state.frontend.external_idle = true;
        log::info!("External launch result: {}", report.json());
        crate::web::spawn_finished(&report.json());
    }

    /// Browser: start the launch the page queued with `start_spawn` once
    /// the previous one has fully released its network owners.
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub(super) fn poll_queued_spawn(state: &mut AppState) {
        if !state.frontend.external_idle
            || crate::app::shell_wol::online_cleanup_pending(state)
            || state.match_state.lan_service.is_some()
        {
            return;
        }
        let Some(request) = crate::web::take_queued_spawn() else {
            return;
        };
        state.frontend.external_idle = false;
        state.frontend.external_launch_error = None;
        state.frontend.external_match_result = None;
        state.frontend.quit_cascade = None;
        log::info!("External launch: starting the next queued spawn");
        if let Err(error) = Self::start_spawn_session(state, request) {
            let error = format!("CnCNet startup failed: {error:#}");
            log::error!("{error}");
            crate::web::status(&error);
            Self::finish_browser_external_launch(state, Some(&error));
        }
        state.platform.window.request_redraw();
    }

    /// docs/spawn.md "Scripted leave and resign": run the external
    /// multiplayer launch's requested actions as the abort box presses a
    /// player makes. Requests come from the desktop test switches
    /// `VERA20K_SPAWN_LEAVE_AFTER_SECONDS` / `VERA20K_SPAWN_RESIGN_AFTER_SECONDS`
    /// (read only here, i.e. only by `-SPAWN` multiplayer launches) or the
    /// browser exports `spawn_leave()` / `spawn_resign()`. Ordinary play,
    /// single-player spawns and the built-in lobbies never reach this.
    pub(super) fn poll_spawn_actions(state: &mut AppState) {
        if !state.frontend.external_launch
            || state.frontend.external_progress.mode == ExternalLaunchMode::Single
        {
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            if state.frontend.external_launch && crate::web::take_spawn_action().is_some() {
                log::warn!("Spawn action ignored: not a multiplayer launch");
            }
            return;
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            use std::sync::OnceLock;
            static TIMERS: OnceLock<SpawnActionTimers> = OnceLock::new();
            let timers = TIMERS.get_or_init(|| {
                SpawnActionTimers::from_values(
                    std::env::var("VERA20K_SPAWN_LEAVE_AFTER_SECONDS")
                        .ok()
                        .as_deref(),
                    std::env::var("VERA20K_SPAWN_RESIGN_AFTER_SECONDS")
                        .ok()
                        .as_deref(),
                )
            });
            let progress = &mut state.frontend.external_progress;
            if let Some(started) = progress.match_started {
                let elapsed = started.elapsed();
                for action in timers.due(elapsed, &mut progress.actions_fired) {
                    log::info!(
                        "Spawn test switch: requesting {action:?} after {elapsed:?} in the match"
                    );
                    progress.pending_actions.push(action);
                }
            }
        }
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        while let Some(action) = crate::web::take_spawn_action() {
            log::info!("Spawn action requested by the page: {action:?}");
            state
                .frontend
                .external_progress
                .pending_actions
                .push(action);
        }
        let Some(&action) = state.frontend.external_progress.pending_actions.first() else {
            return;
        };
        if Self::apply_spawn_action(state, action) {
            state.frontend.external_progress.pending_actions.remove(0);
        }
    }

    /// One scripted action; false while it must wait (loading, an outcome
    /// or exit already running, a network failure box).
    fn apply_spawn_action(state: &mut AppState, action: SpawnAction) -> bool {
        use crate::ui::game_screen::GameScreen;
        use crate::ui::shell::abort::{AbortButton, AbortSecondary};
        match &state.frontend.screen {
            GameScreen::InGame => {
                if state.match_state.sim_runtime.is_none()
                    || state.match_state.scenario_exit.is_some()
                    || state.match_state.scenario_outcome.is_some()
                    || Self::house_outcome_wait_pending(state)
                    || crate::app::match_runtime::network_failure::pending(state)
                {
                    return false;
                }
                // The player's route: open the abort box (its init picks
                // Observe/Leave from the live match), then press a button.
                Self::enter_in_game_menu_state(
                    state,
                    crate::ui::pause_menu::InGameMenuState::AbortConfirm,
                );
                let secondary = state.match_state.match_presentation.abort_secondary;
                match action {
                    SpawnAction::Leave => {
                        log::info!(
                            "Spawn action: pressing Leave (abort box secondary {secondary:?})"
                        );
                        crate::app::input::abort::activate(state, AbortButton::Leave);
                    }
                    SpawnAction::Resign if secondary == AbortSecondary::Observe => {
                        log::info!("Spawn action: resigning through the abort box's Observe");
                        crate::app::input::abort::activate(state, AbortButton::Secondary);
                    }
                    SpawnAction::Resign => {
                        log::warn!(
                            "Spawn action: resign unavailable (abort box secondary {secondary:?}); nothing pressed"
                        );
                        Self::enter_in_game_menu_state(
                            state,
                            crate::ui::pause_menu::InGameMenuState::Closed,
                        );
                    }
                }
                true
            }
            GameScreen::MissionResult { .. } => {
                if action == SpawnAction::Leave {
                    log::info!("Spawn action: pressing Continue on the score screen");
                    if Self::score_shell_active(state) {
                        Self::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::ScoreContinue,
                        );
                    } else {
                        Self::leave_mission_result_screen(state);
                    }
                } else {
                    log::warn!("Spawn action: the match already ended; resign ignored");
                }
                true
            }
            // Joining (the menu backdrop) or loading: wait for the match.
            // After it, the launch already returned and reports its result.
            GameScreen::Loading => false,
            GameScreen::MainMenu => state.frontend.external_progress.match_started.is_some(),
        }
    }

    /// Spawn-only test switch (tools/spawn/README.md): with
    /// `VERA20K_SPAWN_HIDDEN_WINDOW=1`, a `-SPAWN` launch creates its window
    /// invisible and keeps servicing it, so automated runs show nothing.
    /// Read only for `-SPAWN` launches; ordinary play never consults it.
    pub fn spawn_hidden_window_requested() -> bool {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            std::env::var_os("VERA20K_SPAWN_HIDDEN_WINDOW").is_some_and(|value| value == "1")
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            false
        }
    }

    /// This `-SPAWN` launch's window is the opt-in invisible test window.
    pub(super) fn invisible_spawn_window(state: &AppState) -> bool {
        state.frontend.external_launch
            && Self::spawn_hidden_window_requested()
            && state.platform.window.is_visible() == Some(false)
    }

    /// The local result of the external launch, once its match ended; the
    /// relay `finish` reports it, and launch hosts read it here.
    pub(crate) fn external_match_result(state: &AppState) -> Option<ExternalMatchResult> {
        state.frontend.external_match_result
    }

    /// Record the end-of-match owner's decision for an external launch only.
    pub(crate) fn record_external_match_result(state: &mut AppState, result: ExternalMatchResult) {
        if state.frontend.external_launch {
            state.frontend.external_match_result.get_or_insert(result);
        }
    }

    /// An external launch that cannot reach or finish its match ends the
    /// process with this error, as a failed spawn startup does.
    pub(crate) fn fail_external_launch(state: &mut AppState, error: &str) {
        log::error!("External launch failed: {error}");
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        crate::web::status(&format!("CnCNet launch failed: {error}"));
        state
            .frontend
            .external_launch_error
            .get_or_insert_with(|| format!("CnCNet launch failed: {error}"));
        Self::resume_shell_after_match(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::frontend::cncnet_spawn::{SpawnGame, SpawnRequest};

    #[test]
    fn spawn_action_switches_are_off_by_default_and_fire_once() {
        use std::time::Duration;
        let off = SpawnActionTimers::from_values(None, None);
        let mut fired = SpawnActionsFired::default();
        assert_eq!(off, SpawnActionTimers::default());
        assert!(off.due(Duration::from_secs(10_000), &mut fired).is_empty());
        assert_eq!(
            SpawnActionTimers::from_values(Some("x"), Some("-1")),
            SpawnActionTimers::default()
        );
        let timers = SpawnActionTimers::from_values(Some(" 90 "), Some("30"));
        assert!(timers.due(Duration::from_secs(29), &mut fired).is_empty());
        assert_eq!(
            timers.due(Duration::from_secs(30), &mut fired),
            [SpawnAction::Resign]
        );
        assert!(timers.due(Duration::from_secs(31), &mut fired).is_empty());
        assert_eq!(
            timers.due(Duration::from_secs(95), &mut fired),
            [SpawnAction::Leave]
        );
        assert!(timers.due(Duration::from_secs(200), &mut fired).is_empty());
        let mut fresh = SpawnActionsFired::default();
        assert_eq!(
            timers.due(Duration::from_secs(100), &mut fresh),
            [SpawnAction::Resign, SpawnAction::Leave]
        );
    }

    #[test]
    fn exit_codes_are_distinct_and_keep_zero_for_quit() {
        use ExternalMatchResult::*;
        let codes: Vec<_> = [Quit, Won, Lost, Draw, Disconnected]
            .into_iter()
            .map(ExternalMatchResult::exit_code)
            .collect();
        assert_eq!(codes, [0, 10, 11, 12, 13]);
        assert!(!codes.contains(&FAILED_LAUNCH_EXIT_CODE));
    }

    #[test]
    fn report_takes_the_decided_result_or_quit_and_failure_wins() {
        let progress = ExternalLaunchProgress {
            mode: ExternalLaunchMode::Relay,
            match_id: Some("m".into()),
            source_index: Some(1),
            match_started: Some(Instant::now()),
            match_last: None,
            frames: 781,
            opponents_left: true,
        };
        let won = ExternalLaunchReport::new(Some(ExternalMatchResult::Won), Some(&progress), None);
        assert_eq!((won.result, won.exit_code), ("won", 10));
        let value: serde_json::Value = serde_json::from_str(&won.json()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"result":"won","exit_code":10,"mode":"relay","match_id":"m",
                "source_index":1,"frames":781,"duration_seconds":0.0,"opponents_left":true,
                "reason":null})
        );
        let quit = ExternalLaunchReport::new(None, None, None);
        assert_eq!(
            (quit.result, quit.exit_code, quit.mode),
            ("quit", 0, ExternalLaunchMode::Single)
        );
        let failed = ExternalLaunchReport::new(
            Some(ExternalMatchResult::Lost),
            Some(&progress),
            Some("boom"),
        );
        assert_eq!((failed.result, failed.exit_code), ("failed", 1));
        assert_eq!(failed.reason.as_deref(), Some("boom"));
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn result_file_is_replaced_whole() {
        let dir = crate::map::source::test_support::TestDirectory::new("spawn-result");
        let path = dir.path().join(SPAWN_RESULT_FILE);
        std::fs::write(&path, "old").unwrap();
        let report = ExternalLaunchReport::new(Some(ExternalMatchResult::Lost), None, None);
        write_spawn_result(&path, &report).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["result"], "lost");
        assert_eq!(written["exit_code"], 11);
        // No temporary file remains beside it.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(SPAWN_RESULT_FILE)]);
    }

    fn network(text: &str) -> crate::skirmish_launch::SkirmishLaunchSession {
        let SpawnGame::Network(network) = SpawnRequest::parse(text).unwrap().game else {
            panic!("network launch");
        };
        network.session
    }

    const TWO: &str = "[Settings]\nName=P0\nSide=0\nColor=0\nPlayerCount=2\nSourceIndex=0\n\
        Seed=7\nSpecialHouseIsAlly=No\n[Other1]\nName=P1\nSide=1\nColor=1\nSourceIndex=1\n\
        Ip=10.0.0.2\n";

    #[test]
    fn opponents_left_needs_every_opposing_human_gone_and_no_opposing_ai() {
        let launch = network(TWO);
        assert!(!opponents_left(&launch, &[0, 1]));
        assert!(opponents_left(&launch, &[0]));
        // Not a frozen match (no sources listed): nothing is decided.
        assert!(!opponents_left(&launch, &[]));

        // An opposing AI still plays.
        let launch = network(&format!("{TWO}[HouseHandicaps]\nMulti3=0\n[HouseCountries]\nMulti3=2\n[HouseColors]\nMulti3=3\n").replace("Seed=7", "Seed=7\nAIPlayers=1"));
        assert!(!opponents_left(&launch, &[0]));

        // Three humans in two teams: the ally's departure is not enough.
        let three = "[Settings]\nName=P0\nSide=0\nColor=0\nPlayerCount=3\nSourceIndex=0\n\
            Seed=7\nSpecialHouseIsAlly=No\n[Other1]\nName=P1\nSide=1\nColor=1\nSourceIndex=1\n\
            Ip=10.0.0.2\n[Other2]\nName=P2\nSide=2\nColor=2\nSourceIndex=2\nIp=10.0.0.3\n\
            [HouseTeams]\nMulti1=0\nMulti2=0\nMulti3=1\n";
        let launch = network(three);
        assert!(!opponents_left(&launch, &[0, 2]));
        assert!(opponents_left(&launch, &[0, 1]));
        // Only the ally remains with the local player: still opponents gone.
        assert!(opponents_left(&launch, &[0]));
    }

    #[test]
    fn browser_gate_admits_repeat_spawns_only_after_finished() {
        let mut gate = BrowserSpawnGate::default();
        assert!(gate.may_replace_launch_files() && gate.may_replace_resources());
        let start = gate.start_spawn().unwrap();
        assert_eq!(start, BrowserSpawnStart::StartEngine);
        gate.commit(start);
        assert!(gate.started() && !gate.may_replace_resources());
        assert!(!gate.may_replace_launch_files());
        assert!(gate.start_spawn().is_err());
        assert!(gate.start_game().is_err());
        gate.finished();
        assert!(gate.may_replace_launch_files() && !gate.may_replace_resources());
        assert!(gate.start_game().is_err());
        let next = gate.start_spawn().unwrap();
        assert_eq!(next, BrowserSpawnStart::QueueNext);
        gate.commit(next);
        assert!(!gate.may_replace_launch_files());
        assert!(gate.start_spawn().is_err());

        // start_game keeps its one-shot contract and never becomes a host.
        let mut game = BrowserSpawnGate::default();
        game.start_game().unwrap();
        game.finished();
        assert!(game.start_spawn().is_err());
        assert!(!game.may_replace_launch_files());
        assert!(game.start_game().is_err());
    }
}
