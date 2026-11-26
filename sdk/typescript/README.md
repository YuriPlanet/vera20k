# @vera20k/sdk — TypeScript SDK for Wasm modules

Write VERA20K simulation modules (`rules.wasm`) and client modules (`ui.wasm`) in TypeScript. The SDK compiles them to Wasm ahead of time with [AssemblyScript](https://www.assemblyscript.org), pinned to exactly 0.28.20, as RFC 0001 section 5a specifies. The game loads the compiled module like any other. TypeScript never reaches the game.

It has the same capabilities as the Rust SDK (`sdk/rust/vera20k-sdk`), under TypeScript names. The property, event and action tables, and the event and decision views, are generated from the Rust SDK (`npm run tables`). An engine test checks the property table against the engine's own.

## Start a module

```sh
# from a checkout of the repository (the package is not published yet):
cd sdk/typescript && npm install && cd -
node sdk/typescript/bin/vera20k.js init my-module   # or: init my-ui --ui
cd my-module
npm install
npm run build                     # build/rules.wasm
npm run check                     # vera20k generate, then tsc
npm run watch                     # rebuild on save
```

```ts
import { Action, Event, KilledEvent, NO_HANDLE, Rules, Status, Techno, Trigger,
         log, namedAction, onScenarioStart } from "@vera20k/sdk";

function onKill(e: KilledEvent): void {
  const house = Techno.owner(e.killer);
  if (house != NO_HANDLE && Action.addMoney(house, 200) == Status.Ok) log("Bounty: +200");
}
namedAction("OnKill", onKill);          // INI: Action1=Wasm:OnKill

onScenarioStart((): void => {
  Trigger.on(Event.Killed).bind(Rules.technoType("E1")).enqueue(onKill).done();
});
```

## What `vera20k build` does

- Compiles `src/index.ts` (the `entry` in `vera.json`) with the SDK's exports file, keeping only the exports the module registers something for. A module without `onFrame` exports no `on_frame`, so the engine makes no call for it each frame.
- Makes top-level code the `vera_init` (or `client_init`) export, never a `start` section.
- Emits no `env.abort`, `env.trace` or `env.seed` import, and uses only the Wasm features the engine accepts.
- Writes the `vera.*` custom sections:
  - `vera.actions` from the `namedAction("Name", fn)` string literals;
  - `vera.vars` and `vera.notify` from `vera.json`;
  - `vera.delivery`, the delivery area at bytes 8..1024. The build reserves that range with `--memoryBase 1024`.
- Generates `src/generated/vars.ts` and `notify.ts` from `vera.json`.

`vera.json`:

```json
{
  "kind": "rules",
  "entry": "src/index.ts",
  "out": "build/rules.wasm",
  "vars": [{ "name": "Wave", "scope": "global", "visibility": "public", "initial": 1 }],
  "notify": ["WaveStarted"]
}
```

## TypeScript rules

The code is AssemblyScript-compatible TypeScript. Both `tsc` and the compiler accept the same source.

- Write every parameter and return type, e.g. `(): void => {}`. Use `i32`, `i64` and `f32`.
- No closures over local variables, `any`, unions other than `T | null`, object literals, or `try`/`catch`.
- References (`Techno`, `House`, `Cell`, `TypeRef`, …) are plain `i64` values. Read and write them through namespace functions, e.g. `Techno.health(t)` and `Techno.setMission(t, Mission.Hunt)`.
- Calls that write return a `Status`. Calls that read or create return the value, which is the empty value (`0`, `NO_HANDLE`, `NO_ID`) on failure; `lastStatus()` then gives the reason.
- Event and decision views (`KilledEvent`, `TakeDamageDecision`, …) read the record in place. They are valid only during their callback. Decision handlers must not write, and should not allocate.

## Layout

| Path | Contents |
| --- | --- |
| `assembly/` | The SDK: `core.ts` (calls, references, actions, search, orders), `triggers.ts`, `records.ts`, `vars.ts`, `exports.ts`, `client/` |
| `assembly/generated/` | Tables and views generated from the Rust SDK |
| `bin/vera20k.js` | `init`, `build`, `watch` |
| `bin/transform.js` | The compiler transform: named actions and the exports to keep |
| `examples/` | Ports of the Rust examples; `npx tsc -p examples` checks them all |
| `template/` | The project `vera20k init` copies |

## Tests

`npm test` builds every example, type-checks them, and checks each module's imports and sections. The engine runs the built examples when the test environment names them:

```sh
cd sdk/typescript && npm install && npm test
E=$PWD/examples
VERA20K_TEST_RULES_WASM=$E/minimal-rules/build/rules.wasm \
VERA20K_TEST_DECIDE_WASM=$E/decide-damage/build/rules.wasm \
VERA20K_TEST_TS_WASM=$E/minimal-rules/build/rules.wasm:$E/tutorial-bounty/build/rules.wasm:$E/decide-damage/build/rules.wasm \
RA2_DIR=<retail install> cargo test -p vera20k-core --lib -- a_rust_built typescript_modules_restore_their_complete_state
```

`typescript_modules_restore_their_complete_state` compares the entire serialized module state immediately after a save/load and after another 30 frames. Both matches must keep the module running.

RFC 0001 open question 2 is still open. With the same module paths, `cargo test -p vera20k-core --lib typescript_modules_resume_from_a_save_exactly -- --ignored` runs the separate 9,000-frame compiler experiment. It also requires bounded memory and identical whole-game saves. The original headless experiment produced different whole-game saves with both the TypeScript examples and the Rust control, despite matching module digests. Passing the short module regression does not establish whole-game equality.
