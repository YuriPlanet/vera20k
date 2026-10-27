//! VERA20k core — the GPU-free engine crate (`vera20k-core`).
//!
//! Asset parsing, rules, maps, the simulation, lockstep and headless scenario
//! loading. It never depends on rendering, windowing, audio playback or the
//! app; the client crate (`src/lib.rs`) re-exports these modules, so
//! `crate::sim::...` paths read the same in both crates.

// The client's tests build this crate with `test-support` but without
// cfg(test), so test-only code there lacks its in-crate test callers; the
// normal and cfg(test) builds keep the dead-code and unused-import lints.
#![cfg_attr(
    all(feature = "test-support", not(test)),
    allow(dead_code, unused_imports)
)]

#[cfg(all(target_arch = "wasm32", target_os = "unknown", not(feature = "wasm")))]
compile_error!("browser builds require --features wasm");

// Asset parsers — .mix, .shp, .vxl, .pal, .tmp, .hva, .csf, .aud
// No dependencies on game modules. Standalone parser library.
pub mod assets;

// Game data from rules.ini and art.ini.
// Defines every unit type, building, weapon, warhead.
pub mod rules;

// Opaque process-RNG cursor receipt shared by map generation and simulation.
// Neither owner depends on the other's implementation through this DTO.
pub mod rng_continuation;

// Game simulation — EntityStore, fixed-point math, deterministic logic.
// NEVER depends on render/, ui/, audio/, net/.
pub mod sim;

// Map loading, terrain tiles, theater system.
pub mod map;

// Multiplayer — deterministic lockstep command transport.
pub mod net;

// Shared utilities — config, fixed-point math, color helpers.
pub mod util;

/// Headless retail-scenario loading for parity runs (no GPU, no window).
pub mod headless_scenario;

// App-level skirmish startup contract shared by the menu shell and map loader.
pub mod skirmish_launch;
pub mod skirmish_modes;

// Fixture files that library tests read from the checkout; client tests reach
// them through the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
pub mod test_fixture;
