//! Explicitly enabled offline debug controls, adapted from @1372704087's
//! trainer in PR #2083. This module is absent without `debug-tools`.
//!
//! The network thread only queues a bounded request. The game thread checks
//! the current world/session again before reaching the simulation's owner.

use std::collections::VecDeque;

use crate::sim::cheats::{CheatFlags, TrainerAction};

const MAX_PENDING: usize = 32;
pub(crate) const MAX_REQUEST_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DebugCommand {
    Toggle { switch: DebugSwitch },
    CycleSpeedType { forward: bool },
    FrameStep {},
    SetCheats { flags: CheatFlags },
    CheatAction { action: TrainerAction },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DebugSwitch {
    TerrainCosts,
    Heightmap,
    CellGrid,
    FootPath,
    UnitInspector,
    DebugPause,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    session: u64,
    command: DebugCommand,
}

#[derive(Default)]
pub(crate) struct CommandInbox {
    session: Option<u64>,
    pending: VecDeque<DebugCommand>,
}

impl CommandInbox {
    pub(crate) fn set_session(&mut self, session: Option<u64>) {
        if session != self.session {
            self.pending.clear();
            self.session = session;
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<(), ()> {
        if bytes.len() > MAX_REQUEST_BYTES || self.pending.len() == MAX_PENDING {
            return Err(());
        }
        let request: Request = serde_json::from_slice(bytes).map_err(|_| ())?;
        if self.session != Some(request.session) || request.session == 0 {
            return Err(());
        }
        if let DebugCommand::SetCheats { flags } = request.command
            && (!(0..=1_000_000_000).contains(&flags.money_target) || flags.damage_scale > 100)
        {
            return Err(());
        }
        self.pending.push_back(request.command);
        Ok(())
    }

    pub(crate) fn drain(&mut self, session: u64) -> Vec<DebugCommand> {
        if self.session != Some(session) {
            self.pending.clear();
            return Vec::new();
        }
        self.pending.drain(..).collect()
    }
}

/// A replaced world or a lost viewer invalidates every outstanding request.
pub(crate) fn reset_session(state: &mut crate::app::AppState) {
    state.diag.debug_session_epoch = state
        .diag
        .debug_session_epoch
        .checked_add(1)
        .filter(|epoch| *epoch <= 9_007_199_254_740_991)
        .unwrap_or(0);
    if let Some(server) = state.diag.telemetry_server.as_ref() {
        server.set_debug_session(None);
    }
    if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
        runtime.simulation.clear_debug_tools();
    }
}

fn context(
    mode: crate::app::match_runtime::session::SessionMode,
    active: bool,
) -> crate::sim::cheats::DebugSessionContext {
    use crate::app::match_runtime::session::SessionMode;
    use crate::sim::cheats::DebugSessionContext;
    if !active {
        return DebugSessionContext::Disabled;
    }
    match mode {
        SessionMode::Campaign | SessionMode::Skirmish => DebugSessionContext::Offline,
        SessionMode::Lan | SessionMode::Wol | SessionMode::Other => DebugSessionContext::Network,
    }
}

/// Runs outside tick admission so a paused game can answer a control request.
pub(crate) fn service(state: &mut crate::app::AppState) {
    let active = state.diag.telemetry_active()
        && state.frontend.screen == crate::ui::game_screen::GameScreen::InGame
        && state.match_state.scenario_exit.is_none()
        && state.match_state.scenario_outcome.is_none();
    let context = context(state.match_state.session_mode(), active);
    let allowed = if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
        runtime.simulation.set_debug_session_context(context);
        let logging = runtime.simulation.debug_event_logging();
        runtime.simulation.set_debug_event_logging(logging);
        runtime.simulation.trainer_available()
    } else {
        false
    };
    let epoch = state.diag.debug_session_epoch;
    let commands = if let Some(server) = state.diag.telemetry_server.as_ref() {
        server.set_debug_session((allowed && epoch != 0).then_some(epoch));
        if allowed {
            server.drain_debug_commands(epoch)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    for command in commands {
        apply(state, command);
    }
    if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
        state.match_state.sandbox_full_visibility = runtime.simulation.debug_reveal_map();
    }
}

fn close_unmodified_recording(state: &mut crate::app::AppState) {
    if state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| !runtime.simulation.trainer_modified())
    {
        crate::app::match_runtime::sim_tick::close_replay_segment_for_new_timeline(state);
    }
}

fn set_flags(state: &mut crate::app::AppState, flags: CheatFlags) {
    if !state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.trainer_available())
    {
        return;
    }
    close_unmodified_recording(state);
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    if let Err(error) = runtime.simulation.set_cheat_flags(flags) {
        log::warn!("Debug trainer request rejected: {error}");
        return;
    }
    runtime
        .simulation
        .apply_debug_tools(Some(&runtime.resources.rules));
    state.match_state.sandbox_full_visibility = runtime.simulation.debug_reveal_map();
}

pub(crate) fn toggle_reveal(state: &mut crate::app::AppState) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let mut flags = runtime.simulation.cheat_flags();
    flags.reveal_map = !flags.reveal_map;
    set_flags(state, flags);
}

pub(crate) fn toggle_inspector(state: &mut crate::app::AppState) {
    if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
        let enabled = !runtime.simulation.debug_event_logging();
        runtime.simulation.set_debug_event_logging(enabled);
    }
}

fn apply(state: &mut crate::app::AppState, command: DebugCommand) {
    use crate::app::input::dispatch;
    match command {
        DebugCommand::SetCheats { flags } => set_flags(state, flags),
        DebugCommand::CheatAction { action } => {
            close_unmodified_recording(state);
            if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
                match runtime.simulation.request_trainer_action(action) {
                    Ok(()) => runtime
                        .simulation
                        .apply_debug_tools(Some(&runtime.resources.rules)),
                    Err(error) => log::warn!("Debug trainer action rejected: {error}"),
                }
            }
        }
        DebugCommand::Toggle { switch } => match switch {
            DebugSwitch::TerrainCosts => dispatch::toggle_pathgrid_overlay(state),
            DebugSwitch::Heightmap => dispatch::toggle_heightmap_overlay(state),
            DebugSwitch::CellGrid => dispatch::toggle_cell_grid_overlay(state),
            DebugSwitch::FootPath => dispatch::toggle_foot_path_overlay(state),
            DebugSwitch::DebugPause => dispatch::toggle_debug_pause(state),
            DebugSwitch::UnitInspector => toggle_inspector(state),
        },
        DebugCommand::CycleSpeedType { forward } => {
            dispatch::cycle_terrain_cost_speed_type(state, forward);
        }
        DebugCommand::FrameStep {} => dispatch::request_debug_frame_step(state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: &[u8] = br#"{"session":7,"command":{"cmd":"frame_step"}}"#;

    #[test]
    fn only_current_enabled_world_can_queue_commands() {
        let mut inbox = CommandInbox::default();
        assert!(inbox.push(STEP).is_err());
        inbox.set_session(Some(7));
        inbox.push(STEP).unwrap();
        inbox.set_session(Some(8));
        assert!(
            inbox.drain(8).is_empty(),
            "old world commands were discarded"
        );
        assert!(inbox.push(STEP).is_err());
        inbox.set_session(Some(7));
        inbox.push(STEP).unwrap();
        inbox.set_session(None);
        assert!(inbox.drain(7).is_empty());
    }

    #[test]
    fn request_schema_and_queue_are_bounded() {
        let mut inbox = CommandInbox::default();
        inbox.set_session(Some(7));
        for bad in [
            br#"{"session":7,"command":{"cmd":"shell","command":"anything"}}"#.as_slice(),
            br#"{"session":7,"command":{"cmd":"frame_step","extra":true}}"#,
            br#"{"session":7,"command":{"cmd":"frame_step"},"extra":true}"#,
            br#"{"session":7,"command":{"cmd":"set_cheats","flags":{"damage_scale":101}}}"#,
            br#"{"session":7,"command":{"cmd":"set_cheats","flags":{"money_target":-1}}}"#,
        ] {
            assert!(
                inbox.push(bad).is_err(),
                "accepted {:?}",
                std::str::from_utf8(bad)
            );
        }
        assert!(inbox.push(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
        for _ in 0..MAX_PENDING {
            inbox.push(STEP).unwrap();
        }
        assert!(inbox.push(STEP).is_err());
        assert_eq!(inbox.drain(7).len(), MAX_PENDING);
        inbox.push(STEP).unwrap();
        assert_eq!(inbox.drain(7), vec![DebugCommand::FrameStep {}]);
    }

    #[test]
    fn only_known_offline_modes_admit_tools() {
        use crate::app::match_runtime::session::SessionMode;
        use crate::sim::cheats::DebugSessionContext;
        for mode in [SessionMode::Campaign, SessionMode::Skirmish] {
            assert_eq!(context(mode, true), DebugSessionContext::Offline);
            assert_eq!(context(mode, false), DebugSessionContext::Disabled);
        }
        for mode in [SessionMode::Lan, SessionMode::Wol, SessionMode::Other] {
            assert_eq!(context(mode, true), DebugSessionContext::Network);
        }
    }
}
