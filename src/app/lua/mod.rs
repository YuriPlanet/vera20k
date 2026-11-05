//! Lua mod host: the WolfCTOS-compatible scripting layer.
//!
//! WolfCTOS ships a Lua scripting layer for Yuri's Revenge: `scripts/mods/<id>/main.lua`
//! returns a table whose `Update(frame)` the engine calls once per logical frame,
//! and engine events arrive as global functions (`OnScenarioStart`, ...). This
//! module reproduces that contract on top of VERA's own `SimRuntime`, so the
//! same Lua mods run without source changes.
//!
//! Layering: Lua lives in the app layer only. The simulation never links
//! `mlua`; scripts observe a read-only snapshot and submit `Command` envelopes
//! through the same admission path as player input
//! (`Simulation::issue_local_command`), so mod orders stay inside the
//! deterministic command stream.
//!
//! The `lua` cargo feature is opt-in and desktop-only. Without it (and on wasm)
//! [`LuaHost`] is an inert placeholder and the game behaves exactly as before.

use std::path::PathBuf;

/// Where the host looks for scripts, mirroring WolfCTOS's `scripts/` tree.
///
/// `game_root` is the retail installation (the folder holding `ra2.mix` and the
/// modder's `mods/` and `scripts/`); `settings_root` is VERA's writable
/// profile, which may carry user-installed mods. Both are optional because the
/// launcher can start before a game folder is configured.
#[derive(Debug, Default, Clone)]
#[cfg_attr(not(feature = "lua"), allow(dead_code))]
pub(crate) struct LuaPaths {
    pub(crate) game_root: Option<PathBuf>,
    pub(crate) settings_root: Option<PathBuf>,
}

/// The read-only World / House API that mods query each frame.
/// Installed once at startup via `api::install`; per-frame state flows
/// through `api::set_current` / `api::clear_current` around the dispatch.
#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod api;
/// The read-only per-frame snapshot the app builds and the API closures read.
/// Not feature-gated: the app builds it for the desktop host and the inert
/// host's `refresh_snapshot` signature stays identical without the feature.
/// Its readers beyond `build()` are Lua-only, hence the default-build allow.
#[cfg_attr(not(feature = "lua"), allow(dead_code))]
pub(crate) mod snapshot;
/// Lua userdata for technos and houses, plus the command queue mods fill.
#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod userdata;
#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod engine;
#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod loader;
#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[cfg(test)]
mod tests;
#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use engine::LuaHost;

#[cfg(not(feature = "lua"))]
mod inert;
#[cfg(not(feature = "lua"))]
pub(crate) use inert::LuaHost;

#[cfg(all(feature = "lua", target_arch = "wasm32", target_os = "unknown"))]
mod inert;
#[cfg(all(feature = "lua", target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use inert::LuaHost;
