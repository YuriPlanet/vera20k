//! A `rules.wasm` with a decision trigger (RFC 0001 section 6f).
//!
//! At scenario start it makes one `TakeDamage` decision trigger and binds
//! it to every object on the map. Each hit then asks `halve`
//! synchronously, before the damage is written, and the object takes half.
//! The engine test
//! `sim::ext_modules::decide_tests::a_rust_built_decide_module_halves_damage`
//! runs it.
#![no_std]

use vera20k_sdk::prelude::*;

const HALVE: i32 = 1;

fn start() {
    let Ok(trigger) = Trigger::on(Event::TAKE_DAMAGE).decide_sync(HALVE) else {
        log("decide-damage: trigger_create failed");
        return;
    };
    let mut find = Find::objects();
    let mut page = [0i64; 64];
    let mut after = 0;
    while let Ok((count, next)) = find.page(&mut page, after) {
        for object in &page[..count] {
            let _ = trigger.bind(Techno(*object));
        }
        if next == 0 {
            break;
        }
        after = next;
    }
}

fn halve(mut decision: TakeDamageDecision) {
    decision.set_result(decision.previous() / 2);
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_decide!(HALVE => halve);
