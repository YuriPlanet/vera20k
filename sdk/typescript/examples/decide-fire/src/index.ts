// A `rules.wasm` answering decisions through their result area (RFC 0001
// section 6f, appendix 4.3 and 4.5):
//
// - `Fire`: every shot of a GI (`E1`) is planned twice. The first link's
//   area holds the native shot as one `ProjectileSpec`; the handler copies
//   it into entry 1 and answers 2, so each shot launches two bullets.
// - `FatalDamage`: a dying GI never plays the unit-lost announcement and
//   always throws debris (`FatalDamageDecision.with`).
//
// Both handlers run synchronously and allocate nothing: they read and
// write the area in place and count in plain globals. On frame 300 the
// module logs what it saw.
import {
  Event, Every, FatalDamageDecision, FireDecision, NO_HANDLE, Rules, Trigger,
  frame, log, onFrame, onScenarioStart,
} from "@vera20k/sdk";

let doubled: i32 = 0;
let refused: i32 = 0;
let deaths: i32 = 0;

function doubleShot(d: FireDecision): void {
  // The native shot (or the previous module's first entry).
  const first = d.previousEntry(0);
  const second = d.planEntry(1);
  if (first == null || second == null) {
    refused++;
    return;
  }
  second.copyFrom(first);
  if (d.setPlanLength(2)) doubled++;
}

function quietDeath(d: FatalDamageDecision): void {
  let plan = d.with(FatalDamageDecision.ANNOUNCE, false);
  plan |= FatalDamageDecision.DEBRIS;
  d.setResult(plan);
  deaths++;
}

onScenarioStart((): void => {
  const gi = Rules.technoType("E1");
  if (gi == NO_HANDLE) {
    log("decide-fire: no E1");
    return;
  }
  if (Trigger.on(Event.Fire).bind(gi).decideSync(doubleShot) == NO_HANDLE) {
    log("decide-fire: the Fire trigger was refused");
  }
  if (Trigger.on(Event.FatalDamage).bind(gi).decideSync(quietDeath) == NO_HANDLE) {
    log("decide-fire: the FatalDamage trigger was refused");
  }
});

onFrame(new Every(300), (): void => {
  if (frame() == 0) return;
  log("decide-fire: doubled " + doubled.toString() + " shots, " + refused.toString() +
    " without room, " + deaths.toString() + " quiet deaths");
});
