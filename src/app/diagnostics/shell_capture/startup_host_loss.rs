//! Opt-in observation of an original host killed after ordinary loading100,
//! before its private startup certificate is installed. Native loading and wait
//! decisions remain with LoadingSession and LiveCommandSession respectively.
//! Receipt `uptime_ms` is the native continuous system clock. Compare it only
//! within the same game process, never with controller launch-relative times or
//! another machine's clock.

use super::fault_files::{read_control, write_atomic};
use super::*;
use crate::app::loading::pump::NetworkLoadingObservation;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn sidecar(output: &Path, kind: &str) -> PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(format!(".startup.{kind}.json"));
    PathBuf::from(name)
}

/// Borrowed for one loading pump. The result stays in LoadingSession; this
/// observer retains only its completed diagnostic projection until install.
pub(crate) struct StartupLoadingObserver<'a> {
    config: &'a LiveOnlineConfig,
    frame: u32,
    uptime_ms: u128,
    completed: Option<Value>,
}

impl<'a> StartupLoadingObserver<'a> {
    pub(crate) fn new(config: &'a LiveOnlineConfig, frame: u32, uptime_ms: u128) -> Self {
        Self {
            config,
            frame,
            uptime_ms,
            completed: None,
        }
    }

    pub(crate) fn defer_install(
        &mut self,
        state: &AppState,
        view: NetworkLoadingObservation<'_>,
    ) -> Result<bool> {
        let config = self.config;
        let launch = view
            .startup
            .launch_session()
            .context("startup fault loading launch")?;
        let local = launch.local_player_index;
        ensure!(
            launch.humans.len() == 3
                && launch.humans[local].name == config.account
                && view.rows.local_player_index == local
                && view.rows.participants.len() == 3
                && view.stage == "ready_to_install"
                && view.timeout.is_none()
                && view.progress.current_percent() == 100.0
                && view.incomplete.is_empty()
                && view
                    .rows
                    .participants
                    .iter()
                    .enumerate()
                    .all(|(source, row)| {
                        source == local
                            || row
                                .remote_progress
                                .as_ref()
                                .is_some_and(|progress| progress.current_percent() == 100.0)
                    }),
            "startup checkpoint requires the ordinary complete original three-player loading result"
        );
        let sim = view.simulation.context("completed loading world")?;
        ensure!(
            sim.session().binary_frame() == 0
                && sim.session().tick() == 0
                && sim.session().network_command_timing().is_none(),
            "startup victim advanced or installed live timing before its handoff"
        );
        let checkpoint = config
            .startup_host_loss
            .as_deref()
            .context("startup checkpoint")?;
        let mut snapshot = super::loading_host_loss::loading_snapshot(
            config,
            state,
            &view,
            self.frame,
            self.uptime_ms,
            checkpoint,
        )?;
        let output = config.loading_output.as_ref().context("startup output")?;
        if config.account == config.host {
            let path = sidecar(output, "ready");
            if !path.try_exists()? {
                snapshot["schema"] = json!("vera20k.startup-host-ready.v1");
                write_atomic(&path, &snapshot)?;
            }
            // Diagnostic input gate only. The ordinary complete decision and
            // reliable100 publication already happened; no loader is re-run.
            return Ok(true);
        }
        self.completed = Some(snapshot);
        Ok(false)
    }

    pub(crate) fn before_install(&mut self) -> Result<()> {
        let mut snapshot = self
            .completed
            .take()
            .context("startup install missed normal complete")?;
        ensure!(
            self.config.account != self.config.host,
            "startup victim reached private installation"
        );
        snapshot["schema"] = json!("vera20k.startup-host-install.v1");
        write_atomic(
            &sidecar(
                self.config
                    .loading_output
                    .as_ref()
                    .context("startup output")?,
                "install",
            ),
            &snapshot,
        )
    }

    pub(crate) fn installed(&self, state: &AppState) -> Result<()> {
        ensure!(
            state.frontend.screen == GameScreen::InGame
                && state
                    .match_state
                    .live_network
                    .as_ref()
                    .is_some_and(|live| { live.startup_pending() && live.peers().len() == 2 }),
            "startup survivor did not install the full three-player private admission"
        );
        Ok(())
    }
}

pub(crate) fn read_killed(config: &LiveOnlineConfig, match_id: &Value) -> Result<Option<Value>> {
    let output = config.loading_output.as_ref().context("startup output")?;
    let Some(control) = read_control(&sidecar(output, "killed"), 4096)? else {
        return Ok(None);
    };
    let requested = control["request_after_launch_ms"]
        .as_u64()
        .context("startup kill request time")?;
    let started = control["kill_started_after_launch_ms"]
        .as_u64()
        .context("startup kill start time")?;
    let finished = control["kill_succeeded_after_launch_ms"]
        .as_u64()
        .context("startup kill completion time")?;
    ensure!(
        control["schema"] == "vera20k.startup-host-killed.v1"
            && control["phase"] == "startup-killed"
            && control["checkpoint_id"] == json!(config.startup_host_loss)
            && &control["match_id"] == match_id
            && !match_id.is_null()
            && control["victim_account"] == config.host
            && control["victim_process_id"]
                .as_u64()
                .is_some_and(|pid| pid > 0)
            && control["victim_ready_render_frame"].as_u64().is_some()
            && control["owned_kill_succeeded"] == true
            && requested <= started
            && started <= finished,
        "startup kill control belongs to another process/match/attempt or lacks acknowledged ownership"
    );
    Ok(Some(control))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartupDialogProcess {
    account: String,
    process_id: u32,
    render_frame: u32,
}

/// Input permission from the same external controller that owns the kill.
/// It certifies two observed dialogs, never a gameplay vote or wait decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartupVoteControl {
    schema: String,
    phase: String,
    checkpoint_id: String,
    match_id: String,
    account: String,
    process_id: u32,
    ready_render_frame: u32,
    all_dialogs_observed: bool,
    dialog_processes: Vec<StartupDialogProcess>,
}

impl StartupVoteControl {
    fn validate(&self, ready: &Value, survivors: &[&str]) -> Result<()> {
        let mut expected = survivors.to_vec();
        expected.sort_unstable();
        let mut actual: Vec<_> = self
            .dialog_processes
            .iter()
            .map(|proof| proof.account.as_str())
            .collect();
        actual.sort_unstable();
        ensure!(
            self.schema == "vera20k.startup-host-vote.v1"
                && self.phase == "startup-vote"
                && ready["schema"] == "vera20k.startup-host-dialog.v1"
                && ready["checkpoint_id"] == self.checkpoint_id
                && ready["startup"]["match_id"] == self.match_id
                && ready["account"] == self.account
                && ready["process_id"] == self.process_id
                && ready["render_frame"] == self.ready_render_frame
                && self.all_dialogs_observed
                && expected.len() == 2
                && expected[0] != expected[1]
                && self.dialog_processes.len() == 2
                && actual == expected
                && self
                    .dialog_processes
                    .iter()
                    .all(|proof| proof.process_id > 0)
                && self.dialog_processes.iter().any(|proof| {
                    proof.account == self.account
                        && proof.process_id == self.process_id
                        && proof.render_frame == self.ready_render_frame
                }),
            "startup vote control is not bound to both fresh survivor dialogs in this match"
        );
        Ok(())
    }
}

pub(crate) fn read_vote(
    config: &LiveOnlineConfig,
    ready: &Value,
    survivors: &[&str],
) -> Result<Option<StartupVoteControl>> {
    let output = config.loading_output.as_ref().context("startup output")?;
    let Some(control) = read_control(&sidecar(output, "vote"), 4096)? else {
        return Ok(None);
    };
    let control: StartupVoteControl = serde_json::from_value(control)?;
    control.validate(ready, survivors)?;
    Ok(Some(control))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog_grant() -> (Value, StartupVoteControl) {
        let ready = json!({"schema":"vera20k.startup-host-dialog.v1",
            "checkpoint_id":"0123456789abcdef0123456789abcdef",
            "startup":{"match_id":"same-live-match"},"account":"SurvivorA",
            "process_id":11,"render_frame":200});
        let grant = serde_json::from_value(json!({"schema":"vera20k.startup-host-vote.v1",
            "phase":"startup-vote","checkpoint_id":ready["checkpoint_id"],
            "match_id":ready["startup"]["match_id"],"account":"SurvivorA",
            "process_id":11,"ready_render_frame":200,"all_dialogs_observed":true,
            "dialog_processes":[{"account":"SurvivorB","process_id":22,"render_frame":240},
                {"account":"SurvivorA","process_id":11,"render_frame":200}]}))
        .unwrap();
        (ready, grant)
    }

    #[test]
    fn startup_vote_requires_both_distinct_survivor_dialogs() {
        let (ready, mut grant) = dialog_grant();
        let survivors = ["SurvivorA", "SurvivorB"];
        assert!(grant.validate(&ready, &survivors).is_ok());
        grant.dialog_processes.pop();
        assert!(grant.validate(&ready, &survivors).is_err());
        grant
            .dialog_processes
            .push(grant.dialog_processes[0].clone());
        assert!(grant.validate(&ready, &survivors).is_err());
    }

    #[test]
    fn startup_vote_rejects_an_earlier_dialog_process_or_match() {
        let (ready, grant) = dialog_grant();
        let survivors = ["SurvivorA", "SurvivorB"];
        for field in ["render_frame", "process_id"] {
            let mut other = ready.clone();
            other[field] = json!(999);
            assert!(grant.validate(&other, &survivors).is_err());
        }
        let mut other = ready.clone();
        other["startup"]["match_id"] = json!("another-live-match");
        assert!(grant.validate(&other, &survivors).is_err());
        let mut other = grant;
        other.dialog_processes[1].render_frame -= 1;
        assert!(other.validate(&ready, &survivors).is_err());
    }
}
