// `ui.wasm` of the round trip in RFC 0001 section 10, the same as the Rust
// SDK's `sdk/examples/ui-reinforcements/ui`: a "buy reinforcements"
// button. A click sends message type 1 to the map's `rules.wasm`; each
// render frame the button shows only while this player's
// `ReinforcementsBought` is 0, so it hides once the simulation delivered
// the tank. Spectators have no house and see no button.
//
// The button is the map layout's `BuyReinforcements` part when the map's
// `[VeraUI]` has one, else a button this module creates in the overlay.
import {
  ClickEvent, Client, ClientEvent, ClientVar, Local, NO_HANDLE, Node, NodeKind, NodeProps, NotifyEvent,
  NotifyName, Status, Ui, lastStatus, onClientEvent, onRender, onStart,
} from "@vera20k/sdk/client";

/** The simulation module's variable, read under its visibility (private: this player's own house only). */
const BOUGHT = new ClientVar("ReinforcementsBought");
const REINFORCED = new NotifyName("Reinforced");
/** Message type 1: buy. */
const BUY_MESSAGE = new Uint8Array(1);
BUY_MESSAGE[0] = 1;

let buy: Node = NO_HANDLE;

onStart((): void => {
  let button = Ui.find("BuyReinforcements");
  if (button == NO_HANDLE) {
    button = Ui.createWith(NodeKind.Button, NO_HANDLE, new NodeProps().at(16, 120).size(180, 24));
  }
  if (button == NO_HANDLE) {
    Client.log("ui-reinforcements: no button");
    return;
  }
  Node.setLabel(button, "Reinforcements: Rhino tank, $2000");
  Node.setClickable(button, true);
  buy = button;
});

onClientEvent(ClientEvent.Click, (e: ClickEvent): void => {
  if (e.node == buy && Client.sendMessage(BUY_MESSAGE) != Status.Ok) {
    // Spectators, or over the message rate.
    Client.log("ui-reinforcements: the message was refused");
  }
});

onClientEvent(ClientEvent.Notify, (e: NotifyEvent): void => {
  if (REINFORCED.is(e)) Local.playSpeech("EVA_ReinforcementsHaveArrived");
});

onRender((dt: f32, alpha: f32): void => {
  if (buy == NO_HANDLE) return;
  // Observers have no house, and scope 0 is no house variable's.
  const me = Client.viewer();
  let show = false;
  if (me != NO_HANDLE) {
    const bought = BOUGHT.get(me);
    show = lastStatus() == Status.Ok && bought == 0;
  }
  Node.setVisible(buy, show);
});
