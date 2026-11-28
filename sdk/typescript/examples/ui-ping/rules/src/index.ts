// `rules.wasm` of the client module network test, the same as the Rust
// SDK's `sdk/examples/ui-ping/rules`: every `Ping` message (one byte, 7)
// gives its sender one credit and notifies the sender's player with
// `Pong`. The credit puts each message into the state hash, so a room whose
// machines disagree about a message desyncs.
import { Action, House, log, onMessage } from "@vera20k/sdk";
import { Pong } from "./generated/notify";

const PING: u8 = 7;

onMessage((sender: House, data: Uint8Array): void => {
  if (data.length != 1 || data[0] != PING) return;
  Action.addMoney(sender, 1);
  Pong.send(sender);
  log("UI-PING message");
});
