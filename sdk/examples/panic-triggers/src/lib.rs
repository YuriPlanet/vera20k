//! A `rules.wasm` with a runtime trigger (RFC 0001 sections 6a and 7).
//!
//! At scenario start it makes one repeating `TakeDamage` trigger whose
//! module action is `OnHit`, and binds it to every object on the map. Each
//! hit then reaches `on_hit` in the module phase, which sets Red Alert 2
//! local variable 7 and logs the damage. The engine test
//! `sim::ext_modules::trigger_tests::a_rust_built_trigger_module_receives_its_records`
//! runs it.
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::named_actions! { ON_HIT = "OnHit" => 1 }

fn start() {
    let Ok(trigger) = Trigger::on(Event::TAKE_DAMAGE).enqueue(ON_HIT).done() else {
        log("panic-triggers: trigger_create failed");
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

fn on_hit(event: TakeDamageEvent) {
    let _ = Var::local(7).set(1);
    let mut line = [0u8; 48];
    let prefix = b"panic-triggers: hit for ";
    line[..prefix.len()].copy_from_slice(prefix);
    let len = prefix.len() + write_decimal(&mut line[prefix.len()..], event.damage());
    if let Ok(text) = core::str::from_utf8(&line[..len]) {
        log(text);
    }
}

/// Writes `value` in decimal at the start of `out`; returns its length.
fn write_decimal(out: &mut [u8], value: i64) -> usize {
    let mut digits = [0u8; 20];
    let mut n = value.unsigned_abs();
    let mut len = 0;
    loop {
        digits[len] = b'0' + (n % 10) as u8;
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let sign = usize::from(value < 0);
    if sign == 1 {
        out[0] = b'-';
    }
    for i in 0..len {
        out[sign + i] = digits[len - 1 - i];
    }
    sign + len
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_events!(ON_HIT => on_hit);
