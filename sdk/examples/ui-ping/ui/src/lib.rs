//! `ui.wasm` of the client module network test: 120 render frames after
//! the match starts it sends one `Ping` message by itself, and logs when the
//! simulation's `Pong` arrives. Spectators send nothing.
#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

use vera20k_sdk::client;
use vera20k_sdk::client::prelude::*;

static FRAMES: AtomicU32 = AtomicU32::new(0);
static PONG: NotifyName = NotifyName::new("Pong");

const SEND_AT: u32 = 120;

fn start() {
    client::log("UI-PING started");
}

fn render(_dt: f32, _alpha: f32) {
    let frames = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    if frames != SEND_AT || Client::viewer().is_none() {
        return;
    }
    match Client::send_message(&[7]) {
        Ok(()) => client::log("UI-PING sent"),
        Err(_) => client::log("UI-PING refused"),
    }
}

fn on_notify(event: &ClientEventRecord) {
    if PONG.is(event) {
        client::log("UI-PING pong");
    }
}

vera20k_sdk::on_start!(start);
vera20k_sdk::on_render!(render);
vera20k_sdk::on_client_event!(ClientEvent::Notify => on_notify);
