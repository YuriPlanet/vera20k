// The module of the TypeScript modding tutorial
// (docs/wasm-modules/ts-tutorial.zh-CN.md), the same as the Rust SDK's
// `sdk/examples/tutorial-bounty`: whoever kills a GI or a conscript gets
// 200 credits.
import {
  Action, Event, KilledEvent, NO_HANDLE, Rules, Status, Techno, Trigger,
  log, namedAction, onScenarioStart,
} from "@vera20k/sdk";

/** One kill: pay the killer's owner. */
function onKill(e: KilledEvent): void {
  // The killer may be gone already (it died in the same frame).
  const house = Techno.owner(e.killer);
  if (house == NO_HANDLE) return;
  if (Action.addMoney(house, 200) == Status.Ok) {
    log("Bounty: +200 credits");
  }
}

// INI triggers can call it as `Wasm:OnKill`.
namedAction("OnKill", onKill);

onScenarioStart((): void => {
  log("Bounty: 200 credits for every GI or conscript killed.");
  // One trigger: the Killed event of every E1 (GI) and E2 (conscript).
  const trigger = Trigger.on(Event.Killed);
  const names: string[] = ["E1", "E2"];
  for (let i = 0; i < names.length; i++) {
    const infantry = Rules.technoType(names[i]);
    if (infantry != NO_HANDLE) trigger.bind(infantry);
  }
  // When it fires, a record goes to `onKill` in the module phase.
  if (trigger.enqueue(onKill).done() == NO_HANDLE) {
    log("Bounty: the trigger was refused");
  }
});
