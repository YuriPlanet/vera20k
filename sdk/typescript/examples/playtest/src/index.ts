// A `rules.wasm` to try in a skirmish, the same as the Rust SDK's
// `sdk/examples/playtest`, with effects a player sees:
//
// - at the start it says what it does in the message list;
// - every 30 seconds (450 frames) each human player gets 1000 credits;
// - GIs, conscripts, Grizzlies and Rhinos owned by a human player take
//   half damage (a `TakeDamage` decision with a house clause).
import {
  Action, Event, Every, Find, NO_HANDLE, Op, Prop, Rules, Status, TakeDamageDecision, Trigger,
  lastStatus, log, onFrame, onScenarioStart,
} from "@vera20k/sdk";

/** The types whose hits are halved for human players. */
const TOUGH: StaticArray<string> = ["E1", "E2", "MTNK", "HTNK"];
const humans = new StaticArray<i64>(16);

function halve(d: TakeDamageDecision): void {
  d.setResult(d.previous / 2);
}

onScenarioStart((): void => {
  log("Wasm test module: +1000 credits every 30 s for human players;");
  log("their GIs, conscripts, Grizzlies and Rhinos take half damage.");
  const trigger = Trigger.on(Event.TakeDamage).whereHouse(Prop.IsHuman, Op.Eq, 1);
  for (let i = 0; i < TOUGH.length; i++) {
    const ty = Rules.technoType(TOUGH[i]);
    if (ty != NO_HANDLE) trigger.bind(ty);
  }
  // A failed binding is a refusal too, as the Rust SDK's `decide_sync` answers.
  if (trigger.decideSync(halve) == NO_HANDLE || lastStatus() != Status.Ok) {
    log("Wasm test module: the half-damage trigger was refused");
  }
});

onFrame(new Every(450), (): void => {
  const page = Find.houses().where(Prop.IsHuman, Op.Eq, 1).page(humans, 0);
  if (lastStatus() != Status.Ok) return;
  const count = page.count;
  let paid = false;
  for (let i = 0; i < count; i++) {
    if (Action.addMoney(humans[i], 1000) == Status.Ok) paid = true;
  }
  if (paid) log("Wasm test module: +1000 credits");
});
