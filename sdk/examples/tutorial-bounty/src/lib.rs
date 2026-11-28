//! The module of the modding tutorial (docs/wasm-modules/rust-tutorial.zh-CN.md):
//! whoever kills a GI or a conscript gets 200 credits.
#![no_std]

use vera20k_sdk::prelude::*;

// The module's named actions: `ON_KILL` is the key `on_events!` dispatches by.
vera20k_sdk::named_actions! { ON_KILL = "OnKill" => 1 }

/// Runs once when the match starts.
fn start() {
    log("Bounty: 200 credits for every GI or conscript killed.");
    // One trigger: the Killed event of every E1 (GI) and E2 (conscript).
    let mut trigger = Trigger::on(Event::KILLED);
    for name in ["E1", "E2"] {
        if let Ok(infantry) = TypeRef::techno_type(name) {
            trigger = trigger.bind(infantry);
        }
    }
    // When it fires, a record goes to `on_kill` in the module phase.
    if trigger.enqueue(ON_KILL).done().is_err() {
        log("Bounty: the trigger was refused");
    }
}

/// One kill: pay the killer's owner.
fn on_kill(event: KilledEvent) {
    // The killer may be gone already (it died in the same frame).
    let Ok(house) = event.killer().owner() else {
        return;
    };
    if Action::add_money(house, 200).is_ok() {
        log("Bounty: +200 credits");
    }
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_events!(ON_KILL => on_kill);
