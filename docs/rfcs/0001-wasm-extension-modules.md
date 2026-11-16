# RFC 0001: Wasm extension modules

**English** · [简体中文](0001-wasm-extension-modules.zh-CN.md)

The two language versions carry the same RFC version and change in the same
commit. Where they differ, the English text applies.

| | |
| --- | --- |
| RFC version | 0.0.5 |
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
terms Red Alert 2
modders already know: triggers, events, actions, tags, missions, local and
global variables, and frames, and adds conditions, attached effects and marks.

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
2. Every player runs identical modules with identical results: no desync on
   any supported platform, including the browser build.
3. Outside modules never stop the game. A module that fails is disabled and the
   player is told why.
4. A module cannot read files, use the network, read the clock or see what the
   player is not allowed to see.
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
  indices, the renderer's graphics API.
- Adding new object types during a match.
- Replacing the original trigger system. Modules extend it.

## Terminology

The API uses Red Alert 2 terms wherever one exists. Names in code and INI
follow this table.

| Term | Meaning |
| --- | --- |
| **Extension module** | A compiled `.wasm` file. Not called a *script*: in RA2, scripts are AI team scripts (`[ScriptTypes]`). |
| **Trigger** | As in maps: an event plus one or more actions, owned by a house, with difficulty flags. VERA adds optional conditions. |
| **Event** | A trigger's condition. Original event numbers keep their meaning. |
| **Action** | What a trigger does. Original actions keep their numbers. A new action runs a module function. |
| **Condition** | A filter the engine checks natively before a trigger's actions run. |
| **Tag** | What attaches triggers to an object or a cell (CellTag). An object holds one tag. |
| **Mark** | A text marker an object or a house carries, tested by conditions, with an optional integer value and a visibility level. Not a tag: an object may carry any number of marks. |
| **Attached effect** | A per-object state with a duration, such as a multiplier or an immunity (the Phobos AttachEffect term). |
| **Mission** | An object's order: Move, Attack, Guard, Area Guard, Hunt, Sleep and the rest. |
| **Local / Global** | Local and global variables, as in maps. |
| **Frame** | One logic frame of the simulation, the unit of `ROF`, `Rate` and timers. |
| **Render frame** | One drawn frame on one machine. Not synchronized. |
| **Override** | A named decision point where a module may change the engine's result. |
| **House, Techno, Building, Vehicle, Infantry, Aircraft, Projectile, Weapon, Warhead** | As in the INI files. |
| **Cell, lepton** | Map coordinates. One cell is 256 leptons. |
| **Operation budget** | How much work a module may do per call, measured in deterministic operation units. |

## What a modder sees

### Files

```
Mods/MyMod/
  mod.ini        ; Needs=, WasmApi=1, policy keys
  rulesmd.ini    ; binds tags to types
  rules.wasm     ; simulation module
  ui.wasm        ; interface module
```

A map carries its modules as text, the way it already carries `IsoMapPack5`:
compressed, base64-encoded and split into numbered lines.

```ini
[VeraWasm]
Api=1
RulesSize=48213          ; decoded size, checked on load
UISize=9120

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
Event=Damaged
Action=Wasm:giPanic      ; calls the function giPanic exported by rules.wasm
Repeat=yes
```

The key names and the exact section format are not final (see
[Unresolved questions](#unresolved-questions)). Map triggers edited in
FinalAlert2 can also call module functions through the new action.

### Module code

The engine sees only the compiled module. The source and the compiler stay in
the author's tools. The same module in the two official SDKs:

```ts
// TypeScript (AssemblyScript-compatible)
import { action, onFrame, Mission, Techno, Ctx } from "@vera20k/sdk";

export const giPanic = action((ctx: Ctx, techno: Techno) => {
  if (techno.healthPercent < 30) {
    techno.setMission(Mission.Hunt);
  }
});

export const checkWaves = onFrame({ every: 15 }, (ctx: Ctx) => {
  if (ctx.local(3).get()) {
    // ...
  }
});
```

```rust
// Rust SDK
use vera20k_sdk::prelude::*;

#[vera::action(export = "giPanic")]
fn gi_panic(ctx: &mut Ctx, techno: Techno) {
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

Each SDK builds `rules.wasm` or `ui.wasm` with one command. Package names are
placeholders. See section 5a for languages and SDKs.

## Design

### 1. Two worlds

| | Simulation (`rules.wasm`) | Client (`ui.wasm`) |
| --- | --- | --- |
| Runs on | every machine, in lockstep | the local machine only |
| Deterministic | yes | no |
| May change | game state, through engine owners | interface, display, sound |
| Reaches the game by | actions and missions | sending a message into the command stream |
| Engine side | `sim/` | `ui/`, `render/`, `audio/` |

The split follows the existing rule that `sim/` never depends on `render/`,
`ui/`, `audio/` or `net/`. Data flows from simulation to client through
visibility-filtered queries and local notifications. Data flows from client to
simulation only through `sendMessage`, which enters the command stream like a
player order, so replays and online play see it.

### 2. Sources: mods and maps

Mods and maps use the same API and the same SDK. They differ in what the
engine grants them.

| | Mod | Map |
| --- | --- | --- |
| Order | first | after the mod, as INI layers |
| `onRules` | after the mod's INI layer | after the map's INI layer, sees the mod's result |
| Operation budget | declared by the mod, capped by the engine | smaller default, the mod may raise it |
| Size | file size | embedded size limit |
| Interface | may replace or rearrange the match screen | overlays only, unless the mod allows more |

The mod controls what a map's modules may do:

```ini
[Mod]
WasmApi=1
MapWasm=yes          ; no: a map with modules cannot start under this mod
MapUI=overlay        ; overlay, full or no
```

With no mod selected, map modules are allowed and map interfaces are overlays.

Conflict rules:

- **Tag bindings on types** come only from the mod's rules layer. Map modules
  create triggers and tags in code (section 7), so a map's INI cannot erase a
  mod's binding by layering.
- **Triggers** from both sources fire. The mod's module runs first.
- **Overrides** chain: the mod's module computes a result, the map's module
  receives it and may change it, and the engine uses the final value. Each link
  receives both the native value and the previous link's result. A later link
  may lift an earlier link's veto. A value override returns an explicit
  "handled" flag rather than a special value.
- **Actions and missions** go through the same owners in execution order.
- **Peer calls** (later phase): a mod may publish functions that a map calls
  with `callPeer("mod", name, bytes)`. Only functions the mod exports for
  this purpose can be called.

### 3. Loading and validation

On load the engine:

1. decodes embedded modules and checks their declared size;
2. parses the module and accepts imports only from `vera:*` interfaces whose
   versions this release supports;
3. rejects threads, relaxed SIMD and 64-bit memory, and canonicalizes NaN
   results of floating-point operations;
4. adds operation metering and sets the memory limit;
5. checks that the exports the INI and the lifecycle need exist and have the
   right signatures.

A failure before the match is shown in the lobby with its reason, and the match
cannot start with that module. A failure during a match (trap, budget exceeded,
invalid argument) disables that module at the same frame on every machine, and
the game tells the players which module stopped and why. The match continues,
and what the module left behind is handled this way:

- its triggers are disabled;
- its overrides are skipped: the next module in the chain, or the native
  result, applies;
- its projectiles on a module path fly on straight toward their target;
- attached effects and marks it added stay, because the engine owns them.

Errors that are part of normal play return a status and the module carries on:
a handle whose object is gone, an unknown name, an action an engine rule
refuses (for example a cell that cannot be built on). Errors that show a broken
module disable it: an invalid argument, a trap, an exceeded budget.

`docs/modding.md` currently says a mod is only data and VERA20k never runs a
program from one. That text changes to say modules run only in a sandbox with
no access to files, the network or the clock. The Mods menu and the map list
mark mods and maps that contain modules.

### 4. Determinism

All machines run the same module bytes, so results depend only on how the
engine executes them.

- No threads, no relaxed SIMD, canonical NaNs.
- No clock, no host random source, no WASI. Modules draw random numbers from
  a stream of their own, derived from the scenario seed, so a module never
  shifts the native streams: a module that draws nothing leaves every native
  result unchanged. Actions that run native code draw from the native streams
  exactly as that code does. Each API call documents its draws.
- Names (type IDs, section names) compare without regard to case, as gamemd's
  INI lookups do.
- The operation budget is counted by instrumentation added at load, not by an
  engine's built-in fuel. Every engine, native or browser, then counts the same
  operations and stops at the same point.
- Positions, speeds and other game values cross the boundary as integers
  (leptons, fixed-point raw values). Fractions and multipliers use the
  simulation's fixed-point format, `I16F16` raw values (`SimFixed`). A module
  may use floating point internally.
- Module memory, mutable globals and tables are part of the simulation state:
  saved in snapshots and included in the state hash.

### 5. Execution model

Simulation module exports:

| Export | When |
| --- | --- |
| `onRules` | once, after the INI layers load; may change and add types |
| `onScenarioStart` | once, at the start of the scenario |
| `onFrame` | once per frame per subscribed module, or every N frames |
| actions | when a trigger runs them |
| overrides | synchronously, when the engine reaches that decision |
| `onMessage(player, bytes)` | when a client message reaches the simulation |

Client module exports: `onStart`, `onRender(dt, alpha)` once per render
frame, and interface callbacks such as clicks and hotkeys.

Module calls run in one fixed phase of the simulation frame, in module load
order. Triggers raised during the frame whose actions call module functions are
queued and delivered in that phase, before `onFrame`. Overrides are the
exception: they run when the engine needs the answer. Object references are
generational handles, so a handle to an object that died before delivery reads
as gone and never as a reused slot.

Ordering rules:

- A trigger whose actions include a module function runs all its actions, in
  their order, in the module phase. A trigger without module actions runs as in
  the original.
- Events raised while the module phase runs are delivered in the next frame's
  module phase, so triggers cannot loop within one frame.
- Overrides only read state and return a result. They cannot run actions,
  because the engine is in the middle of a calculation when it calls them.
- A delivered event reports what already happened and cannot cancel a native
  result such as a death. Changing that needs an override or an attached
  effect (for example, prevent death).
- Frequent events (damage, fire, projectile created or detonated, crush, move
  failed) and every override are raised only for bound types. There is no
  subscription to all objects.
- After a save is loaded, module memory is restored with the snapshot.
  `onScenarioStart` does not run again and there is no separate load call.
  Handles stored in module memory refer to the same objects after loading.

Each module has an operation budget per frame. Subscriptions are opt-in: a
module that exports no `onFrame` and binds no triggers costs nothing per
frame.

### 5a. Languages and SDKs

The contract between engine and module is the Wasm interface alone. Any
language whose compiler produces a module that passes the load checks works,
and the engine treats every module the same way.

- **Official SDKs: TypeScript and Rust.** Both are generated from the one
  interface definition (section 12), carry the same functions under each
  language's naming style, and are versioned with the API. Each comes with
  templates, a build command, a watch mode and examples.
- **TypeScript SDK.** Compiles TypeScript to Wasm ahead of time, so the module
  is small enough to embed in maps. The compiler it uses is an open question.
  Its API stays inside the common subset of TypeScript and AssemblyScript: no
  `any`, no union types other than `T | null`, no callbacks that capture outer
  variables, no `try`/`catch`, and parameters that are classes rather than
  object shapes. The same source then type-checks with `tsc` and compiles with
  AssemblyScript, and the examples in this RFC are valid in both.
- **Rust SDK.** For authors who want full control and speed, and the SDK the
  engine's own compatibility fixtures are written in.
- **Other languages** (C, C++, Zig, Go, ...) use the published interface
  definition; their bindings are community work.
- **Imports.** A module may import only `vera:*` interfaces. The engine does
  not provide WASI and the project publishes no WASI shim. A language whose
  runtime needs WASI (for clocks, random numbers or files) works only if its
  own toolchain removes those imports.
- **Size.** Languages that bundle a large runtime or garbage collector into
  every module produce bigger files, which may exceed the map embedding limit.
  That is a trade-off of the author's toolchain, not of the engine.

### 6. API layers

| Layer | Content | First API version |
| --- | --- | --- |
| L1 Actions and missions | every original trigger action, missions, production, house and object commands | yes |
| L2 Triggers | original events plus new ones (weapon fired, projectile detonated, ...) delivered to actions | yes |
| L3 Overrides | named decisions: target choice, damage, production permission, taking over a weapon's shot, ... | two or three, chosen from modder requests |
| L4 Per-frame behaviors | batched per-frame control of objects of a bound type | no, later |

Queries are read-only and iterate in a deterministic order. Every L1 action
calls the existing Rust port of that action (for example in
`sim/trigger_runtime`); modules never get a second implementation.

Overrides are named after game concepts, such as `Override.Damage` or
`Override.TargetChoice`, never after native functions, so they survive engine
refactors. `Override.Fire` lets a module take over a weapon's shot, so the
native projectile is not created (section 9).

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

A trigger has one event, any number of conditions and one or more actions.

- **Events** come in two kinds. *Delivered events* happen at a moment (damage
  taken, weapon fired, killed) and are queued for the module phase. *Polled
  events* describe a state (health below a value, money above a value); the
  engine checks them natively at the interval the trigger sets, and the trigger
  fires while the state holds.
- **Conditions** filter a trigger before any module code runs. The engine
  checks them natively and calls the module only when all of them hold. A
  condition can be negated. Logic beyond that (OR across conditions,
  comparisons between two objects, counting) belongs in module code.
- **Actions** are original trigger actions, the atomic actions of section 6b,
  or module functions.
- Every event names the object it is attached to and, where there is one, the
  other object involved (attacker, target, transport, producer). Conditions can
  test either.

```ts
Trigger.create({
  event: Event.TakeDamage,
  conditions: [Cond.healthBelowPercent(30), Cond.not(Cond.hasMark("Panicked"))],
  actions: [Action.wasm("giPanic")],
  difficulty: Difficulty.All,
});
```

Candidate delivered events for version 1:

| Event | Payload |
| --- | --- |
| TakeDamage | source (nullable), damage |
| HitByWarhead | warhead, final damage |
| AttackedBy | attacker, weapon index (nullable), projectile (nullable) |
| FatalDamage | attacker (nullable), damage |
| Killed | killer (nullable) |
| Fire | target (nullable), weapon index, projectile (nullable) |
| SpawnFire | spawned unit, target (nullable), weapon index, projectile (nullable) |
| HitTarget | target, damage, projectile |
| Aimed | target |
| ProjectileIncoming | projectile, launcher (nullable) |
| Birth | producer (nullable), effect (nullable), build time (nullable) |
| Enter / Exit | transport or building, passengers, occupied size, garrison or transport |
| EnteredBy / ExitedBy | passenger, passengers, occupied size, garrison or transport |
| Crushed | crusher |
| HouseTransfer | new house, whether by mind control; raised before the transfer |
| HouseTransferred | previous house, whether by mind control; raised after the transfer |
| EffectStart / EffectExpire / EffectStacks | effect, stack count |
| Timer | frame of the previous firing |
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
animation, remove, kill, deal damage through the warhead and armor rules (with a source for kill credit
and experience), set health directly (no credit), create an object, set a
mission, teleport to a cell (as the Chronosphere does; when the cell is
blocked, a stated rule picks a nearby cell or the action fails), clear the
target, clear a
disguise, search an area (a query that returns objects), play a sound, add
experience, add money, change the tech tree, transfer the owner, clear mind
control, change ammo, add or remove passengers, add or remove an attached
effect, add or remove a mark, create a projectile (type, start, target object
or coordinate, source for kill credit and experience, initial module data),
detonate a warhead at a coordinate with a damage and a source, detonate a
projectile now, remove a projectile without detonating it,
end the game for a house (win or lose) and set scores, write a line to the
in-game log.

Attached effects are per-object states with a duration, owned by
`sim/attach_effect.rs` (section 8). Candidates: tint, attached animation,
prevent death, transform into another type, replace a weapon, motion (thrown,
pushed), teleport, multipliers for damage taken, firepower, speed and rate of
fire, range, cloak, immunity to a warhead or attack type, cannot fire, no
target search, damage modifier, cannot be selected, ignores player orders, not
a legal target, house power off.

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
effects add and remove them; conditions test them. A mark may carry one
integer value and a visibility level: its own house, allies, players who can
see the object, or everyone. Client queries return only the marks their
player may see. Marks do not nest.

### 6c. Queries

Queries are read-only. Results come in object creation order, a rule that does
not depend on how the engine stores objects. In client modules
they are filtered by what the local player can see.

- **Objects:** in a cell, in a radius or a rectangle, by type, house or
  category; an object's type, owner, health, position, mission, target,
  veterancy, ammo, passengers, attached effects and marks.
- **Houses:** money, power output and drain, owned buildings and met
  prerequisites, tech level, allies and enemies, side, whether defeated.
- **Map:** size; a cell's land type, height, overlay and ore, passability for a
  movement zone, whether a house can see it; waypoints and cell tags.
- **Rules:** any key of any type, as a parsed value.
- **Scenario:** frame number, difficulty, game mode, number of players.
- **Probes:** whether a weapon could fire at a target now, whether a target is
  in range, whether a type can be placed at a cell for a house, and the closest
  cell where it can be. They reuse the engine's own rules.

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
const t = Trigger.create({
  house: ctx.house("Americans"),
  event: Event.Destroyed,
  actions: [Action.wasm("onConvoyLost")],
  difficulty: Difficulty.All,
});
const tag = Tag.create([t], TagRepeat.Repeating);
techno.setTag(tag);     // replaces the object's tag, as in the original
cell.setTag(tag);       // CellTag
t.disable();
```

- Created triggers and tags live in the existing trigger runtime's lists, next
  to the map's. There is no separate store.
- They get deterministic IDs that cannot collide with the map's, and they are
  saved and hashed with the trigger runtime.
- An object holds one tag (`ObjectClass` `AttachedTag`, `+0x34`). A tag may
  link several triggers. Attaching several tags to one object would be a VERA
  extension and needs its own RFC.
- Type bindings (`Wasm.Tags`) do not use that slot. They are kept as separate
  subscriptions of the object, so an object placed on the map with its own tag
  keeps it and still receives its type's triggers.
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
  use it, as the project contract requires.
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
`onLaunch` gives each one its path. With `Override.Fire` the module takes over
the shot and the native projectile is not created. Without it, the native
projectile leaves during the unit's update and the module's projectiles in the
module phase of the same frame, one frame of movement behind.

### 10. Client module

**Interface tree.** The engine holds the interface tree, lays it out and draws
it. A module changes it with a small set of operations: create and remove
nodes, set properties (text, image, position, visibility), find nodes by name,
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
export const onStart = start((ui: Ui) => {
  ui.find("ObjList").setItems(["TXT_OBJ_RADAR", "TXT_OBJ_CONVOY"]);
  ui.create(Panel.anchor(Anchor.BottomLeft).child(Text.csf("TXT_WAVE")));
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
- Each mod gets a small key-value store on the player's machine for campaign
  progress, achievements and settings. Only client modules can use it. Data
  that should affect a match reaches the simulation through `sendMessage`, so
  it is part of the command stream.
- Images, animations and models are referenced by name, never by file format,
  so later asset formats (a separate RFC) need no API change.
- Sounds and messages; `log()` writes to the in-game console.
- The simulation module sends named notifications to one player's client with
  `notify(player, name, bytes)`; the client module receives them in
  `onNotify(name, bytes)`.
- Every query is filtered by what the local player can see. A spectator's
  client modules see what the spectator sees, the whole map when the room
  allows it. An enemy object the player cannot see now is absent from results;
  the engine keeps no last-seen copies.
- Player-installed interface modules come in a later phase. A mod or a room
  may forbid them.

### 11. Online play

- Embedded modules travel with the map. Mod modules are part of the existing
  mod file check.
- The lobby compares module hashes and required API versions. A player whose
  game does not support a required version is refused with a message that
  names the version needed.
- The engine fills in the sender of every message, so a module cannot forge
  it, and limits message size and messages per frame.
- Notifications from the simulation module to a client reach only the player
  they are for, under the same visibility rules as queries.
- Online play exchanges only commands, never state. A rejoining player
  re-simulates the match from the start, modules included; module messages are
  commands, so nothing extra is sent. A module's cost per frame therefore also
  slows catch-up.
- Replays record the module hashes and API versions. A replay whose modules do
  not match is refused with a message.
- Saves record the module hashes. A save whose modules do not match the
  installed mod or map is refused with a message naming them.
- The change updates [Networking](../Networking.md) and states what a released
  game, a mixed room and a rollback do with modules.

### 12. Versioning and the ABI

- One interface definition (WIT syntax) is the source of the API, the SDK
  bindings, the reference documentation and the compatibility checks.
- Interfaces are versioned packages: `vera:sim@1`, `vera:client@1`.
  Unstable interfaces live under `vera:unstable/*`, require an explicit opt-in
  and may change without notice.
- Every published version keeps working. Older versions are thin adapters over
  the current implementation, never a second implementation.
- For every published version, CI runs a set of committed `.wasm` fixtures.
  Breaking one fails the build.
- Functions may be deprecated, with a warning to the module author, but a
  published version is never removed.
- A published function never changes its units or meaning. A change is a new
  function or a new version.
- Whether the binary interface is a core-module ABI (integer handles, pointer
  and length pairs) or the Component Model is an open question.

### 13. Tooling

| Need | Provided |
| --- | --- |
| Start | templates for `rules.wasm` and `ui.wasm` in TypeScript and Rust; an example mod in both |
| Size | SDK release settings for small modules; documented size guidance per language; map embedding limit |
| Maps | `asset` commands to embed, extract and validate modules |
| Iteration | the SDK's watch mode rebuilds on save; in development mode the game watches the `.wasm` file and reloads it (module state resets) |
| Errors | the failing export and the error in game; the SDKs pass messages and source locations to the engine |
| Cost | an overlay with each module's operations and trigger deliveries per frame |
| Docs | reference generated from the interface definition; task-based guides |
| Map editors | FinalAlert2 data entries for the new events and actions |
| Testing | run a map with its modules headless and check results, reusing the headless scenario runner |

### 14. Project contract

- The feature is in every build and turns on only when a mod or map provides a
  module. It is never behind a build feature.
- Every module call reaches state through its existing owner. The API adds no
  competing state.
- The feature is documented under `docs/` for modders.
- Module failures are reported in game, never only in the log.

## Process

1. **Draft.** This document is discussed in public. Changes bump the RFC
   version and are listed in the [changelog](#changelog).
2. **Open questions answered.** Experiments outside `src/` answer the
   unresolved questions below. Results are summarized here.
3. **Accepted.** The maintainer marks a version as accepted. Development starts
   only then.
4. **Preview.** The API ships as `0.x`. Breaking changes are allowed and listed
   in a changelog. At least one real mission or mode is rebuilt with it, and
   outside modders try it.
5. **Stable.** `vera:sim@1` and `vera:client@1` freeze, limited to what the
   preview used. The compatibility fixtures go into CI.
6. **Growth.** Each new interface or override is a small follow-up RFC and a
   new minor version.

Planned order after acceptance:

| Phase | Content |
| --- | --- |
| P1 | `rules.wasm`: `onRules`, `onScenarioStart`, `onFrame`, L1, L2 with conditions, attached effects, marks, queries, AI teams, type tag bindings, the trigger action, map embedding, `asset` commands, TypeScript and Rust SDKs and templates |
| P2 | `ui.wasm`: interface tree with INI layout, display properties, visibility filter, messages |
| P3 | real mission rebuilt with modules, outside modder trial, freeze of version 1 |
| Later | overrides one by one, L4 behaviors, peer calls, player interface modules |

## Alternatives considered

- **Lua run by the engine** (as Recoil does). Easy for modders, but float and
  table-order determinism must be patched into the interpreter, and the API
  grows as one unversioned namespace.
- **A language runtime in the engine** (Lua or JavaScript shipped with
  VERA20k for modules to import). Modules would be smaller, but the engine
  would own a language runtime forever and have to pin one build per API
  version. Rejected: languages stay in the authors' tools.
- **Native plugins.** Not portable to the browser build, not sandboxed.
- **Simulation queries as a given house.** Not adopted for now. Simulation
  queries see everything; client queries are filtered.
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
2. **ABI.** Core-module ABI or Component Model?
3. **TypeScript compiler.** Which compiler the TypeScript SDK uses: a
   compiler for a TypeScript subset to Wasm (such as AssemblyScript), or a
   JavaScript engine compiled into each module. The engine is the same either
   way; the choice decides module size, speed and how much ordinary TypeScript
   works.
4. **Metering.** Which instrumentation, and what does it cost at 20,000 units?
5. **INI format.** Named sections as shown, the original comma-separated
   trigger encoding so FinalAlert2 can edit bindings, or both? What prefix do
   the keys use?
6. **Event and action numbers.** Which range is free of Ares and Phobos?
7. **FinalAlert2.** Does it keep unknown sections such as `[VeraWasmRulesPack]`
   when it saves a map?
8. **Limits.** Embedded size, memory and default operation budgets for mods and
   maps.
9. **Frame phase.** Where in the simulation frame the module phase sits, and
   where the module projectile arm sits relative to collision and detonation.
10. **Save size.** Module memory in snapshots: compression and the memory cap.
11. **First overrides.** Which two or three overrides ship in version 1.
12. **Allowlist.** Which keys can change per type during a match in version 1.
13. **Version 1 catalog.** Which events, conditions, actions and attached
    effects from sections 6a and 6b ship in version 1.
14. **Effect stacking.** The stacking rule and exclusive group of each attached
    effect.
15. **Interface layout.** Where a mod and a map keep layout sections, and the
    widget set of version 1.
16. **Map text.** Maps cannot ship string tables. How a map module's text is
    translated.
17. **Campaigns.** Whether a mission's modules start fresh or continue the
    previous mission's state.

## Changelog

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
