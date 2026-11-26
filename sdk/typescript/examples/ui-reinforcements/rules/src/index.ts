// `rules.wasm` of the round trip in RFC 0001 section 10, the same as the
// Rust SDK's `sdk/examples/ui-reinforcements/rules`: the client module's
// button sends message type 1; this module checks it, creates a Rhino tank
// (`HTNK`) for the sender at waypoint 3, takes 2000 credits and sets the
// sender's `ReinforcementsBought` variable, which the client module reads
// to hide the button. It also notifies the sender's player (`Reinforced`),
// whose client module plays EVA speech.
import { Action, House, NO_HANDLE, Rules, Status, Waypoint, lastStatus, log, onMessage } from "@vera20k/sdk";
import { ReinforcementsBought } from "./generated/vars";
import { Reinforced } from "./generated/notify";

/** Message type 1: buy the reinforcements. The message is the one byte. */
const BUY: u8 = 1;
const PRICE: i32 = 2000;
const DROP_WAYPOINT: i32 = 3;

// One message from a client module of this map; `sender` is the house the
// engine put on the command, which the client cannot forge. Everything else
// is untrusted: check it all.
onMessage((sender: House, data: Uint8Array): void => {
  if (data.length != 1 || data[0] != BUY) return;
  // Once per house, and only with the money.
  const bought = ReinforcementsBought.get(sender);
  if (lastStatus() != Status.Ok || bought != 0) return;
  const money = House.money(sender);
  if (lastStatus() != Status.Ok || money < PRICE) return;
  const htnk = Rules.technoType("HTNK");
  if (htnk == NO_HANDLE) return;
  // An occupied cell refuses: charge only for a tank that exists.
  if (Action.createObject(Waypoint.number(DROP_WAYPOINT), htnk, sender) == NO_HANDLE) return;
  Action.addMoney(sender, -PRICE);
  // Visible to the client module's next read.
  ReinforcementsBought.set(sender, 1);
  Reinforced.send(sender);
  log("ui-reinforcements: reinforcements delivered");
});
