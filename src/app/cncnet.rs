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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "docs/spawn.md reserves exit code 12 for a draw")
    )]
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
    /// A watcher's launch (docs/spawn.md "Watching"): no source and no
    /// result of its own.
    pub(crate) watch: bool,
    /// First and latest frame service with the match on screen.
    pub(crate) match_started: Option<Instant>,
    pub(crate) match_last: Option<Instant>,
    /// The last observed simulation frame (`binary_frame`).
    pub(crate) frames: u32,
    /// The spawn test switch `VERA20K_SPAWN_FRAME_COST`'s record
    /// (`spawn_frame_cost`), kept from the match's first frame on screen.
    pub(crate) frame_cost: Option<crate::app::spawn_frame_cost::FrameCost>,
    /// Every opposing playing participant had left or disconnected while
    /// the local player was still in the match (see [`opponents_left`]).
    pub(crate) opponents_left: bool,
    /// Scripted actions (docs/spawn.md "Scripted leave and resign") waiting
    /// to run, and which desktop test switches already fired.
    pub(crate) pending_actions: Vec<SpawnAction>,
    #[cfg_attr(
        all(target_arch = "wasm32", target_os = "unknown"),
        expect(dead_code, reason = "the browser build runs no scripted actions")
    )]
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
    /// `VERA20K_SPAWN_LEAVE_AT_FRAME`'s Leave in the match, and its Continue
    /// on the score page after it.
    leave_at_frame: bool,
    leave_score: bool,
}

/// The desktop test switch `VERA20K_SPAWN_LEAVE_AT_FRAME=<frame>`
/// (tools/spawn/README.md): press Leave once the match has reached that
/// simulation frame and Continue on the score page that follows, so a
/// launch of any kind, one human against computers too, ends by itself after
/// a set stretch of the match. `frames` is the launch's last observed
/// simulation frame; `on_score` whether the score page is up.
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    expect(dead_code, reason = "the browser build runs no test switch")
)]
fn leave_at_frame_due(
    leave_at: u32,
    frames: u32,
    on_score: bool,
    fired: &mut SpawnActionsFired,
) -> Option<SpawnAction> {
    if !fired.leave_at_frame {
        fired.leave_at_frame = frames >= leave_at;
        return fired.leave_at_frame.then_some(SpawnAction::Leave);
    }
    if on_score && !fired.leave_score {
        fired.leave_score = true;
        return Some(SpawnAction::Leave);
    }
    None
}

/// The desktop test switches `VERA20K_SPAWN_LEAVE_AFTER_SECONDS` and
/// `VERA20K_SPAWN_RESIGN_AFTER_SECONDS`: seconds after the match appears.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnActionTimers {
    leave: Option<std::time::Duration>,
    resign: Option<std::time::Duration>,
}

#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    expect(dead_code, reason = "the browser build runs no scripted actions")
)]
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
    /// Multiplayer launches only: the local SourceIndex; none for a watcher.
    pub(crate) source_index: Option<usize>,
    /// `true` for a watcher's launch (docs/spawn.md "Watching"), whose
    /// result is always `quit`; absent otherwise.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) watch: bool,
    pub(crate) frames: u32,
    /// Wall time the match was on screen, in seconds (millisecond precision).
    pub(crate) duration_seconds: f64,
    pub(crate) opponents_left: bool,
    /// The failure message of a `failed` launch.
    pub(crate) reason: Option<String>,
    /// Relay matches only: the match's network quality record.
    pub(crate) network_quality:
        Option<crate::app::match_runtime::network_quality::NetworkQualityReport>,
    /// With the spawn test switch `VERA20K_SPAWN_FRAME_COST`: what the match
    /// cost the main thread (tools/spawn/README.md); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_cost: Option<crate::app::spawn_frame_cost::FrameCostReport>,
    /// `rules_mismatch` when spawn.ini's `ExpectedRulesDigest` refused the
    /// launch; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) failure: Option<&'static str>,
    /// With that failure: this game's rules parts for the launched mode
    /// (docs/spawn.md, "Rules parts").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rules_parts: Option<crate::app::rules_parts::RulesParts>,
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
            watch: progress.watch,
            frames: progress.frames,
            duration_seconds: duration,
            opponents_left: progress.opponents_left,
            reason: failure.map(str::to_owned),
            network_quality: None,
            frame_cost: progress.frame_cost.as_ref().and_then(|cost| cost.report()),
            failure: None,
            rules_parts: None,
        }
    }

    /// The launch failed because its rules differ from the expected ones.
    pub(crate) fn rules_mismatch(&mut self, parts: crate::app::rules_parts::RulesParts) {
        self.failure = Some("rules_mismatch");
        self.rules_parts = Some(parts);
    }

    pub(crate) fn json(&self) -> String {
        serde_json::to_string(self).expect("report serializes")
    }
}

/// The players of a decided network match whose Houses won
/// ([`crate::sim::world::Simulation::network_match_winners`]) and who were
/// still connected, by their launch names; `None` while the match runs.
fn match_winners(
    session: &crate::app::match_runtime::session::MatchSession,
    sim: &crate::sim::world::Simulation,
) -> Option<Vec<String>> {
    let winners = sim.network_match_winners()?;
    let launch = session.network_setup()?;
    let connected = session.network_connected_source_indices();
    Some(
        launch
            .humans
            .iter()
            .enumerate()
            .filter(|&(source, human)| {
                !human.is_observer()
                    && connected.contains(&source)
                    && session
                        .network_source_owner(source)
                        .and_then(|owner| sim.interner().get(&owner))
                        .is_some_and(|owner| winners.contains(&owner))
            })
            .map(|(_, human)| human.name.clone())
            .collect(),
    )
}

/// A player's `result` as the running match decides it, and the players who
/// won it once it is decided. A player who leaves or drops after the game
/// decided their House keeps that result: won once Flag_To_Win
/// (`0x004FC9E0`) set it, lost once Flag_To_Lose (`0x004FCBDE..0x004FCC42`)
/// or MPlayer_Defeated (`+0x1F5` at `0x004FC0C2`) did, though the result
/// route had not run yet. Only a running match decides: its world sets
/// ScenarioStarted (`0x00A8ED5C`) when it installs and clears it when it
/// returns, so a world the shell keeps from an earlier match, or none while
/// the next one loads, decides nothing.
fn running_match_result(
    result: ExternalMatchResult,
    session: &crate::app::match_runtime::session::MatchSession,
    sim: Option<&crate::sim::world::Simulation>,
) -> Option<(ExternalMatchResult, Option<Vec<String>>)> {
    let sim = sim.filter(|sim| sim.session().started())?;
    let local = sim
        .session()
        .current_house()
        .and_then(|owner| sim.houses().get(&owner));
    let result = match (result, local) {
        (ExternalMatchResult::Quit | ExternalMatchResult::Disconnected, Some(house))
            if house.has_won() =>
        {
            ExternalMatchResult::Won
        }
        (ExternalMatchResult::Quit | ExternalMatchResult::Disconnected, Some(house))
            if house.has_lost() || house.is_defeated() =>
        {
            ExternalMatchResult::Lost
        }
        (result, _) => result,
    };
    Some((result, match_winners(session, sim)))
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

/// Spawn-only test switches (tools/spawn/README.md): a `-SPAWN` multiplayer
/// launch plays as a slow PC once its match is on screen.
///
/// `VERA20K_SPAWN_SLOW_DRAW_MS` and `VERA20K_SPAWN_SLOW_TICK_MS` add that many
/// milliseconds to each drawn picture and to each simulation tick throughout,
/// the tick's inside the cost the relay's pace reports carry.
///
/// `VERA20K_SPAWN_SLOW_PLAN` adds delays that change over the match, to each
/// frame pass (drawn or not) and each simulation tick. The plan is
/// `second:draw_ms:tick_ms[:unreported_ms]` points, comma separated, from
/// the match's start; the delays go linearly between points and hold after
/// the last, so one point is a PC slow by the same amount throughout and
/// several make it slow down, speed up or hitch as a match grows. The tick's
/// delay is inside the cost the relay's pace reports carry; the unreported
/// one is spent on each tick too but outside it, as a world copy for the
/// picture is (2026-11-21, match 1e9fac21: 8 ms ticks, 55 frames a second).
/// Both may be set; their tick delays add up. Read only for such launches;
/// ordinary play never consults them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SpawnSlowPc {
    /// Each frame pass, drawn or not (the plan).
    pub(crate) draw: std::time::Duration,
    /// Each drawn picture only (`VERA20K_SPAWN_SLOW_DRAW_MS`).
    pub(crate) drawn: std::time::Duration,
    pub(crate) tick: std::time::Duration,
    pub(crate) unreported: std::time::Duration,
}

/// One point of `VERA20K_SPAWN_SLOW_PLAN`: seconds into the match, then the
/// draw, tick and unreported tick delays in milliseconds.
type SlowPoint = (f64, [f64; 3]);

impl SpawnSlowPc {
    pub(crate) fn of(frontend: &crate::app::frontend::state::FrontendState) -> Self {
        let progress = &frontend.external_progress;
        if !frontend.external_launch || progress.mode == ExternalLaunchMode::Single {
            return Self::default();
        }
        let Some(started) = progress.match_started else {
            return Self::default();
        };
        let mut slow = Self::at(Self::plan(), started.elapsed().as_secs_f64());
        let (drawn, tick) = Self::fixed();
        slow.drawn = drawn;
        slow.tick += tick;
        slow
    }

    /// The delays `plan` gives `second` into the match.
    fn at(plan: &[SlowPoint], second: f64) -> Self {
        let Some(&(_, mut delays)) = plan.first() else {
            return Self::default();
        };
        for pair in plan.windows(2) {
            let ((from, before), (to, after)) = (pair[0], pair[1]);
            if second >= to {
                delays = after;
            } else if second > from {
                let share = (second - from) / (to - from);
                delays = std::array::from_fn(|i| before[i] + (after[i] - before[i]) * share);
            }
        }
        let millis = |value: f64| std::time::Duration::from_secs_f64(value.max(0.0) / 1000.0);
        Self {
            draw: millis(delays[0]),
            tick: millis(delays[1]),
            unreported: millis(delays[2]),
            ..Self::default()
        }
    }

    /// `VERA20K_SPAWN_SLOW_PLAN`'s points in time order; None when malformed.
    fn parse(text: &str) -> Option<Vec<SlowPoint>> {
        let mut plan = text
            .split(',')
            .filter(|point| !point.trim().is_empty())
            .map(|point| {
                let fields = point
                    .trim()
                    .split(':')
                    .map(|field| field.trim().parse::<f64>().ok())
                    .collect::<Option<Vec<_>>>()?;
                match fields[..] {
                    [second, draw, tick] => Some((second, [draw, tick, 0.0])),
                    [second, draw, tick, unreported] => Some((second, [draw, tick, unreported])),
                    _ => None,
                }
            })
            .collect::<Option<Vec<_>>>()?;
        plan.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(plan)
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn plan() -> &'static [SlowPoint] {
        static PLAN: std::sync::OnceLock<Vec<SlowPoint>> = std::sync::OnceLock::new();
        PLAN.get_or_init(|| {
            let Ok(text) = std::env::var("VERA20K_SPAWN_SLOW_PLAN") else {
                return Vec::new();
            };
            match Self::parse(&text) {
                Some(plan) => {
                    log::info!("Spawn test switch: playing as a slow PC, plan {plan:?}");
                    plan
                }
                None => {
                    log::warn!(
                        "Spawn test switch: ignoring malformed VERA20K_SPAWN_SLOW_PLAN {text:?}"
                    );
                    Vec::new()
                }
            }
        })
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn plan() -> &'static [SlowPoint] {
        &[]
    }

    /// `VERA20K_SPAWN_SLOW_DRAW_MS` and `VERA20K_SPAWN_SLOW_TICK_MS`: the
    /// drawn picture's and the tick's delays throughout the match.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn fixed() -> (std::time::Duration, std::time::Duration) {
        static FIXED: std::sync::OnceLock<(std::time::Duration, std::time::Duration)> =
            std::sync::OnceLock::new();
        *FIXED.get_or_init(|| {
            let millis = |name| {
                std::time::Duration::from_millis(
                    std::env::var(name)
                        .ok()
                        .and_then(|value| value.trim().parse().ok())
                        .unwrap_or(0),
                )
            };
            let fixed = (
                millis("VERA20K_SPAWN_SLOW_DRAW_MS"),
                millis("VERA20K_SPAWN_SLOW_TICK_MS"),
            );
            if fixed != Default::default() {
                log::info!(
                    "Spawn test switch: playing as a slow PC, drawn {:?}, tick {:?}",
                    fixed.0,
                    fixed.1
                );
            }
            fixed
        })
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn fixed() -> (std::time::Duration, std::time::Duration) {
        Default::default()
    }

    /// Spend `delay` on this thread, as a slow PC would.
    pub(crate) fn spend(delay: std::time::Duration) {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let _ = delay;
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
        let mut request = request.admit_loaded_modes(&state.frontend.skirmish_modes)?;
        request.check_country_limits(state.process_assets.reads_ares_way())?;
        if let SpawnGame::Network(network) = &mut request.game {
            // VERA: a launcher's room has no host to share rules: each
            // player plays their own, which its ExpectedRulesDigest and the
            // start agreement (`lan_service::install_gameplay`) compare. The
            // Building Production Queue stays off: the launcher's probe
            // leaves it out (`spawn_probe::existing_personal_rules`), and a
            // direct launch whose players differ would only time out.
            let mut rules =
                crate::skirmish_launch::HostRules::from_own(state.process_assets.personal_rules());
            rules.building_production_queue = false;
            // The expectations below hash the rules this match plays.
            state.process_assets.use_personal_rules(
                rules
                    .personal_rules()
                    .map_err(|error| anyhow::anyhow!("the player's rules do not read: {error}"))?,
            );
            network.session.options.host_rules = rules;
        }
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
        // The mod this launch asked for is not in use (`app::mods`): end on
        // the reason, which a start without the mod would bury in a rules
        // mismatch with the other players.
        if let Some(reason) = state.frontend.mods.refusal() {
            anyhow::bail!("{reason}");
        }
        Self::check_spawn_expectations(&state.platform, &state.process_assets, &request)?;
        state.frontend.external_launch = true;
        state.frontend.external_progress = ExternalLaunchProgress::default();
        if let SpawnGame::Network(network) = &request.game {
            let progress = &mut state.frontend.external_progress;
            progress.source_index = Some(network.session.local_player_index);
            match &network.transport {
                SpawnTransport::Relay(relay) => {
                    progress.mode = ExternalLaunchMode::Relay;
                    progress.match_id = Some(relay.match_id.clone());
                    // A watcher stands in source 0's seat but is not that
                    // player (docs/spawn.md "Watching").
                    if relay.watch {
                        progress.watch = true;
                        progress.source_index = None;
                    }
                }
                SpawnTransport::Direct(_) => progress.mode = ExternalLaunchMode::Direct,
            }
        }
        state.persistence.options_profile.game_speed = request.game_speed;
        Self::destroy_current_shell_dialog(&mut state.frontend);
        state.frontend.shell_first_paint_slide = None;
        let mut clock = SpawnSeedClock { seed: request.seed };
        match request.game {
            SpawnGame::Skirmish(session) => {
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
                            state.platform.game_config.as_ref(),
                            &mut state.frontend,
                            &mut state.match_state,
                            &mut state.process_assets,
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
                    "Relay spawn: match={}, source={}, humans={}, ai={}, speed={}, watch={}",
                    relay.match_id,
                    network.session.local_player_index,
                    network.session.humans.len(),
                    network.session.opponents.len(),
                    request.game_speed,
                    relay.watch
                );
                crate::app::shell_wol::begin_spawned_match(state, network.session, relay, seed)?;
            }
        }
        Ok(())
    }

    /// `[Settings] Expected*` (docs/spawn.md): reject the launch before it
    /// starts when this build, its rules or the client map differ from what
    /// the launcher probed.
    fn check_spawn_expectations(
        platform: &crate::app::PlatformState,
        process_assets: &crate::app::process_assets::ProcessAssets,
        request: &SpawnRequest,
    ) -> Result<()> {
        let session = match &request.game {
            SpawnGame::Skirmish(session) => Some(&**session),
            SpawnGame::Network(network) => Some(&network.session),
            SpawnGame::Campaign(_) => None,
        };
        let map = session.and_then(|session| session.selected_map_file.as_deref());
        let checked = request.expected.mismatch(
            &crate::app::shell_network_setup::agreement::version(),
            || {
                let session = session.context("launch has no game mode")?;
                process_assets
                    .multiplayer_rules_digest(&session.mode.override_file)
                    .map_err(anyhow::Error::msg)
            },
            || {
                let map = map.context("launch has no map")?;
                let root = &platform
                    .game_config
                    .as_ref()
                    .context("no asset directory")?
                    .paths
                    .ra2_dir;
                let assets = process_assets
                    .manager()
                    .context("retail assets are unavailable")?;
                Ok(
                    crate::app::spawn_probe::spawn_map_identity(root, map, assets)?
                        .map
                        .source_sha256,
                )
            },
        );
        checked.map_err(|mut error| {
            if let (Some(mismatch), Some(session)) = (
                error.downcast_mut::<crate::app::frontend::cncnet_spawn::RulesMismatch>(),
                session,
            ) {
                match process_assets.multiplayer_rules_parts(&[session.mode.override_file.as_str()])
                {
                    Ok(mut parts) => {
                        mismatch.parts = parts.pop();
                        if let Some(parts) = &mismatch.parts {
                            log::info!("Rules parts: {}", parts.json());
                        }
                    }
                    Err(parts_error) => log::warn!("No rules parts: {parts_error}"),
                }
            }
            error
        })
    }

    /// Sample the running match for the launch report (docs/spawn.md):
    /// frames, time on screen and opponent departure. External launches only.
    pub(super) fn poll_external_launch_progress(
        frontend: &mut crate::app::frontend::state::FrontendState,
        match_state: &crate::app::match_runtime::state::MatchState,
    ) {
        if !frontend.external_launch
            || frontend.screen != crate::ui::game_screen::GameScreen::InGame
        {
            return;
        }
        let Some(runtime) = match_state.sim_runtime.runtime() else {
            return;
        };
        let frame = runtime.simulation.session().binary_frame();
        let left = match_state.session.network_setup().is_some_and(|launch| {
            opponents_left(
                launch,
                match_state.session.network_connected_source_indices(),
            )
        });
        let progress = &mut frontend.external_progress;
        let now = Instant::now();
        if progress.match_started.is_none() {
            log::info!("External launch: match on screen at frame {frame}");
            progress.match_started = Some(now);
            progress.frame_cost = crate::app::spawn_frame_cost::FrameCost::requested();
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            crate::web::status("spawn:match_started");
        }
        progress.match_last = Some(now);
        // A line every 900 frames, so a hidden launch's log shows it plays
        // (or, watching, catches up).
        if frame / 900 > progress.frames / 900 {
            log::info!("External launch: frame {frame}");
        }
        progress.frames = progress.frames.max(frame);
        if let Some(cost) = &mut progress.frame_cost {
            cost.observe_frame(frame);
        }
        if left && !progress.opponents_left {
            log::info!("External launch: every opposing participant has left the match");
            progress.opponents_left = true;
        }
    }

    /// This launch's report from its final state.
    ///
    /// `rules_mismatch` names this game's rules parts when its spawn.ini's
    /// `ExpectedRulesDigest` refused the launch and the state that read them
    /// is gone.
    pub(crate) fn external_launch_report(
        frontend: Option<&crate::app::frontend::state::FrontendState>,
        network_quality: Option<&crate::app::match_runtime::network_quality::NetworkQualityStats>,
        failure: Option<&str>,
        rules_mismatch: Option<&crate::app::rules_parts::RulesParts>,
    ) -> ExternalLaunchReport {
        let failure = failure
            .or_else(|| frontend.and_then(|frontend| frontend.external_launch_error.as_deref()));
        let mut report = ExternalLaunchReport::new(
            frontend.and_then(Self::match_result),
            frontend.map(|frontend| &frontend.external_progress),
            failure,
        );
        report.network_quality = network_quality.and_then(|quality| quality.report());
        if let Some(parts) = rules_mismatch
            .or_else(|| frontend.and_then(|frontend| frontend.external_rules_mismatch.as_ref()))
        {
            report.rules_mismatch(parts.clone());
        }
        report
    }

    /// Desktop: write `spawn_result.json` beside spawn.ini and return the
    /// process exit status; `None` for a launch that is not `-SPAWN`.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    pub fn finish_spawn_launch(&self) -> Option<i32> {
        let path = self.spawn_result_path.as_deref()?;
        let report = Self::external_launch_report(
            self.state.as_ref().map(|state| &state.frontend),
            self.state
                .as_ref()
                .map(|state| &state.match_state.network_quality),
            self.startup_failure.as_deref(),
            self.spawn_rules_mismatch.as_ref(),
        );
        log::info!("External launch result: {}", report.json());
        if let Err(error) = write_spawn_result(path, &report) {
            log::error!("Could not write {}: {error:#}", path.display());
        }
        Some(report.exit_code)
    }

    /// Browser: close the external launch through its existing network owners.
    /// The host report is delivered by poll_browser_exit after they settle.
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub(super) fn finish_browser_external_launch(state: &mut AppState, failure: Option<&str>) {
        if state.frontend.browser_exit_pending {
            return;
        }
        if let Some(error) = failure {
            state
                .frontend
                .external_launch_error
                .get_or_insert_with(|| error.to_owned());
        }
        Self::browser_match_ended(
            &state.frontend,
            &state.match_state,
            ExternalMatchResult::Quit,
        );
        crate::web::request_exit(state.frontend.external_launch_error.as_deref());
        crate::app::shell_wol::discard_spawned_visit(&mut state.frontend);
        if state.match_state.lan_service.is_some() {
            crate::app::lan_service::close(&state.frontend, &mut state.match_state);
        }
        if state.match_state.session.network_return_context().is_some() {
            state.match_state.session.leave_network();
        }
        state.frontend.network_setup_protocol = None;
        Self::destroy_current_shell_dialog(&mut state.frontend);
        state.frontend.browser_exit_pending = true;
    }

    /// docs/spawn.md "Scripted leave and resign": run the external
    /// multiplayer launch's requested actions as the abort box presses a
    /// player makes. Requests come from the desktop test switches
    /// `VERA20K_SPAWN_LEAVE_AFTER_SECONDS` / `VERA20K_SPAWN_RESIGN_AFTER_SECONDS`
    /// (read only here, i.e. only by `-SPAWN` multiplayer launches) or the
    /// browser exports `spawn_leave()` / `spawn_resign()`. Ordinary play,
    /// single-player spawns and the built-in lobbies never reach this. The
    /// desktop test switch `VERA20K_SPAWN_LEAVE_AT_FRAME` is read here too,
    /// by every `-SPAWN` launch ([`leave_at_frame_due`]).
    pub(super) fn poll_spawn_actions(state: &mut AppState) {
        if !state.frontend.external_launch {
            return;
        }
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            use std::sync::OnceLock;
            static LEAVE_AT_FRAME: OnceLock<Option<u32>> = OnceLock::new();
            let leave_at = *LEAVE_AT_FRAME.get_or_init(|| {
                std::env::var("VERA20K_SPAWN_LEAVE_AT_FRAME")
                    .ok()
                    .and_then(|value| value.trim().parse().ok())
            });
            let on_score = matches!(
                state.frontend.screen,
                crate::ui::game_screen::GameScreen::MissionResult { .. }
            );
            let progress = &mut state.frontend.external_progress;
            if let Some(action) = leave_at.and_then(|leave_at| {
                leave_at_frame_due(
                    leave_at,
                    progress.frames,
                    on_score,
                    &mut progress.actions_fired,
                )
            }) {
                log::info!(
                    "Spawn test switch: requesting {action:?} at frame {}",
                    progress.frames
                );
                progress.pending_actions.push(action);
            }
        }
        if state.frontend.external_progress.mode == ExternalLaunchMode::Single {
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            if crate::web::take_spawn_action().is_some() {
                log::warn!("Spawn action ignored: not a multiplayer launch");
            }
            Self::apply_pending_spawn_action(state);
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
        Self::apply_pending_spawn_action(state);
    }

    /// Run the first waiting scripted action, if it can run now.
    fn apply_pending_spawn_action(state: &mut AppState) {
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
                if !state.match_state.sim_runtime.is_installed()
                    || state.match_state.scenario_exit.is_some()
                    || state.match_state.scenario_outcome.is_some()
                    || Self::house_outcome_wait_pending(&state.match_state.sim_runtime)
                    || crate::app::match_runtime::network_failure::pending(
                        &state.frontend.screen,
                        &state.match_state,
                    )
                {
                    return false;
                }
                // The player's route: open the abort box (its init picks
                // Observe/Leave from the live match), then press a button.
                Self::enter_in_game_menu_state(
                    &mut state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &mut state.persistence,
                    crate::ui::pause_menu::InGameMenuState::AbortConfirm,
                );
                let secondary = state.match_state.match_presentation.abort_secondary;
                match action {
                    SpawnAction::Leave => {
                        log::info!(
                            "Spawn action: pressing Leave (abort box secondary {secondary:?})"
                        );
                        crate::app::input::abort::activate(
                            &mut state.platform,
                            &state.renderer,
                            &state.diag,
                            &mut state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio,
                            &mut state.persistence,
                            AbortButton::Leave,
                        );
                    }
                    SpawnAction::Resign if secondary == AbortSecondary::Observe => {
                        log::info!("Spawn action: resigning through the abort box's Observe");
                        crate::app::input::abort::activate(
                            &mut state.platform,
                            &state.renderer,
                            &state.diag,
                            &mut state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio,
                            &mut state.persistence,
                            AbortButton::Secondary,
                        );
                    }
                    SpawnAction::Resign => {
                        log::warn!(
                            "Spawn action: resign unavailable (abort box secondary {secondary:?}); nothing pressed"
                        );
                        Self::enter_in_game_menu_state(
                            &mut state.platform,
                            &state.renderer,
                            &mut state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio,
                            &mut state.persistence,
                            crate::ui::pause_menu::InGameMenuState::Closed,
                        );
                    }
                }
                true
            }
            GameScreen::MissionResult { .. } => {
                if action == SpawnAction::Leave {
                    log::info!("Spawn action: pressing Continue on the score screen");
                    if Self::score_shell_active(&state.frontend) {
                        Self::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellSlideKind::Score,
                            App::leave_mission_result_screen,
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
            // Read once: a launch sets it, and every pass asks.
            static REQUESTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            *REQUESTED.get_or_init(|| {
                std::env::var_os("VERA20K_SPAWN_HIDDEN_WINDOW").is_some_and(|value| value == "1")
            })
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            false
        }
    }

    /// This `-SPAWN` launch's window is the opt-in invisible test window.
    pub(super) fn invisible_spawn_window(
        platform: &crate::app::PlatformState,
        frontend: &crate::app::frontend::state::FrontendState,
    ) -> bool {
        frontend.external_launch
            && Self::spawn_hidden_window_requested()
            && platform.window.is_visible() == Some(false)
    }

    /// The local result of an external launch's or online lobby match's
    /// match, once it ended; the relay `finish` reports it, and launch hosts
    /// read it here.
    pub(crate) fn match_result(
        frontend: &crate::app::frontend::state::FrontendState,
    ) -> Option<ExternalMatchResult> {
        frontend.match_result
    }

    /// The decided network match's winners still in it, for the online
    /// lobby's `finish`.
    pub(crate) fn match_winners(
        frontend: &crate::app::frontend::state::FrontendState,
    ) -> Option<&[String]> {
        frontend.match_winners.as_deref()
    }

    /// When the match's local result was decided, for the online lobby's
    /// `finish`.
    pub(crate) fn match_result_at(
        frontend: &crate::app::frontend::state::FrontendState,
    ) -> Option<Instant> {
        frontend.match_result_at
    }

    /// An online match starts: no earlier match's result may stand for it.
    pub(crate) fn clear_match_result(frontend: &mut crate::app::frontend::state::FrontendState) {
        frontend.match_result = None;
        frontend.match_winners = None;
        frontend.match_result_at = None;
    }

    /// Record the end-of-match owner's decision for an external launch or an
    /// online lobby match, as [`running_match_result`] reads it from the
    /// running match; the first decision wins.
    pub(crate) fn record_match_result(
        frontend: &mut crate::app::frontend::state::FrontendState,
        match_state: &crate::app::match_runtime::state::MatchState,
        result: ExternalMatchResult,
    ) {
        use crate::app::match_runtime::session::SessionMode;
        // A watcher has no result: the followed player's is not its own.
        if (!frontend.external_launch && match_state.session_mode() != SessionMode::Wol)
            || frontend.match_result.is_some()
            || crate::app::match_runtime::watching::active(match_state)
        {
            return;
        }
        let sim = match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.simulation);
        let Some((result, winners)) = running_match_result(result, &match_state.session, sim)
        else {
            return;
        };
        frontend.match_result = Some(result);
        frontend.match_winners = winners;
        frontend.match_result_at = Some(Instant::now());
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

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::app::frontend::cncnet_spawn::{SpawnGame, SpawnRequest};

    #[test]
    fn a_slow_plan_goes_linearly_between_its_points_and_holds_after() {
        use std::time::Duration;
        let plan = SpawnSlowPc::parse("120:10:2:6, 0:0:0,180:4:0").unwrap();
        let at = |second| SpawnSlowPc::at(&plan, second);
        assert_eq!(at(0.0), SpawnSlowPc::default());
        assert_eq!(at(60.0).draw, Duration::from_millis(5));
        assert_eq!(at(60.0).tick, Duration::from_millis(1));
        assert_eq!(at(60.0).unreported, Duration::from_millis(3));
        assert_eq!(at(150.0).draw, Duration::from_millis(7));
        assert_eq!(at(1000.0).draw, Duration::from_millis(4));
        assert_eq!(at(1000.0).tick, Duration::ZERO);
        let steady = SpawnSlowPc::parse("0:8:1").unwrap();
        assert_eq!(
            SpawnSlowPc::at(&steady, 999.0).draw,
            Duration::from_millis(8)
        );
        assert_eq!(SpawnSlowPc::at(&[], 5.0), SpawnSlowPc::default());
        assert!(SpawnSlowPc::parse("0:8").is_none());
        assert!(SpawnSlowPc::parse("0:8:1:2:3").is_none());
        assert!(SpawnSlowPc::parse("x:1:2").is_none());
    }

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
    fn leave_at_frame_leaves_the_match_once_then_the_score_page_once() {
        let mut fired = SpawnActionsFired::default();
        assert_eq!(leave_at_frame_due(900, 899, false, &mut fired), None);
        assert_eq!(
            leave_at_frame_due(900, 900, false, &mut fired),
            Some(SpawnAction::Leave)
        );
        // The match is still up while the Leave runs: nothing more to press.
        assert_eq!(leave_at_frame_due(900, 905, false, &mut fired), None);
        assert_eq!(
            leave_at_frame_due(900, 905, true, &mut fired),
            Some(SpawnAction::Leave)
        );
        assert_eq!(leave_at_frame_due(900, 905, true, &mut fired), None);
        // A score page before the frame is another match's: it stays.
        let mut early = SpawnActionsFired::default();
        assert_eq!(leave_at_frame_due(900, 10, true, &mut early), None);
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
            ..Default::default()
        };
        let won = ExternalLaunchReport::new(Some(ExternalMatchResult::Won), Some(&progress), None);
        assert_eq!((won.result, won.exit_code), ("won", 10));
        let value: serde_json::Value = serde_json::from_str(&won.json()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"result":"won","exit_code":10,"mode":"relay","match_id":"m",
                "source_index":1,"frames":781,"duration_seconds":0.0,"opponents_left":true,
                "reason":null,"network_quality":null})
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
        // docs/spawn.md "Watching": a watcher's report names no source and
        // says it watched.
        let watched = ExternalLaunchProgress {
            source_index: None,
            watch: true,
            ..progress
        };
        let value: serde_json::Value =
            serde_json::from_str(&ExternalLaunchReport::new(None, Some(&watched), None).json())
                .unwrap();
        assert_eq!(
            (&value["result"], &value["source_index"], &value["watch"]),
            (
                &serde_json::json!("quit"),
                &serde_json::Value::Null,
                &serde_json::json!(true)
            )
        );
    }

    #[test]
    fn only_the_running_match_decides_a_leaving_players_result() {
        // The shell keeps the last match's world, its local House defeated,
        // while the next match loads: a player who leaves then has no result.
        let mut sim = crate::sim::world::Simulation::new();
        let local = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                local, 0, None, true, 0, 10,
            ))
            .set_is_defeated_for_test(true);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(local));
        let session = crate::app::match_runtime::session::MatchSession::default();
        let quit = |sim: Option<&crate::sim::world::Simulation>| {
            running_match_result(ExternalMatchResult::Quit, &session, sim)
        };
        assert_eq!(quit(None), None);
        assert_eq!(quit(Some(&sim)), None);
        // In its own match, the defeated player's leaving is a loss.
        sim.publish_scenario_started();
        assert_eq!(quit(Some(&sim)), Some((ExternalMatchResult::Lost, None)));
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
}
