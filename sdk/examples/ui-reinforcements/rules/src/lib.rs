//! `rules.wasm` of the round trip in RFC 0001 section 10: the client
//! module's button sends message type 1; this module checks it, creates a
//! Rhino tank (`HTNK`) for the sender at waypoint 3, takes 2000 credits and
//! sets the sender's `ReinforcementsBought` variable, which the client module
//! reads to hide the button. It also notifies the sender's player
//! (`Reinforced`), whose client module plays EVA speech.
//!
//! Build with `cargo build --release` in `sdk/examples/ui-reinforcements`,
//! then rename `target/wasm32-unknown-unknown/release/ui_reinforcements_rules.wasm`
//! to `rules.wasm`; `ui_reinforcements_ui.wasm` is the map's `ui.wasm`.
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::vars! {
    // Private: only the house's own player reads it (RFC 6e).
    pub static BOUGHT: HouseVar = ("ReinforcementsBought", Private, 0);
}

vera20k_sdk::notify_names! {
    pub static REINFORCED: Notify = "Reinforced";
}

/// Message type 1: buy the reinforcements. The message is the one byte.
const BUY: u8 = 1;
const PRICE: i32 = 2000;
const DROP_WAYPOINT: u32 = 3;

/// One message from a client module of this map; `sender` is the house the
/// engine put on the command, which the client cannot forge. Everything
/// else is untrusted: check it all.
fn buy_message(sender: House, data: &[u8]) {
    if data.len() != 1 || data[0] != BUY {
        return;
    }
    // Once per house, and only with the money.
    if BOUGHT.get(sender) != Ok(0) || !sender.money().is_ok_and(|m| m >= i64::from(PRICE)) {
        return;
    }
    let Ok(htnk) = TypeRef::techno_type("HTNK") else {
        return;
    };
    // An occupied cell refuses: charge only for a tank that exists.
    if Action::create_object(Waypoint::number(DROP_WAYPOINT), htnk, sender).is_err() {
        return;
    }
    let _ = Action::add_money(sender, -PRICE);
    // Visible to the client module's next read.
    let _ = BOUGHT.set(sender, 1);
    let _ = REINFORCED.send(sender, &[]);
    log("ui-reinforcements: reinforcements delivered");
}

vera20k_sdk::on_message!(buy_message);
