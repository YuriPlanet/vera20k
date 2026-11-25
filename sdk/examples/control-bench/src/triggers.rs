//! Area `triggers` of the bench (see `lib.rs`). Marks are named `triggers.<point>`.

use vera20k_sdk::prelude::*;

/// `vera_init`.
pub fn init() {}

/// `on_scenario_start`.
pub fn start() {}

/// The frame the arena opens: the houses and `crate::arena` are known.
pub fn open() {}

/// Every later frame.
pub fn frame(_now: i32) {}

/// Records whose key is in this area's range.
pub fn events(_record: &EventRecord) {}

/// Decisions whose key is in this area's range.
pub fn decide(_record: &mut DecideRecord) {}
