//! `rules.wasm` of the client module network test: every `Ping` message
//! (one byte, 7) gives its sender one credit and notifies the sender's
//! player with `Pong`. The credit puts each message into the state hash, so a
//! room whose machines disagree about a message desyncs.
#![no_std]

use vera20k_sdk::prelude::*;

vera20k_sdk::notify_names! {
    pub static PONG: Notify = "Pong";
}

const PING: u8 = 7;

fn ping(sender: House, data: &[u8]) {
    if data != [PING] {
        return;
    }
    let _ = Action::add_money(sender, 1);
    let _ = PONG.send(sender, &[]);
    log("UI-PING message");
}

vera20k_sdk::on_message!(ping);
