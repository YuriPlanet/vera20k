// A `rules.wasm` with a decision trigger (RFC 0001 section 6f), the same as
// the Rust SDK's `sdk/examples/decide-damage`. At scenario start it makes
// one `TakeDamage` decision trigger and binds it to every object on the
// map; each hit then asks `halve` synchronously, before the damage is
// written, and the object takes half.
//
// RFC open question 2 asks for a module that allocates each frame: `tally`
// builds a string every frame, so the collector runs during the match.
import {
  Event, Every, Find, NO_HANDLE, TakeDamageDecision, Trigger, TriggerRef,
  frame, log, onFrame, onScenarioStart,
} from "@vera20k/sdk";

function halve(d: TakeDamageDecision): void {
  d.setResult(d.previous / 2);
}

onScenarioStart((): void => {
  const trigger = Trigger.on(Event.TakeDamage).decideSync(halve);
  if (trigger == NO_HANDLE) {
    log("decide-damage: trigger_create failed");
    return;
  }
  const find = Find.objects();
  const page = new StaticArray<i64>(64);
  let after: i64 = 0;
  do {
    const result = find.page(page, after);
    for (let i = 0; i < result.count; i++) TriggerRef.bind(trigger, page[i]);
    after = result.next;
  } while (after != 0);
});

let lines: string[] = [];

function tally(): void {
  // Allocates every frame; keeps the last 30 strings.
  lines.push("frame " + frame().toString());
  if (lines.length > 30) lines = lines.slice(lines.length - 30);
  if (frame() % 900 == 0) log("decide-damage: " + lines[lines.length - 1]);
}

onFrame(new Every(1), tally);
