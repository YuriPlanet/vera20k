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

#[cfg(feature = "lua")]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod engine;
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
