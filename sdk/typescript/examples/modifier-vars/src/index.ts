// A `rules.wasm` that uses a modifier and declared variables, the same as
// the Rust SDK's `sdk/examples/modifier-vars`.
//
// At scenario start it doubles the firepower of the first object for 30
// frames, marks that object in an object variable and raises the global
// `Wave` from its default 7 to 8 (both declared in `vera.json`). Each frame
// it watches the modifier and logs once it has expired. The engine test
// `sim::ext_modules::tests::a_rust_built_modifier_module_runs` runs it.
import {
  Every, FIXED_ONE, Find, Modifier, NO_HANDLE, Prop, Status, fixed, get, lastStatus, log,
  onFrame, onScenarioStart,
} from "@vera20k/sdk";
import { Marked, Wave } from "./generated/vars";

/** The modified object and the modifier; 0 until set, and once expired. */
let target: i64 = 0;
let boost: i64 = 0;

onScenarioStart((): void => {
  const first = Find.objects().first();
  const made = Modifier.multiply(first, Prop.FirepowerMultiplier, fixed(2.0), 30);
  if (made == NO_HANDLE) {
    log("modifier-vars: modify failed");
    return;
  }
  target = first;
  boost = made;
  const power = get(first, Prop.FirepowerMultiplier);
  if (lastStatus() == Status.Ok && power == 2 * FIXED_ONE) log("modifier-vars: doubled");
  const marked = Marked.set(first, 1) == Status.Ok;
  const wave = Wave.get();
  const raised = lastStatus() == Status.Ok && Wave.set(wave + 1) == Status.Ok;
  if (marked && raised) log("modifier-vars: variables set");
});

onFrame(new Every(1), (): void => {
  if (boost == 0) return;
  get(boost, Prop.Remaining);
  if (lastStatus() != Status.Gone) return;
  const power = get(target, Prop.FirepowerMultiplier);
  if (lastStatus() == Status.Ok && power == FIXED_ONE) {
    log("modifier-vars: expired");
    boost = 0;
  }
});
