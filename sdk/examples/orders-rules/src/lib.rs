//! A `rules.wasm` built with the Rust SDK (RFC 0001 preview): rules access
//! and house orders.
//!
//! In `on_rules` it doubles the GI's (`E1`) `Strength`. On its first frame
//! with a computer house that has a vehicle, it orders that vehicle two
//! cells east, once. The engine test `sim::ext_modules::tests::a_rust_built_orders_module_runs`
//! runs it.
#![no_std]

use core::sync::atomic::{AtomicBool, Ordering};

use vera20k_sdk::prelude::*;

fn rules() {
    let mut buf = [0u8; 16];
    let strength = Rules::get("E1", "Strength", &mut buf)
        .ok()
        .and_then(|text| text.trim().parse::<i32>().ok())
        .unwrap_or(125);
    let mut text = [0u8; 12];
    if Rules::set("E1", "Strength", format(strength * 2, &mut text)).is_ok() {
        log("orders-rules: doubled E1 Strength");
    }
}

/// Whether the order went out.
static ORDERED: AtomicBool = AtomicBool::new(false);

fn tick() {
    if ORDERED.load(Ordering::Relaxed) {
        return;
    }
    let (Ok(is_human), Ok(category), Ok(cell)) = (
        Prop::named("IsHuman"),
        Prop::named("Category"),
        Prop::named("Cell"),
    ) else {
        return;
    };
    let mut houses = [0i64; 16];
    let Ok((count, _)) = Find::houses()
        .where_prop(is_human, Op::Eq, 0)
        .page(&mut houses, 0)
    else {
        return;
    };
    for &house in &houses[..count] {
        let house = House(house);
        let unit = Techno(
            Find::objects()
                .owned_by(house)
                .where_prop(category, Op::Eq, Category::Vehicle as i64)
                .first(),
        );
        if unit.is_none() {
            continue;
        }
        let Ok(at) = vera20k_sdk::get(unit, cell).map(Cell) else {
            continue;
        };
        let units = [unit];
        if house
            .order(&Order::move_to(&units, Cell::at(at.x() + 2, at.y())))
            .is_ok()
        {
            log("orders-rules: ordered");
            ORDERED.store(true, Ordering::Relaxed);
            return;
        }
    }
}

/// `value` in decimal, written into `buf`.
fn format(value: i32, buf: &mut [u8; 12]) -> &str {
    let mut at = buf.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        buf[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("0")
}

vera20k_sdk::on_rules!(rules);
vera20k_sdk::on_frame!(tick);
