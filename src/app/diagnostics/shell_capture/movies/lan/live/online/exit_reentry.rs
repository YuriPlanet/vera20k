//! Opt-in external HTTPS outage receipt within LiveRoundFlow. This observer
//! never sends a lobby command, changes a timer or replaces a cleanup owner.

use super::*;
use crate::app::diagnostics::shell_capture::fault_files::{read_control, write_atomic};
use std::path::{Path, PathBuf};

pub(super) struct ExitReentry {
    pub(super) started: Instant,
    ready: Value,
    blocked: Option<Value>,
    failure: Option<Value>,
    restored: Option<Value>,
    first_menu: Option<Value>,
    lobby: Option<Value>,
    final_menu: Option<Value>,
}

fn uptime_ms() -> u64 {
    crate::app::match_runtime::frame_pacer::wall_clock_ms()
}

fn sidecar(path: &Path, kind: &str) -> PathBuf {
    path.with_extension(format!("{kind}.json"))
}

fn matches_control(control: &Value, phase: &str, ready: &Value) -> bool {
    control["schema"] == "vera20k.online-exit-control.v1"
        && control["phase"] == phase
        && control["checkpoint_id"] == ready["checkpoint_id"]
        && control["process_id"] == ready["process_id"]
        && control["ready_render_frame"] == ready["render_frame"]
}

impl ExitReentry {
    pub(super) fn receipt(&self) -> Value {
        json!({"schema":"vera20k.online-exit-reentry.v1", "ready":self.ready,
            "blocked":self.blocked,"failure":self.failure,"restored":self.restored,
            "first_menu":self.first_menu,"lobby":self.lobby,"final_menu":self.final_menu})
    }
}

impl LiveRoundFlow {
    pub(super) fn reentering(&self) -> bool {
        self.exit_reentry
            .as_ref()
            .is_some_and(|exit| exit.first_menu.is_some())
    }

    pub(super) fn begin_exit_checkpoint(&mut self, state: &AppState, frame: u32) -> Result<()> {
        ensure!(
            self.rounds.len() == 2 && self.exit_reentry.is_none(),
            "exit fault must follow both completed rounds"
        );
        ensure!(
            crate::app::shell_wol::online_exit_ready(state),
            "previous match cleanup has not acknowledged completion"
        );
        let config = self.online_config();
        let path = config
            .exit_prefix_output
            .as_ref()
            .context("exit checkpoint path")?;
        for kind in ["blocked", "failed", "restored"] {
            ensure!(
                !sidecar(path, kind).try_exists()?,
                "stale online exit control exists"
            );
        }
        let ready = json!({"schema":"vera20k.online-exit-ready.v1",
            "checkpoint_id":config.exit_checkpoint.as_ref().context("exit checkpoint nonce")?,
            "process_id":std::process::id(),"account":config.account,
            "render_frame":frame,"uptime_ms":uptime_ms(),"completed_rounds":2,
            "authenticated_lobby":true,"cleanup_idle":true});
        write_atomic(path, &ready)?;
        self.exit_reentry = Some(ExitReentry {
            started: Instant::now(),
            ready,
            blocked: None,
            failure: None,
            restored: None,
            first_menu: None,
            lobby: None,
            final_menu: None,
        });
        self.record(
            frame,
            "Both complete rounds returned; fresh HTTPS exit fault checkpoint saved",
        );
        Ok(())
    }

    pub(super) fn exit_block_applied(
        &mut self,
        state: &AppState,
        rendered: PresentedShell,
        frame: u32,
    ) -> Result<bool> {
        let path = self
            .online_config()
            .exit_prefix_output
            .clone()
            .context("exit checkpoint path")?;
        let exit = self.exit_reentry.as_mut().context("exit checkpoint")?;
        let Some(control) = read_control(&sidecar(&path, "blocked"), 4096)? else {
            return Ok(false);
        };
        let blocked = control["blocked_uptime_ms"]
            .as_u64()
            .context("scoped block time")?;
        let now = uptime_ms();
        ensure!(
            matches_control(&control, "blocked", &exit.ready)
                && blocked
                    >= exit.ready["uptime_ms"]
                        .as_u64()
                        .context("exit Ready time")?
                && now >= blocked
                && now - blocked <= 15_000
                && u64::from(frame)
                    > exit.ready["render_frame"]
                        .as_u64()
                        .context("exit Ready frame")?,
            "HTTPS block does not acknowledge this fresh process/checkpoint"
        );
        ensure!(
            lobby_ready(state, rendered) && crate::app::shell_wol::online_exit_ready(state),
            "online admission changed before the external exit fault"
        );
        exit.blocked = Some(control);
        Ok(true)
    }

    /// Observe the actual retained RetryingCommand failure, then its matching
    /// acknowledgement/closed response channel. Worker retries continue normally.
    pub(super) fn observe_exit_cleanup(&mut self, state: &AppState, frame: u32) -> Result<bool> {
        let Some(exit) = self.exit_reentry.as_mut() else {
            return Ok(true);
        };
        if exit.first_menu.is_some() {
            return Ok(true);
        }
        let observation = state.frontend.online_cleanup.logout_observation();
        let blocked = exit.blocked.as_ref().context("acknowledged HTTPS block")?;
        let path = self
            .config
            .as_ref()
            .and_then(|config| config.exit_prefix_output.as_ref())
            .context("exit checkpoint path")?;
        if exit.failure.is_none()
            && observation.pending
            && observation.failures > 0
            && !observation.acknowledged
        {
            ensure!(
                observation.request_id.is_some(),
                "logout failure lost its retained request"
            );
            let failure = json!({"schema":"vera20k.online-exit-failed.v1",
                "checkpoint_id":exit.ready["checkpoint_id"],"process_id":std::process::id(),
                "ready_render_frame":exit.ready["render_frame"],"render_frame":frame,
                "uptime_ms":uptime_ms(),"logout":observation});
            write_atomic(&sidecar(path, "failed"), &failure)?;
            exit.failure = Some(failure);
        }
        let Some(failure) = &exit.failure else {
            ensure!(
                !observation.acknowledged && observation.pending,
                "HTTPS exit completed without an observed retained command failure"
            );
            return Ok(false);
        };
        ensure!(
            observation.request_id == failure["logout"]["request_id"].as_u64(),
            "retained logout identity changed after the actual failure"
        );
        if exit.restored.is_none() {
            let Some(control) = read_control(&sidecar(path, "restored"), 4096)? else {
                return Ok(false);
            };
            let restored = control["restored_uptime_ms"]
                .as_u64()
                .context("HTTPS restore time")?;
            ensure!(
                matches_control(&control, "restored", &exit.ready)
                    && control["logout_request_id"] == failure["logout"]["request_id"]
                    && control["failure_render_frame"] == failure["render_frame"]
                    && restored
                        >= failure["uptime_ms"]
                            .as_u64()
                            .context("logout failure time")?
                    && restored
                        >= blocked["blocked_uptime_ms"]
                            .as_u64()
                            .context("HTTPS block time")?
                    && restored
                        - blocked["blocked_uptime_ms"]
                            .as_u64()
                            .context("HTTPS block time")?
                        <= 45_000
                    && restored <= uptime_ms()
                    && control["blocked_packets"]
                        .as_u64()
                        .is_some_and(|count| count > 0),
                "HTTPS restoration was not tied to this actual failed logout"
            );
            exit.restored = Some(control);
        }
        if !observation.acknowledged || !observation.worker_closed {
            return Ok(false);
        }
        let restored = exit.restored.as_ref().unwrap()["restored_uptime_ms"]
            .as_u64()
            .context("HTTPS restoration time")?;
        ensure!(
            observation
                .acknowledged_uptime_ms
                .is_some_and(|time| time >= restored),
            "logout acknowledgement predates actual HTTPS restoration"
        );
        Ok(true)
    }

    /// Return false only when the first settled menu begins the second normal
    /// entry. The completed rounds/first-menu receipt remain immutable evidence.
    pub(super) fn observe_exit_menu(&mut self, state: &AppState, frame: u32) -> Result<bool> {
        let Some(exit) = self.exit_reentry.as_mut() else {
            return Ok(true);
        };
        let observation = state.frontend.online_cleanup.logout_observation();
        ensure!(
            observation.acknowledged && observation.worker_closed && !observation.pending,
            "online exit did not acknowledge logout and close its worker"
        );
        let menu = json!({"process_id":std::process::id(),"render_frame":frame,"uptime_ms":uptime_ms(),
            "menu":self.menu_return.as_ref().context("settled menu receipt")?,"logout":observation});
        if exit.first_menu.is_none() {
            ensure!(
                exit.restored.is_some() && exit.failure.is_some(),
                "exit fault restoration was not observed"
            );
            ensure!(
                observation.request_id
                    == exit.failure.as_ref().unwrap()["logout"]["request_id"].as_u64(),
                "first menu acknowledged a different logout"
            );
            exit.first_menu = Some(menu);
            self.record(frame, "Same failed logout acknowledged and worker closed; normal same-process online reentry begins");
            return Ok(false);
        }
        let lobby = exit
            .lobby
            .as_ref()
            .context("actual second authenticated lobby")?;
        let lobby_time = lobby["uptime_ms"].as_u64().context("second lobby time")?;
        ensure!(
            observation
                .acknowledged_uptime_ms
                .is_some_and(|time| time >= lobby_time),
            "final logout reused the former visit's acknowledgement"
        );
        exit.final_menu = Some(menu);
        Ok(true)
    }

    pub(super) fn observe_reentry_lobby(&mut self, state: &mut AppState, frame: u32) -> Result<()> {
        if !crate::app::shell_wol::online_exit_ready(state) {
            return Ok(());
        }
        let account = self.online_config().account.clone();
        ensure!(
            state.frontend.wol_account.nickname == account
                && state.match_state.session.network_setup().is_none()
                && state.match_state.live_network.is_none(),
            "second online admission resurrected old setup state"
        );
        let exit = self
            .exit_reentry
            .as_mut()
            .context("second online admission")?;
        ensure!(
            exit.first_menu.is_some() && exit.lobby.is_none(),
            "duplicate online reentry admission"
        );
        exit.lobby = Some(
            json!({"process_id":std::process::id(),"account":account,"render_frame":frame,
            "uptime_ms":uptime_ms(),"interactive":true,"normal_login":true,
            "old_worker_closed":true,"old_setup_retired":true}),
        );
        let rect = crate::app::shell_wol::lobby::layout(state)
            .page
            .button_rect(wol_lobby::BACK)
            .context("second lobby Back")?;
        click(state, rect);
        self.record(
            frame,
            "Second actual authenticated lobby admitted; ordinary Back clicked for final logout",
        );
        self.phase = RoundPhase::ExitWelcome;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_controls_bind_the_public_nonce_process_and_exact_ready_frame() {
        let ready = json!({"checkpoint_id":"0123456789abcdef0123456789abcdef", "process_id":42,"render_frame":100});
        let mut control = json!({"schema":"vera20k.online-exit-control.v1", "phase":"blocked",
            "checkpoint_id":ready["checkpoint_id"],"process_id":42,"ready_render_frame":100});
        assert!(matches_control(&control, "blocked", &ready));
        control["process_id"] = json!(43);
        assert!(!matches_control(&control, "blocked", &ready));
        control["process_id"] = json!(42);
        control["ready_render_frame"] = json!(99);
        assert!(!matches_control(&control, "blocked", &ready));
        control["ready_render_frame"] = json!(100);
        assert!(!matches_control(&control, "restored", &ready));
    }
}
