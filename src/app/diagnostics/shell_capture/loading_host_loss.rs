//! Opt-in read-only proof of an externally killed online loading publisher.
//! The loader supplies its actual frozen startup/progress/timer and result world.
//! No checkpoint waits, publishes progress, changes membership or advances a tick.

use super::fault_files::{read_control, write_atomic};
use super::*;
use crate::app::loading::pump::NetworkLoadingObservation;
use crate::sim::command::{
    MegaMissionOrder, MegaMissionRecord, ProcessTimeRecord, StopRecord, TimingRecord,
};
use crate::sim::world::Simulation;
use serde_json::{Value, json};

pub(crate) fn sidecar(output: &Path, kind: &str) -> PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(format!(".loading.{kind}.json"));
    PathBuf::from(name)
}

/// Ephemeral borrow for one loading pump. Only the expired diagnostic snapshot
/// travels across the existing disconnect callback; it is not a second timer.
pub(crate) struct HostLossObserver<'a> {
    config: &'a LiveOnlineConfig,
    frame: u32,
    uptime_ms: u128,
    expired: Option<Value>,
}

pub(crate) enum LoadingObserver<'a> {
    HostLoss(HostLossObserver<'a>),
    Startup(super::startup_host_loss::StartupLoadingObserver<'a>),
    Cancel(super::loading_runtime::CancelLoadingObserver<'a>),
}

impl ShellCaptureSession {
    pub(crate) fn loading_observer(&self) -> Option<LoadingObserver<'_>> {
        let config = self.request.live_online.as_ref()?;
        let uptime_ms = self.started_at?.elapsed().as_millis();
        if config.loading_cancel.is_some() {
            Some(LoadingObserver::Cancel(
                super::loading_runtime::CancelLoadingObserver {
                    config,
                    frame: self.frames_seen,
                    uptime_ms: u128::from(crate::app::match_runtime::frame_pacer::wall_clock_ms()),
                },
            ))
        } else if config.startup_host_loss.is_some() {
            Some(LoadingObserver::Startup(
                super::startup_host_loss::StartupLoadingObserver::new(
                    config,
                    self.frames_seen,
                    u128::from(crate::app::match_runtime::frame_pacer::wall_clock_ms()),
                ),
            ))
        } else {
            config.loading_host_loss.as_ref()?;
            Some(LoadingObserver::HostLoss(HostLossObserver {
                config,
                frame: self.frames_seen,
                uptime_ms,
                expired: None,
            }))
        }
    }
}

impl LoadingObserver<'_> {
    pub(crate) fn observe(
        &mut self,
        state: &AppState,
        view: NetworkLoadingObservation<'_>,
    ) -> Result<()> {
        match self {
            Self::HostLoss(observer) => observer.observe(state, view),
            Self::Startup(_) | Self::Cancel(_) => Ok(()),
        }
    }

    /// This diagnostic decision is queried only after the ordinary complete
    /// decision and terminal100 publication. The LoadingSession retains the
    /// single result if its opt-in victim has not handed it to installation.
    pub(crate) fn defer_install(
        &mut self,
        state: &AppState,
        view: NetworkLoadingObservation<'_>,
    ) -> Result<bool> {
        match self {
            Self::HostLoss(_) => Ok(false),
            Self::Startup(observer) => observer.defer_install(state, view),
            Self::Cancel(observer) => observer.defer_install(state, view),
        }
    }

    pub(crate) fn before_install(&mut self) -> Result<()> {
        match self {
            Self::HostLoss(_) => Ok(()),
            Self::Startup(observer) => observer.before_install(),
            Self::Cancel(observer) => observer.before_install(),
        }
    }

    pub(crate) fn removed(
        &mut self,
        state: &AppState,
        sim: &mut Simulation,
        timed_out: &[usize],
    ) -> Result<()> {
        match self {
            Self::HostLoss(observer) => observer.removed(state, sim, timed_out),
            Self::Startup(_) | Self::Cancel(_) => {
                ensure!(
                    timed_out.is_empty(),
                    "startup fault reached a loading timeout before install"
                );
                Ok(())
            }
        }
    }

    pub(crate) fn installed(&self, state: &mut AppState) -> Result<()> {
        match self {
            Self::HostLoss(observer) => observer.installed(state),
            Self::Startup(observer) => observer.installed(state),
            Self::Cancel(_) => Ok(()),
        }
    }
}

impl HostLossObserver<'_> {
    pub(crate) fn observe(
        &mut self,
        state: &AppState,
        view: NetworkLoadingObservation<'_>,
    ) -> Result<()> {
        let Some(launch) = view.startup.launch_session() else {
            return Ok(());
        };
        // The initial configuration remains three players. The later normal
        // two-player room intentionally has no external loading fault.
        if launch.humans.len() != 3 {
            return Ok(());
        }
        let config = self.config;
        let root = config
            .loading_output
            .as_ref()
            .context("loading fault output")?;
        let host_source = launch
            .humans
            .iter()
            .position(|human| human.name == config.host)
            .context("original loading host")?;
        let local = launch.local_player_index;
        ensure!(
            view.rows.local_player_index == local && view.rows.participants.len() == 3,
            "loading progress rows changed frozen source identity"
        );
        ensure!(
            launch.humans[local].name == config.account,
            "loading account binding changed"
        );
        let mut snapshot = self.snapshot(state, &view)?;
        if config.account == config.host {
            let kind = if view.stage == "before_complete" {
                "complete"
            } else {
                "ready"
            };
            let path = sidecar(root, kind);
            if !path.try_exists()?
                && (kind == "complete" || view.progress.current_percent() < 100.0)
            {
                snapshot["schema"] = json!(if kind == "complete" {
                    "vera20k.loading-host-complete.v1"
                } else {
                    "vera20k.loading-host-ready.v1"
                });
                write_atomic(&path, &snapshot)?;
            }
            return Ok(());
        }
        if !matches!(view.stage, "awaiting_peers" | "timed_out") {
            return Ok(());
        }
        let Some(control) = read_control(&sidecar(root, "killed"), 4096)? else {
            return Ok(());
        };
        validate_control(config, &control)?;
        let incomplete = &view.incomplete;
        if view.progress.current_percent() < 100.0
            || incomplete.as_slice() != [host_source]
            || view
                .rows
                .participants
                .iter()
                .enumerate()
                .any(|(source, row)| {
                    source != local
                        && source != host_source
                        && row
                            .remote_progress
                            .as_ref()
                            .is_none_or(|progress| progress.current_percent() < 100.0)
                })
        {
            return Ok(());
        }
        let sim = view.simulation.context("awaiting-peer result world")?;
        ensure!(
            sim.session().binary_frame() == 0 && sim.session().tick() == 0,
            "loading fault advanced simulation before the barrier"
        );
        ensure!(
            sim.session().network_command_timing().is_none(),
            "loading installed live timing early"
        );
        snapshot["control"] = control;
        if view.stage == "timed_out" {
            self.expired = Some(snapshot);
        } else if !sidecar(root, "prefix").try_exists()? {
            ensure!(
                view.timeout
                    .is_some_and(|timer| timer.remaining(view.observed_bucket) > 0),
                "loading prefix was not observed inside its real countdown"
            );
            snapshot["schema"] = json!("vera20k.loading-host-prefix.v1");
            write_atomic(&sidecar(root, "prefix"), &snapshot)?;
        }
        Ok(())
    }

    pub(crate) fn installed(&self, state: &mut AppState) -> Result<()> {
        ensure!(
            state.frontend.screen == GameScreen::InGame && state.match_state.live_network.is_some(),
            "loading host-loss install failed"
        );
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("installed world")?
            .simulation;
        ensure!(
            sim.session().binary_frame() == 0 && sim.session().tick() == 0,
            "loading observer missed frame-zero install"
        );
        state
            .match_state
            .match_diagnostics
            .begin_command_observation();
        Ok(())
    }

    fn snapshot(&self, state: &AppState, view: &NetworkLoadingObservation<'_>) -> Result<Value> {
        loading_snapshot(
            self.config,
            state,
            view,
            self.frame,
            self.uptime_ms,
            self.config
                .loading_host_loss
                .as_deref()
                .context("loading checkpoint")?,
        )
    }

    /// Called immediately after the ordinary684536->5DA750 loading removal,
    /// before finish_fresh_scenario_read installs live command timing.
    pub(crate) fn removed(
        &mut self,
        state: &AppState,
        sim: &mut Simulation,
        timed_out: &[usize],
    ) -> Result<()> {
        let Some(mut snapshot) = self.expired.take() else {
            return Ok(());
        };
        let root = self
            .config
            .loading_output
            .as_ref()
            .context("loading output")?;
        let prefix = read_control(&sidecar(root, "prefix"), 262144)?
            .context("fresh loading prefix was not published")?;
        ensure!(
            prefix["control"] == snapshot["control"],
            "loading control changed before timeout"
        );
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("frozen loading roster")?;
        let host_source = launch
            .humans
            .iter()
            .position(|human| human.name == self.config.host)
            .context("original loading host")?;
        let host_house = crate::app::match_runtime::network_match::source_house(
            &state.match_state.session,
            sim,
            host_source,
        )
        .context("original loading host House")?;
        let owner = *sim
            .houses()
            .order()
            .get(usize::try_from(host_house)?)
            .context("loading host House index")?;
        let queued_remove_player = sim
            .network_command_queues()
            .local_records()?
            .iter()
            .filter(|record| record.opcode() == crate::sim::command::REMOVE_PLAYER_OPCODE)
            .count();
        let active = state.match_state.session.network_connected_source_indices();
        ensure!(
            timed_out == [host_source]
                && active.len() == 2
                && !active.contains(&host_source)
                && sim.houses()[&owner].is_to_die()
                && sim.session().binary_frame() == 0
                && sim.session().tick() == 0
                && sim.session().network_command_timing().is_none()
                && queued_remove_player == 0,
            "ordinary loading timeout did not remove only the original publisher before timing install"
        );
        snapshot["schema"] = json!("vera20k.loading-host-removed.v1");
        snapshot["prefix"] = prefix;
        snapshot["removed_sources"] = json!(timed_out);
        snapshot["active_sources"] = json!(active);
        snapshot["host_house"] = json!(host_house);
        snapshot["host_to_die"] = json!(sim.houses()[&owner].is_to_die());
        snapshot["native_frame"] = json!(sim.session().binary_frame());
        snapshot["tick"] = json!(sim.session().tick());
        snapshot["timing_installed"] = json!(sim.session().network_command_timing().is_some());
        snapshot["queued_remove_player"] = json!(queued_remove_player);
        ensure!(
            snapshot["loading"]["timeout_remaining"] == 0,
            "loading disconnect did not follow actual timer expiry"
        );
        write_atomic(&sidecar(root, "removed"), &snapshot)
    }
}

fn validate_control(config: &LiveOnlineConfig, control: &Value) -> Result<()> {
    let requested = control["request_after_launch_ms"]
        .as_u64()
        .context("loading kill request time")?;
    let started = control["kill_started_after_launch_ms"]
        .as_u64()
        .context("loading kill start time")?;
    let finished = control["kill_succeeded_after_launch_ms"]
        .as_u64()
        .context("loading kill receipt time")?;
    ensure!(
        control["schema"] == "vera20k.loading-host-killed.v1"
            && control["phase"] == "loading-killed"
            && control["checkpoint_id"].as_str() == config.loading_host_loss.as_deref()
            && control["victim_account"] == config.host
            && control["victim_ready_render_frame"].as_u64().is_some()
            && control["owned_kill_succeeded"] == true
            && requested <= started
            && started <= finished,
        "loading kill control belongs to another attempt or has no owned kill acknowledgement"
    );
    Ok(())
}

/// Capture observations only. Original configuration3 is immutable. The input
/// plan for the later new room comes from actual surviving House bindings.
#[derive(Default)]
pub(crate) struct LoadingHostLoss {
    prefix: Option<Value>,
    removed: Option<Value>,
    survivor_names: Vec<String>,
    gameplay: Vec<Value>,
    executions: Vec<Value>,
}

impl LoadingHostLoss {
    pub(crate) fn rematch_host(&self) -> Option<&str> {
        self.survivor_names.first().map(String::as_str)
    }

    pub(crate) fn rematch_map(&self) -> Option<&str> {
        self.gameplay.first()?.get("startup")?.get("map")?.as_str()
    }

    pub(crate) fn begin_round(
        &mut self,
        config: &LiveOnlineConfig,
        state: &mut AppState,
        round: usize,
    ) -> Result<()> {
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("loading-loss live world")?
            .simulation;
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("frozen round roster")?;
        let active = state.match_state.session.network_connected_source_indices();
        let host_source = launch
            .humans
            .iter()
            .position(|human| human.name == config.host);
        if round == 1 {
            let root = config.loading_output.as_ref().context("loading output")?;
            let prefix =
                read_control(&sidecar(root, "prefix"), 262144)?.context("loading prefix")?;
            let removed =
                read_control(&sidecar(root, "removed"), 262144)?.context("loading removal")?;
            validate_control(config, &prefix["control"])?;
            ensure!(
                launch.humans.len() == 3
                    && active.len() == 2
                    && removed["prefix"] == prefix
                    && removed["control"] == prefix["control"]
                    && removed["process_id"] == std::process::id()
                    && removed["checkpoint_id"].as_str() == config.loading_host_loss.as_deref()
                    && host_source.is_some_and(|source| !active.contains(&source)),
                "initial live roster does not preserve the proven loading timeout"
            );
            let mut survivors: Vec<_> = active
                .iter()
                .map(|&source| {
                    let house = crate::app::match_runtime::network_match::source_house(
                        &state.match_state.session,
                        sim,
                        source,
                    )
                    .context("survivor House")?;
                    Ok((house, launch.humans[source].name.clone()))
                })
                .collect::<Result<_>>()?;
            survivors.sort_by_key(|(house, _)| *house);
            self.survivor_names = survivors.into_iter().map(|(_, name)| name).collect();
            self.prefix = Some(prefix);
            self.removed = Some(removed);
        } else {
            ensure!(
                round == 2
                    && launch.humans.len() == 2
                    && active.len() == 2
                    && launch
                        .humans
                        .iter()
                        .all(|human| self.survivor_names.contains(&human.name))
                    && !launch.humans.iter().any(|human| human.name == config.host),
                "rematch did not admit exactly the two surviving accounts"
            );
        }
        self.check_membership(state, round)?;
        let startup = network_startup_receipt(state, launch, Some(sim))?;
        if round == 2 {
            ensure!(
                startup["match_id"] != self.gameplay[0]["match_id"],
                "rematch reused the old match identity"
            );
        }
        self.gameplay.push(json!({"round":round,"process_id":std::process::id(),
            "match_id":startup["match_id"],"startup":startup,
            "roster_names":launch.humans.iter().map(|human|&human.name).collect::<Vec<_>>(),
            "active_sources":active,"survivor_names":self.survivor_names,
            "peer_house":state.match_state.live_network.as_ref().context("live transport")?.peers()[0].house_id(),
            "publisher_house":sim.network_timing_host_house(),
            "publisher_name":publisher_name(state, sim)}));
        ensure!(
            state
                .match_state
                .match_diagnostics
                .command_observations_mut()
                .is_some(),
            "loading owner did not arm the borrowed execution observer before the first live frame"
        );
        Ok(())
    }

    pub(crate) fn check_membership(&self, state: &AppState, round: usize) -> Result<()> {
        let launch = state
            .match_state
            .session
            .network_setup()
            .context("loading-loss roster")?;
        let sim = &state
            .match_state
            .sim_runtime
            .as_ref()
            .context("loading-loss world")?
            .simulation;
        let active = state.match_state.session.network_connected_source_indices();
        let live = state
            .match_state
            .live_network
            .as_ref()
            .context("survivor live transport")?;
        ensure!(
            launch.humans.len() == if round == 1 { 3 } else { 2 }
                && active.len() == 2
                && live.peers().len() == 1
                && active
                    .iter()
                    .all(|&source| self.survivor_names.contains(&launch.humans[source].name)),
            "loading-loss live membership changed outside the proven removal"
        );
        let remote = active
            .iter()
            .copied()
            .find(|&source| source != launch.local_player_index)
            .context("other live survivor")?;
        ensure!(
            Some(i32::from(live.peers()[0].house_id()))
                == crate::app::match_runtime::network_match::source_house(
                    &state.match_state.session,
                    sim,
                    remote
                ),
            "survivor private peer is bound to a different House"
        );
        ensure!(
            publisher_name(state, sim).as_deref() == self.rematch_host(),
            "canonical live publisher is outside the surviving House order"
        );
        Ok(())
    }

    pub(crate) fn collect(
        &mut self,
        state: &mut AppState,
        round: usize,
        before_frame: u32,
    ) -> Result<()> {
        self.check_membership(state, round)?;
        for event in state
            .match_state
            .match_diagnostics
            .take_command_observations()?
        {
            // Preserve the exact same completed prefix on both renderers.
            // Later whole-match hashes/results retain their existing owner.
            if event.frame >= before_frame {
                continue;
            }
            let record = &event.record;
            let timing = TimingRecord::decode(record);
            let process = ProcessTimeRecord::decode(record);
            let movement = MegaMissionRecord::decode(record).and_then(|event| match event.order {
                MegaMissionOrder::Move { target_x, target_y } => Some((target_x, target_y)),
                _ => None,
            });
            let stop = StopRecord::decode(record);
            let input = if timing.is_some() {
                "TIMING"
            } else if process.is_some() {
                "PROCESS_TIME"
            } else if movement.is_some() {
                "Move"
            } else if stop.is_some() {
                "Stop"
            } else {
                continue;
            };
            ensure!(
                self.executions.len() < 8192,
                "loading-loss execution evidence exceeded its bound"
            );
            self.executions.push(json!({"round":round,"frame":event.frame,
                "record_frame":record.frame_stamp(),"opcode":record.opcode(),"house":record.house_id(),
                "input":input,"applied":event.applied,"actor":event.actor,
                "move_target":movement,"destination":event.destination,"cell":event.cell,"mission":event.mission,
                "process_time_millis":process.map(|event|event.milliseconds),
                "timing":timing.map(|event|json!({"max_ahead":event.max_ahead,
                    "frames_per_second":event.frames_per_second,"frame_send_rate":event.frame_send_rate.get()})),
                "post":event.timing.as_ref().map(|timing|json!({"publisher":timing.publisher,
                    "fps":timing.fps,"frame_send_rate":timing.frame_send_rate,"max_ahead":timing.max_ahead}))}));
        }
        Ok(())
    }

    pub(crate) fn finish_round(
        &mut self,
        state: &mut AppState,
        round: usize,
        before_frame: u32,
    ) -> Result<()> {
        self.collect(state, round, before_frame)?;
        let startup = &self.gameplay[round - 1]["startup"];
        let source_houses: Vec<_> = self
            .survivor_names
            .iter()
            .map(|name| {
                startup["sources"]
                    .as_array()
                    .context("startup sources")?
                    .iter()
                    .find(|source| source["name"] == *name)
                    .and_then(|source| source["house"].as_i64())
                    .context("survivor execution House")
            })
            .collect::<Result<_>>()?;
        for house in &source_houses {
            for input in ["Move", "Stop", "PROCESS_TIME"] {
                ensure!(
                    self.executions.iter().any(|event| event["round"] == round
                        && event["house"] == *house
                        && event["input"] == input
                        && event["applied"] == true),
                    "survivor House{house} has no actual accepted {input} dispatch"
                );
            }
        }
        let last_sample = source_houses
            .iter()
            .map(|house| {
                self.executions
                    .iter()
                    .filter(|event| {
                        event["round"] == round
                            && event["house"] == *house
                            && event["input"] == "PROCESS_TIME"
                            && event["applied"] == true
                    })
                    .filter_map(|event| event["frame"].as_u64())
                    .min()
                    .context("PROCESS_TIME sample")
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max()
            .context("live samples")?;
        let publisher = self.gameplay[round - 1]["publisher_house"]
            .as_i64()
            .context("live publisher")?;
        ensure!(
            self.executions
                .iter()
                .filter(|event| event["round"] == round
                    && event["input"] == "TIMING"
                    && event["applied"] == true
                    && event["house"] == publisher
                    && event["frame"]
                        .as_u64()
                        .is_some_and(|frame| frame > last_sample && frame >= 256))
                .count()
                >= 2,
            "surviving publisher did not continue actual adaptive TIMING"
        );
        state
            .match_state
            .match_diagnostics
            .end_command_observation();
        Ok(())
    }

    pub(crate) fn receipt(&self, config: &LiveOnlineConfig) -> Result<Value> {
        ensure!(
            self.prefix.is_some() && self.removed.is_some() && self.gameplay.len() == 2,
            "loading-host-loss evidence is incomplete"
        );
        Ok(
            json!({"checkpoint_id":config.loading_host_loss,"process_id":std::process::id(),
            "prefix":self.prefix,"removed":self.removed,"survivor_names":self.survivor_names,
            "rematch_host":self.rematch_host(),"gameplay":self.gameplay,
            "execution_before_frame":config.frames,"executions":self.executions}),
        )
    }
}

fn publisher_name(state: &AppState, sim: &Simulation) -> Option<String> {
    let house = i32::from(sim.network_timing_host_house()?);
    let launch = state.match_state.session.network_setup()?;
    launch
        .humans
        .iter()
        .enumerate()
        .find_map(|(source, human)| {
            (crate::app::match_runtime::network_match::source_house(
                &state.match_state.session,
                sim,
                source,
            ) == Some(house))
            .then(|| human.name.clone())
        })
}

/// One physical/effective-map and source/House projection shared by both loading
/// observations. It never trusts a previous resident world or copies progress.
pub(super) fn loading_snapshot(
    config: &LiveOnlineConfig,
    state: &AppState,
    view: &NetworkLoadingObservation<'_>,
    frame: u32,
    uptime_ms: u128,
    checkpoint_id: &str,
) -> Result<Value> {
    let launch = view
        .startup
        .launch_session()
        .context("network loading startup")?;
    let crate::app::match_bootstrap::LoadingStartup::Network(startup) = view.startup else {
        bail!("loading host loss requires an admitted network startup");
    };
    let metadata = state
        .match_state
        .session
        .network_map_metadata()
        .context("loading map metadata")?;
    let physical = startup
        .local_map_source()
        .and_then(|source| source.physical_identity())
        .context("verified local loading map")?;
    ensure!(
        physical.0 == usize::try_from(metadata.byte_length)?
            && physical.1 == metadata.source_sha256
            && startup.effective_map_digest() == Some(metadata.effective_digest.as_str()),
        "loading map source is outside the accepted agreement"
    );
    let progress: Vec<_> = view
        .rows
        .participants
        .iter()
        .enumerate()
        .map(|(source, row)| {
            let progress = row.remote_progress.as_ref().unwrap_or(view.progress);
            json!({"source":source,"percent":progress.current_percent()})
        })
        .collect();
    Ok(json!({
        "checkpoint_id":checkpoint_id,"account":config.account,
        "host":config.host,"players":launch.humans.len(),"process_id":std::process::id(),
        "render_frame":frame,"uptime_ms":uptime_ms,
        "startup":network_startup_receipt(state, launch, view.simulation)?,
        "map_admission":{"source_sha256":metadata.source_sha256,
            "effective_digest":metadata.effective_digest,"byte_length":metadata.byte_length,
            "capacity":metadata.player_capacity},
        "loading":{"stage":view.stage,"local_progress":view.progress.current_percent(),
            "progress":progress,"frame":view.simulation.map_or(0, |sim|sim.session().binary_frame()),
            "tick":view.simulation.map_or(0, |sim|sim.session().tick()),
            "timeout_start":view.timeout.map(|timer|timer.start_frame()),
            "timeout_period":view.timeout.map(|timer|timer.duration()),
            "timeout_remaining":view.timeout.map(|timer|timer.remaining(view.observed_bucket)),
            "observed_bucket":view.observed_bucket}
    }))
}
