//! VERA20k — client library root.
//!
//! Re-exports all modules so the game and the tool binaries in `src/bin/`
//! can access the engine's functionality. The binary entry point (main.rs)
//! delegates to this library for all logic.
//!
//! The GPU-free engine (assets, rules, map, sim, net, util and the headless
//! scenario) is the `vera20k-core` crate (`src/core_lib.rs`). Its modules are
//! re-exported here, so `crate::sim::...` and `vera20k::sim::...` keep naming
//! them.
pub use vera20k_core::{
    assets, headless_scenario, map, net, rules, sim, skirmish_launch, skirmish_modes, util,
};

// Cargo unifies dependency features across a workspace. Refuse a client built
// without its Ares consumers when another package enables the core capability.
const _: () = assert!(
    cfg!(feature = "ares") == rules::ARES_ENABLED,
    "enable Ares through the vera20k package's ares feature"
);
// The same boundary for Phobos: the capability is independent of Ares and is
// likewise enabled through the client package.
const _: () = assert!(
    cfg!(feature = "phobos") == rules::PHOBOS_ENABLED,
    "enable Phobos through the vera20k package's phobos feature"
);
// The same boundary for the NPatch placeholder.
const _: () = assert!(
    cfg!(feature = "npatch") == rules::NPATCH_ENABLED,
    "enable NPatch through the vera20k package's npatch feature"
);
// The same boundary for VERA20K's own VeraX extension capability.
const _: () = assert!(
    cfg!(feature = "verax") == rules::VERAX_ENABLED,
    "enable VeraX through the vera20k package's verax feature"
);

// Opaque process-RNG cursor receipt shared by map generation and simulation.
use vera20k_core::rng_continuation;

// Headless asset browsing for automated callers — the `asset` binary.
// Sits above assets/ and rules/; never depends on sim/, ui/, audio/, or net/.
pub mod asset_tools;

// GPU rendering — wgpu-based sprite, terrain, voxel rendering.
// Reads from sim/ state but never mutates it.
pub mod render;

// Menus, dialogs and the in-game sidebar: layout and state, drawn by app/ and render/.
pub mod ui;

// Sound/music via rodio.
pub mod audio;

// The app orchestrator. Public for testing but not intended for direct use.
pub mod app;

// Source-level dependency guards for the domain-boundaries ledger.
#[cfg(test)]
mod architecture_guards;

// Fixture files that library tests read from the checkout.
#[cfg(test)]
use vera20k_core::test_fixture;
