//! A `rules.wasm` that uses a modifier and declared variables (RFC 0001
//! preview).
//!
//! At scenario start it doubles the firepower of the first object for 30
//! frames, marks that object in an object variable and raises the global
//! `Wave` from its default 7 to 8. Each frame it watches the modifier and
//! logs once it has expired. The engine test
//! `sim::ext_modules::tests::a_rust_built_modifier_module_runs` runs it.
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::vars! {
    static WAVE: GlobalVar = ("Wave", Public, 7);
    static MARKED: ObjectVar = ("Marked", Private, 0);
}

/// The modified object and the modifier; 0 until set, and once expired.
static mut TARGET: i64 = 0;
static mut BOOST: i64 = 0;

fn start() {
    let target = Find::objects().first();
    let Ok(boost) = Modifier::multiply(Techno(target), Prop::FirepowerMultiplier, fixed(2.0), 30)
    else {
        log("modifier-vars: modify failed");
        return;
    };
    // SAFETY: a module runs on one thread; nothing borrows these statics.
    unsafe {
        TARGET = target;
        BOOST = boost.0;
    }
    if vera20k_sdk::get(Techno(target), Prop::FirepowerMultiplier) == Ok(2 * FIXED_ONE) {
        log("modifier-vars: doubled");
    }
    let marked = MARKED.set(Techno(target), 1).is_ok();
    let raised = WAVE.get().and_then(|wave| WAVE.set(wave + 1)).is_ok();
    if marked && raised {
        log("modifier-vars: variables set");
    }
}

fn tick() {
    // SAFETY: as above.
    let (target, boost) = unsafe { (TARGET, BOOST) };
    if boost == 0 {
        return;
    }
    if vera20k_sdk::get(Modifier(boost), Prop::Remaining) == Err(Status::Gone)
        && vera20k_sdk::get(Techno(target), Prop::FirepowerMultiplier) == Ok(FIXED_ONE)
    {
        log("modifier-vars: expired");
        // SAFETY: as above.
        unsafe { BOOST = 0 };
    }
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(tick);
