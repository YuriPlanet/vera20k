//! Opt-in runtime proof of real preinstall service cancellation and progress.
//! LoadingSession retains the only completed result. These controls submit
//! ordinary owner intents; no response, progress, timer, packet or cancel is
//! manufactured by the diagnostic.

use super::fault_files::{read_control, write_atomic};
use super::*;
use crate::app::loading::pump::NetworkLoadingObservation;
use serde_json::{Value, json};

pub(crate) fn sidecar(output: &Path, kind: &str) -> PathBuf {
    let mut path = output.as_os_str().to_owned();
    path.push(format!(".loading-cancel.{kind}.json"));
    PathBuf::from(path)
}

pub(crate) struct CancelLoadingObserver<'a> {
    pub config: &'a LiveOnlineConfig,
    pub frame: u32,
    pub uptime_ms: u128,
}
impl CancelLoadingObserver<'_> {
    pub(crate) fn defer_install(
        &self,
        state: &AppState,
        view: NetworkLoadingObservation<'_>,
    ) -> Result<bool> {
        let fault = self
            .config
            .loading_cancel
            .as_ref()
            .context("loading cancel configuration")?;
        if sidecar(&fault.output, "returned").try_exists()? {
            return Ok(false);
        }
        let launch = view
            .startup
            .launch_session()
            .context("current network load")?;
        ensure!(
            launch.humans.len() == 2
                && launch.local_player().name == self.config.account
                && view.stage == "ready_to_install"
                && view.incomplete.is_empty()
                && view.progress.current_percent() == 100.0
                && view.timeout.is_none()
                && view
                    .rows
                    .participants
                    .iter()
                    .enumerate()
                    .all(|(source, row)| source == launch.local_player_index
                        || row
                            .remote_progress
                            .as_ref()
                            .is_some_and(|p| p.current_percent() == 100.0)),
            "loading cancellation must follow ordinary complete and real terminal100 from both sources"
        );
        let sim = view.simulation.context("completed loading world")?;
        ensure!(
            sim.session().binary_frame() == 0
                && sim.session().tick() == 0
                && sim.session().network_command_timing().is_none(),
            "preinstall cancellation advanced or installed gameplay"
        );
        let mut ready = super::loading_host_loss::loading_snapshot(
            self.config,
            state,
            &view,
            self.frame,
            self.uptime_ms,
            &fault.checkpoint_id,
        )?;
        ready["schema"] = json!("vera20k.loading-cancel-ready.v1");
        ready["kind"] = json!(fault.kind.as_str());
        ready["world_hash"] = json!(sim.state_hash());
        ready["rng_state"] = json!(sim.clone_scenario_rng().state());
        let path = sidecar(&fault.output, "ready");
        if let Some(first) = read_control(&path, 262144)? {
            ensure!(
                first["startup"] == ready["startup"]
                    && first["map_admission"] == ready["map_admission"]
                    && first["world_hash"] == ready["world_hash"]
                    && first["rng_state"] == ready["rng_state"],
                "held preinstall world, RNG or admission changed"
            );
        } else {
            write_atomic(&path, &ready)?;
        }
        Ok(true)
    }
    pub(crate) fn before_install(&self) -> Result<()> {
        let fault = self
            .config
            .loading_cancel
            .as_ref()
            .context("loading cancel configuration")?;
        ensure!(
            sidecar(&fault.output, "returned").try_exists()?,
            "fault attempt installed instead of retiring its result"
        );
        Ok(())
    }
}

pub(crate) enum CancelStep {
    Ordinary,
    Waiting,
    Recovered(Value),
}

/// Runs in the existing capture after-present input owner, before the ordinary
/// loader safe boundary. Only a real callback may retire the loading attempt.
pub(crate) fn cancellation_step(
    config: &LiveOnlineConfig,
    state: &mut AppState,
    presented: PresentedShell,
    lobby_settled: bool,
    frame: u32,
    trigger: &mut Option<Value>,
) -> Result<CancelStep> {
    let Some(fault) = config.loading_cancel.as_ref() else {
        return Ok(CancelStep::Ordinary);
    };
    if sidecar(&fault.output, "returned").try_exists()? {
        return Ok(CancelStep::Ordinary);
    }
    state
        .match_state
        .match_diagnostics
        .begin_loading_cancel_observation();
    let Some(ready) = read_control(&sidecar(&fault.output, "ready"), 262144)? else {
        return Ok(CancelStep::Ordinary);
    };
    let Some(control) = read_control(&sidecar(&fault.output, "control"), 8192)? else {
        ensure!(
            state.frontend.screen == GameScreen::Loading,
            "fault attempt left loading before its matching control"
        );
        return Ok(CancelStep::Waiting);
    };
    ensure!(
        control["schema"] == "vera20k.loading-cancel-control.v1"
            && control["phase"] == "loading-cancel"
            && control["checkpoint_id"] == fault.checkpoint_id
            && control["kind"] == fault.kind.as_str()
            && control["match_id"] == ready["startup"]["match_id"]
            && !control["match_id"].is_null()
            && control["request_after_launch_ms"].as_u64().is_some()
            && control["processes"]
                .as_array()
                .is_some_and(|rows| rows.len() == 2
                    && rows.iter().any(|row| row["account"] == config.account
                        && row["process_id"] == std::process::id()
                        && row["ready_render_frame"] == ready["render_frame"]))
            && control["processes"]
                .as_array()
                .is_some_and(
                    |processes| ready["startup"]["sources"].as_array().is_some_and(
                        |sources| sources.len() == processes.len()
                            && sources.iter().all(|source| processes
                                .iter()
                                .filter(|process| process["account"] == source["name"])
                                .count()
                                == 1)
                    )
                )
            && ready["process_id"] == std::process::id()
            && ready["account"] == config.account,
        "loading control does not bind this exact attempt/game process"
    );
    if state.frontend.screen == GameScreen::Loading {
        ensure!(
            crate::app::shell_wol::online_match_id(state) == ready["startup"]["match_id"].as_str()
                && state.match_state.live_network.is_none(),
            "fault lost current preinstall admission"
        );
        if trigger.is_none() {
            let host = config.account == config.host;
            let action = if fault.kind == LoadingCancelKind::Departure && host {
                crate::app::lan_service::close(state);
                "existing_transport_close_then_worker_leave"
            } else if fault.kind == LoadingCancelKind::Https {
                "ordinary_worker_leave"
            } else {
                "await_authenticated_departure"
            };
            if fault.kind == LoadingCancelKind::Https || host {
                crate::app::shell_wol::request(
                    state,
                    crate::app::shell_wol::WolServiceRequest::LeaveChannel,
                );
            }
            *trigger = Some(json!({"render_frame":frame,"action":action}));
        }
        return Ok(CancelStep::Waiting);
    }
    ensure!(
        state.match_state.live_network.is_none(),
        "canceled preinstall attempt reached gameplay"
    );
    if presented != PresentedShell::WolLobby
        || !lobby_settled
        || !crate::app::shell_wol::online_exit_ready(state)
    {
        return Ok(CancelStep::Waiting);
    }
    let visit = state
        .frontend
        .wol
        .as_ref()
        .context("authenticated WOL visit")?;
    let events = state
        .match_state
        .match_diagnostics
        .loading_cancel_observations()
        .context("loading cancellation journal")?;
    ensure!(
        events.len() < 16,
        "loading cancellation observation overflow"
    );
    let host_source = ready["startup"]["sources"]
        .as_array()
        .context("loading sources")?
        .iter()
        .find(|row| row["name"] == config.host)
        .context("original host source")?["source"]
        .as_u64();
    let expected = if fault.kind == LoadingCancelKind::Departure && config.account != config.host {
        "authenticated_departure"
    } else {
        "deployed_channel_left"
    };
    ensure!(
        events.iter().any(|event| event.event == expected
            && event.match_id.as_deref() == ready["startup"]["match_id"].as_str()
            && if expected == "deployed_channel_left" {
                event.worker_owner_taken
                    && (fault.kind == LoadingCancelKind::Departure || event.loading_active)
            } else {
                event.loading_active && event.source.map(|s| s as u64) == host_source
            }),
        "preinstall cancellation missed its real deployed/authenticated cause"
    );
    ensure!(
        visit.authenticated()
            && state.frontend.loading_session.is_none()
            && state.match_state.session.network_setup().is_none()
            && state.match_state.startup.accepted().is_none()
            && state.process_assets.is_available(),
        "cancellation did not return its one asset/startup lease"
    );
    let result = json!({"schema":"vera20k.loading-cancel-returned.v1","checkpoint_id":fault.checkpoint_id,
        "kind":fault.kind.as_str(),"account":config.account,"process_id":std::process::id(),
        "ready":ready,"control":control,"trigger":trigger,
        "events":events,"lobby_return":{"render_frame":frame,"authenticated":true,"presented":true,
        "loading_retired":true,"setup_retired":true,"startup_retired":true,"live_owner_retired":true,
        "assets_available":true,"finish_leave_acknowledged":true}});
    write_atomic(&sidecar(&fault.output, "returned"), &result)?;
    state
        .match_state
        .match_diagnostics
        .end_loading_cancel_observation();
    Ok(CancelStep::Recovered(result))
}

pub(crate) fn observe_progress(state: &mut AppState) {
    if let Some(service) = state.match_state.lan_service.as_mut() {
        service.observe_loading();
    }
}

pub(crate) fn progress_receipt(state: &AppState) -> Result<Option<Value>> {
    use crate::net::global::{GlobalDestination, LoadingObservation};
    let observed = state
        .match_state
        .live_network
        .as_ref()
        .and_then(|live| {
            let global = live.global();
            Some((
                global.loading_observation()?.clone(),
                global.authenticated_acknowledgments(),
            ))
        })
        .or_else(|| {
            state
                .match_state
                .lan_service
                .as_ref()?
                .loading_observation()
        });
    let Some((log, authenticated_acknowledgments)) = observed else {
        return Ok(None);
    };
    ensure!(
        !log.overflowed(),
        "loading progress observation exceeded its bounded log"
    );
    let destination = |value: &GlobalDestination| match value {
        GlobalDestination::Broadcast { .. } => "broadcast".to_owned(),
        GlobalDestination::Direct {
            endpoint,
            port_override,
        } => {
            let mut address = *endpoint;
            if *port_override != 0 {
                address.set_port(*port_override);
            }
            address.to_string()
        }
    };
    let events=log.records().iter().map(|event|match event {
        LoadingObservation::Admitted{sequence,destination:d,percent,reliable} => json!({"kind":"admitted","sequence":sequence,"destination":destination(d),"percent":percent,"reliable":reliable}),
        LoadingObservation::Retired{sequence,destination:d} => json!({"kind":"retired","sequence":sequence,"destination":destination(d)}),
        LoadingObservation::Applied{source,sequence,percent,displayed,applied} => json!({"kind":"applied","source":source,"sequence":sequence,"percent":percent,"displayed":displayed,"applied":applied}),
    }).collect::<Vec<_>>();
    Ok(Some(
        json!({"match_id":crate::app::shell_wol::online_match_id(state),
        "authenticated_acknowledgments":authenticated_acknowledgments,
        "overflowed":false,"events":events}),
    ))
}
