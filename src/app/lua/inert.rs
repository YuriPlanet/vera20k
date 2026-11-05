//! The [`LuaHost`] of builds that carry no Lua VM.
//!
//! Builds without the `lua` cargo feature, and every wasm build, keep this
//! zero-sized placeholder so the app owns a host unconditionally. It has no
//! state and never touches the simulation, so those builds behave exactly as
//! they did before the Lua layer existed.

use super::LuaPaths;

/// Inert stand-in for the desktop Lua host.
pub(crate) struct LuaHost;

impl LuaHost {
    pub(crate) fn new(_paths: LuaPaths) -> Self {
        Self
    }

    /// Committed-frame hook: nothing to dispatch.
    pub(crate) fn on_frame(&mut self, _frame: u64) {}

    /// Scenario-install hook: no scripts can listen.
    pub(crate) fn on_scenario_start(&mut self) {}

    /// Snapshot refresh from the app: nothing to store in this build. Kept
    /// for surface parity with the desktop host, whose call sites in the app
    /// are not feature-gated.
    pub(crate) fn refresh_snapshot(
        &mut self,
        _snapshot: crate::app::lua::snapshot::FrameSnapshot,
    ) {
    }

    /// HUD lines queued by scripts: none in this build.
    pub(crate) fn take_messages(&mut self) -> Vec<String> {
        Vec::new()
    }
}
