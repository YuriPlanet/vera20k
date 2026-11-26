// A `rules.wasm` built with the TypeScript SDK: the same module as the Rust
// SDK's `sdk/examples/minimal-rules`. Every 15th frame it pays house 0 100
// credits and logs it; on frame 30 it sets Red Alert 2 local variable 5.
// The engine test `sim::ext_modules::tests::a_rust_built_module_runs` runs
// either build.
import { Action, House, Status, Var, Every, frame, log, onFrame, onScenarioStart } from "@vera20k/sdk";

const HOUSE: House = House.fromIndex(0);

onScenarioStart((): void => {
  log("minimal-rules: started");
});

onFrame(new Every(1), (): void => {
  if (frame() % 15 == 0 && Action.addMoney(HOUSE, 100) == Status.Ok) {
    log("minimal-rules: paid 100");
  }
  if (frame() == 30) {
    Var.local(5).set(1);
  }
});
