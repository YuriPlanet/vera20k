//! A `rules.wasm` to try in a skirmish (RFC 0001 preview), with effects a
//! player sees:
//!
//! - at the start it says what it does in the message list;
//! - every 30 seconds (450 frames) each human player gets 1000 credits;
//! - GIs, conscripts, Grizzlies and Rhinos owned by a human player take
//!   half damage (a `TakeDamage` decision with a house clause).
//!
//! Build it with `cargo build --release`, rename the `.wasm` to
//! `rules.wasm` and put it in a mod folder, or embed it in a map with
//! `asset wasm-embed` (docs/wasm-modules/guide.md).
#![no_std]

use vera20k_sdk::prelude::*;

const HALVE: i32 = 1;
/// The types whose hits are halved for human players.
const TOUGH: [&str; 4] = ["E1", "E2", "MTNK", "HTNK"];

fn start() {
    log("Wasm test module: +1000 credits every 30 s for human players;");
    log("their GIs, conscripts, Grizzlies and Rhinos take half damage.");
    let mut trigger = Trigger::on(Event::TAKE_DAMAGE).where_house(Prop::IsHuman, Op::Eq, 1);
    for name in TOUGH {
        if let Ok(ty) = TypeRef::techno_type(name) {
            trigger = trigger.bind(ty);
        }
    }
    if trigger.decide_sync(HALVE).is_err() {
        log("Wasm test module: the half-damage trigger was refused");
    }
}

fn pay() {
    let mut humans = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1);
    let mut page = [0i64; 16];
    let Ok((count, _)) = humans.page(&mut page, 0) else {
        return;
    };
    let mut paid = false;
    for house in &page[..count] {
        paid |= Action::add_money(House(*house), 1000).is_ok();
    }
    if paid {
        log("Wasm test module: +1000 credits");
    }
}

fn halve(mut decision: TakeDamageDecision) {
    decision.set_result(decision.previous() / 2);
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(every = 450, pay);
vera20k_sdk::on_decide!(HALVE => halve);
