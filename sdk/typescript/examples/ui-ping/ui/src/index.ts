// `ui.wasm` of the client module network test, the same as the Rust SDK's
// `sdk/examples/ui-ping/ui`: 120 render frames after the match starts it
// sends one `Ping` message by itself, and logs when the simulation's `Pong`
// arrives. Spectators send nothing.
import {
  Client, ClientEvent, NO_HANDLE, NotifyEvent, NotifyName, Status, onClientEvent, onRender, onStart,
} from "@vera20k/sdk/client";

const SEND_AT: u32 = 120;
const PING = new Uint8Array(1);
PING[0] = 7;
const PONG = new NotifyName("Pong");

let frames: u32 = 0;

onStart((): void => {
  Client.log("UI-PING started");
});

onRender((dt: f32, alpha: f32): void => {
  frames++;
  if (frames != SEND_AT || Client.viewer() == NO_HANDLE) return;
  if (Client.sendMessage(PING) == Status.Ok) {
    Client.log("UI-PING sent");
  } else {
    Client.log("UI-PING refused");
  }
});

onClientEvent(ClientEvent.Notify, (e: NotifyEvent): void => {
  if (PONG.is(e)) Client.log("UI-PING pong");
});
