# RFC 0001: Wasm extension modules

**English** · [简体中文](0001-wasm-extension-modules.zh-CN.md)

The two language versions carry the same RFC version and change in the same
commit. Where they differ, the English text applies.

| | |
| --- | --- |
| RFC version | 0.0.6 |
| Status | Draft |
| Created | 2026-11-16 |
| API covered | `vera:sim@1`, `vera:client@1` (proposed) |

**Development gate.** No engine code for this feature lands until this RFC
reaches an accepted version. Until then, work is limited to discussing and
revising this document, and to throwaway experiments that answer the
[open questions](#unresolved-questions). Those experiments stay outside `src/`
and are not merged.

## Summary

Let mods and maps ship compiled WebAssembly modules (`rules.wasm`, `ui.wasm`)
that VERA20k runs in a sandbox. A module can change rules after they load,
react to triggers, issue actions and missions, override chosen game
decisions, and build its own interface. Authors write modules in any language
that compiles to WebAssembly, with their own tools; the engine only runs the
compiled `.wasm`. Official SDKs exist for TypeScript and Rust. The API uses the
terms Red Alert 2 modders already know: triggers, events, actions, tags,
missions, local and global variables, and frames, and adds conditions,
attached effects and marks.

Every function, event, condition, effect and override, with its owner in the
engine and its sources, is listed in the
[API catalog appendix](0001-wasm-extension-modules-api.md).

The API is versioned from its first release. A published API version keeps
working in every later VERA20k release.

## Motivation

INI files and map triggers cannot express much of what modders ask for:
custom projectile paths, wave logic, new superweapon behavior, a mission
objectives panel. Ares and Phobos added many keys for this, and each new idea
still needs engine work. A scripting layer lets modders build these
themselves, while the engine stays the single owner of game state.

An API cannot be taken back once mods depend on it, so it has to be designed
in public, grown in small versioned steps and covered by tests that keep
every published version running.

## Goals

1. Modders can change gameplay and interface without engine changes.
2. Every player runs identical simulation modules with identical results: no
   desync on any supported platform, including the browser build.
3. Outside modules never stop the game. A module that fails is disabled and the
   player is told why.
4. No module can read files or use the network, and no simulation module can
   read the clock; a client module gets render timing for display only and is
   metered in operations, not time (section 3). The engine's queries never leak
   hidden information: a client module's queries expose only what its
   player's own interface could show (appendix 7.7), and anything else reaches
   a client only through notifications. What a simulation module chooses to
   notify a player of is its author's responsibility (section 11).
5. With no module present, the game plays exactly as it does today.
6. Battles of 20,000 units do not get slower because of modules that only a few
   types use.
7. An RA2 modder can start from a template and see a change in game within
   minutes.

## Non-goals

- Native plugins (`.dll`, `.so`), or loading anything outside the sandbox.
- Source code in the engine. The engine never parses, compiles or interprets
  a source language, and ships no language runtime (see
  [Alternatives](#alternatives-considered)).
- Exposing engine internals: memory layouts, native addresses, internal object
  indices or storage slots (a handle is the object's creation serial,
  section 5), the renderer's graphics API.
- Adding new object types during a match.
- Replacing the original trigger system. Modules extend it.

## Terminology

The API uses Red Alert 2 terms wherever one exists. Names in code and INI
follow this table.

| Term | Meaning |
| --- | --- |
| **Extension module** | A compiled `.wasm` file. Not called a *script*: in RA2, scripts are AI team scripts (`[ScriptTypes]`). |
| **Trigger** | As in maps: one or more events (all must hold) plus one or more actions, owned by a house, with difficulty flags and the original repeat types. VERA adds optional conditions. |
| **Event** | What springs a trigger. Original event numbers keep their meaning. |
| **Action** | What a trigger does. Original actions keep their numbers. A new action runs a module function. |
| **Module trigger** | A trigger that has at least one module action (`Action.wasm`), was created by a module, or is bound through `Wasm.Tags` or `Trigger.bindType`. A map trigger that carries a module action counts. |
| **Condition** | A filter the engine checks natively before a trigger's actions run. |
| **Tag** | What attaches triggers to an object or a cell (CellTag). An object holds one tag. |
| **Mark** | A text marker an object or a house carries, tested by conditions, with an integer value and a visibility level. Not a tag: an object may carry any number of marks. |
| **Attached effect** | A per-object state with a duration, such as a multiplier or an immunity (the Phobos AttachEffect term). |
| **Mission** | An object's order: Move, Attack, Guard, Area Guard, Hunt, Sleep and the rest. |
| **Local / Global** | Local and global variables, as in maps. |
| **Frame** | One logic frame of the simulation, the unit of `ROF`, `Rate` and timers. |
| **Render frame** | One drawn frame on one machine. Not synchronized. |
| **Override** | A named decision point where a module may change the engine's result. |
| **House, Techno, Building, Vehicle, Infantry, Aircraft, Projectile, Weapon, Warhead** | As in the INI files. |
| **Cell, lepton** | Map coordinates. One cell is 256 leptons. |
| **Operation budget** | How much work a module may do per frame, measured in deterministic operation units: a base amount plus an allowance per record delivered that frame (section 5). Module code and the host calls it makes both count. |
| **Creation serial** | A 64-bit number the engine gives every object and projectile when it is created and never reuses. It is the object's handle, and it orders query results, override calls and delivered records across objects, independently of how the engine stores objects or the order of its loops. |
| **Safe point** | A moment that is not inside any calculation, where an event VERA adds is raised and its trigger's original and atomic actions run. A batched or parallel engine section counts as one calculation (section 5). |

## What a modder sees

### Files

```
Mods/MyMod/
  mod.ini        ; Needs= and the other mod.ini keys; no module keys
  rulesmd.ini    ; [VeraWasm] API version, budgets, map policy; binds tags to types
  rules.wasm     ; simulation module
  ui.wasm        ; interface module
```

A map carries its modules as text, the way it already carries `IsoMapPack5`:
compressed, base64-encoded and split into numbered lines.

```ini
[Basic]
VeraMinVersion=0.9.0     ; example; older releases refuse the map with a message (section 11)

[VeraWasm]
Api=1.0                  ; major.minor of the API the modules need (section 12)
RulesSize=48213          ; decoded size, checked on load
UISize=9120
StorageId=MyCampaign     ; the client store (section 10); default: the map's file name

[VeraWasmRulesPack]
1=eJzt3XlcVOX+B...
2=...

[VeraWasmUIPack]
1=...
```

The `asset` tool gains commands to embed a module into a map, extract it, and
run the same load checks the game runs.

### Binding triggers to a type

```ini
; rulesmd.ini in the mod
[E1]
Wasm.Tags=GIPanicTag     ; every E1 receives this tag's triggers

[GIPanicTag]
Trigger=GIPanic

[GIPanic]
Event=TakeDamage
Action=Wasm:giPanic      ; runs the module action giPanic of the mod's rules.wasm (section 2)
Repeat=yes
```

Each E1 runs `GIPanic` as its own instance (section 7). The key names and the
exact section format are not final (see
[Unresolved questions](#unresolved-questions)). Map triggers edited in
FinalAlert2 can also call module functions through the new action.

### Module code

The engine sees only the compiled module. The source and the compiler stay in
the author's tools. The same module in the two official SDKs:

```ts
// TypeScript (AssemblyScript-compatible)
import { action, onFrame, Every, Techno, Mission, EventRecord, Ctx } from "@vera20k/sdk";

action("giPanic", (ctx: Ctx, e: EventRecord): void => {
  const techno = e.subject;
  if (Techno.healthPercent(techno) < 30) {
    Techno.setMission(techno, Mission.Hunt);
  }
});

onFrame(new Every(15), (ctx: Ctx): void => {
  if (ctx.local(3).get()) {
    // ...
  }
});
```

```rust
// Rust SDK
use vera20k_sdk::prelude::*;

#[vera::action(name = "giPanic")]
fn gi_panic(ctx: &mut Ctx, e: EventRecord) {
    let techno = e.subject();
    if techno.health_percent() < 30 {
        techno.set_mission(Mission::Hunt);
    }
}

#[vera::on_frame(every = 15)]
fn check_waves(ctx: &mut Ctx) {
    if ctx.local(3).get() {
        // ...
    }
}
```

Each SDK builds `rules.wasm` or `ui.wasm` with one command. The build lists
the module's action names in a Wasm custom section `vera.actions`, which gives
each name an index (section 12). Package names are placeholders. See section
5a for languages and SDKs.

## Design

### 1. Two worlds

| | Simulation (`rules.wasm`) | Client (`ui.wasm`) |
| --- | --- | --- |
| Runs on | every machine, in lockstep | the local machine only |
| Deterministic | yes | no |
| May change | game state, through engine owners | interface, display, sound |
| Reaches the game by | actions and missions | standard player orders and messages, through the command stream |
| Engine side | `sim/` | `ui/`, `render/`, `audio/` |

The split follows the existing rule that `sim/` never depends on `render/`,
`ui/`, `audio/` or `net/`. Data flows from simulation to client through
visibility-filtered queries and local notifications. Data flows from client to
simulation only through the command stream: standard player orders, which the
engine validates like any player input, and `sendMessage`, which enters the
command stream like a player order (section 10), so replays and online play
see both.

### 2. Sources: mods and maps

Mods and maps use the same API and the same SDK. They differ in what the
engine grants them.

| | Mod | Map |
| --- | --- | --- |
| Order | first | after the mod, as INI layers |
| `onRules` | after `RULESMD.INI`, `LANGRULE.INI` and the mode layer | after the map layer, sees the mod's result |
| Operation budget | declared in the mod's `[VeraWasm]`, capped by the engine | smaller default, the mod's `[VeraWasm]` may raise it |
| Size | file size | embedded size limit |
| Interface | may replace or rearrange the match screen | overlays only, unless the mod allows more |

The mod controls what a map's modules may do, in a `[VeraWasm]` section of its
`rulesmd.ini`:

```ini
; rulesmd.ini in the mod
[VeraWasm]
Api=1.0              ; major.minor of the API the mod's modules need
Budget=200000        ; the mod module's base budget per frame, capped by the engine
MapBudget=50000      ; the base budget a map's module may use under this mod
MapWasm=yes          ; no: a map with modules cannot start under this mod
MapUI=overlay        ; overlay, full or no
```

These keys live in `rulesmd.ini` because the mod digest covers that file and
not `mod.ini`, so every player in a room compares them (section 11). The
engine reads each `[VeraWasm]` section from its own file, the mod's from the
mod's `rulesmd.ini` before any other layer and the map's from the map, and
never layers one over the other, so a map cannot change the mod's keys; a
map's own `[VeraWasm]` section carries only its embedding keys, its `Api=` and
its `StorageId=`. Key names and values are placeholders (open questions 4
and 7).

With no mod selected, map modules are allowed, map interfaces are overlays and
map modules get the engine's default budget.

The rules layers run in this order: `RULESMD.INI`, `LANGRULE.INI`, the mode
layer, the mod module's `onRules`, the map layer, the map module's `onRules`.
Each `onRules` reads the rules as processed after the layers before it, and the
engine processes the rules again after its writes, as for INI text. A type ID
never changes once assigned: a later layer may add types, which get new IDs,
but never invalidates an existing one, so an ID resolved in the mod's
`onRules` stays valid for the whole scenario.

Conflict rules:

- **Tag bindings on types** in INI (`Wasm.Tags`) are read only from the
  mod's `rulesmd.ini`; a `Wasm.Tags` key in a map is ignored, with a warning
  in the lobby. Maps bind types in code with `Trigger.bindType` and its
  weapon, warhead and projectile forms (section 7), so a map cannot erase a
  mod's binding by layering.
- **Names and messages.** A module name in a mod's INI or triggers
  (`Action=Wasm:name`, an override binding) resolves to the mod's modules;
  one in a map's INI or triggers resolves to the map's modules; an explicit
  prefix (`Wasm:mod.name`, `Wasm:map.name`) reaches the other. `Action.wasm`
  in module code resolves the same way from the creating module's source.
  Override bindings in INI are read per source, so a map's binding adds the
  map's link to the chain and never replaces the mod's. A client message
  (`sendMessage`) reaches every simulation module's `onMessage`, and a
  notification (`notify`) every client module's `onNotify`; both carry their
  source module (mod or map), which the receiver reads.
- **Triggers** from both sources fire. The mod's module runs first.
- **Overrides** chain: the mod's module computes a result, the map's module
  receives it and may change it, and the engine uses the final value. Each link
  receives both the native value and the previous link's result, as the
  signatures in the appendix (section 8) show. A later link
  may lift an earlier link's veto. A value override returns an explicit
  "handled" flag rather than a special value.
- **Actions and missions** go through the same owners in execution order.
- **Peer calls** (later phase): a mod may publish functions that a map calls
  with `callPeer("mod", name, bytes)`. Only functions the mod exports for
  this purpose can be called.

### 3. Loading and validation

On load the engine:

1. decodes embedded modules and checks their declared size;
2. parses the module, accepts imports only from `vera:*` interfaces whose
   major version this release supports at the minor version the module needs
   (section 12), checks that those imports match the `Api=` of the module's
   `[VeraWasm]` section, which the lobby has already read, and refuses a
   module beyond the load-time shape limits (number of functions, size of a
   function body, locals, imports), so a map cannot stall the lobby while it
   compiles; each `vera.*` custom section starts with a format version, and
   the names in it are UTF-8 and must be ASCII;
3. accepts only the Wasm features of an allowlist: the MVP core plus mutable
   globals, sign extension, non-trapping float-to-int conversion, bulk memory
   and multi-value. Everything else, including SIMD, reference types,
   garbage collection, multiple memories, threads, 64-bit memory, exceptions
   and tail calls, is rejected, so all module state is memory, globals and
   one table of functions (section 4). A module with a `start` section is
   rejected, so no module code runs while it is instantiated. The engine also
   canonicalizes NaN results of floating-point operations by rewriting the
   bytecode, not by an engine flag, so every engine, the browser's included,
   gets the same NaNs;
4. adds operation metering and a call-depth counter to the bytecode, in one
   instrumentation pass: the depth limit is set well below the real stack
   limit of every supported engine, so a deep recursion stops at the same
   call on every machine. A module is never re-entered (section 5), so at
   most one call per module is on the stack and the engine frames between
   module calls stay bounded. The engine allocates the module's memory cap
   when it instantiates the module, so `memory.grow` within the cap never
   fails; a host that cannot allocate it fails the match at load, a match
   error (below);
5. checks that the exports the lifecycle needs exist and have the right
   signatures, and that every module action the INI names
   (`Action=Wasm:name`, section 2) is in the `vera.actions` table of the
   module it resolves to;
6. checks the notification names that the simulation modules loaded in the
   match declare in their `vera.notify` custom section, the mod's and the
   map's together, for hash collisions (section 10): a collision within one
   module refuses that module, and a collision between the mod's and the
   map's names refuses the map's module; the message names both.

The SDK turns a module's top-level initialization into an exported init
function. The engine calls it once, before the module's first other call
(`onRules` when the module exports it, otherwise `onScenarioStart`; in a
client module, before `onStart`), and never again after a save is loaded,
since module memory is restored. In the init function only name lookups
(section 15) work, `Mark.id` included, and they see the rules as processed
so far; other host calls return `Unsupported`.

The init function, `onRules` and `onScenarioStart` each run with their own
one-off budget, larger than a frame's (open question 7).

A failure before the match is shown in the lobby with its reason, and the match
cannot start with that module. A failure in `onRules` also stops the match
from starting, with a message naming the module, because the rules it was
writing are incomplete. A failure during a match, `onScenarioStart` included
(trap, budget exceeded, call depth exceeded, invalid argument), disables that
module at the same frame on every machine, and the game tells the players
which module stopped and why. A failure of the host itself, such as running
out of memory while allocating the cap or an internal error of the Wasm
engine, is not a module failure, because it cannot be shown to happen on
every machine at the same frame: it is a match error, and the game stops the
match with a message, as it does for a desync.

The match continues after a module is disabled, and what the module left
behind is handled this way:

- triggers the module created are disabled. In every other trigger (a map
  trigger, or one bound through `Wasm.Tags`) only its module actions are
  skipped, and the trigger's original and atomic actions keep running; its
  queued module actions are dropped;
- its overrides are skipped: the next module in the chain, or the native
  result, applies. For the override call that failed, the engine uses the
  previous link's value, or the native value;
- its projectiles on a module path fly on straight toward their target;
- what stays: attached effects and marks it added, because the engine owns
  them; its rules writes from `onRules` and its `rules.set` overlays; the AI
  definitions, tags and triggers it created (the triggers disabled); and
  whatever the failing call had already done, since actions run at once are
  not undone;
- its memory stays in snapshots and in the state hashes, unchanged
  from the moment it stopped, so saves and rejoining reproduce it.

Client modules run on one machine, so their failures are local. Each client
module has a budget per render frame, measured in operation units by the same
metering, not by the clock, so it can be enforced on the browser's main
thread: a call that exceeds it, or traps, is stopped, and the module is
disabled on that machine only, with a message to that player. It cannot
affect the match.

Errors that are part of normal play return a status and the module carries on:
a handle that is `NO_HANDLE` or whose object is gone gives `Gone`, a name that
matches nothing gives `NotFound`, and an action an engine rule refuses (for
example a cell that cannot be built on) or an atomic action's target that
turns out not to fit when its trigger runs (section 6b) gives `Refused`, and
a feature that is off in this game (for example an Ares action without Ares)
or a host call made where it is not available gives `Unsupported`. A call on a handle
that is `NO_HANDLE` or whose object is gone, including a method that returns
nothing (such as an interface node's), does nothing and sets the last status
(`ctx.lastStatus()`, or its client equivalent) to `Gone`; it never disables
the module. An atomic action run by a trigger has no module code to receive
its status: its error status (`Refused`, `Gone` or `NotFound`) is written to
the in-game log and the action is skipped; nothing else happens. Errors that
show a broken module disable it: an invalid argument (including an invalid
atomic action argument, such as a target kind that can never fit the action,
found when `Trigger.create` runs, section 6b), a trap, an exceeded budget or
call depth.

`docs/modding.md` currently says a mod is only data and VERA20k never runs a
program from one. That text changes to say modules run only in a sandbox with
no access to files or the network, and simulation modules none to the clock.
The Mods menu and the map list mark mods and maps that contain modules.

### 4. Determinism

All machines run the same module bytes, so results depend only on how the
engine executes them.

- Only the allowlisted Wasm features (section 3), canonical NaNs (by
  bytecode rewriting), and a call-depth limit counted by the
  instrumentation.
- No clock, no host random source, no WASI. Each module draws random numbers
  from a stream of its own, seeded from the scenario seed and the module's
  slot (mod or map), so a module never
  shifts the native streams: a module that draws nothing leaves every native
  result unchanged. Actions that run native code draw from the native streams
  exactly as that code does. Each API call documents its draws.
- Names (type IDs, section names) compare without regard to case, by ASCII
  rules only (`A` to `Z`), as gamemd's INI lookups do. Strings cross the
  boundary as UTF-8.
- The operation budget is counted by instrumentation added at load, not by an
  engine's built-in fuel. Every engine, native or browser, then counts the same
  operations and stops at the same point. Host calls are charged to the same
  budget by the work they do (for example per object scanned or returned, per
  cell checked), with the charge of each call kind in the interface
  definition (section 12).
- The metering units, the charge of each host call kind, the default budgets
  and the limits are fixed per API major version, so a module that ran within
  them keeps running in every later release.
- Positions, speeds and other game values cross the boundary as integers
  (leptons, fixed-point raw values). Fractions and multipliers use the
  simulation's fixed-point format, `I16F16` raw values (`SimFixed`). A module
  may use floating point internally.
- Module memory, mutable globals, the module's table (saved as the function
  indices it holds), the instrumentation's counters (operations used, call
  depth) and the module's random stream are part of the simulation state:
  saved in snapshots and included in the state hashes (section 15). Saves are
  taken only between module calls.
- Calls into a simulation module never come from parallel or batched code,
  and their order across objects does not depend on the engine's loop order
  (section 5).

### 5. Execution model

Simulation module exports (each simulation call also receives `ctx`, each
client call `ui`, as the appendix shows):

| Export | When |
| --- | --- |
| init function | generated by the SDK from top-level code; once, before the module's first other call, never after a save is loaded (section 3) |
| `onRules(rules)` | once, at the module's place in the rules layers (section 2); may change and add types |
| `onScenarioStart(ctx)` | once, at the start of the scenario |
| `onFrame(ctx)` | once per frame when exported; the SDK runs each registration in the frames where `frame % N == 0` |
| module actions | registered with the SDK, not Wasm exports (section 12); queued when their trigger fires, and run in the module phase from `onEvents` |
| `onEvents(ctx, batch)` | generated by the SDK; in the module phase, once per module with queued module actions, after `onMessage` and before `onFrame` |
| overrides | at a fixed step of the frame when the engine reaches that decision, in the order below |
| `onMessage(ctx, sender, source, data)` | in the module phase, when a client message has reached the simulation; `source` is the sending client module's (section 2) |

Client module exports: `onStart(ui)`, `onRender(ui, dt, alpha)` once per
render frame, `onNotify(ui, source, id, data)`, and interface callbacks such
as clicks and hotkeys.

Simulation module code runs only inside the simulation's phases. Local code,
such as the sidebar, the cursor, the interface, rendering and client queries,
never calls a simulation module; it reads the result the simulation last
computed. For example, `Override.ProductionAllowed` is evaluated in the
simulation and the sidebar shows its cached result (appendix section 8).

Module calls run in one fixed phase of the simulation frame, module by module
in load order (the mod's, then the map's). Each module first receives its
`onMessage` calls, then its `onEvents` batch, then `onFrame`. The module
actions of triggers that fire during the frame are queued, each with the
payload of the event that fired its trigger, and delivered in that phase as
one batch per module (section 15). A module receives events only this way:
there are no stand-alone event records. Overrides are the exception: they run
when the engine needs the answer. A handle is the object's creation serial, a
64-bit number that is never reused, and the engine maps serials to objects, so
a handle to an object that died before delivery reads as gone and never
refers to another object.

Calls into a module are never made from parallel or batched code. A batched or
parallel engine section (for example the damage commit or the parallel target
scan; the appendix names each raise site) counts as one calculation. VERA
events raised inside it are collected, and their triggers spring right after
the section ends, in the same frame and before the next phase: that is their
safe point, and their original and atomic actions run "at once" from there.
Overrides reached inside such a section are batched: the engine collects the
section's queries, makes one call per module with all of them, then applies
the results. Across objects, override calls, the triggers sprung after a
section and the records of a batch are ordered by the subject's creation
serial, never by the engine's loop order, so an engine change that reorders or
parallelizes its loops for speed does not change that order. Within one
object, calls follow the order in which the engine raised them, which may
change between engine versions. Records whose subject is `NO_HANDLE` (house
and logic triggers, and cell triggers whose event has no subject) come first,
in the order the trigger runtime springs them.

**No re-entry.** While a module's call is on the stack, the engine never calls
that module again. Its overrides are skipped for the calculations reached
from that call (the previous link's value, or the native value, is used), and
the events raised by actions it issues spring their triggers as usual, whose
module actions are queued as always. Calls into other modules are allowed: a
direct action of the mod's module may reach the map module's override.

Ordering rules:

- Original actions and the atomic actions of section 6b run when their
  trigger fires, exactly as original actions do in the original, also in
  triggers that contain module actions. Only module actions (module functions)
  are queued; they run in the module phase as one batch per module, in the
  order above. In a trigger with the actions [original A, module B, atomic C],
  A and C run first and B runs later in the same frame. An author who needs a
  strict order writes all of the trigger's actions as module code and calls
  original actions through the API (L1).
- Every event VERA adds is raised at a safe point: a moment that is not inside
  any calculation. After a calculation has completed is a safe point (for
  example `TakeDamage` after the damage has been committed), and so is before
  one begins (for example `HouseTransfer` before the transfer begins). Inside
  a batched or parallel section, the safe point is right after the section
  ends (above). The trigger's original and atomic actions run at once from
  that point. After an event raised before a calculation, the engine
  re-checks that the calculation is still valid before doing it: when an
  action has removed the object or changed its owner, the transfer does not
  happen and `HouseTransferred` is not raised; after the actions of
  `FatalDamage`, which is raised before the death chain, the engine re-checks
  whether the damage is still fatal (for example an action healed the object
  or added a prevent-death effect), and the death chain runs only if it is.
- Actions that run at once can chain within a frame: damage raises
  `TakeDamage`, its trigger deals damage with an atomic action, which raises
  `TakeDamage` again, and so on. The engine limits the depth of a chain per
  frame when the chain includes a module trigger (see
  [Terminology](#terminology)), an event VERA adds or an atomic action, and
  also caps the total number of such chained trigger firings per frame, since
  a trigger that springs two others at each level would otherwise fire
  2^depth times; a trigger that would exceed either limit fires in the next
  frame instead, so no such chain loops forever. Such a deferred trigger
  springs at the start of the next frame's module phase, before that phase's
  queued actions, with the payload it had: its original and atomic actions
  run then, so one frame later than without the limit, and its module actions
  join that frame's batch. It is dropped if it was disabled or removed
  meanwhile; its conditions are checked again when it springs, its house is
  the subject's owner at that moment, and an action on a subject that is gone
  gets `Gone`. Pending deferred triggers are saved and hashed. The limits are
  open question 17. A chain made only of original events and original
  actions is never deferred and runs exactly as in the original (Goal 5).
- A queued module action is dropped when the game has ended, or when its
  trigger was disabled or removed before the module phase. Handles to objects
  that are gone read as gone.
- A trigger that fires while the module phase runs (for example because a
  module action dealt damage) runs its original and atomic actions at once;
  its module actions are queued for the next frame's module phase, so module
  code cannot loop within one frame.
- Overrides return a result and cannot run actions, because the engine is in
  the middle of a calculation when it calls them. They may read state and keep
  their own state in module memory and draw from the module's random stream:
  the call order above makes that deterministic. The order across objects
  stays the same across engine versions; the order within one object may
  change between them (above). `Override.Fire` returns a shot plan that the
  engine carries out itself (section 9).
- A delivered event reports what already happened, and a module action, which
  runs later in the module phase, cannot cancel a native result such as a
  death. Changing that needs an override, an attached effect (for example,
  prevent death), or an original or atomic action on an event raised before
  the calculation (`FatalDamage`, `HouseTransfer`), which the engine
  re-checks (above).
- Frequent events (damage, fire, projectile detonated, crush, move
  failed) and every override are raised only for bound types (object, weapon,
  warhead or projectile types; section 6a). There is no subscription to all
  objects.
- After a save is loaded, module memory is restored with the snapshot.
  Neither the init function nor `onScenarioStart` runs again, and there is no
  separate load call. Handles stored in module memory refer to the same
  objects after loading, because a handle is a creation serial, not a
  storage position.

Each module has an operation budget per frame: a base amount plus an
allowance for each record delivered to it that frame, so a binding that
produces more records in a larger battle also gets more budget. Host calls
are charged to it by the work they do (section 4). Override calls draw from
the same frame budget, each adding the same allowance as a record; once the
budget is used up, the module's overrides are skipped for the rest of the
frame (the previous link's value, or the native value, applies) and the
module is disabled at the end of the frame. Each module's queue holds at
most a fixed number of records per frame; records beyond the cap are kept,
in order, and delivered first in the next frame, so a short burst delays a
module's reactions instead of disabling it. Deferred records are saved and
hashed. If a module's deferred backlog exceeds a fixed number of frames'
worth of records, the module cannot keep up and is disabled, with a message.
The base amount, the allowance, the cap and the backlog limit are open
question 7. Subscriptions are opt-in: a module that exports no `onFrame` and
binds no triggers costs nothing per frame.

### 5a. Languages and SDKs

The contract between engine and module is the Wasm interface alone. Any
language whose compiler produces a module that passes the load checks works,
and the engine treats every module the same way.

- **Official SDKs: TypeScript and Rust.** Both are generated from the one
  interface definition (section 12), carry the same functions under each
  language's naming style, and are versioned with the API. Each comes with
  templates, a build command, a watch mode and examples.
- **TypeScript SDK.** Compiles TypeScript to Wasm ahead of time, so the module
  is small enough to embed in maps. The compiler it uses is open question 2.
  Its API stays inside the common subset of TypeScript and AssemblyScript: no
  `any`, no union types other than `T | null`, no callbacks that capture outer
  variables, no `try`/`catch`, and parameters that are classes rather than
  object shapes. Handles are plain integers, so they have no methods: the
  TypeScript SDK uses namespace functions that take the handle first
  (`Techno.healthPercent(t)`, `Techno.setMission(t, Mission.Hunt)`), and
  setup arguments are classes built with their constructors
  (`new TriggerDef(...)`), not object literals. Every handle value (a
  creation serial) stays below 2^53, so an SDK built on a JavaScript engine
  can hold it in a number. Top-level code becomes the init function
  (section 3). The
  same source then type-checks with `tsc` and compiles with AssemblyScript,
  and the examples in this RFC are valid in both.
- **Rust SDK.** For authors who want full control and speed, and the SDK the
  engine's own compatibility fixtures are written in. It may keep methods on
  its handle newtypes (`techno.health_percent()`).
- **Other languages** (C, C++, Zig, Go, ...) use the published interface
  definition and its mapping onto the core-module ABI (section 12); their
  bindings are community work.
- **Imports.** A module may import only `vera:*` interfaces. The engine does
  not provide WASI and the project publishes no WASI shim. A language whose
  runtime needs WASI (for clocks, random numbers or files) works only if its
  own toolchain removes those imports.
- **Size and features.** A compiler that targets Wasm garbage collection or
  another rejected feature (section 3), as some Kotlin, Dart and Java
  toolchains do, produces modules that fail the load checks. Languages that
  bundle a large runtime or garbage collector in linear memory into every
  module produce bigger files, which may exceed the map embedding limit. That
  is a trade-off of the author's toolchain, not of the engine.

### 6. API layers

| Layer | Content | First API version |
| --- | --- | --- |
| L1 Actions and missions | every original trigger action, missions, production, house and object commands | yes |
| L2 Triggers | original events plus new ones (weapon fired, projectile detonated, ...) delivered to actions | yes |
| L3 Overrides | named decisions: target choice, damage, production permission, taking over a weapon's shot, ... | two or three, chosen from modder requests (open question 10) |
| L4 Per-frame behaviors | batched per-frame control of objects of a bound type | no, later |

Queries are read-only and iterate in a deterministic order. Every L1 action
calls the existing Rust port of that action (for example in
`sim/trigger_runtime`); modules never get a second implementation.

Overrides are named after game concepts, such as `Override.Damage` or
`Override.TargetChoice`, never after native functions, so they survive engine
refactors. `Override.Fire` lets a module take over a weapon's shot: it returns
a shot plan, a list of projectiles with their parameters, which the engine
creates at once in place of the native projectile (section 9).

For decisions made very often, data comes before callbacks: target priority is
a multiplier carried as an attached effect, plus a `TargetChanged` event, and
per-call overrides are kept for rarer decisions. `Override.CommandAllowed` and
`Override.Death` are further candidates. Missions that a module defines and
runs are left to a later RFC.

L4 calls the module once per frame per behavior type with all objects of that
type, never once per object.

**Granularity.** One API function is one game operation with exactly one
owner in the engine: fire a weapon, add an attached effect, set a mission,
change health, create an object. Larger behaviors, such as an air strike or a
mind-control chain, are built by authors from these.

Coordinates are parameters of any operation: targets, spawn points, teleport
destinations. A module can, for example, launch a nuke at any cell. What is
not exposed is a step that bypasses an owner, such as writing an existing
object's position field or an internal field. An object's position carries
cell occupancy, collision, sight, its map layer and its transport links, and
only the engine's paths keep them consistent, so an object changes place only
by teleport, a motion effect or a move mission. Projectiles are the exception
of section 9, level 4: they occupy no cells, and the engine clamps their
position and still runs collision.

### 6a. Triggers: events, conditions and actions

As in the original, a trigger has one or more events, all of which must hold,
any number of conditions, one or more actions and one of the original repeat
types.

- **Events** come in two kinds. *Delivered events* happen at a moment (damage
  taken, weapon fired, killed); the trigger fires at that moment, its original
  and atomic actions run at once, and its module actions are queued, each with
  the event's payload, for the module phase (section 5). A delivered event
  that VERA adds is raised at a safe point, a moment that is not inside any
  calculation: after the calculation that produced it has completed, or
  before one begins, as `HouseTransfer` is raised before the transfer begins
  (section 5). *Polled events* describe a state (health below a value, money
  above a value); the engine checks them natively at the interval the trigger
  sets, and the event fires on the edge: at the check where the state has
  become true, and not again until it has been false at a check. The checks
  of different objects are staggered: an object is checked in the frames
  where `frame % interval == serial % interval`, with `serial` its creation
  serial; a house poll uses the house's index in the house array instead.
- **Conditions** filter a trigger before any module code runs. The engine
  checks them natively and calls the module only when all of them hold. A
  condition can be negated. Logic beyond that (OR across conditions,
  comparisons between two objects, counting) belongs in module code.
- **Actions** are original trigger actions, the atomic actions of section 6b,
  or module functions. Original and atomic actions run when the trigger
  fires; module functions are queued for the module phase (section 5). A
  module function is also the only way a module receives an event.
- Every event names the object it is attached to and, where there is one, the
  other object involved (attacker, target, transport, producer). Conditions can
  test either.
- Events are raised only for bound objects: an object whose type binds the
  trigger (`Wasm.Tags`), an object whose own tag holds it, and, for weapon
  events (Fire, which carries the projectile handle, HitTarget and projectile
  Detonated), any object that uses a weapon, warhead or projectile type
  binding it. A weapon binding fires for every shooter of that weapon, whatever
  its type:

  ```ini
  [RedEye2]                   ; a weapon
  Wasm.Tags=MissileFiredTag   ; every shooter of RedEye2 raises its Fire triggers
  ```

- A trigger bound to a type or to a weapon, warhead or projectile type runs
  as its own instance on each bound object, with its own repeat state: a
  one-time trigger fires once per object. The trigger's house is the
  subject's owner at that moment, and the tag holder is the object that
  raised the event (section 7).

```ts
const PANICKED = Mark.id("Panicked");   // top level: runs once, in the SDK's init function (section 3); calls take the ID

onScenarioStart((ctx: Ctx): void => {
  const def = new TriggerDef([new EventDef(Event.TakeDamage)], [Action.wasm("giPanic")]);
  def.conditions = [Cond.healthBelowPercent(30), Cond.not(Cond.hasMark(PANICKED))];
  Trigger.bindType(Trigger.create(def), ctx.typeId("E1"));   // every E1 runs its own instance (section 7)
});
```

Candidate delivered events for version 1:

| Event | Payload |
| --- | --- |
| TakeDamage | source (optional), damage |
| HitByWarhead | warhead, final damage |
| AttackedBy | attacker, weapon index (optional), projectile (optional) |
| FatalDamage | attacker (optional), damage |
| Killed | killer (optional) |
| Fire | target (optional), weapon index, projectile (optional) |
| SpawnFire | spawned unit, target (optional), weapon index, projectile (optional) |
| HitTarget | target, damage, projectile |
| Detonated | projectile, cell, source (optional), warhead |
| TargetChanged | new target (optional: set when a target is chosen, empty when it is cleared) |
| ProjectileIncoming | projectile, launcher (optional) |
| Birth | producer (optional), effect (optional), build time (optional) |
| TransportEntered / TransportExited | raised on the passenger (subject); transport or building (other), passengers, occupied size, garrison or transport |
| PassengerAdded / PassengerRemoved | raised on the transport or building (subject); passenger (other), passengers, occupied size, garrison or transport |
| Crushed | crusher |
| HouseTransfer | new house, whether by mind control; raised before the transfer |
| HouseTransferred | previous house, whether by mind control; raised after the transfer |
| EffectStart / EffectStacks | effect, stack count |
| EffectExpire | effect type (the effect itself is gone) |
| Timer | frame of the previous firing; fires every period the trigger sets |
| SuperWeaponLaunched | superweapon type, house, target cell; lets a module implement a superweapon's effect |

Candidate polled events: health, ammo, passengers, transport space, ore
storage, current mission, moving or stopped, land type, house money, house
power, effect stacks, distance to the other object.

Further candidate events from the research: Built, Idle, MoveFailed, Arrived,
Promoted, Cloaked, Spotted, Lost, AmmoChanged, MissionChanged, HouseDefeated,
PlayerLeft, StorageFull, MotionEnded.

Candidate conditions: relation (self, ally, enemy, neutral), category
(infantry, vehicle, building, aircraft, projectile), layer (air, ground,
water), type ID, armor, civilian house, health value or percent, land type,
house power state, house money, has an effect (and its stacks), has a mark,
source of an effect, distance within a weapon's range.

Payload values are integers or handles; damage is an integer, as in the
original, and the simulation API has no floating-point values. History, such as
the total damage one attacker has dealt, is kept by the module in its own
memory.

**Not in the engine:** recorders, operators and expression keys in INI. A
module keeps such state in its memory and computes it in a few lines of code,
and that state is saved and hashed with the module. INI stays data, not a
programming language.

### 6b. Atomic actions and attached effects

Candidate instant actions for version 1: fire a weapon (by index or name, at a
cell, an object or the current target), launch a superweapon, spawn an
animation, remove, kill, deal damage through the warhead and armor rules
(with a source for kill credit and experience), set health directly (no
credit), create an object, set a mission, teleport to a cell (as the
Chronosphere does; when the cell is blocked, a stated rule picks a nearby
cell or the action fails), clear the target, clear a disguise, search an area
(a query that returns objects), play a sound, add experience, add money, change the tech tree, transfer the owner, clear mind
control, change ammo, add or remove passengers, add or remove an attached
effect, add or remove a mark, create a projectile (type, start, target object
or coordinate, source for kill credit and experience, initial module data),
detonate a warhead at a coordinate with a damage and a source, detonate a
projectile now, remove a projectile without detonating it,
end the game for a house (win or lose) and set scores, write a line to the
in-game log.

In a trigger, an atomic action is written `Action.atomic(kind, target,
params)`, beside `Action.original` and `Action.wasm`. It covers an action
whose parameters fit: one target, one ID, up to two integers, one cell and one
house. `Log` is the one accepted exception: it also carries its line of text.
An action that needs more, such as creating a projectile, is written as a
module action. The target is an explicit parameter: the subject (the default),
the other object of the event, the tag holder (the objects whose tag holds the
trigger, or the cell for a CellTag), the event's projectile, a house, or a
cell. A waypoint is given as a cell that carries its number:
`Cell.waypoint(n)` returns such a cell, and `Trigger.create` resolves it to
the waypoint's cell, since waypoints do not move. A waypoint the map does not
have is a map data error, not a module error: `Trigger.create` returns
`NO_HANDLE` with `NotFound`, creates no trigger and logs it, and the module
stays enabled; in INI it is reported in the lobby at load. A kind that uses no
target (for example setting a tech level, or `Log`) ignores the target. For a
trigger bound to a type or a weapon, warhead or projectile type, the tag
holder is the object that raised the event. Fixed rules cover the rest:

- An explicit house (the house target, the new owner of a transfer, the owner
  of created objects) is the house given in the parameters; when none is
  given, the trigger's house is used, which for a bound trigger is the
  subject's owner at that moment.
- A source (of damage, a kill or a detonation) is the event's other object and
  is optional: when the event has none, the action runs with no source and
  gives no kill credit. An action that needs a second object to act on (a
  passenger) uses the event's other object; when the event has none, the
  action gets `Gone` and is skipped, and the status is logged.
- `FireWeapon` fires the weapon type in the parameters when one is given,
  else the weapon in the slot the parameters give (default 0, the primary
  weapon). It aims at, in order of precedence: the cell in the parameters if
  one is given, else the event's other object, else the shooter's current
  target; with none of them it gets `Gone` and is skipped.
- An atomic action that requires a cell (for example `LaunchSuperWeapon`)
  and is given none is an invalid argument.
- The cell of an object target is the cell the object is in; a `Projectile`
  target counts as an object target, and its cell is the one under the
  projectile. A coordinate is the center of the given cell, or of the
  target's cell, except for a projectile target, whose coordinate is the
  projectile's center.
- The appendix lists the target kinds and fields each kind uses; the exact
  mapping of each kind to its parameters belongs to the interface definition
  (section 12).

`Trigger.create` checks the arguments of each atomic action: an invalid one,
including a target kind that can never fit the action (for example a cell for
a kill), is an invalid argument from module code and disables the module
(section 3). Atomic actions written in INI are checked at load and reported in
the lobby; their INI form is open question 4. A target that turns out not to
fit when the trigger runs (for example a tag holder that is a cell, for a
kill) is a data error, not a broken module: the action gets `Refused`, its
status is written to the in-game log, it is skipped, and the module stays
enabled (section 3). Atomic actions run at once and may raise further events,
so they can chain within a frame up to the depth limit of section 5.

Attached effects are per-object states with a duration, owned by
`sim/attach_effect.rs` (section 8). Candidates: tint, attached animation,
prevent death, transform into another type, replace a weapon, motion (thrown,
pushed), teleport, multipliers for damage taken, firepower, speed and rate of
fire, range, target priority multiplier (section 6), cloak, immunity to a
warhead or attack type, cannot fire, no target search, damage modifier, cannot
be selected, ignores player orders, not a legal target, house power off.

- Each effect type declares how it stacks: stack up to a limit, replace the
  older one, or refuse the newer one. Effects that cannot run together
  (transform, teleport, motion) form an exclusive group, and the owner keeps
  one at a time.
- Effect properties follow Phobos AttachEffect where it has a key: duration,
  delay, interval, cooldown before reapplying, maximum firings, source,
  inherited on transform or creation, ignores the Iron Curtain. New properties
  use the same reader.
- Motion is added to the object's movement by the engine, never written as
  coordinates.
- Text, values or buttons shown over an object are client display properties
  (section 10), not effects.

An object carries any number of marks, and so does a house. Actions and
effects add and remove them; conditions test them. A mark carries one integer
value and a visibility level: its own house, allies, players who can see the
object, or everyone. Both are given when the mark is added
(`addMark(t, mark, value, visibility)`); `markValue` reads the value and
`setMarkValue` changes it. Client queries return only the marks their player
may see. Marks do not nest. Mark names are registered only by simulation code;
the registry is simulation state, saved and hashed, and a client module only
looks names up (appendix, IDs).

### 6c. Queries

Queries are read-only. Results are sorted by creation serial, a rule that does
not depend on how the engine stores objects, and are written into a buffer
the module provides (section 15). Three lists keep an order of their own: an
object's attached effects come in attach order, its marks in the order they
were added, and houses in the order of the house array. In client modules a
query exposes only what the player's own interface could show (appendix
7.7): full information about the player's own house and objects, public
facts about other houses, and the type, position, owner and health of the
other houses' objects the player can see.

- **Objects:** in a cell, in a radius or a rectangle, by type, house or
  category; an object's type, owner, health, position, mission, target,
  veterancy, ammo, passengers, attached effects and marks.
- **Houses:** money, power output and drain, owned buildings and met
  prerequisites, tech level, allies and enemies, side, whether defeated.
- **Map:** size; a cell's land type, height, overlay and ore, passability for a
  movement zone, whether a house can see it; waypoints and cell tags.
- **Rules:** any key of any type, as a parsed value.
- **Scenario:** frame number, difficulty, game mode, number of players.
- **Probes** (candidates for version 1, open question 12): whether a weapon
  could fire at a target now, whether a target is in range, whether a type can
  be placed at a cell for a house, and the closest cell where it can be. They
  reuse the engine's own rules.

### 6d. AI

Computer houses keep their AI. Version 1 lets modules create task forces and
team types (`[TaskForces]`, `[TeamTypes]`), assign AI team scripts
(`[ScriptTypes]`), start and disband teams, and enable or disable AI triggers
(`[AITriggerTypes]`), through the existing owners (`sim/ai_team_creation.rs`,
`sim/team_script_vm.rs`). Teaching the AI to use mechanics a mod adds is left to
a later RFC.

### 7. Triggers and tags at run time

Modules create triggers and tags with the same model as maps:

```ts
// inside an action (ctx, e):
const def = new TriggerDef([new EventDef(Event.Killed)], [Action.wasm("onConvoyLost")]);
def.house = ctx.house("Americans");
const t = Trigger.create(def);
const tag = Tag.create([t], TagRepeat.Repeating);
Techno.setTag(e.subject, tag);   // replaces the object's tag, as in the original
const cell = new Cell(40, 52);
cell.setTag(tag);                // CellTag; Cell is a class, not a handle
Trigger.disable(t);

const panic = Trigger.create(new TriggerDef([new EventDef(Event.TakeDamage)], [Action.wasm("giPanic")]));
Trigger.bindType(panic, ctx.typeId("E1"));   // as Wasm.Tags=: every E1 runs its own instance
```

- Created triggers and tags live in the existing trigger runtime's lists, next
  to the map's. There is no separate store.
- They get deterministic IDs that cannot collide with the map's, and they are
  saved and hashed with the trigger runtime.
- An object holds one tag (`ObjectClass` `AttachedTag`, `+0x34`). A tag may
  link several triggers. Attaching several tags to one object would be a VERA
  extension and needs its own RFC.
- Type bindings (`Wasm.Tags` in INI, `Trigger.bindType` in code, and
  `Trigger.bindWeaponType`, `bindWarheadType` and `bindProjectileType` for
  the weapon events) do not use that slot. They are kept as separate
  subscriptions, so an object placed on the map with its own tag keeps it and
  still receives its type's triggers. A binding made in code applies at once
  to every object of the type, mod and map modules alike.
- An object that gets a tag holding a module trigger or a trigger on an event
  VERA adds (`Techno.setTag`) gets a flag of its own, so the event sites raise
  those events for it even when its type binds nothing (section 15).
- Each bound object runs a bound trigger as its own instance: its repeat
  state is per object, so a one-time trigger fires once for each object.
  Disabling or removing the trigger acts on every instance. The trigger's
  house is the subject's owner at the moment it springs, and its tag holder
  is the object that raised the event.
- Removing an object or a tag clears the links through the trigger runtime's
  existing cleanup.

New event and action numbers are allocated in a range that does not collide
with Ares or Phobos.

### 8. Changing rules

| When | What | How |
| --- | --- | --- |
| `onRules` | any key; new types | through the native INI readers, clamps and post-read passes; included in the lobby hash |
| During a match, per type | keys on a published allowlist | `rules.set("E1", "Speed", "6")`: the value is parsed like INI text, takes effect from the next frame, invalidates the dependent caches, and is saved as a rules overlay |
| During a match, per object | multipliers, veterancy, owner, type change, health | through the existing per-object owners, including attached effects (section 6b) |

- Per-object multipliers reuse the owner that already exists in
  `sim/attach_effect.rs`. Part of that code is still behind
  `cfg(feature = "phobos")`; it moves to a run-time switch before modules can
  use it, as the project contract requires. The owner is available whenever
  the module feature is on (section 14). The Phobos AttachEffect INI keys
  (`AttachEffect.*` on types, weapons and warheads, `[AttachEffectTypes]`)
  are read only when the Phobos switch is on, so stock play stays stock;
  effect types a module defines are created through the module API
  (`onRules`) and do not need those keys. The effect kinds only VERA has
  switch on the same way.
- Each allowlisted key documents what a change does to objects already on the
  map (for example, how current health follows a new `Strength`).
- New types cannot be added during a match.
- With Ares or Phobos switched on, modules see their keys in the rules. The
  allowlist states for each extension key whether it may change during a
  match.

### 9. Example: projectile paths

All four levels run in `rules.wasm`, because they decide what is hit.
`ui.wasm` may add purely visual effects such as trails and lights.

| Level | Mechanism | Cost |
| --- | --- | --- |
| 1 | `onRules` sets existing keys (`Arcing`, `ROT`, `Vertical`, Phobos `Trajectory` when Phobos is on) | none during play |
| 2 | triggers on weapon fired and projectile detonated: extra projectiles, splits, chains | per event, only for bound types |
| 3 | `onLaunch` returns a path (Bezier, parabola, spiral, waypoints); the engine moves the projectile along it in fixed point | one call per projectile |
| 4 | `onTrajectoryFrame(type, bullets)` changes speed or position, or asks to detonate, for all projectiles of the type at once | one call per type per frame |

The engine adds a module arm to `ProjectileTrajectory` beside the three native
flight arms (`BulletClass::AI` `0x004666E0`). Collision stays in
`world::projectile_collision`, detonation admission in `sim/projectile`, and
damage and warheads in `combat`. The engine clamps module output: speed and
position limits, and leaving the map detonates or removes the projectile. Each
projectile has 16 bytes of module data that is saved and hashed.

To fire several projectiles with different paths at once, an action on the
weapon's Fire event creates them, each with its own type and module data, and
`onLaunch` gives each one its path. The action is a module function, which
reads the shooter, the target and the native projectile from its event record
(`e.subject`, `e.other`, `e.projectile`). The native projectile leaves during
the unit's update and the module's projectiles in the module phase, so they
trail it by the time between the two: up to one frame of movement, depending
on where the module phase sits (open question 8). Creating a projectile takes
more parameters than an atomic action has (section 6b), so a trigger creates
projectiles only through a module action.

`Override.Fire` avoids that delay. It returns a shot plan: a list of
projectiles to create, each with its type, module data and parameters. The
engine creates them at once, at the moment the native shot would leave, and
creates no native projectile; an empty plan fires nothing, and "not handled"
keeps the native shot. Every other native effect of the shot still happens:
the rate-of-fire timer, ammunition use and reload, the burst counter, the
report sound, the muzzle animation, recoil and decloaking or revealing on
fire. The plan's projectiles raise `ProjectileIncoming` and the target's
`AttackedBy` as native projectiles do. The `Fire` event is still raised,
with `projectile` set to `NO_HANDLE`. When the mod's and the map's modules
both bind the shot, the map's link receives the mod's plan and may keep,
change or replace it. Where the fire step is a batched section, the
projectiles are created when the section ends (section 5).

### 10. Client module

**Interface tree.** The engine holds the interface tree, lays it out and draws
it. A module changes it with a small set of operations: create and remove
nodes, set properties (label, art, position, visibility), find nodes by name,
and receive clicks, hover and hotkeys. Changes are batched per render frame;
the tree is not rebuilt every frame. No graphics API is exposed.

Authors write an interface in two parts:

- **Static layout in INI**, with the syntax and keys of `veraui.ini` (`Type`,
  `Hold`, `X`, `Y`, `Width`, `Height`, `Parts`, `Label`). This RFC adds the
  `List` part type and `Label` for `Text` parts. The engine reads it
  when the match starts; nothing is precompiled. An interface without dynamic
  parts needs no module.
- **Dynamic parts in module code**: find nodes by name and change them, or
  create new ones.

```ini
[Mission.Field]
Parts=Messages,Timers,Banner,Powers,Clock,Objectives

[Mission.Objectives]
Type=Column
Hold=TopRight
Width=240
Parts=ObjTitle,ObjList

[Mission.ObjTitle]
Type=Text
Label=TXT_OBJECTIVES    ; new for Text: a string-table line, follows the game language

[Mission.ObjList]
Type=List               ; new part type
```

```ts
onStart((ui: Ui): void => {
  Node.setItems(ui.find("ObjList"), ["TXT_OBJ_RADAR", "TXT_OBJ_CONVOY"]);
  ui.create(new Column(Hold.BottomLeft, [new Text("TXT_WAVE")]));
});
```

A React- or JSX-style library can live inside an SDK: it computes differences
inside the module and sends the same operations, and the engine does not know
it exists. Such a library is TypeScript only, because AssemblyScript has no
closures, and is not part of the official API.

**Other client features:**

- Display properties per object: image, palette, tint, extra animations,
  visibility, and text, values or buttons shown over it. Properties that change
  gameplay, such as cloaking, belong to the simulation module.
- Selection and button clicks are client events. To change the game, the module
  sends a message (section 1) and the simulation module handles it in
  `onMessage`, so online play and replays stay in step.
- A client module may issue standard player orders (select, move, attack,
  deploy, build), which the engine validates like any player input.
  `sendMessage` is for custom logic.
- Each mod and each map has its own small key-value store on the player's
  machine, with a size quota, for campaign progress, achievements and
  settings. A mod's store is keyed by its `[Mod] Name`; a map's by its
  storage ID (`[VeraWasm] StorageId=`, default the map's file name), so a
  changed map keeps its store as long as the ID stays the same. A map's
  client module uses the map's store, also under a mod, so a downloaded map
  cannot read or change the mod's data. Only client modules
  can use a store. Data that should affect a match reaches the simulation
  through `sendMessage`, so it is part of the command stream.
- Images, animations and models are referenced by name, never by file format,
  so later asset formats (a separate RFC) need no API change.
- Sounds and messages; `log()` writes to the in-game console.
- Notifications are named, and a name resolves once to an ID:
  `Notify.id("WaveStarted")` returns a `NotifyId`. The ID is the 32-bit
  FNV-1a hash of the name's UTF-8 bytes after ASCII case folding (only `A` to
  `Z` fold), computed the same way on every machine and in both the
  simulation and the client module. A simulation module declares ahead of
  time the names it sends: the SDK build writes them, from a list in the
  module's project configuration, into a Wasm custom section `vera.notify`.
  The format of that list is SDK documentation, not engine API. Client modules
  declare nothing, and a declared name that no client module receives is not
  an error. In a simulation module, `Notify.id` for a name the module did not
  declare returns `NO_ID` and sets `NotFound`. In a client module it returns
  an ID only for a name that some simulation module of the match declared,
  because the engine keeps the table of declared names; any other name gives
  `NO_ID` and `NotFound`, so a client never matches an undeclared name by
  hash. When it loads the modules, the
  engine checks the declared names of all simulation modules loaded in the
  match, the mod's and the map's together, for collisions, and refuses modules
  whose names collide, with a message naming them (section 3, step 6), so
  `Notify.id` returns an ID for every declared name.
  The simulation module sends a notification to one player's client with
  `ctx.notify(house, id, data)`, or to every spectator's client with
  `ctx.notifySpectators(id, data)`; every client module receives it in
  `onNotify(ui, source, id, data)`, with the sending module's source
  (section 2). A replay viewer receives the notifications of the
  player whose view they watch, or the spectators' ones. Spectators cannot
  send messages into the simulation: `sendMessage` from a spectator's client
  is refused.
- Client queries expose only what the player's own interface could show
  (appendix 7.7): everything about the player's own house and objects; for
  other houses only public facts (name, side, alliance, defeated); for objects
  of other houses the player can see now only type (the disguise, for a
  disguised enemy), position, owner and health. Passengers, targets,
  missions, superweapon charge, money and power of other houses, global and
  local variables, triggers and tags are never visible, and visibility
  queries answer only for the player's own house. Anything else a client
  needs reaches it through notifications. A spectator's client modules see
  what the spectator sees, the whole map when the room allows it. An object
  of another house the player cannot see now is absent from results; the
  engine keeps no last-seen copies.
- Player-installed interface modules come in a later phase. A mod or a room
  may forbid them.

### 11. Online play

- Embedded modules travel with the map. Mod modules are part of the existing
  mod file check.
- The lobby compares module hashes and required API versions; it reads each
  `[VeraWasm] Api=` before loading the modules. Since rooms require one game
  version (below), the version check matters where versions can differ: a
  map or mod that needs an API version the room's game lacks cannot be
  chosen, and offline, a save or a replay is refused, each with a message
  that names the version needed. The mod's `[VeraWasm]` keys (budgets, map policy,
  API version) are in its `rulesmd.ini`, which the mod digest covers, so every
  player compares them; `mod.ini`, which the digest leaves out, holds no
  module keys.
- The engine fills in the sender of every message, so a module cannot forge
  it, and limits message size and messages per frame.
- Notifications from the simulation module to a client reach only the player
  they are for, or the spectators. Their content is the module's own bytes,
  and the module's author is responsible for it: the engine cannot filter it.
- Online play exchanges only commands, never state. A rejoining player
  re-simulates the match from the start, modules included; module messages are
  commands, so nothing extra is sent. A module's cost per frame therefore also
  slows catch-up. During catch-up, notifications are not delivered and client
  modules do not run; they receive one `onStart` after catch-up.
- Replays record the module hashes and API versions. A replay whose modules do
  not match is refused with a message.
- Saves record the module hashes. A save whose modules do not match the
  installed mod or map is refused with a message naming them.
- Rooms require every player to run the same version, so a mixed room never
  starts and online play is safe.
- Offline, a build from before module support plays a module map without its
  modules: it ignores the `[VeraWasm*]` sections, and its triggers' module
  actions do nothing. This is a stated residual, since released builds cannot
  change. To cover later releases, the release before module support adds a
  map key, `[Basic] VeraMinVersion=` (name not final), and refuses a map that
  needs a later version with a message naming it; module maps set it, so
  every release from then on refuses them instead of playing them wrongly.
- Saves and replays record the game version and the module hashes; an older
  build refuses those a later version made, with a message.
- The change updates [Networking](../Networking.md) and states what a released
  game, a mixed room and a rollback do with modules.

### 12. Versioning and the ABI

- One interface definition, written in WIT syntax, is the source of the API.
  The project's own generator produces from it the Rust and TypeScript SDK
  bindings, the reference documentation and the compatibility checks. WIT is
  only the interface description language: modules do not use the Component
  Model's canonical ABI.
- The binary interface is a core-module ABI: a core Wasm module's imports and
  exports, integer handles and IDs, pointer-and-length pairs for strings,
  bytes and buffers, and results the host writes into buffers the module
  provides. The interface definition states how each of its types maps onto
  this ABI. The reasons:
  - every engine the browser build may use, the browser's own and
    interpreters included, runs core modules (section 14, open question 1);
  - section 15 needs zero-copy buffers: the host writes query results, event
    records and batches straight into module memory;
  - all module state stays in linear memory, globals and the table, so
    saves, the state hashes and rejoining capture it (section 4);
  - AssemblyScript, which produces the small modules a map can embed, builds
    core modules (section 5a, open question 2);
  - the core specification is frozen.
- A later major version may add a Component Model adapter over the core ABI.
  Existing modules keep the core ABI and are not affected.
- Interfaces are versioned packages: `vera:sim@1`, `vera:client@1`. A
  version is major.minor. A new major is incompatible, and every major is
  kept forever through adapters; a new minor only adds. A module states the
  major and the lowest minor it needs through its imports, and the map or mod
  states the same in `[VeraWasm] Api=`. The lobby reads `Api=` before
  loading and checks that every player's game supports that major at that
  minor or later; loading checks that the module's imports match it
  (section 3).
- Unstable interfaces live under `vera:unstable/*`, require an explicit
  opt-in and may change without notice. They and the `0.x` previews are not
  published versions: they keep no adapters, and a module that uses them runs
  only on the build it was made for.
- Every published version keeps working. Older versions are thin adapters over
  the current implementation, never a second implementation. An adapter never
  hands an old module a value its version did not define: records of events
  that version does not know are not delivered to it, and a newer status or
  enum value reaches it as a value its version defines, chosen per enum in the
  interface definition.
- For every published version, CI builds a set of compatibility fixtures from
  Rust SDK sources committed in the repository and runs them; no `.wasm`
  binaries are committed. Breaking one fails the build. This set grows with
  every version, a deliberate exception to the project's rule of one to three
  regression tests per fix.
- The metering units, the charge of each host call kind, the default budgets
  and the limits are part of a major version and fixed with it (section 4).
- Functions may be deprecated, with a warning to the module author, but a
  published version is never removed.
- A published function never changes its units or meaning. A change is a new
  function or a new version.
- The custom sections `vera.actions` and `vera.notify` and the delivery-area
  declaration each start with a format version. Names in them are UTF-8,
  must be ASCII and fold by ASCII rules only.
- `Trigger.create` and the INI resolve an `Action.wasm` name through the
  module's `vera.actions` table to an index; `EventRecord.action` carries
  that index, and the SDK's generated `onEvents` dispatches on it. Action
  functions therefore need not be Wasm exports.

### 13. Tooling

| Need | Provided |
| --- | --- |
| Start | templates for `rules.wasm` and `ui.wasm` in TypeScript and Rust; an example mod in both |
| Size | SDK release settings for small modules; documented size guidance per language; map embedding limit |
| Maps | `asset` commands to embed, extract and validate modules |
| Iteration | the SDK's watch mode rebuilds on save; in development mode, offline only, the game watches the `.wasm` file and reloads it (module state resets) |
| Errors | the failing export and the error in game; the SDKs pass messages and source locations to the engine |
| Cost | an overlay with each module's operations and trigger deliveries per frame |
| Docs | reference generated from the interface definition; task-based guides |
| Map editors | FinalAlert2 data entries for the new events and actions |
| Testing | run a map with its modules headless and check results, reusing the headless scenario runner |

### 14. Project contract

- The extension-module feature (not to be confused with the `wasm`,
  `wasm-net` and `wasm-threads` build features of the browser build) is in
  every build and turns on at run time when a mod or map provides a module or
  uses any of its keys (`Wasm.Tags`, INI atomic actions, `[VeraWasm]`). It is
  never behind a build feature. The attached effect owner is available
  whenever it is on; the Phobos AttachEffect INI keys are still read only
  when the Phobos switch is on (section 8).
- The Wasm runtime is chosen per platform behind platform build features,
  which the project contract allows for platform features: a native engine on
  desktop, and the browser's own WebAssembly or another engine in the browser
  build (open question 1). The simulation reaches the runtime through one
  interface, so `vera20k-core` and `sim/` keep compiling for the browser.
- Every module call reaches state through its existing owner. The API adds no
  competing state.
- The feature is documented under `docs/` for modders.
- Module failures are reported in game, never only in the log.

### 15. Performance

The engine keeps one copy of the game state. Modules hold integer handles and
IDs and ask for data when they need it; nothing mirrors the world into module
memory. These rules keep the boundary cheap and are part of the interface
definition and rely on the core-module ABI (section 12):

- **IDs, not names, on hot paths.** Type, house, weapon, warhead, projectile,
  mark and effect names, and the other named kinds the appendix lists, resolve
  once to integer IDs (`ctx.typeId("E1")`,
  `Mark.id("Panicked")`), usually in `onRules` or `onScenarioStart`; an ID
  never changes once assigned (section 2). Every other call takes IDs. Calls
  that take names (lookups, `rules.set`, trigger setup) are for setup and rare
  changes; the SDKs cache the IDs. A notification ID is a stable hash of its
  case-folded name, the same on every machine and on both sides (section 10).
- **One delivery per module per frame.** The frame's queued module actions
  reach each module in one call, as fixed-size records in the order of
  section 5 (subject creation serial, then raise order within one object). Each
  record is one module action with the payload of the event that fired its
  trigger; there are no stand-alone event records. The record layouts are
  part of the interface definition. The SDK reserves the delivery area in
  module memory and states its size at load; the per-frame queue cap
  (section 5) bounds what one call delivers.
- **Queries write into the module's buffer.** Results go straight into a
  buffer the module provides; the engine allocates nothing per query. Results
  are sorted by creation serial (with the three exceptions of section 6c), at
  a cost that grows with the size of the result, not of the world. Large
  results are paged: a paged query's result set is fixed when its first page
  is read within a module call, kept in one reusable scratch buffer per
  module, and later pages in the same call continue that set even if the
  world changes between them.
- **Host calls cost what they do.** Each host call is charged to the
  operation budget by the work it does, per object scanned or returned or per
  cell checked, so a module cannot stall every machine with cheap-looking
  calls (section 4).
- **Side tables.** Marks, subscriptions and module data per projectile live
  in side tables keyed by handle, and type bindings in tables keyed by type,
  so only objects and types that use them pay. Each event site checks one
  flag of the object's type, or of the object itself when it got a tag with
  module triggers (section 7); with neither set, nothing else runs.
- **Polled events** have a minimum interval, and the engine spreads the checks
  of different objects over different frames by creation serial modulo the
  interval (section 6a).
- **Module memory in hashes.** Online desync detection uses the per-frame
  multiplayer checksum. Every 15 frames it includes a hash of each module's
  memory, which the memory cap bounds, so a divergence in module memory is
  detected online. The replay log's diagnostic hash (every 15 frames) and the
  full state hash also cover module memory. Their cost is part of open
  question 3.
- **No allocation per event in the SDKs.** Handles and IDs are value types:
  in TypeScript, handles are 64-bit integers below 2^53 and IDs 32-bit
  integers; in Rust, both are newtypes. Objects are
  created only for setup calls such as `Trigger.create`.
- **Per-frame batches** (L4 behaviors, module projectile paths) are written
  directly into module memory and read back from it, a few dozen bytes per
  object.
- **Budgets for the slowest platform.** Metering slows module code, and the
  browser build may interpret modules. In lockstep the slowest machine sets the
  pace, so default budgets are sized for the slowest supported platform and
  measured before acceptance (open questions 1 and 3).
- **No module, no cost.** Without modules, each event site costs one flag
  check and nothing else runs.

## Process

1. **Draft.** This document is discussed in public. Changes bump the RFC
   version and are listed in the [changelog](#changelog).
2. **Open questions answered.** Experiments outside `src/` answer the
   unresolved questions below. Results are summarized here.
3. **Accepted.** The maintainer marks a version as accepted. Development starts
   only then.
4. **Preview.** The API ships as `0.x`. Breaking changes are allowed and listed
   in a changelog; a preview is not a published version (section 12). At
   least one real mission or mode is rebuilt with it, and outside modders try
   it.
5. **Stable.** `vera:sim@1` and `vera:client@1` freeze, limited to what the
   preview used, the two or three first overrides of open question 10
   included. The compatibility fixtures go into CI.
6. **Growth.** Each new interface or override is a small follow-up RFC and a
   new minor version.

Planned order after acceptance (stages 1 to 3, not to be confused with the
appendix's proposals P1 to P21):

| Stage | Content |
| --- | --- |
| 1 | `rules.wasm`: `onRules`, `onScenarioStart`, `onFrame`, L1, L2 with conditions, attached effects, marks, queries, AI teams, type tag bindings, the trigger action, the two or three first overrides (open question 10), map embedding, `asset` commands, TypeScript and Rust SDKs and templates |
| 2 | `ui.wasm`: interface tree with INI layout, display properties, visibility filter, messages |
| 3 | real mission rebuilt with modules, outside modder trial, freeze of version 1 |
| Later | further overrides one by one, L4 behaviors, peer calls, player interface modules |

## Alternatives considered

- **Lua run by the engine** (as Recoil does). Easy for modders, but float and
  table-order determinism must be patched into the interpreter, and the API
  grows as one unversioned namespace.
- **A language runtime in the engine** (Lua or JavaScript shipped with
  VERA20k for modules to import). Modules would be smaller, but the engine
  would own a language runtime forever and have to pin one build per API
  version. Rejected: languages stay in the authors' tools.
- **Native plugins.** Not portable to the browser build, not sandboxed.
- **The Component Model and its canonical ABI** as the binary interface. Not
  adopted (section 12): browsers do not run components natively, and an
  interpreter in the browser build would need a component runtime as well;
  the canonical ABI passes results through allocations in the module, where
  section 15 needs the host to write into the module's own buffers; resource
  handles live in a per-instance table outside module memory, which saves,
  the state hashes and rejoining would have to capture separately;
  AssemblyScript does not produce components; and the component
  specification is still changing, while the core specification is frozen.
  WIT stays as the interface description language. A later major version
  may add a Component Model adapter over the core ABI without affecting
  existing modules.
- **Simulation queries as a given house.** Not adopted for now. Simulation
  queries see everything; client queries are filtered.
- **Overrides kept pure by the host** (state written during an override
  thrown away, no random draws). Not adopted: a fixed call order (section 5)
  makes override state deterministic at a lower cost.
- **Messages between client modules.** Not adopted. All traffic between
  players goes through the simulation, so online play keeps one channel.
- **Only more INI keys.** Keeps working for simple cases and stays the first
  choice where it is enough, but cannot express general logic.

## Prior art

- **Recoil (Spring).** Synced and unsynced code are separate; unsynced code
  reaches the simulation only through network messages; per-object values
  carry visibility levels; costly callbacks fire only for objects a script
  watches. Lessons taken: no raw graphics API, versioned packages instead of one
  global namespace, opt-in callbacks.
- **Red Alert 3 (SAGE).** Data files bind object events to script functions,
  with inheritance, and handlers receive the object. Taken: binding in data,
  not in code.
- **Phobos.** `Trajectory=` types for projectiles; AttachEffect for per-object
  modifiers.
- **Community trigger, effect and condition design** (a spreadsheet shared
  during this RFC's discussion). Taken: the trigger as event, conditions and
  actions; delivered and polled events; the event list with typed payloads; the
  lists of instant actions, lasting effects and conditions. Not taken:
  recorders and operators in INI, which module code replaces.

## Unresolved questions

1. **Browser build.** Can the browser build run modules with results identical
   to the native build, and with which engine? Wasmtime may not run inside a
   browser.
2. **TypeScript compiler.** Which compiler the TypeScript SDK uses: a
   compiler for a TypeScript subset to Wasm (such as AssemblyScript), or a
   JavaScript engine compiled into each module. The engine is the same either
   way; the choice decides module size, speed and how much ordinary TypeScript
   works.
3. **Metering.** Which instrumentation for the operation count and the call
   depth, and what does it cost at 20,000 units? What does hashing the whole
   module memory every 15 frames in the multiplayer checksum cost, and what
   do the other state hashes that cover it cost when replaying?
4. **INI format.** Named sections as shown, the original comma-separated
   trigger encoding so FinalAlert2 can edit bindings, or both? What prefix do
   the keys use, given that bindings, conditions and atomic actions also work
   without a module?
5. **Event and action numbers.** Which range is free of Ares and Phobos?
6. **FinalAlert2.** Does it keep unknown sections such as `[VeraWasmRulesPack]`
   when it saves a map?
7. **Limits.** Embedded size, memory, load-time shape limits, the base
   operation budget and per-record allowance, the charge of each host call
   kind, the one-off budgets of the init function, `onRules` and
   `onScenarioStart`, the per-frame queue cap, the backlog limit, the client
   modules' operation budget per render frame, the minimum poll interval,
   message size and rate, and the store quota, for mods and maps. They are
   fixed per API major version (section 12).
8. **Frame phase.** Where in the simulation frame the module phase sits, and
   where the module projectile arm sits relative to collision and detonation.
9. **Save size.** Module memory in snapshots: compression and the memory cap.
10. **First overrides.** Which two or three overrides ship in version 1.
11. **Allowlist.** Which keys can change per type during a match in version 1.
12. **Version 1 catalog.** Which events, conditions, actions, attached
    effects, queries and probes from sections 6a to 6c, and which other `v1?`
    entries of the appendix, ship in version 1.
13. **Effect stacking.** The stacking rule and exclusive group of each attached
    effect.
14. **Interface layout.** Where a mod and a map keep layout sections, and the
    widget set of version 1.
15. **Map text.** Maps cannot ship string tables. How a map module's text is
    translated.
16. **Campaigns.** Whether a mission's modules start fresh or continue the
    previous mission's state.
17. **Chain limits.** The per-frame limit on the depth of a chain that
    includes a module trigger, an event VERA adds or an atomic action, and
    the per-frame cap on the total number of such chained trigger firings
    (section 5).

## Changelog

- **0.0.6** (2026-11-16): performance rules (section 15): IDs instead of
  names on hot paths, one delivery per module per frame of its queued module
  actions, each carrying its event, query
  results in module buffers, side tables, staggered polls, module memory in
  the state hashes, no allocation per event in the SDKs, budgets for the
  slowest platform; module actions batched per module per frame while original
  actions keep their original timing (replaces the 0.0.3 rule that deferred
  whole mixed triggers); weapon, warhead and projectile bindings for weapon
  events; more ID kinds; atomic actions in a trigger run when it fires, and only
  module functions are queued; no stand-alone event records: a module receives
  events only through triggers whose action is a module function, one record
  per queued action; "projectile launched" event removed (weapon events are
  Fire, HitTarget and Detonated); `Aimed` merged into `TargetChanged`, plus a
  target priority multiplier effect; action functions take
  `(ctx, e: EventRecord)`; notification names resolve to a `NotifyId`; client
  code uses the `veraui.ini` names (`new Column(Hold.BottomLeft, parts)`); actions
  that run at once chain within a frame, and a chain that includes a module
  trigger (one that has a module action, was created by a module or is bound
  through `Wasm.Tags`, a map trigger with a module action included), an event
  VERA adds or an atomic action is limited to a per-frame depth, beyond which a
  trigger fires in the next frame (open question 17, engine gap G-T11), while
  a chain of only original events and original actions runs exactly as in the
  original (Goal 5); events VERA
  adds are raised at safe points, moments that are not inside any calculation
  (after a calculation completes, or before one begins, as for
  `HouseTransfer`); each atomic action in a trigger takes an explicit target
  (subject, other, tag holder, the event's projectile, house or cell): the tag
  holder of a CellTag is the cell, a waypoint is given as its cell
  (`Cell.waypoint(n)`, resolved when `Trigger.create` runs, since waypoints do
  not move), and a kind that uses no target (such as `SetTechLevel` or `Log`)
  ignores it; a target kind that can never fit the action (such as `Cell` for
  `Kill`) is an invalid argument found by `Trigger.create`, while a target
  that turns out not to fit when the trigger runs (such as a `TagHolder` that
  is a cell) gives `Refused`, is logged and skipped, without disabling the
  module; an atomic action is written `Action.atomic(kind, target, params)`,
  with `AtomicKind` and `AtomicParams`, and covers only an action whose
  parameters fit one target, one ID, up to two integers, one cell and one
  house, with the text of `Log` as the one accepted exception, while anything
  more, such as creating a projectile, is a module action; `AtomicParams` has
  a `house` field: an explicit house (the `House` target, the new owner of
  `SetOwner`, the owner of created objects) comes from `params.house`, and the
  trigger's house is used when it is `NO_HANDLE`; a source (of damage, a kill
  or a detonation) is the event's other object and is optional: for `Kill`,
  `Damage` and the `Detonate` kinds an event without one runs the action with
  no source and no kill credit, and only an action that needs a second object
  to act on (a passenger) gets `Gone` without one and is skipped, while
  `FireWeapon` aims at `params.cell` if given, else the event's other object,
  else the shooter's current target; a coordinate is the center of a cell,
  and the cell of an object target includes a `Projectile` target, whose cell
  is the one under the projectile (its center for a coordinate); the appendix lists the target
  kinds and fields each `AtomicKind` uses, and the exact mapping belongs to
  the interface definition (section 12); `Trigger.create` checks the
  arguments of each `Action.atomic`, and an invalid one disables the module,
  while atomic actions written in INI are checked at load and
  reported in the lobby (INI form: open question 4); an atomic action run by
  a trigger writes its error status (`Refused`, `Gone` or `NotFound`) to the
  in-game log and is skipped, and nothing else happens; `Detonated` added to
  the table of candidate delivered events; notification IDs are a stable hash
  of the case-folded name; the SDK build declares the names a simulation
  module sends in a Wasm custom section `vera.notify`, from a list in the
  module's project configuration whose format is SDK documentation, not engine
  API; client modules declare none, and a declared name that no client module
  receives is not an error; the collision check at load runs over the declared
  names of all simulation modules of the match, the mod's and the map's
  (section 3, step 6); `Notify.id` returns `NO_ID` with `NotFound` in a
  simulation module for a name it did not declare, and in a client module for
  a name that no simulation module of the match declared (the engine keeps
  the declared table, so a client never matches an undeclared name by hash);
  status values: `Gone` for a handle that is `NO_HANDLE` or whose object is
  gone, `NotFound` for a name that matches nothing, `Refused` for an atomic
  action's target that turns out not to fit when its trigger runs; a
  call on a handle that is `NO_HANDLE` or gone, including a method that
  returns nothing, does nothing and sets the last status to `Gone` without
  disabling the module; the new passenger-side events are named
  `TransportEntered` / `TransportExited` and the transport-side events
  `PassengerAdded` / `PassengerRemoved`, so they no longer clash with original
  event names such as `EnteredBy`; the client display method `setValue` is
  renamed `setNumber`; handles are never
  nullable and `NO_HANDLE` means none, and `Node` and `SuperWeapon` are
  handles, while `Path` and `DisguiseInfo` stay classes that may be `null`;
  client API names follow the layout keys `Label` and `Art` (`Text.label`,
  `setLabel`, `setArt`), a bar's fill is set with `setFill`, and `Value` keeps
  only its INI meaning, a data source such as `Credits` or `Power`;
  notifications carry the module's own bytes, which the engine cannot filter;
  `Ctx.event()` removed; a waypoint the map does not have is a map data
  error, not a module error: `Trigger.create` given an atomic action whose
  `Cell.waypoint(n)` does not exist returns `NotFound`, creates no trigger,
  logs it and leaves the module enabled, and in INI it is reported in the
  lobby at load; house marks are part of the mark API with the same tier as
  object marks (`addMark`, `removeMark`, `hasMark`, `marks` on `House`), and
  `House` stays a target of `AddMark` and `RemoveMark`; related text and the
  appendix updated to match. Changes after an independent review, in the same
  version: simulation module code runs only inside simulation phases, and
  local code (sidebar, cursor, interface, rendering) reads the result the
  simulation last computed; calls into modules never come from parallel code,
  and override calls and delivered records are ordered by the subject's
  creation serial, then raise order within one object, not by the engine's loop order,
  so overrides may keep module state (new term: creation serial); load
  checks add shape limits, NaN canonicalization by bytecode rewriting, a call
  depth limit counted in the metering pass, a memory cap allocated at
  instantiation, and host failures become match errors; a failure in
  `onRules` stops the match from starting; disabling a module disables the
  triggers it created and only skips its module actions elsewhere, uses the
  previous link's value for a failed override, and keeps its memory in
  snapshots and the hash; client modules have a budget per render frame
  and are disabled only locally; the mod's budget and map policy keys move
  from `mod.ini` to `[VeraWasm]` in the mod's `rulesmd.ini`, which the mod
  digest covers; the mark registry is simulation state and client modules
  only look names up; online rooms require equal versions, an older offline
  build playing a module map without its modules is a stated residual, and
  the release before module support adds a minimum-version map key; the
  budget is per frame, a base amount plus an allowance per delivered record,
  with a queue cap beyond which records move to the next frame; overrides
  are v1 candidates, two or three of which ship in version 1;
  `Override.Fire` returns a shot plan the engine carries out at once;
  type- and weapon-bound triggers run one instance per object, with the
  subject's owner as the trigger's house and the subject as tag holder;
  `Trigger.bindType` and its weapon, warhead and projectile forms; the module
  feature switches on from modules or any of its keys, and the attached
  effect owner modules need does not depend on the Phobos switch; `Cell.waypoint(n)`
  returns a cell carrying the waypoint number, which `Trigger.create`
  resolves; a trigger has one or more events, all of which must hold, and the
  original repeat types; the mod module's `onRules` runs after `RULESMD.INI`,
  `LANGRULE.INI` and the mode layer, the map module's after the map layer,
  and IDs never change once assigned; API versions are major.minor, previews
  and unstable interfaces are not published, adapters never pass values an
  old version did not define;
  `Action.wasm` names resolve through a `vera.actions` table to the index
  `EventRecord.action` carries; TypeScript examples use namespace functions
  and constructors, and handles stay below 2^53; marks carry a value and a
  visibility given when added, with `markValue` and `setMarkValue`; polled
  events fire on the edge, staggered by creation serial; deferred chain
  triggers spring at the start of the next frame's module phase; after
  `HouseTransfer` the engine re-checks the transfer; the state hashes cover
  the whole module memory; notifications may go to a player or all
  spectators, who cannot send messages; each mod and each map has its own
  store; query results are sorted by creation serial and a paged result set
  is fixed within a module call; the Wasm runtime is chosen per platform;
  Goal 4 reworded; event terminology, the `EffectExpire` payload, the
  `Timer` period, export signatures, phase order, probe tiers, the
  notification hash (32-bit FNV-1a, ASCII case folding), the module random
  stream per slot, offline-only hot reload, CI fixtures built from sources
  and the `Override.Fire` timing clarified. Changes after a second
  independent review, in the same version: a batched or parallel engine
  section counts as one calculation, the triggers of VERA events raised in it
  spring right after it ends, and overrides reached in it are batched (one
  call per module, then the results are applied); no re-entry: while a
  module's call is on the stack its own overrides are skipped, and calls into
  other modules are allowed; client queries expose only what the player's own
  interface could show, with a client column on every query table (Goal 4
  reworded); a Wasm feature allowlist replaces the list of rejected
  features, and a table is saved as function indices; no `start` section, and
  the SDK's init function runs once before the first other call and never
  after a save is loaded; section 15 states that its rules assume a
  core-module ABI; host calls are charged to the operation budget by the work
  they do; module names in INI and triggers resolve to the modules of the
  same source, with `Wasm:mod.` and `Wasm:map.` prefixes, override bindings
  are read per source, and messages and notifications carry their source;
  `Wasm.Tags` is read only from the mod's `rulesmd.ini`; override signatures
  receive the native and the previous link's value, and their query and
  result classes are defined; the native side effects of a shot taken over
  by `Override.Fire` are listed; `EventRecord` gains the trigger and the tag
  holder; order across objects by creation serial, within one object by
  raise order, which may change between engine versions; a per-frame cap on
  chained trigger firings, and re-checks for deferred triggers; a module
  whose backlog grows beyond a limit, or whose budget runs out during
  overrides, is disabled; metering units, budgets and limits are fixed per
  API major version; one-off budgets for the init function, `onRules` and
  `onScenarioStart`; the memory cap is allocated at instantiation; client
  budgets are metered in operations; no notifications during rejoin
  catch-up; the multiplayer checksum hashes module memory every 15 frames;
  `Api=` is read by the lobby and checked against the imports, custom
  sections carry a format version, and names are ASCII; `FireWeapon` takes
  `id` before the weapon slot; `FatalDamage` re-checks whether the damage is
  still fatal; Phobos AttachEffect keys are read only with the Phobos switch,
  and modules define effect types through the API; a handle is the object's
  creation serial; per-object flags for objects given a module tag; client
  store IDs; `ai.createTeam` has one owner; new gaps for module hashes,
  map embedding and menu marking; plan phases renamed stages; undefined
  types defined; `Status.Unsupported` in the error model; examples fixed
  (`onStart`, `setClickable`, `cell`); Chinese wording fixed. Changes after
  the maintainer's ABI decision, in the same version: the binary interface is
  a core-module ABI (integer handles, pointer-and-length pairs, the host
  writes into module buffers), and WIT stays the interface description
  language from which the project's own generator produces the Rust and
  TypeScript SDK bindings, the reference documentation and the compatibility
  checks, not the Component Model's canonical ABI (section 12), for browser
  and interpreter support, the zero-copy buffers of section 15, module state
  kept wholly in linear memory for saves, hashes and rejoining, AssemblyScript
  support for small modules a map can embed, and the frozen core
  specification; a later major version may add a Component Model adapter over
  the core ABI without affecting existing modules; the ABI question removed
  from the open questions, the later ones renumbered and every reference
  updated; the Component Model added to the alternatives as not adopted;
  section 15 relies on the core-module ABI. Consistency fixes, in the same
  version: client data reaches the simulation through standard player orders
  and messages, both in the command stream (section 1); `onEvents` listed
  among the exports, and module actions marked as not Wasm exports (section
  5); the TypeScript compiler named as open question 2 (section 5a); other
  languages bind through the interface definition's mapping onto the core
  ABI; the section 6a example binds its trigger to E1; IDs are 32-bit
  integers in TypeScript (section 15); open question 12 also covers queries,
  probes and the other `v1?` entries of the appendix; in the appendix,
  signatures for `Trigger.create`, `Tag.create`, `Techno.setTag` and
  `cell.setTag`, the classes `Every`, `Column`, `Row`, `Text` and `List` and
  the enum `Hold` defined, the target of `techno.setMission` optional, a
  trigger bound through `Trigger.bindType` counted as a module trigger, and
  every mark function listed for house marks.
- **0.0.5** (2026-11-16): decisions on the appendix's proposals. Adopted:
  override chaining with lifted vetoes, saves and `onScenarioStart`, mark values,
  visibility and house marks, query order, warhead detonation, projectile
  removal, damage versus health, HouseTransferred, more candidate events,
  bound-type-only frequent events, data before callbacks, events that cannot
  cancel, probe queries, `veraui.ini` layout keys, no last-seen copies for
  client queries, error model, named notifications, fixed-point format, no
  change of units or meaning. Not adopted: query as house, client-to-client
  messages.
- **0.0.4** (2026-11-16): API catalog appendix with its own list of engine gaps
  and proposed RFC changes.
- **0.0.3** (2026-11-16): ordering rules for mixed triggers, events raised in
  the module phase and read-only overrides; what a disabled module leaves
  behind; a random stream for modules; case-insensitive names; type bindings
  outside the tag slot; saves, replays and rejoining; message and notification
  rules; standard player orders and local storage for client modules;
  queries (6c); AI teams (6d); projectile creation, `Override.Fire` and volley
  timing; superweapon event; game end and scores; log; FinalAlert2 data and a
  headless test runner; assets referenced by name.
- **0.0.2** (2026-11-16): triggers gain conditions and polled events
  (section 6a); atomic actions, attached effects and marks (section 6b); API
  granularity rule; interface tree with INI layout (section 10); recorders and
  operators stay out of the engine; coordinates are parameters, while an
  object's position changes only by teleport, motion or a move mission;
  simulation API values are integers in the examples too.
- **0.0.1** (2026-11-16): first draft.
