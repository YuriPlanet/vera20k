//! A `rules.wasm` built with the Rust SDK (RFC 0001 preview).
//!
//! Every 15th frame it pays house 0 100 credits and logs it; on frame 30 it
//! sets Red Alert 2 local variable 5. The engine test
//! `sim::ext_modules::tests::a_rust_built_module_runs` runs it.
#![no_std]

use vera20k_sdk::prelude::*;

const HOUSE: House = House::from_index(0);

fn start() {
    log("minimal-rules: started");
}

fn tick() {
    if frame() % 15 == 0 && Action::add_money(HOUSE, 100).is_ok() {
        log("minimal-rules: paid 100");
    }
    if frame() == 30 {
        let _ = Var::local(5).set(1);
    }
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(tick);
