//! RA2 Engine — library root.
//!
//! Re-exports all modules so integration tests and future binary targets
//! can access the engine's functionality. The binary entry point (main.rs)
//! delegates to this library for all logic.

// Asset parsers — .mix, .shp, .vxl, .pal, .tmp, .hva, .csf, .aud
// No dependencies on game modules. Standalone parser library.
pub mod assets;

// Headless asset browsing for automated callers — the `asset` binary.
// Sits above assets/ and rules/; never depends on sim/, ui/, audio/, or net/.
pub mod asset_tools;

// GPU rendering — wgpu-based sprite, terrain, voxel rendering.
// Reads from sim/ state but never mutates it.
pub mod render;

// Game data from rules.ini and art.ini.
// Defines every unit type, building, weapon, warhead.
pub mod rules;

// Opaque process-RNG cursor receipt shared by map generation and simulation.
// Neither owner depends on the other's implementation through this DTO.
pub(crate) mod rng_continuation;

// Game simulation — EntityStore, fixed-point math, deterministic logic.
// NEVER depends on render/, ui/, sidebar/, audio/, net/.
pub mod sim;

// egui menus and dialogs (NOT the in-game sidebar).
pub mod ui;

// Custom wgpu sidebar — pixel-perfect RA2 art, not egui.
pub mod sidebar;

// Sound/music via rodio.
pub mod audio;

// Map loading, terrain tiles, theater system.
pub mod map;

// Multiplayer — deterministic lockstep command transport.
pub mod net;

// Shared utilities — config, fixed-point math, color helpers.
pub mod util;

// The app orchestrator. Public for testing but not intended for direct use.
pub mod app;

/// Headless retail-scenario loading for parity runs (no GPU, no window).
pub mod headless_scenario;
pub mod match_bootstrap;

// App-level skirmish startup contract shared by the menu shell and map loader.
pub mod skirmish_cooperative;
pub mod skirmish_launch;
pub mod skirmish_modes;
pub mod skirmish_persistence;

// Source-level dependency guards for the domain-boundaries ledger.
#[cfg(test)]
mod architecture_guards;

// Fixture files that library tests read from the checkout.
#[cfg(test)]
mod test_fixture;
