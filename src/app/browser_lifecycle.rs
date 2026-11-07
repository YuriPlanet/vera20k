//! Wasm host notifications at the existing scenario and application boundaries.
//! These are browser product semantics, not additional native gameplay rules.

use serde_json::json;
use winit::event_loop::{ActiveEventLoop, ControlFlow};

use super::cncnet::{ExternalLaunchReport, ExternalMatchResult};
use super::{App, AppState, Instant};

impl App {
    /// Snapshot before the existing teardown clears clocks, map identity and
    /// network membership. Reading the score snapshot spends no Scenario RNG.
    pub(super) fn browser_match_ended(state: &AppState, fallback: ExternalMatchResult) {
        if !crate::web::match_running() {
            return;
        }
        let sim = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation);
        let outcome = sim.and_then(|sim| {
            state
                .match_state
                .local_player_owner()
                .and_then(|owner| sim.interner().get(owner))
                .and_then(|owner| sim.ready_outcome_for_owner(owner))
        });
        let result = Self::match_result(state)
            .or_else(|| {
                outcome.map(|outcome| match outcome.kind {
                    crate::sim::house_state::HouseOutcomeKind::Victory => ExternalMatchResult::Won,
                    crate::sim::house_state::HouseOutcomeKind::Defeat => ExternalMatchResult::Lost,
                })
            })
            .unwrap_or(fallback);
        let mut report = if state.frontend.external_launch {
            Self::external_launch_report(Some(state), None)
        } else {
            let mut report = ExternalLaunchReport::new(Some(result), None, None);
            report.network_quality = state.match_state.network_quality.report();
            report
        };
        if !state.frontend.external_launch {
            report.frames = sim.map_or(0, |sim| sim.session().binary_frame());
            report.duration_seconds = f64::from(
                state
                    .match_state
                    .scenario_elapsed_clock
                    .elapsed_seconds(crate::app::match_runtime::frame_pacer::wall_clock_ms()),
            );
        }
        let mut report = serde_json::to_value(report).expect("launch report serializes");
        report["map"] = serde_json::to_value(&state.match_state.loaded_map_source)
            .expect("map source serializes");
        report["map_name"] = json!(state.match_state.map_basic.name);
        report["local_house"] = json!(state.match_state.local_player_owner());
        report["seed"] = json!(sim.map(|sim| sim.session().seed().to_string()));
        // Null means the native score owner did not finalize a score (e.g.
        // abort/co-op); never fabricate a winner bonus for those paths.
        report["scores"] =
            json!(
                sim.and_then(|sim| sim.terminal_score_snapshot().map(|snapshot| {
                    snapshot.rows.iter().map(|row| json!({
                "house": sim.interner().resolve(row.owner),
                "country": row.country.map(|country| sim.interner().resolve(country)),
                "survived": row.survived,
                "kills": row.kills, "losses": row.losses, "built": row.built,
                "raw_score": row.raw_score, "score": row.score,
            })).collect::<Vec<_>>()
                }))
            );
        crate::web::match_ended(report);
    }

    /// Stop accepting game input while the established online cleanup owners
    /// finish. Only then deliver the report and immediately exit winit.
    pub(super) fn poll_browser_exit(state: &mut AppState, event_loop: &ActiveEventLoop) -> bool {
        if !state.frontend.browser_exit_pending {
            return false;
        }
        let deadline = crate::app::shell_wol::poll(state);
        if crate::app::shell_wol::online_cleanup_pending(state)
            || state.match_state.lan_service.is_some()
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                deadline.unwrap_or_else(|| Instant::now() + std::time::Duration::from_millis(16)),
            ));
            return true;
        }
        let report = Self::external_launch_report(Some(state), None);
        crate::web::spawn_finished(&report.json());
        event_loop.exit();
        true
    }

    /// App::exiting is shared by Quit, the missing-assets card and startup
    /// failures. Drop browser resources before reporting engine-exited.
    pub(super) fn browser_exiting(&mut self) {
        let error = self
            .startup_failure
            .as_deref()
            .or_else(|| {
                self.state.as_ref().and_then(|state| {
                    state
                        .frontend
                        .external_launch_error
                        .as_deref()
                        .or(state.frontend.main_menu_shell_error.as_deref())
                })
            })
            .map(str::to_owned);
        crate::web::request_exit(error.as_deref());
        if let Some(state) = self.state.as_mut() {
            Self::browser_match_ended(state, ExternalMatchResult::Quit);
            // A direct window/error exit may bypass external-launch cleanup.
            // Deliver any retained report before the terminal notification.
            if state.frontend.external_launch {
                let report = Self::external_launch_report(Some(state), error.as_deref());
                crate::web::spawn_finished(&report.json());
            }
            state.audio.stop_theme();
            if let Some(sfx) = state.audio.sfx_player.as_mut() {
                sfx.stop_all();
            }
            let _ = state
                .platform
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::None);
            state.platform.window.set_cursor_visible(true);
        }
        self.pending_initialization = None;
        let started = self.state.take().is_some();
        crate::web::engine_exited();
        // The host's existing "Exited" status follows a started engine only;
        // a failed startup keeps its "Startup failed" status on the page.
        if started {
            crate::web::status("Exited");
        }
    }
}
