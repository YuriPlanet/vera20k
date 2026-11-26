// The client module's exports (appendix 2.2). `vera20k build` passes this
// file to the compiler beside the module's own entry for `"kind": "ui"` and
// drops each export the module registers nothing for: `client_on_start`
// needs `onStart`, `client_on_render` `onRender`, `client_on_events`
// `onClientEvent`. Top-level code becomes `client_init` (`--exportStart`).

// Imported through the package name, as the module's own code imports the
// SDK, so the compiler loads one copy of it.
import { runStart, runRender, runClientEvents } from "@vera20k/sdk/assembly/client/events";
// Loads the compiler's `abort` handler (`--use abort=`, bin/vera20k.js).
import "@vera20k/sdk/assembly/abort";

export function client_on_start(): void {
  runStart();
}

export function client_on_render(dt: f32, alpha: f32): void {
  runRender(dt, alpha);
}

export function client_on_events(ptr: i32, count: i32): void {
  runClientEvents(<usize>ptr, count);
}
