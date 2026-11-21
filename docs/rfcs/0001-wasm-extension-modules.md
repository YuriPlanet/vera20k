# RFC 0001: Wasm extension modules

[简体中文](0001-wasm-extension-modules.zh-CN.md)

| | |
| --- | --- |
| RFC version | 0.0.20 |
| Status | Draft |
| Created | 2026-11-16 |
| APIs | `vera:sim@1`, `vera:client@1` (proposed) |

**Draft status.** Engine code lands after RFC acceptance. Experiments to answer the [open questions](#open-questions) belong outside the engine source tree.

## Summary

Mods and maps can ship a simulation module, `rules.wasm`, and a UI module, `ui.wasm`. The engine loads compiled modules in a sandbox. Simulation modules change gameplay through properties, variables, events and actions. Client modules build interfaces with nodes. Official SDKs target TypeScript and Rust. Other languages can target the [module ABI](#5a-languages-and-sdks). The [API appendix](0001-wasm-extension-modules-api.md) specifies function signatures, fixed layouts and numeric IDs.

## Goals

1. Let modders change gameplay and the interface without modifying the engine.
2. All players run the same simulation modules with identical results on every supported platform, including browsers, without desyncs.
3. Report module errors before a match and refuse to start. During a match, disable a faulty module, explain the reason to players and continue the match.
4. Modules have no file or network access. Simulation modules have no clock. Client rendering time is for display only. Meter work by operation count. Clients can read locally visible state and permitted variables. Simulation modules choose what their notifications disclose.
5. Preserve stock gameplay when neither the mod nor the map supplies modules or uses extension keys.
6. Modules used by only a few objects or types must not slow down battles with 20,000 units.
7. A Red Alert 2 modder can start from a template and see the result in game within minutes.

## Non-goals

- Native plugins (`.dll`, `.so`) or anything loaded outside the sandbox.
- Source code in the engine. The engine never parses, compiles or interprets a source language or embeds its runtime.
- Exposing engine internals such as memory layouts, native addresses, internal object indices, storage slots or the renderer's graphics API. Object handles are creation serials (section 5).
- Adding object types during a match.
- Replacing the stock trigger system. Modules extend it.
- Editing map terrain. Modules cannot place or remove overlays such as walls and bridges or change tiles. Stock actions that add or remove ore remain available.

## Terms

| Term | Meaning |
| --- | --- |
| Reference / handle | A reference identifies an object, type, cell, waypoint or global state. A simulation object handle is a creation serial that is never reused. Client nodes use local serials. Zero is null. |
| Trigger / tag | A trigger contains events, filters and actions. Stock maps attach triggers to objects or cells through tags. Modules can also bind them directly. |
| Module trigger | A trigger created by a module, bound through `Vera.Tags` or `trigger_bind`, or carrying a module action or decision handler. |
| Safe point | A point before a calculation, after its completion or after a batch commits, at which a trigger's immediate actions may run (section 5). |
| Frame / render frame | Game time is measured in simulation logic frames. A render frame is a local display cycle. Map distances use leptons, with 256 leptons per cell. |

## What a modder sees

### Files

```
Mods/MyMod/
  mod.ini        ; Needs= and other mod.ini keys. No module keys here.
  rulesmd.ini    ; [VeraWasm] API version, budgets and map policy. Binds tags to types.
  rules.wasm     ; Simulation module
  ui.wasm        ; UI module
```

A map declares its API and module sizes in `[VeraWasm]`. The `[VeraWasmRulesPack]` and `[VeraWasmUIPack]` sections hold compressed, base64-encoded data split across numbered lines. The `asset` tool embeds, extracts and validates the modules for loading. See the [appendix INI keys](0001-wasm-extension-modules-api.md#ini-keys) for keys, defaults and encoding.

Embedding sections do not depend on comments, empty sections, section order or key order, which map editors drop or rearrange on save. `asset` can re-embed and validate a map an editor has saved.

### Binding a trigger to a type

```ini
; The mod's rulesmd.ini
[E1]
Vera.Tags=GIPanicTag     ; Every E1 receives this tag's trigger

[GIPanicTag]
Trigger=GIPanic

[GIPanic]
Event=TakeDamage
Where1=Subject.HealthPercent<30      ; All filter clauses must pass (section 6a)
Where2=Var(Subject,Panicked)=0
Action1=SetVar(Subject,Panicked,1)   ; Immediate atomic action: later damage to this E1 in the same frame fails Where2
Action2=Wasm:giPanic     ; Run giPanic in the mod's rules.wasm (section 2)
Repeat=Repeating         ; A TagRepeat value
```

Each E1 runs its own instance of `GIPanic` (section 7). Only an E1 below 30% health with `Panicked` equal to 0 delivers a record. The engine filters out damage to other E1s. The atomic `SetVar` action writes the variable when the trigger fires, so each E1 delivers just one record. Section 6a defines the syntax. Without a module, declare variables in `[VeraVars]`. Map triggers edited in FinalAlert2 call module functions through action 100036.

### Module code

Only the compiled module reaches the engine. Authors keep the source and compiler in their toolchain. Both official SDKs express the same behavior below. Bind the type in either INI or code. With the INI above, register the named callback and omit trigger creation in code.

```ts
// TypeScript (AssemblyScript-compatible)
import { Trigger, Event, Prop, Op, Techno, Mission, Var, VarTarget, ActionDef, Target, Rules,
         TakeDamageEvent, namedAction, onScenarioStart, onFrame, Every } from "@vera20k/sdk";

import { Panicked } from "./generated/vars";

function giPanic(e: TakeDamageEvent): void {
  Techno.setMission(e.victim, Mission.Hunt);   // One host call: set(subject, Mission, Hunt)
}
namedAction("giPanic", giPanic);       // String literal: the SDK extracts it at build time into vera.actions for INI lookup

onScenarioStart((): void => {
  Trigger.on(Event.TakeDamage)
    .bind(Rules.technoType("E1"))                  // One instance per E1 (section 7)
    .where(Prop.HealthPercent, Op.Lt, 30)
    .whereVar(VarTarget.Subject, Panicked, Op.Eq, 0)
    .immediate(ActionDef.setVar(Target.Subject, Panicked, 1))   // Atomic action runs when the trigger fires
    .enqueue(giPanic).done();                              // Calls trigger_create and trigger_bind
});

onFrame(new Every(15), (): void => {
  if (Var.local(3).get() != 0) {    // Red Alert 2 local variable 3
    // ...
  }
});
```

```rust
// Rust SDK
use vera20k_sdk::prelude::*;
use vera20k_sdk as vera;

use generated::vars::PANICKED;

fn gi_panic(e: &TakeDamageEvent) {
    e.victim().set_mission(Mission::Hunt);
}

#[vera::on_scenario_start]
fn start() {
    let panic_action = named_action!("giPanic", gi_panic);
    Trigger::on(Event::TakeDamage)
        .bind(rules::techno_type("E1"))
        .where_prop(Prop::HealthPercent, Op::Lt, 30)    // where is a Rust keyword
        .where_var(VarTarget::Subject, PANICKED, Op::Eq, 0)
        .immediate(ActionDef::set_var(Target::Subject, PANICKED, 1))
        .enqueue(panic_action).done();
}

#[vera::on_frame(every = 15)]
fn check_waves() {
    if Var::local(3).get() != 0 {
        // ...
    }
}
```

Each SDK builds `rules.wasm` or `ui.wasm` with one command. Functions such as `Techno.setMission` and `Trigger.on` are generated from the appendix tables. Each wrapper makes only the host calls listed in [appendix section 3](0001-wasm-extension-modules-api.md#3-sdk-mappings), usually exactly one (section 5a). Package names are placeholders.

### Example: launch a nuke when the first unit fires

Bind a `Fire` trigger to the first unit on the map by creation serial. When it fires, the module action drops a nuke on its target cell.

```ts
// TypeScript (AssemblyScript-compatible)
import { Trigger, Event, Find, Prop, Op, Category, Action, FireEvent,
         NO_HANDLE, onScenarioStart } from "@vera20k/sdk";

function nukeAtTarget(e: FireEvent): void {
  Action.nukeStrike(e.targetCell, e.house);   // Target cell captured at firing, one action call
}

onScenarioStart((): void => {
  const first = Find.objects()
    .where(Prop.Category, Op.Ne, Category.Building)
    .where(Prop.Category, Op.Ne, Category.Projectile)
    .first();                                    // One find: lowest creation serial, or NO_HANDLE
  if (first != NO_HANDLE) {
    Trigger.on(Event.Fire).bind(first).once().enqueue(nukeAtTarget).done();
  }
});
```

- A record is delivered only when this unit fires. Other units' firing sites check their own flag once and return (section 15).
- `.once()` fires the trigger only once. Omit it to launch a nuke on every shot.
- The object binding disappears with the object, so the trigger stops firing when the unit dies. The trigger itself stays in the trigger runtime until removed with `Trigger.remove` (section 7).
- INI cannot select an individual object. `Vera.Tags` binds types. Select individual objects in code.

## Design

### 1. Two worlds

| | Simulation (`rules.wasm`) | Client (`ui.wasm`) |
| --- | --- | --- |
| Interface | `vera:sim@1` | `vera:client@1`, every function prefixed with `client_` |
| Runs on | Every machine in lockstep | Local machine only |
| Deterministic | Yes | No |
| Can change | Gameplay state through its engine owner | UI nodes, camera, cursor, local settings and sound |
| Affects gameplay through | Actions, property writes, variables, missions for any house's units and orders for computer houses | Standard player orders and messages through the command stream |
| Engine side | `sim/` | `ui/`, `render/`, `audio/` |

The existing dependency rule applies. `sim/` never depends on `render/`, `ui/`, `audio/` or `net/`. The loader rejects a simulation module that imports `client_*` functions and a client module that imports simulation functions.

- Simulation to client requires no network traffic. The client reads visible state and variables its visibility permits. A simulation module uses `send_notify` to notify a target house's machine.
- Client to simulation uses only the command stream. Standard player orders undergo the usual input validation. Messages enter the command stream like player orders (section 10). Replays and network games observe both.

### 2. Module sources: mods and maps

Mods and maps use the same API and SDKs. The engine assigns their permissions.

| | Mod | Map |
| --- | --- | --- |
| Order | First | After the mod, following INI layering |
| `on_rules` | After `RULESMD.INI`, `LANGRULE.INI` and the mode layer | After the map layer, sees the mod's results |
| Operation budget | Declared in the mod's `[VeraWasm]`, capped by the engine | Smaller by default, can be raised by the mod's `[VeraWasm]` |
| Size | Subject to file size limits | Subject to an embedding limit |
| UI | Can replace or rearrange the match interface | Overlay only unless the mod grants more access |

The mod controls map modules in the `[VeraWasm]` section of its `rulesmd.ini`.

```ini
; The mod's rulesmd.ini
[VeraWasm]
Api=1.0              ; API major.minor required by the mod's modules
Budget=200000        ; Mod module budget per frame, capped by the engine
MapBudget=100000     ; Map module budget per frame under this mod
MapWasm=yes          ; no: maps with modules cannot start under this mod
MapUI=overlay        ; overlay, full or no
```

- These keys live in `rulesmd.ini`, which the mod digest covers. It excludes `mod.ini`. Every player in the room thus compares the same keys (section 11).
- Read each `[VeraWasm]` from its own file, without layering. Read the mod section from the mod's `rulesmd.ini` before other layers, and the map section from the map. The map cannot change the mod's keys. Its own `[VeraWasm]` holds only embedding keys, `Api=` and `StorageId=`.
- Without a selected mod, map modules are allowed with the engine's default budget and overlay-only UI.
- Values are preview defaults (section 15.4). See the [INI key table](0001-wasm-extension-modules-api.md#ini-keys).
- **Files distributed with a map have only map permissions.** Files a platform distributes with a map, such as map files installed as a hidden mod, have the map as their source. Their `rules.wasm`, `ui.wasm`, `[VeraWasm]` and type bindings cannot obtain a mod's budget, permissions or order. The distributor tells the engine this source, or refuses attached files that carry modules. Map modules ship only in the embedding sections.

**Rules layers.** Apply `RULESMD.INI`, `LANGRULE.INI`, mode rules, the mod's `on_rules`, map rules and the map's `on_rules`, in that order. Each callback sees the processed rules from preceding layers. After it writes, the engine processes the rules again as it would after INI text. Assigned type IDs remain stable. Later layers may append types with new IDs. An ID resolved by the mod's `on_rules` remains valid for the scenario, including after saving and loading.

**Conflict rules**

- **Type tag bindings.** Read INI `Vera.Tags` only from the mod's `rulesmd.ini`. Ignore it in maps and warn in the lobby. Maps bind in code with `trigger_bind` (section 7), so they cannot erase mod bindings.
- **Names.** A module name in a mod's INI or trigger (`Action=Wasm:name` or a decision handler name) resolves to the mod module. A name in a map resolves to the map module. INI may address the other source with `Wasm:mod.name` or `Wasm:map.name`. Action 100036 in map triggers writes `mod.name` or `map.name` (section 6a). Code-created triggers and directly executed `Module` actions can use only their own module's dispatch keys. `lookup(ModuleAction, ...)` accepts no prefix. Variable names (`vera.vars`) also resolve within their source unless explicitly prefixed to reach the other source.
- **Messages and notifications.** `client_send_message` reaches `on_message` in the simulation module of the same source (mod to mod, map to map). `send_notify` reaches every client module on a target machine as a `Notify` client event carrying the sending module's source (mod or map).
- **Triggers.** Both sources' triggers fire. The mod module runs first.
- **Decision events.** The mod produces a result, the map may revise it, and the engine applies the final value. Each handler receives the native value and the preceding result, initially the native value. A later handler may reverse an earlier veto. The decision record has an explicit handled flag. An unhandled request passes the preceding result through unchanged (section 6f).
- **Actions, property writes and modifiers.** All go through the same owner in execution order. Modifier exclusion groups are separate by source. A map modifier cannot displace a mod modifier (section 6c).
- **Peer calls (later phase).** A future minor version may let a mod expose functions callable by the map.

### 3. Loading and validation

At load time, the engine performs these steps in order.

1. Decode embedded modules and verify the declared sizes.
2. Parse the module. Accept only imports from `vera:*` interfaces whose major version is supported and whose minor version requirement the engine meets (section 12). Simulation modules can import only `vera:sim` functions, client modules only `client_`-prefixed `vera:client` functions. Reject cross-world imports. Verify the imports against `Api=` in the source's `[VeraWasm]` section, which the lobby has already read. The imported module name includes the version ([appendix section 1](0001-wasm-extension-modules-api.md#1-simulation-abi)). Before compiling, reject modules exceeding structural limits on module size, function count, individual function body size, locals, imports, exports, types, globals, table entries, segments and custom section entries (section 15.4), so map compilation cannot stall the lobby. Check each `vera.*` custom section against the [appendix layout](0001-wasm-extension-modules-api.md#custom-sections), including its leading format version, ASCII names and absence of truncated or trailing bytes.
3. Accept the Wasm MVP core plus mutable globals, sign extension, non-trapping float-to-integer conversions, multiple return values and, from bulk memory, only `memory.copy` and `memory.fill`. Reject everything else, including SIMD, reference types, garbage collection, multiple memories, threads, 64-bit memory, exceptions, tail calls, passive and declarative segments, `memory.init`, `data.drop`, `table.init`, `table.copy` and `elem.drop`. Segments then have no runtime state and the function table is immutable after instantiation, so the only instance state that changes is memory (including its size) and mutable globals. A module that defines memory must export it as `memory`. Before this subset is widened, specify how the added state is saved and hashed. Reject modules with a `start` section so instantiation executes no module code. Rewrite the bytecode, independently of engine switches, to replace NaN with the positive canonical NaN after every floating-point arithmetic or precision conversion that can produce one. `neg`, `abs`, `copysign`, loads, stores and `reinterpret` are bitwise. Canonicalizing only at boundaries is insufficient: `copysign(1, NaN)` turns the nondeterministic sign of a NaN into an ordinary number.
4. Instrument operation and stack metering in the same pass, a pure function shared by all platforms. Charge each basic block in full before it runs, and bulk memory operations additionally by length. Limit both call depth and weighted stack slots (parameters, locals, operands), well below every supported engine's real stack limit, so deep recursion stops at the same call on every machine. Reaching the engine's stack limit first is a match error. Modules are never reentered (section 5), which bounds engine stack frames between module calls. The same rewrite exports the module's unexported mutable globals (such as a compiler-generated stack pointer), and the engine reads and writes them through those exports to save, load and hash (section 4). At instantiation, allocate the module's maximum memory so `memory.grow` within the limit always succeeds. A host unable to allocate it fails the match at load time (a match error, below).
5. Check that the required lifecycle exports exist with the correct signatures and that every module action or handler name referenced by INI (`Action=Wasm:name`) exists in the resolved module's `vera.actions` table.
6. Validate `vera.vars` declarations. Names must be unique within a module, scope and visibility must be table values, and visibility restricted to objects in view applies only to single-object scope. Assign variable IDs in load order (mod, then map), then resolve INI trigger events, filter clauses and atomic actions. Report unresolved names in the lobby.
7. Check the notification names declared in both simulation modules' `vera.notify` sections for hash collisions (section 10). A collision within a module rejects that module. A collision between mod and map names rejects the map module. Report both names. A name hashing to −1 (`0xFFFFFFFF`) rejects its declaring module. If both sources declare the same name, give them the same ID. The source on the `Notify` client event distinguishes them.

**Initialization.** Run the SDK-generated initialization function once before the first callback. Skip it when loading a save. Initialization may resolve names known up to the current rules layer, probe table entries and log messages. Other calls return `Unsupported`. Initialization, `on_rules` and `on_scenario_start` each receive a one-time budget larger than the frame budget.

**Failures**

| When | Result |
| --- | --- |
| Before a match | Show the cause in the lobby. A match with this module cannot start. |
| In `on_rules` | Refuse to start and identify the module whose rules writes are incomplete. |
| During a match, including `on_scenario_start` (trap, budget or depth exceeded, invalid arguments, write call in `on_decide`) | Disable the module on every machine in the same frame. Tell players which module stopped and why. |
| Host failure (insufficient memory for the maximum allocation, internal Wasm engine error) | Treat as a match error. The engine cannot establish that all machines fail in the same frame. Stop the match and show a message as for a desync. |
| In a client module (per-render-frame budget exceeded, trap) | Stop the call, disable the module on the local machine and tell its player. |

Clients use the same operation-based meter, which also works on a browser's main thread.

**Disabled simulation module.** The match continues.

- Disable triggers it created. Other triggers (map triggers or triggers bound through `Vera.Tags`) skip just its module actions. Discard its queued module actions and orders (section 6g).
- Skip its decision-chain link, including the call that failed. Pass the preceding result to the next link, using the native value for the first link.
- Projectiles on its module-controlled trajectories switch to a straight line toward their targets.
- Preserve completed effects, including modifiers (which expire normally), variables and properties written, rules, triggers it created (disabled), and work already completed by the failing call.
- Keep its memory frozen at the point it stopped, in snapshots and the state hash, so saves and reconnects reproduce it.

**Error model.** Normal gameplay failures return a status and leave the module running.

| Status | Cause |
| --- | --- |
| `Gone` | The reference is 0 or its object no longer exists. |
| `NotFound` | A name or ID has no match, including a waypoint absent from the map. |
| `Refused` | Engine rules reject an action or write, e.g. construction on an occupied cell or writing a property that is currently unwritable. An atomic action's target can also be inapplicable when its trigger executes (section 6b). |
| `Unsupported` | The feature is disabled in this match (e.g. an Ares action without Ares), the engine does not recognize the table entry, or the host call is unavailable in this context. |

- Calls return their results immediately. Writes return a status. Fallible reads return a status and write the value through a pointer into module memory, in the same call. Reading money returns (0, Ok) for a zero balance or (0, Refused) for an unauthorized read. Reading a vanished object gives an empty value and Gone, and the module continues. Batch reads still take one call. See the appendix for signatures.
- An atomic action executed by a trigger has no module code to receive its status. Log `Refused`, `Gone` or `NotFound` in the game, skip the action and do nothing else.
- Disable a faulty module for invalid arguments (including invalid action arguments or filter clauses detected by `trigger_create`, section 6b), a write in `on_decide` (section 6f), a trap or exceeding the operation or call-depth limit.

### 4. Determinism

Every machine runs identical module bytes. Results therefore depend only on how the engine executes them.

- Enforce the Wasm feature allowlist. One bytecode rewrite (section 3) determines NaNs, operation counts and stack limits, independently of engine fuel, NaN switches or stack size.
- Backend and thread are not simulation semantics. The same module, state and input yield the same memory, globals, operation count, stopping point and host call sequence on native engines, browsers, the main thread and the simulation worker.
- Provide no clock, host entropy or WASI. `random(lo, hi)` draws from a stream dedicated to the module, derived from the scenario seed and module slot (mod or map). Module draws never shift the native stream. A module that makes no draws leaves native random results unchanged. Actions that run native code draw from the native stream as that code normally would. Each table entry states whether it draws from this stream (`native`). The native code determines how many draws it makes.
- In `on_decide`, `random` draws from a substream derived from (scenario seed, module slot, frame, event kind, subject creation serial). An absent subject uses serial 0 and adds the event's house and `id` to the key, so production permissions for different houses and types have separate substreams. Further requests with the same key in the same frame continue that substream. Each `on_decide` call carries one record (section 6f), so the host always knows which request a draw belongs to. The number of decision calls never changes the position of the module's main stream.
- **Seed derivation.** The module main stream, decision substreams and native streams use distinct domain tags. Mix the tag and each component separately and completely before truncating to the stream's seed width. Truncation must not lose the tag, and streams must not coincide through XOR of their components.
- Compare names (type IDs, section names, variable names) case-insensitively by folding ASCII `A` through `Z`, matching gamemd INI handling. Strings cross the boundary in UTF-8.
- Load-time instrumentation meters operations independently of engine-provided fuel. Native and browser engines count the same work and stop at the same point. Host functions charge the same budget for work done (per property read, object scanned or returned, cell checked). The interface definition specifies each function's cost. Calls over variable amounts of data charge in segments before the work, never afterwards. Costs are proportional to the engine's actual work, including clause evaluation and items skipped by pagination. A search examines no more items than the caller's remaining budget pays for. If that ends the scan before the per-call limit (section 15.4) does, the call has exceeded its budget.
- Metering units, host function costs, default budgets and limits are fixed per API major version. A module within those limits keeps working in later versions.
- Positions, speeds and other gameplay values cross the boundary as integers (leptons or raw fixed-point values) in `i64`. Fractions and multipliers use raw simulation fixed point, `I16F16` (`SimFixed`). Modules may use floating point internally.
- Module memory and its size, mutable globals, module random streams, whether `on_scenario_start` has run and whether the module is disabled are simulation state. Snapshot and hash them (section 15). Random streams, disabled state and the full payload of queued records also enter the multiplayer sync check. The function table and segments are immutable (section 3), are rebuilt from the module bytes and are not saved. On load, instantiate, then overwrite memory and globals from the snapshot. Engine-owned variables, modifiers and bindings are also saved and hashed. Save only at a frame boundary, before any module call or decision in that frame, when instrumentation counters, frame usage, decision substreams and chain counters hold their initial values and need no saving. A future mid-frame save must save and hash them too.
- Never call simulation modules from parallel or speculative work. Order delivered records across objects by creation serial independently of engine loop order. Issue decision requests only where the engine commits results, in deterministic engine order (section 5).

### 5. Execution model

Simulation modules export [lifecycle functions](0001-wasm-extension-modules-api.md#12-exports) as needed. The SDK invokes module actions by the record's dispatch key. Periodic callbacks run in registration order when `frame % N == 0`.

Simulation modules run only during simulation. UI, rendering and audio read committed results.

**Module phase.** Once per simulation frame, at the end of the Triggers step: after map trigger polling and before shroud, teams, object turns, factories and house updates. It reads all completed results of the previous frame and this frame's map trigger results. Its writes are visible to later steps and the sync check in the same frame. In order it runs modifier expiry, per-frame batched decisions, timed state reversions, deferred chain triggers (below), polls and timers. It then visits modules in load order, mod first, then map, calling `on_message`, the `on_events` batches and `on_frame`. `on_scenario_start` runs in the first module phase and so does not see events raised by that frame's map trigger polling.

Records queued before the first module runs are delivered this frame, including those raised by the earlier steps of this phase. Records raised while any module runs, whichever module they belong to, are delivered next frame, as are events raised in object turns, factories and house updates.

**Delivery.** An ordinary (non-decision) event reaches a module through a firing trigger. Each module action writes a fixed-size record to its module's queue with the payload of the event that fired the trigger. Check filter clauses before enqueueing. A failed filter writes nothing. Deliver queued records in order, only in the module phase, through one or more `on_events` calls. There is no separate event subscription.

- Drop queued module actions if the game has ended or their trigger was disabled or deleted before this module's delivery starts in the frame. That moment fixes the records to deliver this frame. Disabling or deleting a trigger during delivery affects only records held for later frames, so the outcome does not depend on which records share a batch.
- An object that died before delivery reads as gone. Its handle can never identify another object. Events whose subject may disappear before delivery (death, fatal damage, damage, crushing, detonation) carry the location at the time of the event, so the module need not read a vanished subject to recover it.
- If a bound trigger's subject is already gone when the event is raised (e.g. a weapon-bound `Detonated` after its firer dies), keep the subject handle in the record. It reads as gone. The trigger house is the subject's last house (the launching house for projectile events), and the instance belongs to that vanished subject.

**Order.** Across objects, sort records in a batch and triggers firing after a batch commit by subject creation serial. Reordering or parallelizing engine loops does not affect this order. Decision requests are synchronous and arrive in deterministic commit order. That order agrees on every machine but may change across engine versions (section 6f). Within one object, process calls in the order the engine raises them, which may also vary by engine version. Records with subject 0 (house and logic triggers, and cell triggers whose event has no subject) come first, in trigger runtime firing order.

**Commit points, batches and parallel work.** Never call a module from parallel or speculative code. Parallel or precomputed work (speculative target selection, pathfinding batches, pursuit scoring) produces candidates that the object's turn may apply or discard. Such work raises no events and issues no decision requests.

- Raise events at result commits. Damage, firing and projectile advancement commit individually on each object's turn. Raise their events after each commit.
- When the engine commits a batch together, such as production permissions for all houses in a frame, treat the batch as one calculation. Fire its VERA event triggers immediately after commit in the same frame before the next phase. Decide its decision events before commit, one request and one link at a time in a defined order (section 6f).
- Process modifier expiration serially by expiry frame, outside parallel work (section 15).

**No reentry.** While a module call remains on the stack, the engine never calls that module again. Calculations reached by the call skip its link in a decision chain and use the previous or native value. Damage from a module's own actions therefore bypasses its own `TakeDamage` and `DamageIncoming` handlers (its shield does not absorb damage it deals itself). Events from its actions fire triggers and enqueue module actions as usual. Other modules can run. For example, an action executed directly by the mod module can reach a map module decision handler.

**Action timing**

- Stock and atomic actions (section 6b) run when the trigger fires, as stock actions do in the original game, even when the trigger includes a module action. Queue only module actions. For [stock A, module B, atomic C], run A and C first and B later that frame. For strict sequencing, implement all actions in module code and use `action` for each stock action.
- Module code executes `action`, property writes, `modify` and `var_set` immediately.
- `order` only queues. The order executes in the next frame's command step (section 6g).
- A trigger firing while module code runs executes its stock and atomic actions immediately. Queue its module actions, including another module's, for the next frame's module phase to prevent same-frame module loops.

**Safe points.** Raise each new VERA delivery event after a calculation (`TakeDamage` after damage commits), before a calculation (`HouseTransfer` before transfer), or immediately after a batch commits. Run stock and atomic trigger actions immediately at that point. After an event raised before a calculation, recheck the calculation.

- After `HouseTransfer`, cancel the transfer and do not raise `HouseTransferred` if an action already removed the object or changed its house.
- Raise `FatalDamage` after fatal damage commits and before death processing. Proceed with death only if the object still exists and the damage remains lethal. An action may have healed it or added an `Unkillable` modifier.

Delivery events report what has already happened. A module action cannot cancel a native outcome such as death. To intervene, use a decision event (section 6f), a modifier (`Unkillable`) or a stock or atomic action on an event raised before the calculation (`FatalDamage`, `HouseTransfer`).

**Chains.** Immediate actions can chain within one frame. Damage raises `TakeDamage`, whose trigger can deal damage through an atomic action and raise `TakeDamage` again.

- If a chain involves a module trigger, a new VERA event or an atomic action, cap its depth, counted over the whole call stack across modules, and the number of such firings per frame, counted separately per source (mod, map). A trigger firing two more could otherwise produce 2^depth firings. The first firing from an engine event or from an action module code runs directly, and a firing deferred from the previous frame, start chains and do not count. Section 15.4 gives the preview limits. One source exhausting its count does not affect the other.
- Defer a trigger that would exceed either limit. Each source has a cap on pending deferred firings: past it, disable and report that module, or, if the source has no running module, drop the excess and report once. Fire a deferred trigger with its complete original payload, including the event's own house, at the start of the next frame's module phase, before queued actions. Run stock and atomic actions then and include module actions in that frame's batch. Drop it if disabled or deleted in the meantime. Recheck filters and take the subject's current house. Actions on a vanished subject return `Gone`. Save and hash pending deferred triggers.
- Chains made entirely of stock events and stock actions run as in the original game, without deferral (goal 5).

**High-frequency events require bindings.** Damage, firing, hits, projectile detonation, crushing and order execution, along with object- or type-bound decision events, fire only for the corresponding bindings (section 6a). There is no subscription to all objects. This is an engine hard limit (section 15).

**Saves.** Restore module memory from the snapshot. Neither initialization nor `on_scenario_start` runs again. There is no `on_load`. References in module memory still identify the same objects because handles are creation serials independent of storage positions.

**Budgets.** Each module gets a per-frame operation budget B, independent of the frame's record count and set for the slowest supported platform (section 15). In larger battles more records arrive under the same budget. Defer excess work to later frames. B is both the threshold for starting a call and the limit of one call.

- Charge host calls for work done (section 4). Derived properties evaluated by filter clauses and the per-frame `ProductionAllowed` batch also draw from B. Other decisions have their own budgets (below).
- In the module phase, the first `on_message` or `on_events` call of the frame (a fixed maximum number of records, within the delivery area capacity) always starts, so `ProductionAllowed` using up B cannot hold delivery back. Start another only while this frame's usage is below B. Otherwise they stay queued and go first next frame under the backlog rules below. Call `on_frame` every frame as usual, even with B used up. Each call may use up to B by itself, **never reduced by earlier usage in the frame**, so the worst frame stays below 3B plus one decision call's limit plus the decision budgets.
- Decision calls have their own per-frame budget, which module-phase work does not consume. A backlog would otherwise disable every combat veto, such as a shield, for the whole frame. Call only while usage is below it, with a fixed per-call limit. Once it is used up, skip the module's link in decision chains for the rest of the frame, pass the previous value through and report the number skipped in the cost overlay. There are two exceptions. A `ProductionAllowed` request not asked for lack of budget keeps its last cached result, so a veto does not lapse with the budget. `Defeat` has a separate budget. Once that is used up, defeat no house this frame and ask again next frame.
- Never defer a call midway. Stop a call and disable the module (section 3) only when that call by itself uses a whole B (for decisions, the per-call limit). Earlier usage in the frame never turns a normal call into a fault.
- Cap records delivered per module per frame. Keep records held back by the cap or budget in order, save and hash them, and deliver them first next frame. A short burst delays a module. If the backlog exceeds a fixed number of frames' worth of records, disable the module and tell the player it cannot keep up.
- Section 15.4 gives the values.
- A frame with no `on_frame`, pending records, messages or decision requests makes no in-match module callback.

### 5a. Languages and SDKs

The module contract is the Wasm interface of functions and tables in the appendix. Any compiler producing a module that passes load validation can target it. The engine treats every module alike.

- **Official SDKs: TypeScript and Rust.** Generate both from the same interface definition and machine-readable tables (section 12), with identical capabilities and language-appropriate names. Version them alongside the API. Each includes templates, a build command, watch mode and examples.
- The SDK generates named fields per event kind and reads delivery records in place. Triggers use `.immediate()` for immediate actions, `.enqueue()` for queued callbacks and `.decideSync()` for synchronous decisions (`decide_sync` in Rust). See the [SDK mapping](0001-wasm-extension-modules-api.md#3-sdk-mappings) for the host calls made by each wrapper.

- **TypeScript SDK.** Compile TypeScript to Wasm ahead of time and keep the module small enough to embed in a map. The provisional compiler is AssemblyScript, pinned to an exact version with the SDK, pending a minimal compilation experiment (open question 2). It is not full TypeScript: `tsc` serves editors and type checking, AssemblyScript defines numeric semantics, and synchronous decision handlers must not allocate. The API uses the intersection of TypeScript and AssemblyScript.
  - No `any`, unions other than `T | null`, callbacks that capture outside variables, or `try`/`catch`.
  - Pass grouped arguments as constructed classes or builders, without structural object types or object literals.
  - References are plain `i64` values with no methods or per-reference object allocation. In AssemblyScript an accessor such as `t.health` would require an object per reference. Namespace functions take the reference first, e.g. `Techno.health(t)` or `Techno.setMission(t, Mission.Hunt)`.
  - Every reference is less than 2^53, so a JavaScript-engine-based SDK can store it as a number.
  - The SDK supplies type aliases for `i64`, `i32`, `f32` and references (`Techno`, `House`, `Node`, etc.). Its `tsconfig` excludes `lib.dom`, avoiding a collision with DOM `Node`.
  - Top-level code becomes the initialization function (section 3).

  The same source can be type-checked with `tsc` and compiled with AssemblyScript. The examples in this RFC are valid in both.
- **Rust SDK.** Intended for authors seeking full control and higher performance. The engine's own compatibility tests also use it. References are newtypes and can have methods such as `techno.health()`.
- **Build output.** Both SDKs' build commands produce modules that pass the load checks. When the TypeScript SDK compiles with AssemblyScript, it exports top-level code as the initialization function instead of emitting a `start` section, emits no default `env.abort`, `env.trace` or `env.seed` import, and writes the `vera.*` custom sections after compiling. When a Rust module uses the standard library, the Rust SDK runs wasm-opt after the build to rewrite the reference-types encoding brought in by the precompiled standard library into an allowlisted encoding. `no_std` modules skip this step.
- **Other mod languages.** C, C++, Python, .NET (including C#), Go, Ruby, Elixir, Perl and Zig can all target the same ABI. Each toolchain must emit a Wasm core module meeting section 3's restrictions, with imports and exports matching the appendix signatures. Community SDKs can generate wrappers from the tables in section 12. Toolchains requiring an interpreter or runtime must include it in the module and meet the import and size limits below.
- **Imports.** Modules may import only `vera:*` interfaces. The engine provides no WASI and the project ships no WASI shims. A language whose runtime requires WASI clocks, randomness or files is usable only if its own toolchain removes those imports.
- **Size and features.** A compiler targeting Wasm GC or another rejected feature (section 3) produces a module that fails validation. This applies to some Kotlin, Dart and Java toolchains. A toolchain that bundles a large runtime or garbage collector into every module may exceed map embedding limits. Authors choose the toolchain.

**Host tools.** Standalone tools can embed Wasmtime with the following bindings. [Wasmtime embedding documentation](https://docs.wasmtime.dev/lang.html)

| Host language | Wasmtime entry point |
| --- | --- |
| Rust | `wasmtime` crate |
| C | `wasm.h`, `wasi.h`, `wasmtime.h` headers, buildable with CMake |
| C++ | `wasmtime.hh` header |
| Python | `wasmtime` PyPI package |
| .NET | `Wasmtime` NuGet package |
| Go | `wasmtime-go` |
| Ruby | `wasmtime` gem |
| Elixir | `wasmex` Hex package |
| Perl | `Wasm::Wasmtime` in the `Wasm` CPAN distribution |

### 6. Five concepts

| Concept | Functions | Appendix table |
| --- | --- | --- |
| Objects and properties | `get`, `get_many`, `set`, `modify`, `unmodify`, `find`, `rules_get`, `rules_set` | [Properties](0001-wasm-extension-modules-api.md#7-properties) |
| Variables | `var_get`, `var_set`; declarations in `vera.vars` | [Custom sections](0001-wasm-extension-modules-api.md#custom-sections) |
| Events, including decidable events | `trigger_create`, `trigger_remove`, `trigger_bind`, `trigger_unbind`; exports `on_events` and `on_decide` | [Event kinds](0001-wasm-extension-modules-api.md#4-event-kinds) |
| Actions | `action`, `send_notify`, `order` | [Action kinds](0001-wasm-extension-modules-api.md#5-action-kinds) |
| UI nodes (client) | `client_node_create`, `client_node_remove`, `client_node_find`, `client_set`, `client_set_bytes` | [Client nodes](0001-wasm-extension-modules-api.md#710-client-nodes) |

The infrastructure functions are `frame`, `random`, `log`, `has_kind`, `lookup` and `name_of`.

- The functions form a frozen, fixed-size interface. Event kinds, action kinds, properties, name kinds, node kinds and other enumerations are tables. Minor versions may append entries but may never change or delete existing entries. Modules use `has_kind(table, id)` to check availability and fall back on older engines.
- Every stock or atomic action calls the engine's existing implementation of that operation. Every property write goes through its existing owner.
- Decidable events use game concepts as names (`TakeDamage`, `TargetChanged`), so their names survive refactoring of native functions.
- Use data for very frequent decisions. Adjust target priority with a modifier on `TargetPriorityMultiplier` and combine it with `TargetChanged`. Reserve per-occurrence decision calls for less frequent decisions.

Each action asks one existing owner to perform one game operation. Modules compose actions for more complex behavior.

**Positions.** Actions accept target cells, spawn points and destinations. Object position properties are read-only. Teleports, motion actions and move missions change position. Tier 4 in section 9 allows a projectile position to be set, subject to engine clamps and normal collision checks.

### 6a. Triggers, events, filter clauses and actions

A trigger has at least one event. All events must hold for it to fire. Each event may have filters. Repeat modes follow the original game. A trigger needs an action or a decision handler. A trigger with only a decision handler may have zero actions and must have exactly one decidable event (section 6f).

**Subscriptions.** Modules receive events exclusively through triggers from three sources:

- Code uses `trigger_create` to specify events, filters and actions, and `trigger_bind` to bind the trigger to an object, a type, a weapon, warhead or projectile type, or a cell.
- Mod INI uses `Vera.Tags` to bind triggers in a tag to a type (section 2).
- Map triggers use action kind 100036 (`Module`) in `[Actions]`, with parameter type 4 and the module action name as parameter 2, e.g. `100036,4,giPanic,0,0,0,0,A`. The name resolves to the map module. `mod.name` and `map.name` select the source. With its prefix it is at most 31 bytes of ASCII without commas, semicolons or whitespace. `Wasm:name` is only for named sections, since the kind field in `[Actions]` must be numeric.

Triggers may be created, removed, bound or unbound at any time. The `Enabled` property enables or disables them.

**Bindings.** Object events are raised only for bound objects:

- The object itself has a binding, its type has one (through `Vera.Tags` or a type binding), or its own tag contains the trigger.
- Weapon events (`Fire`, `HitTarget` and `Detonated`) also apply to objects using a bound weapon, warhead or projectile type. A weapon binding applies to every shooter using that weapon, regardless of the shooter's type. The shooter is the subject.

  ```ini
  [RedEye2]                   ; a weapon
  Vera.Tags=MissileFiredTag   ; each shooter using RedEye2 raises its Fire trigger
  ```

- High-frequency events (damage, firing, hits, projectile detonation, crushing and command execution) have no all-objects subscription. When all objects are needed, the SDK's `bindAllTypes` binds the trigger to each type individually, making the cost explicit. The cost overlay flags these bindings (section 13).
- Code may bind a trigger to a cell reference only for events marked for cell binding in [appendix 4.1](0001-wasm-extension-modules-api.md#41-stock-events-061). A cell binding for other events is an invalid argument. Map CellTags retain their stock behavior.
- Global events independent of an object (house events, logic events, `Timer`, `SuperWeaponLaunched`) require no binding.
- A type-bound trigger runs a separate instance on each bound object (section 7).

**Event kinds.**

- **Delivered events** occur at a particular instant, such as taking damage, firing or destruction. The trigger fires at that instant. Its stock and atomic actions execute immediately, and its module actions enter the queue with the event payload (section 5). New VERA delivered events are raised at safe points (section 5).
- **Polled events** describe a condition, such as health below a threshold, money above a threshold or a variable equal to a value. The engine checks the condition natively at the trigger's interval, with a lower bound for object scope (section 15.4). It fires on the transition to true and cannot fire again until a check finds the condition false. Objects are checked on frames where `frame % interval == serial % interval`, using the object's creation serial. House polls use the house array index instead. The delivered `Timer` event is unstaggered and fires once per period measured from the trigger's creation frame.
- Ordinary properties use `PropertyPoll`. To test `HealthPercent < 30` every 15 frames, the SDK provides `Trigger.onProperty(Prop.HealthPercent, Op.Lt, 30, 15).bind(E1).enqueue(onLowHealth).done()`. The engine reads the property directly and tracks condition transitions. Appendix 4.4 lists events requiring extra parameters, composite calculations or timer resets on writes.
- **Decidable events** are marked in the event table. A trigger with a decision handler asks the module synchronously at the decision point (section 6f).
- Each event identifies the object to which it is attached (the subject), and any other object involved, such as an attacker, target, transport or producer.
- Record values are integers or references. Damage is an integer as in the original game. The simulation API has no floating-point values.

**Filter clauses.** An event accepts up to N clauses (section 15.4) and all must hold (AND).

- A clause takes the form `Subject|Other|House .property operator value` or `Var(scope,name) operator value`. The operators are `=`, `!=`, `<`, `<=`, `>` and `>=`; the value is constant. `House` is the trigger's house: use the house in its definition if nonzero, otherwise the subject's current owner. Without a subject (global event), use the map's neutral house (`Neutral`).
- The engine checks clauses natively before queueing a record. Failing events are never delivered. Ordinary property clauses are free against the module's budget. Evaluating derived properties such as `NearbyEnemyCount` and `TargetInRange` charges their cost to the owning module (section 5).
- `find` uses the same clause layout (section 6c).
- INI examples are `Where1=Subject.HealthPercent<30` and `Where2=Var(Subject,Panicked)=0`. See "INI format" below.
- Common computed conditions become derived properties, such as `NearbyEnemyCount` and `TargetInRange`. Write other conditions as `if` statements in code. Implement disjunction, cross-object comparisons and counting in code.
- Red Alert 2 variable events (27, 28, 36, 37) remain available.

**INI format.** Mod type bindings use named sections and map triggers use the stock comma-separated encoding. Neither replaces the other. `Vera.Tags` in a map is ignored with a warning, and map `[Events]` and `[Actions]` take no filter clauses.

- Keys that also work without modules begin with `Vera.`, and sections with `Vera`. `Wasm` is only for module policy, embedding and calls by name: `[VeraWasm]`, the two pack sections and `Wasm:name`.
- Trigger sections accept only the keys in the appendix. An unknown key, a repeated key, both numbered and unnumbered forms of one key, a gap in `Event<n>` or `Action<n>` numbering or an unresolvable name refuses the match, naming section and key. Numbering starts at 1, which an unnumbered key means.
- A text argument must be last and takes the remainder, which may contain commas but no semicolons.
- A physical line including its key is at most 511 bytes. An overlong VERA key is an error and is never truncated.
- Without a module, a mod may declare variables in `[VeraVars]` in `rulesmd.ini`, e.g. `Panicked=Object,Private,0` (scope, visibility, default, as in `vera.vars`). They share a table with the mod module's variables, INI first in file order, then the module. A duplicate name refuses the match. Maps cannot declare variables with this section.

Stock, atomic and AI actions execute when the trigger fires. Module actions queue for delivery. See the [binding example](#binding-a-trigger-to-a-type) and the [event tables](0001-wasm-extension-modules-api.md#4-event-kinds) for fields, binding scope and version tiers. Modules keep historical values in variables or module memory. INI supplies no recorders, operators or expressions for historical values.

### 6b. Actions

The [action kind tables](0001-wasm-extension-modules-api.md#5-action-kinds) list stock actions 0..145 (plus 146..149 with Ares enabled), atomic, AI and module actions by number. A kind has two uses:

- Module code executes `action(kind, target, params, len)` directly against an explicit object, house, cell or projectile reference. Trigger, tag and team references go in `other` in the parameters. Actions that create objects return the new object's handle. Other actions return a value of 0. Both return status in the same call.
- A trigger's action list contains the same kind with a target selector and parameters ([trigger definition](0001-wasm-extension-modules-api.md#fixed-layouts)).

Setting health, rank or ammo directly has one write contract. Code uses `set` on the property, and a trigger encodes the same write with `SetProperty`. Both go through the same state owner. The SDK may generate named property setters. Increments, damage attribution and arguments resolved at trigger time have distinct gameplay semantics and retain their actions.

**Atomic actions** are instantaneous game operations, each with one owner: fire a weapon, launch a superweapon, cause damage, kill, remove, create an object, teleport, detonate a warhead, write a variable, modify a property or end the game for a house, among others ([appendix 5.3](0001-wasm-extension-modules-api.md#53-atomic-actions)). Damage follows warhead and armor rules and takes a source for kill credit and veterancy. Setting health directly grants no credit. Teleporting to an occupied cell follows the specified rule for choosing a nearby cell or failing.

Trigger actions select their target with `ActionTarget`:

- `Subject` (default): the event subject.
- `Other`: the other object in the event.
- `TagHolder`: the object holding the trigger's tag, the cell for a CellTag, or the object raising the event for a bound trigger.
- `Projectile`: the projectile in the event.
- `House`: the house in the parameters, falling back to the trigger house for 0.
- `Cell`: the cell in the parameters.

Parameters have a fixed layout with an ID, two small integers, two integer values, a cell, a house, a reference and text ([action parameters](0001-wasm-extension-modules-api.md#fixed-layouts)). Actions available only in code, such as creating a projectile, may carry additional variable-length data. Actions without a target, such as `SetTechLevel` and `Log`, ignore it.

Supply a waypoint as a [waypoint reference](0001-wasm-extension-modules-api.md#references). At `trigger_create`, the engine resolves it to its cell because waypoints never move. A missing waypoint is a map data error: `trigger_create` returns `(0, NotFound)`, creates no trigger, logs the failure and leaves the module enabled. For an INI trigger the lobby reports the error at load time.

Fixed rules resolve the remaining arguments:

- Explicit houses (a `House` target, new owner for `SetOwner`, or owner of a created object) come from the house parameter. A zero value selects the trigger house in a trigger (section 6a) and is invalid in a direct call (section 3). Stock actions implicitly using the trigger house, such as `LightningStormStrike`, use that same house.
- In a trigger, the optional source of damage, a kill or a detonation (`Kill`, `Damage`, `DetonateWeapon`, `DetonateWarhead`) is the event's other object. If absent, the action runs without a source and grants no kill credit. Direct calls use the reference parameter.
- A second object acted on (a passenger) comes from the event's other object in a trigger and from the reference parameter in a direct call. If absent, or if `FireWeapon` has no cell, other object or current target to aim at, the action returns `Gone` and is skipped.
- An object's cell is the cell it occupies. A `Projectile` target uses the cell beneath it. Coordinates are the cell center, except that `Projectile` uses the projectile center. When `CreateObject` targets a building the new object leaves through the factory's native exit ([appendix 5.3](0001-wasm-extension-modules-api.md#53-atomic-actions)).
- [Appendix 5.1](0001-wasm-extension-modules-api.md#51-using-actions) specifies target and field use per action and rules such as how `FireWeapon` aims.

Validation:

- `trigger_create` checks every action's parameters. An always-invalid target kind (e.g. `Kill` targeting `Cell`) or missing required field (the cell for `LaunchSuperWeapon`) disables the module (section 3). Direct `action` calls perform the same checks.
- INI atomic actions are validated at load time and reported in the lobby. Section 6a defines their spelling.
- A target found unsuitable only at execution (e.g. `Kill` on a `TagHolder` that resolves to a cell) is a data error. The action returns `Refused`, logs the status and is skipped. The module stays enabled.
- Atomic actions execute immediately and may raise new events, allowing same-frame chains subject to the limits in section 5.

### 6c. Objects, properties and modifiers

**References.** Every reference is an `i64`: an object handle (unit, building, projectile, house, team, trigger, tag, modifier, superweapon or client node), type reference, cell reference, waypoint reference or global rules reference. The SDK encodes all values below 2^53. Zero means “none” ([appendix conventions](0001-wasm-extension-modules-api.md#references)).

**Properties.** `get(ref, prop)` reads a property, `get_many(refs, n, props, m, out)` reads m properties of n references in one call, and `set(ref, prop, value)` writes a property. The tables group properties by reference kind (object, house, cell, types and rules, globals, triggers and tags, modifiers, client nodes). Each specifies:

- Value type: integer, boolean, fixed point, enum, reference, ID or string.
- Write access and timing: anytime or only in `on_rules`. Writes go through the property's existing owner. Writing `Owner` transfers ownership, and writing `Mission` queues a mission like a player order.
- Client visibility (section 10).
- Whether modifiers apply.

`get` reads the effective value after applying all modifiers to the base value. String properties return a string ID for `name_of`. The SDK returns the string directly.

**Derived and viewpoint properties.** The engine computes properties such as `HealthPercent`, `NearbyEnemyCount` and `TargetInRange`. A few depend on the viewing house (`Relation`, `Revealed`, `Buildable`, `Placeable`). Filters use the trigger house, `find` uses the query's `viewpoint`, and the client uses the local player. Direct simulation `get` has no viewpoint and returns `Unsupported`. `Placeable` also requires a type and is evaluated only in `find` (including `client_find`) with the query's `type`. Other uses return `Unsupported`.

**Modifiers.** `modify(ref, prop, op, value, frames, source, group)` adds a modifier to a modifiable property of an object or house and returns its handle. `unmodify(modifier)` removes it early. SDK methods are `Modifier.add`, `Modifier.multiply` and `Modifier.override`, for example `Modifier.multiply(unit, Prop.SpeedMultiplier, fixed(0.5), 150)`. Each calls `modify` once. Optional source and group encode directly as integers and both default to 0.

- Operations are addition (`add`), fixed-point multiplication (`multiply`) and replacement (`override`). `frames` is the duration in frames, negative for indefinite. `source` is the source object for kill credit and source rules, or 0. `group` is the exclusion group, with 0 for ungrouped.
- The engine owns, saves and hashes stacking, exclusion groups and expiration.
- **Composition.** Let P be the native and Phobos value at that read stage, A the sum of additions, M the multipliers in handle order and O the latest override. A multiplier property is P × (1 + A) × all M, a quantity property is (P + A) × all M, and with an O the result is O. Booleans, IDs, enums, masks and colors accept only override. An operation the property does not accept, or a negative multiplier on a multiplier property, returns `Refused`. Range is first computed in Phobos order, then multiplied by (1 + A) and each M, truncating toward zero after each multiplication, with additions last. O replaces both the Phobos and module terms there. Phobos terms that are warhead-filtered or chance-based stay in their own stage and are not replaced by O. Clamp only module terms, never P. Except for per-weapon properties such as range, compute and cache results on change (section 15).
- **Precision.** Modifier fractions are Q16.16. Phobos effects keep their double values and are not converted to modifiers. Stages and operation order agree, but a fraction without an exact representation may round to a result differing by 1.
- **Stacking.** Group 0 stacks. A nonzero group is keyed by (origin, object or house, property, group) and adding replaces its member. Each (origin, object or house, property) has a layer cap (section 15.4), to which a replacement adds nothing. Exceeding it returns `Refused` and evicts nothing.
- **Refresh.** `modify` has no in-place refresh. A same-group replacement gets a new handle and takes the new value, source and duration, even if shorter, moves last and raises `ModifierExpire` then `ModifierStart`. Refresh by source, stack counts and cooldowns belong to INI behaviors or Phobos effects and are not `modify` arguments.
- **Duration.** Added in frame F with `frames = N`, a modifier expires at the start of the module phase of frame F + N. Zero counts as 1 and negative is indefinite. Leaving the map, riding a transport or being frozen does not pause it. It survives owner changes and in-place conversion and is not transferred when the object is replaced or removed.
- **Origin.** Group replacement and `RemoveModifiers` act only within their origin. `unmodify` removes exactly the given handle, whatever its origin.
- Modifiers are objects with properties (frames remaining, target, source, group). `ModifierStart`, `ModifierStacks` and `ModifierExpire` report them. A house modifier raises these as global events with that house in `house` ([appendix 4.3](0001-wasm-extension-modules-api.md#43-new-delivered-events)).
- Earlier persistent effects become modifiable properties, including multipliers, range, cloaking, firing prohibition and death prevention ([appendix 7.2](0001-wasm-extension-modules-api.md#72-modifiable-properties)). Special cases use the `ImmunityFlags` attack immunity bitmask, overrides of `PrimaryWeapon` and `SecondaryWeapon`, `LaserTrailType`, the `AttachAnim` action, timed `Convert`, future motion actions for throws and pushes, and `Teleport`. Transformation, chrono movement and motion are mutually exclusive. Their owner ensures only one is active.
- Text, numbers or buttons above an object belong to client nodes and are not in version 1 (section 10).
- Enabling the module feature (section 14) enables the modifier owner independently of Phobos. Only the Phobos switch enables reading its AttachEffect INI keys (`AttachEffect.*` on types, weapons and warheads, and `[AttachEffectTypes]`), preserving stock behavior. The Phobos AttachEffect owner holds and times them per effect instance and composes them before modifiers at the same read stage. They are not modifiers: they do not appear in `find(Modifiers)` and raise no modifier events. Shared Phobos and module modifier code ships in every build. Runtime switches enable the respective inputs (section 14).

**Search.** `find(query, len, out, cap)` searches objects, houses, cells, types, teams, triggers, tags, modifiers or superweapons. It writes to a caller-supplied buffer, writes the count written and continuation position to a small result struct and returns the status.

- Search the whole map, a cell plus radius, or a rectangle. Filter by type or house and optional clauses in trigger layout, with the candidate as `Subject`.
- Results follow creation serial independently of engine storage order. Cells use cell index, houses use house array order, modifiers use insertion order and types use type ID.
- SDK `.first()` takes the first match. `.page(buffer, after)` fills an existing buffer with one host call per page. Continue from the previous page's last sort position even if that object was deleted. Each page reads the world at call time, so newly added matches after the cursor may appear in later pages.
- Map and cell queries use cell reference properties: terrain type, height, overlay, ore, passability by movement type, revealed state, placement and CellTag.
- Earlier probes for firing on the current target, primary weapon range and placement become derived or viewpoint properties: `TargetInRange`, `CanFireAtTarget`, `Placeable`. Probes for arbitrary weapons and targets are deferred. Use `find` with `Placeable=1` and module code to locate the nearest legal cell.

See the [property tables](0001-wasm-extension-modules-api.md#7-properties) and [search scopes](0001-wasm-extension-modules-api.md#82-search-scopes-find). Client reads expose only what the local player's own interface can show (section 10).

### 6d. AI

Computer houses keep their AI. The existing concepts cover AI:

- **Definitions** are INI: task forces, team scripts, team types and AI triggers (`[TaskForces]`, `[ScriptTypes]`, `[TeamTypes]`, `[AITriggerTypes]`), with the sections they list, form the AI document (`AIMD.INI` plus map, with unchanged layering). `rules_get` reads them from the merged AI document.
- **Operations** are action kinds: start a team (stock action 4 or `CreateTeamUnlimited` to bypass the count cap), disband a team, add or remove members, and stock AI actions 74 through 79.
- **State** is exposed as properties: team type, script, script cursor and success flag, AI trigger enabled flag and the house-wide AI trigger switch. Writing `Script` or `Cursor` changes a running team's script or jumps to a line.
- **Team script actions** remain a numbered table ([appendix 6.2](0001-wasm-extension-modules-api.md#62-team-script-actions)) used as data in `[ScriptTypes]`.

In version 1, `on_rules` cannot write the AI document: `rules_set` on those four list sections or the sections they list returns `Refused` instead of having no effect silently. Define AI in `AIMD.INI` or the map. Module writes are left to a later minor version.

All AI operations use the existing AI owner. A module can also command a computer house as a player would with `order` (section 6g). Orders go through the command owner, and the native AI keeps running beside them.

### 6e. Variables

Variables replace earlier marks with scoped `i64` integers and explicit visibility.

- **Scope:** global, house or individual object. In `var_get(scope, var)` and `var_set(scope, var, value)`, 0 selects global scope. Otherwise supply a house or object reference. A mismatch with the declaration is an invalid argument.
- **Visibility:** private (no clients for a global, only the owning player for a house or object), allies (also allied players), in sight (individual objects only, also players currently seeing the object), or public (everyone).
- **Declarations.** The `vera.vars` Wasm custom section declares name, scope, visibility and default. The SDK emits it at build time from the project configuration. One declaration generates simulation read/write and client read-only accessors. Global variables use `Wave.get()`; house and object accessors take the matching reference. Accessors resolve and cache match-local IDs at initialization. Filters and atomic actions accept generated variable names. Client reads still check visibility. Dynamic cross-origin references resolve by name. `lookup` returns a variable ID for a name. Names are visible in the module bytes.
- **Red Alert 2 local and global variables** are fixed exceptions: global scope, private, default 0, values 0 or 1 (any nonzero write becomes 1) and fixed IDs ([variable IDs](0001-wasm-extension-modules-api.md#ids)).
- As with map actions, setting a variable resets the timers of triggers waiting on it (events 27, 28, 36, 37 and `VariablePoll`).
- Object-scoped variables disappear with their object. The engine owns all variables, indexes them by reference in side tables, saves and hashes them (section 15).
- Filters can test variables (`Var(Subject,Panicked) = 0`), trigger actions can write them with `SetVar`, and `find` can search with variable clauses.

**Campaigns.** Every mission is a fresh load. Module memory, globals and tables, random streams, declared variables of every scope, triggers, modifiers and queued records start from their initial state. Only the engine's campaign carry-over record persists. Modules use it through variable IDs 100..149 (Red Alert 2's 50 globals, 0 or 1). An authored series must set `Globals=Yes` in `[CarryOver]`. Carried values are written back before `on_scenario_start`, and `on_rules` does not see them. Restarting or retrying uses the record from entering the mission. Loading restores all module state from the save.

### 6f. Decidable events

Decidable events replace the former override points. The event table identifies which events are decidable and defines their result slots. For `TakeDamage` the result is final damage after armor and immunity are settled, for `TargetChanged` the selected target, for `FatalDamage` the death mode, and for `Fire` a firing plan. Decision-only events such as `ProductionAllowed` and `CommandAllowed` have no delivered form.

- A trigger with a decision handler (SDK `.decideSync(fn)`) raises a decision request at the decision point in place of safe-point delivery. Requests follow the event table's binding scopes and filters. Global events such as `Defeat` need no object binding.
- The engine calls `on_decide(ptr, count)` synchronously. Each fixed-size decision record contains the event record, native value, previous result, result slot and handled flag ([fixed layouts](0001-wasm-extension-modules-api.md#fixed-layouts)). Larger results (firing plans, paths, per-frame projectile steps) go in the request's accompanying result area.
- A decision point is where the engine commits a result, such as setting a target during an object's turn, applying damage or firing a shot. Calls follow deterministic engine commit order. The engine never asks for decisions during speculative or parallel precomputation (target acquisition previews, pathfinding batches, pursuit scoring), whose answers may be discarded.
- In version 1 `count` is always 1. Each request and each chain link is its own `on_decide` call. The next link's `previous`, its budget check and its `random` substream follow from the state left by the preceding call. Where the engine commits a batch at one point (e.g. `ProductionAllowed`), order requests by subject creation serial, then house array index, then `id`, and commit after every request is decided. Several decision triggers of one module form links in creation order. The delivery area must hold one record and its result area, whose size is fixed per event kind (section 15.4). If batched decisions are needed later, add them as a new export without changing `on_decide`.
- Handlers may use only [imports](0001-wasm-extension-modules-api.md#11-imports) marked `decide`. Calling a write function disables the module.
- A handler can keep state in module memory. Calls occur only at commit points in deterministic order, giving identical state on every machine. Handler `random` draws from a derived substream (section 4), so decision call counts do not advance the main module RNG.
- Section 2 defines mod/map chaining. Section 5 defines non-reentrancy and budgets.
- Local or frequent readers, including the sidebar and AI, read cached results. For example, `ProductionAllowed` is decided once per frame for each house and bound type, and the sidebar displays that result.

Version 1 freezes three decidable events: `TakeDamage` (settled final damage), `ProductionQueued` (whether to enqueue) and `Defeat` (veto engine defeat). When `ProductionQueued` is refused for a computer house, the engine clears its choice in that production category so it chooses again.

- **Preview, not frozen in version 1:** `ProductionAllowed` (expressible with `AllowType`) and `CommandAllowed` (must first cover computer AI missions).
- **Shields:** preview adds `DamageIncoming` (damage at the receive entry, with shield damage computed by the engine against the armor the trigger declares) and the modifiable property `TargetArmor` (armor for targeting and fire permission). `TakeDamage` asks only when settled damage is above 0, so it cannot implement a shield with its own armor. Both freeze in version 1 only after passing the cases below. Phobos-compatible shields remain in the engine, and modules need not reproduce them point by point.
- **Later:** decision forms of `TargetChanged`, `Fire` and `FatalDamage`, plus `Launch` and `ProjectileFrame`. Modules probe with `has_kind`.

**Shield acceptance.** Write a minimal shield and an overcharging shield with the public SDK (distance 0, multiplier 1):

| Case | Expected |
| --- | --- |
| Hull 0%, shield armor 100%, shield 200, incoming 50 and 100 | Shield loses 50 and 100, health unchanged (no minimum of 1) |
| Shield 30, hull 50%, shield armor 100%, incoming 100 | Shield 0, health loses 35 |
| Shield 20, hull 100%, shield armor 50%, incoming 100 | Shield 0, health loses 60 |
| Negative incoming, shield not full | Shield recovers, health unchanged |
| Ignore-defenses, Iron Curtain, immunity | Not asked, native settlement |
| Falloff, cap or armor multiplier | `value2` equals the engine damage kernel's value |
| Direct-fire weapon, hull 0%, shield armor 100% | Targets and fires while shielded, stops after the break (one frame late is allowed) |
| Several hits in a frame, self-damage, decision budget used up | As in section 5, identical on every machine |
| Saved while overcharging or one hit before the break | Continuing gives the full hash of a continuous run (section 15.1) |
| No module | State hash identical to stock in every frame |

### 6g. House orders

`order(house, ptr, len)` lets a simulation module command a computer house the way a player commands their own. It takes the `Order` layout and [command table](0001-wasm-extension-modules-api.md#83-enums) that client modules pass to `client_send_order` (section 10), covering unit orders, production, building placement, rally points, repair, selling and superweapons.

An action performs an operation directly. An order asks the house to do what its player could, at a player's cost and under a player's checks. `CreateObject` makes a unit for free, while a `QueueProduction` order pays for it and waits for a factory.

- **Houses.** `house` must be a house no player controls (`IsHuman` is 0 and no campaign player controls it) that is not defeated. Otherwise `order` returns `Refused`. A player's house takes orders only from that player, including the player's client module. This limits `order` only: a `Mission` write or the `SetMission` action gives any object, a player's included, a mission through its own house, as its player's click would. A null or non-house reference is an invalid argument (section 3).
- **Timing.** The engine queues the order. An order issued in frame F executes in frame F + 1, in the step that executes received player commands, wherever the module phase falls. The command step follows object turns, so units act on it from the object turns of frame F + 2. That step executes the frame's player commands first, then the mod module's orders, then the map module's, each in issue order. Orders have no network command delay. Every machine issues them in lockstep, and the native AI that runs beside them has no delay either.
- **Admission.** Natively the command step accepts commands only for houses a player controls. It also accepts module orders for computer houses. Stock play never sends a command for a computer house, so stock play is unchanged.
- **Validation.** At execution an order passes the same checks as the identical command from the house's player, against the state at that moment, such as unit ownership, prerequisites and building placement. Units that ignore player orders (`IgnoreOrders`) also ignore module orders. Units that are gone or no longer belong to the house drop out of the order. An order that fails does nothing, as for a player. Production spends credits and build time as it does for a player. At issue time `order` checks only the house, the command table (target and value kinds, every unit entry an object handle, a unit list only where the table lists one) and the per-frame limit. A violation returns `Refused` and queues nothing. An unknown command returns `Unsupported`.
- **Results.** An executed order raises the same events and decisions as the identical player command, including `Commanded`, `CommandAllowed`, `ProductionQueued` and `ProductionCancelled`. `order` executes nothing during the call and raises no events. The module sees the outcome through properties, searches and events in later frames.
- **Costs and limits.** Charge each call per order and per unit entry. Cap orders and unit entries per module per frame (section 15.4).
- **Saves, replays and reconnects.** Orders never enter the network command stream. Save and hash orders waiting for the next frame. Replays and reconnecting players resimulate the modules, which issue the same orders again, so replays need not record them.
- **Native AI.** In version 1 the house's native AI keeps running (section 6d). It may retask the same units or queue other production, and modules cannot take the house over. Modules can narrow it with `AiTriggersActive`, `AutoBaseBuilding` and refusing `ProductionQueued`. Full takeover is a post-freeze candidate (see "Possible future directions").
- **Disabled module.** Discard its queued orders (section 3).

The SDK builds orders with the builders `Client.order` uses ([appendix section 3](0001-wasm-extension-modules-api.md#3-sdk-mappings)).

### 7. Runtime triggers and tags

Modules create triggers using the same model as maps:

```ts
// TypeScript (AssemblyScript-compatible). convoy is the transport handle to watch.
const americans = Rules.country("Americans");
if (americans != NO_HANDLE) {
  const house = Find.houses().country(americans).first();
  if (house != NO_HANDLE) {
    const t = Trigger.on(Event.Killed).house(house).bind(convoy).enqueue(onConvoyLost).done();
    if (t != NO_HANDLE) Trigger.setEnabled(t, false);   // Pause; write true to resume.
  }
}

Trigger.on(Event.CrossesHorizontalLine) // Cell event marked for cell binding in appendix 4.1.
  .bind(Cell.at(40, 52))                // Cell binding.
  .enqueue(onLineCrossed).done();

Trigger.on(Event.TakeDamage).bind(Rules.technoType("E1")).enqueue(giPanic).done();   // Like Vera.Tags=: each E1 runs its own instance.
```

- Newly created triggers join map triggers in the existing trigger runtime list. They have deterministic IDs disjoint from map IDs, and the trigger runtime saves and hashes them.
- Bindings (`Vera.Tags` in INI, `trigger_bind` in code) are separate subscriptions and leave the object tag slot available. Preplaced map objects keep their tags and also receive bound triggers. A type binding made in code applies immediately to every object of that type and is available to both mod and map modules.
- An object has one tag, and a tag may link multiple triggers. Tags support Red Alert 2 compatibility. Map triggers attach through tags. An object's `Tag` property is readable and may be set to another existing tag, replacing the old tag as in the original. Modules use object bindings and cannot create tags. Multiple tags on one object would require a separate VERA RFC.
- An object with a binding, or with a tag containing a module trigger or a trigger on a new VERA event, gets its own flag. Event sites then raise those events for the object even when its type has no bindings (section 15).
- Each bound object runs its own instance of a bound trigger. Repeat state is per object, so a one-shot trigger fires once per object. Type bindings share one trigger definition. Per-object state is allocated only when it differs from the default, such as after a one-shot trigger fires (section 15). Disabling or removing a trigger affects every instance. If the definition's house is 0, the trigger house is the subject's owner at firing time. The tag holder is the object raising the event.
- Removing an object drops its bindings. The trigger remains and may be removed with `trigger_remove`. `trigger_unbind` removes bindings early. Object, tag and trigger removal use existing trigger-runtime cleanup to detach links.
- Numbers: event and action tables are independent, and both keep stock and Ares numbers (events 0..88, actions 0..149). Below 100000 there are only meanings that stock or other extensions already have. New VERA kinds are allocated in 100000..199999, and an allocated number never changes or is reused. In version 1, stock map triggers accept only action 100036 (`Module`) from that range. Another action in that range in `[Actions]`, or any event from it in `[Events]`, refuses the match and names the trigger, because stock treats an unknown event as true. Maps with module actions set `VeraMinVersion`.

### 8. Changing rules

| When | What may change | Mechanism |
| --- | --- | --- |
| `on_rules` | Any rules key, new types (not AI definitions, section 6d) | `rules_set(section, key, value)` takes INI text and uses native INI readers, clamps and postprocessing. Included in the lobby hash. |
| During a match, by type | Nothing in version 1 | `rules_set` and `set` on type or global rules references return `Refused`. Use properties, actions and modifiers for per-object changes. |
| During a match, by object | Health, owner, mission, rank, ammo, variables and modifiable properties such as multipliers | Existing per-object owners through `set`, `modify` or actions (section 6c). |

- `rules_get(section, key, out, cap)` reads the raw string except during module initialization. Common keys also have typed properties on type and global rules references ([appendix 7.7](0001-wasm-extension-modules-api.md#77-types-and-rules)), giving the parsed value the engine actually uses. Keys the engine never reads are accessible only through `rules_get` on merged text. During `on_rules`, `get` sees values processed before the current layer. Writes in this call become visible to `get` after layer processing. A `set` on a type reference is equivalent to calling `rules_set` with the value's INI text.
- A later minor version may open in-match changes key by key. Each key must first list all its readers and state what happens to existing objects, projectiles in flight, production in progress and timers, and which caches to invalidate. Values go into an overlay the simulation owns, saves, hashes and replays, and every reader sees new values only at frame boundaries.
- New types cannot be added during a match.
- When Ares or Phobos is enabled, modules see its rules keys and can change them in `on_rules`. They are likewise read-only during a match.

### 9. Example: projectile trajectories

All four tiers run in `rules.wasm` because they determine what gets hit. `ui.wasm` can add visual effects such as trails and lighting.

| Tier | Mechanism | Cost |
| --- | --- | --- |
| 1 | Set existing keys in `on_rules` (`Arcing`, `ROT`, `Vertical`, and Phobos `Trajectory` when enabled) | No in-match cost |
| 2 | Weapon firing or projectile detonation triggers for additional projectiles, splits at the detonation point or chains | Per event, only for bound types |
| 3 | Decidable `Launch` returns a path (appendix `PathSpec`: Bézier, parabolic, spiral or waypoints). The engine advances the projectile along it using fixed point. | One decision per projectile |
| 4 | Per-frame decidable `ProjectileFrame` runs in the engine's own projectile advancement order, before each bound projectile moves. The handler reads velocity (`VX`, `VY`, `VZ`) and position, then supplies new velocity, position or module data, or requests detonation in the result area (appendix `ProjectileStep`). | One call per bound projectile per frame |

- Tiers 3 and 4 are deferred. They add only event kinds and require no new exports.
- The module path replaces the "compute candidate position" step of native advancement (homing or native trajectory). Before it, waiting animations, lost targets and frame fuses run as usual, and a projectile ended by them is not asked that frame. After it run, as usual and in order, the ordinary trajectory checks (ground, bridge, wall, building, leaving the map), range, firestorm walls, shared collision probing, proximity fuses and detonation. A normally homing projectile on a module path also gets the ordinary trajectory checks. Collision, damage and warheads stay with their owners.
- The engine clamps output velocity and per-frame displacement (section 15.4). A supplied position is the candidate, otherwise current position plus velocity. Requesting detonation is a hit at the candidate, but leaving the map takes priority: the projectile is removed without detonating, as natively. Collision checks the candidate point only, not the segment to it.
- Each projectile has 16 bytes of saved and hashed module data (`Data0` at bytes 0..7 and `Data1` at 8..15, each a little-endian `i64`). Decision handlers cannot call `set`. `ProjectileFrame` writes back module data through `ProjectileStep`, retaining the read-only handler contract.
- To split at the detonation point, use the coordinates in the `Detonated` record ([appendix 4.3](0001-wasm-extension-modules-api.md#43-new-delivered-events)). The module action uses `CreateProjectile` with absolute starting coordinates in `ProjectileSpec` to spawn new projectiles there. The original projectile in the record is already gone.

**Salvos.** Attach a module action to the weapon's `Fire` event and use `CreateProjectile` for each additional projectile, with its own type, module data and starting offset. Distinct custom paths require the future `Launch` event. Until then each follows its native trajectory toward its own target. The action reads the shooter, target and native projectile from the event (SDK fields `shooter`, `target`, `projectile`). The native shot launches during the unit update, while module shots launch in the module phase. They lag by exactly one advancement and start from the shooter's position at that time.

**Decidable `Fire`** avoids this delay. Its result is a firing plan: a list of projectiles to spawn in the result area, each with type, weapon, target, module data and start offset (appendix `ProjectileSpec`). The result slot holds the entry count.

- The engine spawns the listed projectiles at the instant the native shot would launch and omits the native projectile. A handled empty plan fires nothing. An unhandled decision retains the previous result, starting with the native shot at the first link.
- All other native firing effects occur normally: rate-of-fire timer, ammo use and reload, burst counter, sound, muzzle animation, recoil and firing-related uncloaking or reveal.
- Planned projectiles raise `ProjectileIncoming` and the target's `AttackedBy` as native projectiles do. The delivered `Fire` event still occurs with `projectile` equal to 0.
- If mod and map both decide the shot, the map receives the mod's plan and may keep, edit or replace it.

### 10. Client modules

After building the UI tree and reading the INI layout, the engine calls the client's start function. Each render frame delivers events before calling the render function. See [client exports](0001-wasm-extension-modules-api.md#22-exports).

**UI tree.** The engine owns the nodes, lays them out and draws them. Modules find nodes by part name in the active layout (the section name without the layout prefix, i.e. the name in `Parts=`), create or remove nodes and update properties. The engine batches changes per render frame.

**Node ownership.** Every node records its source: engine and mod INI layout, mod module, map INI layout or map module.

- The mod client module may read, write and remove any node to replace or rearrange the match UI.
- With `MapUI=overlay`, the map client module may find and read any node but may write and remove only the map's nodes. Other writes and removals return `Refused`. Its new top-level nodes are drawn above the match UI in the overlay.
- `MapUI=full` gives the map module the same permissions as the mod module. `MapUI=no` skips loading the map's `ui.wasm` and displays a lobby notice.

Authors supply two parts:

- **Static layouts** use INI, without precompilation, and an entirely static UI requires no module. Mods use [`veraui.ini`](../interface-layout.md) with its existing location, side selection, player overlay and comparison rules. Modules must not assume that a part exists. A map writes `[VeraUI]` (`Parts=` lists top-level parts) and `[VeraUI.<part>]` (keys as for added `veraui.ini` parts) in its own file. It is the same for all sides, only adds parts owned by the map and puts top-level parts in the overlay. `MapUI=no` ignores these sections. `MapUI=full` widens only map module permissions and does not let map INI replace the mod layout. Player layouts do not alter map parts.
  - Both add the `Image`, `Bar` and `List` types. A `Button` may have just `Label` or `Art` without `Command` (producing only `Click`). `Text` may use `Label` instead of `Value`. Rows and columns do not nest in version 1.
  - Lookup by name searches the module's own source first, then engine, mod and map, taking the first created within a group.
- **Dynamic behavior** uses module code to find and update nodes by name or create new nodes.

See [client nodes](0001-wasm-extension-modules-api.md#710-client-nodes) for INI node kinds, layout keys and writable properties. TypeScript modules may use React- or JSX-style libraries internally.

**Other client capabilities.**

- Version 1 has no per-object display (`Decoration`) or map markers (`MapMarker`), which are left to later minor versions. Gameplay properties such as cloaking belong to simulation modules.
- Local actions (`client_action`) play sounds and EVA voices, move the camera, select locally, flash an object, display a message or register a hotkey. Camera, cursor and local settings are also client-owned references. `Camera.cell()` reads the camera cell and `Camera.jumpTo(cell)` jumps to a cell. Lepton-precision camera writes are left for later. Version 1 local settings are the read-only `Viewer` and `IsSpectator`. Modules cannot read or write other player settings.
- Clicks on clickable nodes do not reach the map. Map clicks reach the module as `MapClick` client events and normally also reach the game. Setting the cursor's `CaptureMapClick` to 1 sends map clicks exclusively to the module. Game hotkeys take precedence over conflicting module hotkeys. The client may read the cursor's screen position and the cell beneath it (appendix 7.10).
- Unit selection and button clicks are client events. To change the game, send a message for the simulation module's `on_message`, keeping multiplayer and replays consistent.
- Client modules can issue standard player orders (move, attack, deploy, build, etc.) through `client_send_order`. The engine validates these like all player input. Simulation modules use the same command table for computer houses (section 6g) and give any house's units missions (`SetMission`). Messages carry custom logic.
- Images, animations and models are referenced by name, independent of file format, allowing future asset formats (a separate RFC) without API changes.
- `client_log` writes to the in-game console.
- Player-installed UI modules are deferred. The mod or room may prohibit them.

**Map text.** Version 1 adds no translation format for maps and uses CSF in the files distributed with the map: one language per map, under existing CSF rules (`ra2md.csf` replaces the whole table, additive tables require Ares). A standalone map file cannot carry a string table and uses existing CSF entries or literal text. The simulation only passes labels and never looks up CSF or translations, so players' language files do not affect simulation results.

**Storage.** Each mod and each map has its own quota-limited local key/value store for settings, achievements and progress shown only locally (it is not campaign carry-over, does not roll back with results, restarts or loads, and section 6e covers cross-mission state), (`client_storage_read`, `client_storage_write`).

- A mod's store is keyed by its `[Mod] Name`. A map's store is keyed by its storage ID (`[VeraWasm] StorageId=`, defaulting to the map filename). Edited maps retain the store if the ID stays the same.
- A map client module uses the map's store even under a mod. A downloaded map cannot read or change mod data.
- Only client modules access storage. Data affecting a match enters the simulation by message and becomes part of the command stream.
- Keys are 1 to 64 ASCII bytes. A single value is at most 16 KiB. Each mod or map gets at most 256 keys and 256 KiB total, including keys and values. An over-limit write returns `Refused` and leaves storage unchanged. Writing a zero-length value deletes the key. Limits are fixed per API major version (section 12).

**Notifications** travel from the simulation module to clients.

- Resolve a name once to an ID. For notification names `lookup` returns the 32-bit FNV-1a hash of the UTF-8 name after ASCII lowercasing (fold only `A` through `Z`). This is identical on every machine and on both sides. A name hashing to −1 is rejected at load time (section 3, step 7).
- Simulation modules declare the notification names they send. At build time the SDK writes the module project's configured list into the `vera.notify` Wasm custom section (the configuration is SDK documentation, outside the engine API). Client modules declare no names. A declared name may have no client receiver.
- `lookup` returns `(−1, NotFound)` for a name the simulation module did not declare, or, on the client, one no simulation module in this match declared. The engine keeps the declared-name table, so a client cannot match an undeclared name by hash. Load-time collision checks (section 3, step 7) ensure every declared name has an ID.
- `send_notify(to, id, ptr, len)` addresses the client of house `to`'s player. Zero targets every observer client. Each client module there receives a `Notify` event with ID, data and sending module origin. Replay viewers receive the notifications for their chosen player's or observer's perspective. Notification size and per-frame count are bounded (section 15.4). An excess returns `Refused` from `send_notify`.
- Messages from observer clients are rejected.

**Client reads** (`client_get`, `client_get_many`, `client_find`, `client_var_get`) expose only what the local player's UI can show. Notifications carry any other information. The property tables specify access in their client column.

- The player's house and objects (including superweapons) expose entries marked all, own and seen. Properties marked sim and team and modifier objects are simulation-only.
- Other houses, including allies, expose only public information (name, country, side, alliances, defeated and human flags). Money, power, buildings, production, tech level, superweapons and statistics are hidden.
- Currently visible objects owned by other houses expose type, position, owner, health and visible tint and laser trails. Passengers, target, mission, rank, ammo, modifiers and true disguised identity are hidden. A disguised enemy reads as its apparent type and owner.
- Objects under shroud and unseen foreign objects under fog are absent. The engine keeps no last-seen copy. Cloaked enemies are absent unless detected. Cell terrain type and height are public because the map file itself is public. Waypoint numbers are public. Other cell properties follow sight, viewpoint or simulation-only restrictions in appendix 7.6.
- Variables follow their visibility (section 6e). Triggers and tags are simulation-only. Viewpoint properties use only the local player's house.
- Observer modules see the observer's permitted view, potentially the whole map when the room allows it. Observers have no house. Own-house queries have an answer only where the room shows the observer that house's information.
- Client writes are limited to client-owned nodes, camera, cursor and local settings. Clients cannot write simulation state, the simulation never reads client state, and clients have no direct channel to each other.

**Communication.** Simulation-to-client delivery is local. Clients read visible state and variables and the simulation sends notifications with `send_notify`. Client-to-simulation traffic uses the command stream exclusively:

- `client_send_message(ptr, len)` inserts a message into the command stream. After the command delay it reaches the same-origin simulation module's `on_message` on the same frame on every machine.
- `client_send_order(ptr, len)` sends a standard player order.
- Both inputs are untrusted. The simulation validates contents. The engine supplies the sender and modules cannot forge it. Size and rate are bounded (section 15.4). Replays record both.

A complete round trip: the player clicks a button, the UI sends a message, the simulation validates it and writes a house variable, and the UI reads the variable to update the button.

```ts
// ui.wasm — TypeScript (AssemblyScript-compatible)
import { ReinforcementsBought } from "./generated/vars";
const BUY_MESSAGE = new Uint8Array(1);           // Message type 1: purchase.
BUY_MESSAGE[0] = 1;
let buy: Node = 0;

onStart((): void => {
  buy = Ui.find("BuyReinforcements");            // A button from veraui.ini.
  Node.setClickable(buy, true);
});
onClientEvent(ClientEvent.Click, (e: ClickEvent): void => {
  if (e.node == buy) Client.sendMessage(BUY_MESSAGE);   // client_send_message
});
onRender((dt: f32, alpha: f32): void => {
  const me = Client.viewer();                                   // NO_HANDLE for observers.
  if (me == NO_HANDLE) { Node.setVisible(buy, false); return; } // Observers have no house. Scope 0 (global) is invalid for a house variable.
  Node.setVisible(buy, ReinforcementsBought.get(me) == 0);      // client_var_get permits private variables of the player's own house.
});
```

```ts
// rules.wasm — TypeScript (AssemblyScript-compatible)
import { ReinforcementsBought } from "./generated/vars";
const HTNK = Rules.technoType("HTNK");

onMessage((sender: House, data: Uint8Array): void => {
  if (data.length != 1 || data[0] != 1) return;                         // Validate all untrusted message contents.
  if (HTNK == NO_HANDLE) return;
  if (ReinforcementsBought.get(sender) != 0 || House.money(sender) < 2000) return;
  const tank = Action.createObject(Waypoint.cell(3), HTNK, sender);     // Occupied cell: NO_HANDLE (Refused).
  if (tank == NO_HANDLE) return;                                        // Charge only on successful creation.
  Action.addMoney(sender, -2000);
  ReinforcementsBought.set(sender, 1);                                  // Visible on the client's next read.
});
```

### 11. Multiplayer

- Embedded modules travel with the map. Mod modules are included in the existing mod-file check.
- The lobby compares module hashes and required API versions, reading each `[VeraWasm] Api=` before loading modules. A map or mod needing an API unavailable in the room's game cannot be selected. Offline, an incompatible save or replay is rejected. Each refusal identifies the required version.
- Mod `[VeraWasm]` keys reside in `rulesmd.ini`, included in the mod digest compared by every player. No module keys go in `mod.ini`, which the digest excludes.
- The engine supplies the unforgeable sender of each message. Message size and per-frame count are bounded.
- Only the target player or observers receive a notification. The author is responsible for its bytes, which the engine cannot filter.
- Multiplayer exchanges commands only. Reconnecting players resimulate the entire match from the start, including modules. Module messages are commands and need no extra transfer. Module work per frame also slows catch-up. Notifications are suspended and client modules do not run during catch-up. They receive one `client_on_start` afterward.
- Saves and replays record game version, module hashes and API version. A mismatch against the installed mod or map rejects the save or replay and identifies the modules. Older versions reject saves and replays from newer versions with a message.
- Rooms require the same game version for all players. Mixed-version rooms cannot start.
- Offline, older pre-module releases can run a module map without its modules. They ignore `[VeraWasm*]` sections and module trigger actions do nothing. Already released builds cannot be changed, so this is a documented legacy limitation. For subsequent builds the release immediately preceding module support adds a map key `[Basic] VeraMinVersion=` (name tentative) that rejects maps requiring a newer version with a message naming it. Module maps set this key.
- Update [Networking](../Networking.md) to cover released games, mixed-version rooms and rollbacks.

### 12. Versions and ABI

- A WIT-syntax interface definition and machine-readable tables (event kinds, action kinds, properties, name kinds, node kinds and other enums) are the sole API source. Project generators produce Rust and TypeScript SDK wrappers, reference docs and compatibility checks. Other bindings use the same tables. WIT describes the interface. Modules do not use the Component Model canonical ABI.
- The binary interface is a core module ABI: core Wasm imports and exports, `i64` references and values, `i32` IDs, pointer plus length for strings, bytes and buffers, and batch results written by the host into module buffers. Host imports return at most one value: a call that yields a value returns its status and the host writes the value through out pointers into module memory, because stable Rust and AssemblyScript cannot import multi-value functions. Modules may still use multiple return values internally (section 3). Appendix signatures specify values, statuses and pagination positions.
- Freeze imports and exports for both interfaces ([appendix 1](0001-wasm-extension-modules-api.md#1-simulation-abi) and [appendix 2](0001-wasm-extension-modules-api.md#2-client-abi)). Event kinds, action kinds, properties, name kinds, search scopes, node kinds, client event and action kinds, and all other enums are append-only tables extended in minor versions. `has_kind` permits fallback on older engines missing an entry.
- A future major version may layer a Component Model adapter over the core ABI. Existing modules continue using the core ABI unchanged.
- Versioned packages are `vera:sim@1` and `vera:client@1`. Version numbers have major and minor components. New majors are incompatible, and each released major remains available permanently through an adapter. Minor releases are additive. Imports declare the required major and minimum minor through the module name `vera:sim@<major>.<minor>` or `vera:client@<major>.<minor>`. The map or mod declares the same version in `[VeraWasm] Api=`. Both interfaces share a version and release together, so one `Api=` covers both. The lobby checks every player's game against it. Load-time validation checks imports against it (section 3).
- Unstable interfaces live under `vera:unstable/*`, require explicit opt-in and may change without notice. These and `0.x` previews are outside the released-version guarantee. They have no permanent adapter and modules using them run only on the version for which they were built.
- Every released version remains available. A thin adapter wraps the current implementation for older modules. It never passes an undefined value to them. It suppresses events absent from their table, hides newer properties and maps newer status or enum values to values defined in their version, according to mappings in the interface definition.
- CI builds and runs compatibility cases for each released version from Rust SDK sources in the repository. No Wasm binaries are committed. Each release adds cases.
- Metering units, host function costs, default budgets and limits are fixed per major version (section 4).
- Functions and entries may be deprecated with author warnings. They are never removed from a released version.
- Released functions, properties and table entries retain their units and meaning. Changes require a new entry or version.
- `vera.actions`, `vera.notify`, `vera.vars` and `vera.delivery` (delivery area declaration) use [fixed custom section layouts](0001-wasm-extension-modules-api.md#custom-sections) beginning with a format version. Names are UTF-8 restricted to ASCII, with ASCII-only case folding.
- Module actions and decision handlers use module-chosen 32-bit dispatch keys in trigger definitions. Event records carry the key, and SDK-generated `on_events` and `on_decide` dispatch on it. Handlers need no individual Wasm exports. `vera.actions` maps names to keys. Only handlers referenced from INI or maps need names. Keys are local to the module. Cross-origin names resolve only in INI (section 2).

### 13. Tooling

| Need | Provision |
| --- | --- |
| Getting started | TypeScript and Rust templates for `rules.wasm` and `ui.wasm`; one example mod per language |
| Size | Small release builds in the SDK, size guidance per language, map embedding limits |
| Maps | `asset` commands to embed, extract and validate modules |
| Iteration | SDK watch mode rebuilds on save. In offline developer mode the game watches and reloads `.wasm` files, resetting module state. |
| Errors | Show the failing export and error in-game. The SDK passes error details and source location to the engine. When a module carries debug info, a trap's stack trace shows source file and line. |
| Debugging | In developer mode (offline only, desktop): Wasm core dumps on trap; breakpoints, single-stepping, inspecting locals and the call stack, and watching writes to module memory; sampling profiles per function, written as files Firefox Profiler opens. Debug instrumentation does not change the operation count. |
| Cost | Overlay per module per frame: operations and engine work on its behalf (polls, hashing), trigger deliveries, decision requests and skips, queued backlog and used memory (`memory.size`). Warn on `bindAllTypes` triggers. |
| Documentation | Reference generated from the interface and tables, task-based tutorials |
| Map editor | FinalAlert2 data-file entries for new events and actions |
| Testing | Run module maps headlessly and assert results using the existing headless scenario runner |

### 14. Project contract

- Extension module support ships in every build. It is distinct from the browser build features `wasm`, `wasm-net` and `wasm-threads`. Enable it at runtime when a mod or map supplies a module or uses any extension key (`Vera.Tags`, filters, INI atomic actions, `[VeraWasm]`). Never gate gameplay behind a build feature. Enabling modules enables the modifier owner (section 6c).
- Choose the Wasm runtime per platform, behind platform build features as permitted by the project contract. Desktop uses a native engine. Browser provisionally uses an embedded interpreter (open question 1). The simulation accesses it through the same interface, keeping `vera20k-core` and `sim/` buildable for browsers.
- Every module call accesses state through its existing owner. Properties, modifiers and variables each have one owner. The API introduces no competing state.
- Document the feature for modders under `docs/`. Update `docs/modding.md`, which currently describes mods as data and says VERA20k does not run their programs. Explain that modules run in a sandbox without file or network access, and simulation modules also lack clock access. The Mods menu and map list must identify mods and maps containing modules.
- Report module errors in-game as well as in logs.

### 15. Performance

Use 20,000 units and 30 houses as the performance baseline ([simulation thread budget](../sim-thread.md)). Hot paths use IDs or references cached during initialization. Reserve names for initialization and infrequent rules edits. Batch reads, pagination and event delivery use module buffers. Searches allocate no memory and sorting cost scales with results. Write per-frame projectile data directly into module result areas. The SDK reads events in place without allocating per-object or per-event wrappers.

#### 15.1 Implementation requirements

Review each of the following as an implementation acceptance criterion.

1. **Zero module overhead when absent.** Every event site and decision point checks a flag first. When clear, it builds no payload and runs no further module code. Each bound object and each object receiving a tag with a module trigger sets its own flag (section 7). Keep these flags in a side column in entity storage so the check does not load the object. Type, weapon, warhead and projectile-type binding flags live in mutable simulation tables. Rules data is read-only during a match, while type bindings can change.
2. **Cache effective values on change.** Adding, removing or expiring a modifier recomputes the affected effective property and caches it on the entity. Hot loops (movement, firing eligibility and target scoring) read only that cache, without side-table lookups or modifier composition. An unmodified property reads its base value. Store absolute expiry frames and bucket modifiers by expiry. Per-frame cost scales only with modifiers expiring that frame. Derive `Remaining` from expiry. This matches existing engine multiplier layers.
3. **Use Phobos read stages, operation order and rounding for equivalent modifiers** (section 6c), to Q16.16 precision. Shared code ships in every build. Runtime switches enable Phobos keys or module modifiers independently. No build feature gates them (section 14).
4. **Decide only at commit.** Call decidable events synchronously in deterministic engine commit order. Speculative or parallel precomputation (target acquisition previews, pathfinding batches, pursuit scoring) may discard its results and never issues calls. `count` is always 1 (section 6f). Delivered events likewise occur at result commits and never during speculative calculation (section 5).
5. **Decision-handler randomness** uses a substream derived from the request (key in section 4), so the number of decision calls never changes the module's main stream (section 4). A handler may write its own module memory since calls occur at commit in deterministic order.
6. **`ProjectileFrame`** (future tier) runs inside the engine's projectile advancement loop in engine order, just before the projectile moves (section 9). There is no separate pre-advancement phase.
7. **Module memory hashing.** Hash only the memory grown so far (`memory.size`). Rotate through pages, hashing 1/15 each frame and combining with cached digests from other pages. The page count enters the digest, and new pages take digest 0 until their turn instead of being hashed together in the frame they appear, so there is no spike. The digest works on words and stays within the section 15.4 target at the memory limit.
   - The **rotating hash** serves multiplayer sync checks and state hashes recorded periodically during a match. It guarantees only that a page difference persisting until that page's next turn becomes visible within 15 frames of the write, barring digest collisions. A difference that appears and disappears between checks, including one with the rotation's own period, may never be seen.
   - The **full hash** is computed on demand. It rereads all current module memory, neither using nor updating cached digests, and otherwise covers what the state hash covers. Save/load equivalence, cross-platform consistency and replay endpoint checks must use it. To locate the first divergent frame, replay on each machine and compute it every frame.
   - **Saves** hold module bytes, page count, nonzero pages in ascending page order (concatenated, DEFLATE level 1), globals and the rotating digests. On load, check page count and decompressed length against the limits first. Rotating digests only keep online checks after loading equal to a continuous run and are no evidence that memory was restored correctly. Do not save the merged rules text again with modules.
8. **Presentation snapshots exclude module state.** Per-frame copies for drawing exclude module instances, linear memory and globals.
9. **Sparse instances and side tables.** Store variables, modifiers, bindings and per-projectile module data in reference-indexed side tables, and type bindings in type-indexed tables. Allocate only for objects and types using them. Allocate per-object trigger state only when it differs from the default (section 7). Save and hash these tables. Update their contribution to each frame's sync check incrementally on writes without scanning all entries every frame.

10. **Engine work on behalf of modules is bounded.** Polls check whether they are due before collecting holders, and holders of type bindings do not come from scanning every object each frame. Searches use house, type and area indices. Adding and removing queue entries, bindings and modifiers never grows quadratically with the totals. This work is either charged to the module or has a per-frame cap independent of the object count, and the cost overlay shows it.

#### 15.2 Hard limits on modules

These are proposed defaults, with values in section 15.4, fixed per API major (section 12) once measurements confirm them (open question 7).

- **Per-frame operation budget.** Each module gets one budget per frame regardless of record count. The slowest machine sets lockstep speed, metering slows module code, and browsers use an interpreter. Size the budget for the slowest supported platform: the proposal is about 2 ms of operations there. Defer excess work to the next frame, leaving records queued (section 5). Disable and report a module that cannot keep up within the backlog cap.
- **Memory limits.** Map modules: 16 MiB. Mod modules: 32 MiB. Not configurable in version 1. The engine allocates this at instantiation (section 3).
- **Raise events by binding scope.** High-frequency events and decidable events scoped to objects or types fire only for corresponding bindings. Global decisions follow the event table (sections 5 and 6a). The cost overlay warns on `bindAllTypes` (section 13).
- **Charge derived properties.** Charge the owning module for evaluating derived properties such as `NearbyEnemyCount` and `TargetInRange` in a filter (section 6a). The property tables mark costly derived properties.

Provided every host call is charged by work in advance and engine work on its behalf is bounded (15.1 item 10), a poorly written module falls behind, or is disabled when a single call exceeds its limit or its backlog overflows. It cannot stall or desynchronize the match.

#### 15.3 SDK defaults

SDKs and templates stagger periodic work by object or house. Use one `get_many` for multiple objects or properties. Use filters for conditions that can be checked before queueing. Area effects use long-lived modifiers added on entry and removed on exit.

#### 15.4 Preview values

These are preview defaults, not frozen values. Values marked "measure" freeze with version 1 only after the measurements in open questions 7 and 17.

| Item | Value | Basis |
| --- | --- | --- |
| Frame budget B | Mod 200,000. Map 100,000 by default, up to 200,000 set by the mod. A configured value below 20,000 refuses the match. | Measure: calibrate host costs first |
| Decision budget | B/2 per frame, at most 20,000 per call. `Defeat` separately B/4 per frame. | Measure |
| One-time budgets | `vera_init`, `on_rules` and `on_scenario_start` 5,000,000 each. At most 16,384 writes per `on_rules` layer. | Measure |
| Memory | Map 16 MiB, mod 32 MiB | Measure: page digests ≤ 0.5 ms per frame at the limit |
| Module size (decompressed) | Mod 8 MiB. Map 2 MiB per module. | Map transfer limit is 8 MiB |
| Load-time shape | 20,000 functions. 1 MiB body. 1,024 locals per function. 256 distinct imports. 1,024 exports. 1,024 mutable globals. 65,536 table entries. 10,000 types. 10,000 of each segment kind. 4,096 entries each in `vera.actions` and `vera.vars`. | Measure worst compile time |
| Call stack | 512 levels, 16,384 weighted slots | Measure stack high-water marks per platform |
| Delivery | At most 16 per call within the delivery area. 2,048 per frame. Backlog 32,768, queue 34,816. | Measure records per frame in battle |
| Search | Examine per call at most 16,384 items and no more than half the calling callback's budget pays for (at least one), returning a nonzero `next` if unfinished, also when nothing matched. Keys that hold no item, such as cells outside the map, are also charged and capped. `.first()` follows `next`. | Bounds the work of one host call. The budget still bounds the callback, so searches longer than B are paged across frames by the module. |
| Filter clauses | 16 per event and per query | 16 bytes each in the layout |
| Triggers | 1,024 per module, including 16 object poll events. 65,536 bindings. 16 events and 32 actions per trigger, 1,024 bytes per action text. | Measure |
| Poll interval | Object-scope polls and `Timer` ≥ 15 frames, global ≥ 1 frame | Measure |
| Chains | Depth 8. 1,024 per source per frame. 4,096 pending. | Measure (open question 17) |
| Modifiers | 64 layers per (origin, object, property). 65,536 per origin. | Measure memory |
| Variables | 262,144 non-default values per module | Measure saves and hashing |
| Orders | 256 and 4,096 unit entries per frame, 1,024 entries per order | Measure the command step |
| Log | 64 lines per frame, 256 bytes per line | Excess is dropped |
| Notifications | 256 bytes, 256 per frame | Measure in phase 2 |
| Messages | 96 bytes. 2 per player per source per frame and 16 in any 15 frames. | Measure in phase 2 |
| Client | 100,000 operations and 1,024 events per render frame | Measure in phase 2 |
| Result areas | `Fire` 768 bytes, `Launch` 416 bytes, `ProjectileFrame` 56 bytes, others 0 | From appendix layouts |
| Projectiles | Caps on speed and per-frame displacement | Set with `ProjectileFrame` |

## Process

1. **Draft.** Discuss this document publicly. Each revision increments the RFC version and updates the [revision history](#revision-history).
2. **Resolve open questions.** Answer the questions below with throwaway experiments and incorporate conclusions here.
3. **Acceptance.** Development starts after the maintainer marks a version accepted.
4. **Preview.** Release the API as `0.x`; breaking changes are allowed and logged. Previews are outside the released-version guarantee (section 12). Reimplement at least one real mission or game mode and invite external modders to try it.
5. **Stabilization.** Freeze functions in `vera:sim@1` and `vera:client@1`. Tables include only entries marked `v1` in the appendix. Each must be implemented, behave the same in every build and on every supported platform, be reported truthfully by `has_kind` and have a compatibility case. Otherwise mark it `later` for a minor version. Freeze `DamageIncoming` and `TargetArmor` only after the shield cases in section 6f pass. Add compatibility cases to CI.
6. **Growth.** Each batch of new table entries (event kinds, action kinds, properties) or new functions gets a small follow-up RFC and a new minor version.

Planned order after acceptance:

| Phase | Scope |
| --- | --- |
| 1 | `rules.wasm`: stock and new events and actions, filters, properties and modifiers, variables, search, AI actions and properties, type tag bindings, object bindings and trigger actions, the first three decidable events and the preview shield events (section 6f), map embedding, `asset` commands, TypeScript and Rust SDKs and templates |
| 2 | `ui.wasm`: UI tree with INI layout, node kinds and properties, client events, visibility filtering, messages and notifications |
| 3 | Reimplement a real mission with modules, test with external modders and freeze version 1 |
| Later | Additional decidable events individually, module trajectories (`Launch`, `ProjectileFrame`), peer calls, player UI modules |

## Design scope

Use core Wasm modules and versioned function tables. Simulation modules can read full state. Clients read according to local visibility, and cross-client communication goes through the simulation. Ordinary events are batched. Synchronous decisions may write module memory and use decision RNG substreams. Retain data-driven triggers, Phobos trajectories and AttachEffect.

## Possible future directions

Outside version 1. Each item gets a follow-up RFC or a new minor version.

- Additional delivered events: `Built` (`other` builder), `Idle` (mission ended with no queued mission), `MoveFailed` and `Arrived` (`cell` destination), `Stunned` and `Unstunned` (`other` source), `Promoted` (`value` new rank), `Cloaked` and `Decloaked`, `Spotted` and `Lost` (`house` that gained or lost sight of the object), `AmmoChanged` (`value` old, `value2` new), `MissionChanged` (`value` mission), `HouseDefeated`, `PlayerLeft` and `StorageFull` (`house`), `MotionEnded` (`value` reason).
- Module-defined custom missions executed per frame.
- Teach AI to use mechanics introduced by mods.
- Multiple tags on one object.
- A modifier for seeking targets among allies.
- House takeover (the first post-freeze candidate): a mod module claims all house-level decisions of a computer house, which stays a computer house. Units keep their own targeting, retaliation and harvesting. The claim is saved and hashed, and native AI resumes when the module stops or releases it. Per-seat controller selection in the lobby is a separate RFC.
- In-match rules changes by type (section 8), module-written AI definitions (section 6d) and multilingual map text.
- UI-tree data binding (bind a node property to a public variable) and a client hook for rewriting orders.
- Module trajectories (`Launch`, `ProjectileFrame`), motion actions, peer calls and player-installed UI modules (see planned phases above).

## Open questions

Question numbers are unchanged so earlier discussion still resolves. **Decided**: the answer is in the main text. **To measure**: there is a provisional answer, which freezes with version 1 only after the listed experiment.

1. **Browser (to measure).** Provisionally embed the pure Rust interpreter wasmi 2.x, running the same instrumented bytecode as native builds (sections 3 and 4), without using its fuel, NaN or stack limits to define results. Instances and memory live in engine memory, so a frame can move between the main thread and the simulation worker. Experiment: run compute-heavy and host-call-heavy modules, each spending B in one call, in three browsers and on the slowest supported device. A p95 of at most 2 ms settles it. Otherwise measure the browser engine as a backend, which must also solve instances not crossing workers, host access to module memory, catching traps and bitwise access to globals.
2. **TypeScript compiler (to measure).** Provisionally AssemblyScript (section 5a). Experiment: compile equivalents of `tutorial-bounty` and `decide-damage`, the latter allocating each frame, pass load checks and run 9,000 frames with a save and load midway. Equal full hashes, bounded memory and no budget overrun settle it. If only garbage collection overruns, switch to the `minimal` runtime. Failing that, measure WARPO, and failing that ship only the Rust SDK in version 1.
3. **Metering (to measure).** The rules are decided (sections 3, 4 and 15): shared instrumentation, per-operation NaN canonicalization and word-based page digests. Experiment: measure basic-block metering, stack metering and NaN canonicalization separately on native and browser builds, and a candidate digest (e.g. XXH32) on wasm32 over 53 pages and for one full hash. Results adjust only budgets and limits.
4. **INI format (decided).** Mods use named sections and `Vera.Tags`. Maps use stock trigger encoding and action 100036 (section 6a).
5. **Event and action numbers (decided).** Each table uses 100000..199999 (section 7 and appendix).
6. **FinalAlert2 (to measure).** The sources of FA2 v2.0, FA2sp, FA2sp-HDM and World-Altering Editor all keep unknown nonempty sections. Experiment: save maps embedding small, 512 KiB and 3 MiB modules twice each with stock FA2 1.02 and compare extracted modules and save times. Identical content settles it. If truncated, shorten lines or lower the map limit. If sections are deleted, require embedding after editing.
7. **Limits (to measure).** Section 15.4 gives preview values. On the experimental 0.0.15 implementation, native on an Apple M4 with 20,000 units, searching all objects until 200,000 operations ran out was measured at 2.34 ms with one clause and 10.05 ms with eight. Scaling shorter measurements to 200,000 operations gives about 12 µs for pure computation (measured at 168,071 operations) and 160 µs for property reads (measured at 139,815). Until host costs are calibrated by work, 200,000 operations does not mean 2 ms. Byte-wise page digests of 32 MiB take about 2.25 ms per frame and 16 type-bound object polls about 4.7 ms per frame, uncharged (15.1 items 7 and 10). Experiment: name the slowest supported machine, calibrate each class of host call until unit prices agree within 2× and spending B has a p95 of at most 2 ms, then verify the worst frame, bursts of records, worst compile time and all example modules at 20,000 units and 30 houses.
8. **Frame phases (decided).** The module phase is at the end of the Triggers step (section 5). A module trajectory replaces the "compute candidate position" step (section 9).
9. **Save size (decided).** Concatenate nonzero pages, compress with DEFLATE level 1 and do not save rules text again (section 15.1). A typical module is well below 1 MiB. The worst case is bounded only by the memory limit, so 1 MiB is not promised.
10. **First decidable events (decided).** `TakeDamage`, `ProductionQueued` and `Defeat` (section 6f).
11. **Allowlist (decided).** Empty in version 1: no in-match rules changes by type (section 8).
12. **Version 1 inventory (decided).** See appendix section 9 and the tier columns, with the freeze threshold in Process step 5.
13. **Modifier stacking (decided).** See section 6c. Phobos AttachEffect keeps its instances and timers and is not mapped to modifiers.
14. **UI layout and node kinds (decided).** Mods use `veraui.ini`, maps `[VeraUI]` sections (section 10 and appendix 7.10).
15. **Map text (decided).** Use CSF in files distributed with the map, one language per map, with no new format (section 10).
16. **Campaigns (decided).** Each mission starts fresh and only globals in the stock carry-over record persist (section 6e).
17. **Chain bounds (to measure).** Defer past the limits, count per source, with values in section 15.4. Experiment: measure native stack high-water marks at depth 8 on each platform (Windows debug first), which must be at most half the available stack. 1,024 chained firings must take at most 2 ms and a frame of 5,120 firings at most 15 ms, otherwise lower the limits.
18. **House control (decided).** Not in version 1. `order` shares the house with native AI (section 6g). Takeover is a post-freeze candidate.

## Revision history

- **0.0.20** (2026-11-23): Stated where gameplay effects are listed that missions (`Mission` writes, `SetMission`) reach any house's units, a player's included; only `order` is limited to computer houses. The text read as if modules could not command a player's units.
- **0.0.19** (2026-11-23): The first event batch of a frame always starts. `ProductionAllowed` draws from B, so a module that refuses production every frame could otherwise receive no events at all until its backlog stopped it. The worst-frame bound gains one decision call's limit.
- **0.0.18** (2026-11-22): Sized a search page by the caller's budget as well. With two clauses a page of 16,384 items cost more than a whole callback's budget, so the paging 0.0.17 describes could never start. Keys without an item are charged and capped.
- **0.0.17** (2026-11-22): Corrected open question 7: the 4.6 ms search figure in 0.0.16 was extrapolated from 42,442 operations and presented as a measurement. Direct measurement gives 2.34 ms with one clause and 10.05 ms with eight, and the computation and property-read figures are now labelled as extrapolations. Made search pagination consistent with the per-call scan limit: `next` is nonzero whenever a scan is unfinished, `.first()` follows it and a scan never outruns the caller's budget.
- **0.0.16** (2026-11-22): Answered the 18 open questions (12 decided, 6 to measure with provisional answers). From external review: one request per decision call and complete substream keys; bulk memory reduced to `memory.copy` and `memory.fill` with exported memory; rotating hash separated from an on-demand full hash; `TakeDamage` stated as settled damage, with preview `DamageIncoming` and `TargetArmor`. From performance measurements: a used-up budget defers or skips instead of disabling a healthy module, host costs are charged in advance in proportion to work, and engine work for modules is bounded. Numbers now start at 100000, `Wasm.Tags` becomes `Vera.Tags`, files distributed with a map have only map permissions and random stream seeds separate their domains.
- **0.0.15** (2026-11-21): Host imports return a status and write values through out pointers, because stable Rust and AssemblyScript cannot import multi-value functions; frame returns its value directly.
- **0.0.14** (2026-11-19): Added `order` (section 6g), which lets simulation modules command computer houses through the shared command table, and the `Scatter`, `TogglePrimaryFactory` and `Enter` commands. Added open question 18 on house control.
- **0.0.13** (2026-11-18): Aligned triggers, query return values, default event fields and client visibility. Corrected binding examples and load-time validation order.
- **0.0.12** (2026-11-18): Streamlined appendix table columns and consolidated shared rules and exceptions.
- **0.0.11** (2026-11-18): Consolidated repeated examples and interface descriptions. Centralized signatures, layouts, numbers and version tiers in the appendix.
- **0.0.10** (2026-11-18): Added explicit pagination and house lookup by country. Returned values and status in the same host call, generated variable accessors from declarations, unified client cell jumps as a camera action and named modifier methods by operation.
- **0.0.9** (2026-11-18): Consolidated equivalent property writes and uniform property polling. The SDK exposes named fields per event and distinguishes immediate actions, queued callbacks and synchronous decisions in the trigger builder.
- **0.0.8** (2026-11-18): Completed the language requirements for writing mod modules and listed nine host language bindings for embedding Wasmtime in tools.
- **0.0.7** (2026-11-17): Reduced the API to five concepts (objects and properties, variables, events, actions, UI nodes) and frozen functions (23 simulation imports, 7 exports; 21 client imports, 4 exports). Events, actions and properties became append-only tables. Constraints became filter clauses, marks became scoped variables with visibility, persistent effects became property modifiers, queries became `get`, `get_many` and `find`, and override points became synchronous decidable events through `on_decide`. AI functions became actions and properties. Triggers became the sole subscription mechanism and gained object bindings. Clients use nodes, client events and `client_*` functions. Client messages reach only the same-origin simulation module. SDK wrappers are generated from tables. Dropped real-time delivery, separate constraints and a function-per-operation interface.
- **0.0.6** (2026-11-16): Added performance rules (section 15), module actions batched per module per frame, stock and atomic actions at trigger firing, creation serials as handles and sort keys, safe points, batch sections, non-reentrancy and chain limits. Defined atomic action targets and failures, error statuses, notification IDs, client queries limited to player-visible UI state, and core module ABI described in WIT. Reorganized text, moved implementation notes and proposal history out of the RFC, made the appendix reference-only, and removed the English edition pending finalization.
- **0.0.5** (2026-11-16): Resolved appendix proposals on override chaining, numeric marks and house marks, query order, damage versus health, probes, additional candidate events, error model, named notifications and fixed-point format. Declined arbitrary house-perspective queries and direct client-to-client communication.
- **0.0.4** (2026-11-16): Added API catalog appendix.
- **0.0.3** (2026-11-16): Added ordering, state retained after disabling a module, dedicated module RNG, saves, replays and reconnect, client orders and storage, queries, AI teams, spawning projectiles and `Override.Fire`.
- **0.0.2** (2026-11-16): Added constraints and polled events, atomic actions, persistent effects and marks, API granularity principles and a UI tree with INI layouts.
- **0.0.1** (2026-11-16): Initial draft.
