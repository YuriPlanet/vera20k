// The simulation module's exports (appendix 1.2). `vera20k build` passes
// this file to the compiler beside the module's own entry and drops each
// export the module registers nothing for, so a module without `onFrame`
// costs the engine no call per frame. Top-level code becomes `vera_init`
// (`--exportStart`), not a `start` section.

// Imported through the package name, as the module's own code imports the
// SDK, so the compiler loads one copy of it.
import { runScenarioStart, runRules, runFrame, runEvents, runDecide, runMessage } from "@vera20k/sdk/assembly/triggers";
// Loads the compiler's `abort` handler (`--use abort=`, bin/vera20k.js).
import "@vera20k/sdk/assembly/abort";

export function on_scenario_start(): void {
  runScenarioStart();
}

export function on_rules(): void {
  runRules();
}

export function on_frame(): void {
  runFrame();
}

export function on_events(ptr: i32, count: i32): void {
  runEvents(<usize>ptr, count);
}

export function on_decide(ptr: i32, count: i32): void {
  runDecide(<usize>ptr, count);
}

export function on_message(sender: i64, ptr: i32, len: i32): void {
  runMessage(sender, <usize>ptr, len);
}
