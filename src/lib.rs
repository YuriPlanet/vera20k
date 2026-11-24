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
// A focused test build leaves out the other test groups, and with them the
// callers of test-only helpers outside the group (tools/README.md, "Test groups").
#![cfg_attr(
    test_focus,
    allow(
        dead_code,
        unused_imports,
        reason = "a focused test build compiles only its group's tests"
    )
)]

#[cfg(all(target_arch = "wasm32", target_os = "unknown", not(feature = "wasm")))]
compile_error!("browser builds require --features wasm");

pub use vera20k_core::{
    assets, headless_scenario, map, net, rules, sim, skirmish_launch, skirmish_modes, util,
};

// Cargo unifies dependency features across a workspace. Refuse a client built
// without its Phobos consumers when another package enables the core
// capability; the capability is enabled through the client package.
const _: () = assert!(
    cfg!(feature = "phobos") == rules::PHOBOS_ENABLED,
    "enable Phobos through the vera20k package's phobos feature"
);
// VERA20K's own VeraX capability; NPatch needs no boundary, every build
// carries its switch.
const _: () = assert!(
    cfg!(feature = "verax") == rules::VERAX_ENABLED,
    "enable VeraX through the vera20k package's verax feature"
);
// The same boundary for the NPBUG bug-fix capability.
const _: () = assert!(
    cfg!(feature = "npbug") == rules::NPBUG_ENABLED,
    "enable NPBUG through the vera20k package's npbug feature"
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

// VERA's own localized text, layered over the retail CSF (`[VERA] Language=`).
pub mod i18n;

// Sound/music via rodio.
pub mod audio;

// The app orchestrator. Public for testing but not intended for direct use.
pub mod app;

// Process start-up shared by the desktop binary and the Android cdylib, which
// has no `main` of its own. The browser starts in `web` instead, and its build
// has neither the system dialogs nor the spawn-launch result this module uses.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod entry;

// Source-level dependency guards for the domain-boundaries ledger.
#[cfg(all(test, tests_root))]
mod architecture_guards;

// Fixture files that library tests read from the checkout.
#[cfg(test)]
use vera20k_core::test_fixture;

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub mod web;

// Browser protocol tests run on the host without linking JavaScript APIs.
#[cfg(all(feature = "wasm", any(test, all(target_arch = "wasm32", target_os = "unknown"))))]
#[path = "web/lifecycle.rs"]
mod browser_lifecycle;

#[cfg(all(feature = "wasm", any(test, all(target_arch = "wasm32", target_os = "unknown"))))]
#[path = "web/ime_protocol.rs"]
mod browser_ime;
