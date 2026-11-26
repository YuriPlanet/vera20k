//! `ui.wasm` of the round trip in RFC 0001 section 10: a "buy
//! reinforcements" button. A click sends message type 1 to the map's
//! `rules.wasm`; each render frame the button shows only while this
//! player's `ReinforcementsBought` is 0, so it hides once the simulation
//! delivered the tank. Spectators have no house and see no button.
//!
//! The button is the map layout's `BuyReinforcements` part when the map's
//! `[VeraUI]` has one, else a button this module creates in the overlay.
#![no_std]

use vera20k_sdk::client;
use vera20k_sdk::client::prelude::*;

static BUY: NodeSlot = NodeSlot::new();
/// The simulation module's variable, read under its visibility (private:
/// this player's own house only).
static BOUGHT: ClientVar = ClientVar::new("ReinforcementsBought");
static REINFORCED: NotifyName = NotifyName::new("Reinforced");

/// Message type 1: buy.
const BUY_MESSAGE: [u8; 1] = [1];

fn start() {
    let button = Ui::find("BuyReinforcements").or_else(|_| {
        Ui::create_with(
            NodeKind::Button,
            Node(NO_HANDLE),
            &[
                (Prop::X, 16),
                (Prop::Y, 120),
                (Prop::Width, 180),
                (Prop::Height, 24),
            ],
        )
    });
    let Ok(button) = button else {
        client::log("ui-reinforcements: no button");
        return;
    };
    let _ = button.set_label("Reinforcements: Rhino tank, $2000");
    let _ = button.set_clickable(true);
    BUY.set(button);
}

fn on_click(event: &ClientEventRecord) {
    if event.node() == BUY.get() && Client::send_message(&BUY_MESSAGE).is_err() {
        // Spectators, or over the message rate.
        client::log("ui-reinforcements: the message was refused");
    }
}

fn on_notify(event: &ClientEventRecord) {
    if REINFORCED.is(event) {
        let _ = Local::play_speech("EVA_ReinforcementsHaveArrived");
    }
}

fn render(_dt: f32, _alpha: f32) {
    let button = BUY.get();
    if button.is_none() {
        return;
    }
    // Observers have no house, and scope 0 is no house variable's.
    let me = Client::viewer();
    let show = !me.is_none() && BOUGHT.get_house(me) == Ok(0);
    let _ = button.set_visible(show);
}

vera20k_sdk::on_start!(start);
vera20k_sdk::on_render!(render);
vera20k_sdk::on_client_event!(
    ClientEvent::Click => on_click,
    ClientEvent::Notify => on_notify,
);
