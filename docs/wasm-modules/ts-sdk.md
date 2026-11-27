# TypeScript SDK reference

[English](ts-sdk.md) | [简体中文](ts-sdk.zh-CN.md)

Every namespace, class and function of the TypeScript SDK for Wasm modules,
`@vera20k/sdk` (`sdk/typescript` in the VERA20K source), and its `vera20k` command. The
first part covers simulation modules (`rules.wasm`), the second client modules
(`ui.wasm`). The [guide](guide.md) says where modules go and what the limits are;
the tutorials walk through a [simulation module](ts-tutorial.md) and a
[client module](ts-ui-tutorial.md).

The SDK has the same capabilities as the [Rust SDK](rust-sdk.md), under
TypeScript names; where this page is short on the semantics of an entry, the Rust
reference and the [RFC appendix](../rfcs/0001-wasm-extension-modules-api.md) say more. The
property, event and action tables and the event and decision views are generated from
the Rust SDK, so the numbers are the same.

**Tiers.** The API is a preview (package version 0.1.0). The RFC sorts every entry into a
tier: `v1` (a candidate for version 1), `preview` (works now, may change without notice)
and `later` (not in version 1; the engine answers `hasKind` 0 and use returns
`Unsupported`). Where an entry below is not `v1`, it says so.

## Contents

- [Setup](#setup): [the `vera20k` command](#the-vera20k-command), [`vera.json`](#verajson),
  [what `vera20k build` does](#what-vera20k-build-does), [conventions](#conventions)
- Simulation modules: [callbacks](#callbacks), [basic calls](#basic-calls),
  [references](#references), [properties](#properties), [modifiers](#modifiers),
  [variables](#variables), [searches](#searches), [actions](#actions),
  [missions and house orders](#missions-and-house-orders), [rules](#rules),
  [triggers](#triggers), [events](#events), [decisions](#decisions),
  [INI registration](#ini-registration), [notifications and messages](#notifications-and-messages)
- Client modules: [overview](#client-modules), [client callbacks](#client-callbacks),
  [client calls](#client-calls), [reading the game](#reading-the-game),
  [nodes](#nodes), [the player, camera and cursor](#the-player-camera-and-cursor),
  [local actions](#local-actions), [client events](#client-events),
  [storage](#storage)

## Setup

Node.js 20 or later. Until the package is published, use the SDK from a copy of the
VERA20K source: `npm install` once in `sdk/typescript`, then make projects with
`node sdk/typescript/bin/vera20k.js init <dir>`. A project made this way depends on that
copy (`"@vera20k/sdk": "file:…/sdk/typescript"` in its `package.json`).

| Import | For |
| --- | --- |
| `@vera20k/sdk` | Simulation modules (`rules.wasm`) |
| `@vera20k/sdk/client` | Client modules (`ui.wasm`), with the parts of the simulation SDK that are safe there |

The compiler is AssemblyScript, pinned to exactly 0.28.20. TypeScript itself (5.9.3) only
type-checks.

### The `vera20k` command

| Command | Does |
| --- | --- |
| `vera20k init <dir>` | A new simulation project in `<dir>`, which must be empty or missing: `package.json` (scripts `build`, `watch`, `check`), `tsconfig.json`, `vera.json`, `.gitignore`, `src/index.ts`. The package name is the folder's name |
| `vera20k init <dir> --ui` | The same as a client project: `"kind": "ui"` and a client `src/index.ts` |
| `vera20k build [dir] [--out file] [--debug]` | Builds the project in `dir` (default `.`). `--out` overrides `vera.json`'s `out` (relative to the project). `--debug` builds with `-O1` and debug information instead of the size-optimized release build |
| `vera20k watch [dir] [--out file] [--debug]` | Builds, then builds again 100 ms after each change in the entry's folder (`src/`, except `src/generated/`) or in `vera.json` |
| `vera20k generate [dir]` | Checks `vera.json` and writes `src/generated/` from it, without compiling (what `tsc` needs) |

In a project, `npm run build`, `npm run watch` and `npm run check`
(`vera20k generate && tsc --noEmit -p .`) run them; pass options after `--`, as in
`npm run build -- --out ../mod/rules.wasm`. Because `check` generates `src/generated/`
first, it works before the first build.

### `vera.json`

| Key | Default | Meaning |
| --- | --- | --- |
| `kind` | `"rules"` | `"rules"` (simulation module) or `"ui"` (client module) |
| `entry` | `"src/index.ts"` | The file to compile, relative to the project |
| `out` | `"build/rules.wasm"` or `"build/ui.wasm"` | The output file, relative to the project |
| `vars` | `[]` | Declared variables (below) |
| `notify` | `[]` | Notification names the module sends, such as `["WaveStarted"]` |

A `vars` entry:

| Key | Values |
| --- | --- |
| `name` | Printable ASCII without spaces, up to 65,535 bytes; unique ignoring case |
| `scope` | `"global"`, `"house"` or `"object"`; required in a simulation project, where a missing or invalid one is a build error |
| `visibility` | `"private"` (default), `"allies"`, `"insight"` (also `"in-sight"`), `"public"`; ignoring case |
| `initial` | The starting value, an integer (default 0); `default` is accepted as another name |

```json
{
  "kind": "rules",
  "entry": "src/index.ts",
  "vars": [
    { "name": "Wave", "scope": "global", "visibility": "public", "initial": 1 },
    { "name": "Panicked", "scope": "object", "visibility": "private", "initial": 0 }
  ],
  "notify": ["WaveStarted"]
}
```

In a client project (`"kind": "ui"`), `vars` and `notify` declare nothing: they only
generate `ClientVar` and `NotifyName` constants for names a simulation module declares.
Every `vars` entry there becomes a `ClientVar`, so `scope`, `visibility` and `initial` are
not needed and are ignored.

### What `vera20k build` does

1. Reads `vera.json` and checks the variable and notification names (printable ASCII,
   unique ignoring case) and, in a simulation project, each variable's `scope`.
2. Generates, next to the entry, `generated/vars.ts` (one constant per variable:
   `GlobalVar`, `HouseVar` or `ObjectVar` in a simulation project, `ClientVar` in a
   client project) and `generated/notify.ts` (one `Notify`, or `NotifyName`, per name).
   Each is written only when its list is not empty, and only when its text changes; a
   list that becomes empty deletes its file. `vera20k generate` does this step alone. A
   name becomes an identifier by turning characters other than letters, digits, `_` and
   `$` into `_` (`map.Wave` becomes `map_Wave`).
3. Compiles the entry together with the SDK's exports file for the kind, with
   AssemblyScript's incremental runtime, `--noAssert` (`assert` calls are removed) and,
   unless `--debug`, `-O3 --shrinkLevel 2 --converge`.
   - Top-level code becomes the `vera_init` export (`client_init` for a client module),
     never a `start` section.
   - There is no `env.abort` import: a runtime error (an index out of range, a null
     dereference, a `throw`) logs `abort: <message> at <file>:<line>:<column>` through
     the module's own log, then traps.
   - Memory addresses below 1,024 are reserved for the delivery area.
   - Only the lifecycle exports the code registers something for are kept: `on_frame`
     only with an `onFrame` call, `on_events` only with `enqueue`, `enqueueWith`,
     `ActionDef.module`, `namedAction`, `Ini.key` or `Ini.registry`, `on_decide` only
     with `decideSync` or a `namedAction` whose handler takes a decision view, and so
     on, so the engine makes no call the module does not need. The build finds these calls in your
     source text (comments ignored).
4. Collects the `namedAction("Name", fn)` names. Each name must be a string literal.
   Names must be unique ignoring case, and two names whose dispatch keys collide are
   refused ("rename one").
5. Refuses a module that imports anything but its world's functions: `vera:sim@1.0` for
   a simulation module, `vera:client@1.0` for a client module. The message names each
   foreign import, such as `env.seed` (`Math.random`), `env.console.log` (`console`),
   `env.Date.now` (`Date`), or `vera:sim@1.0.log` (a simulation call in a client module).
6. Appends the custom sections: `vera.delivery` (the delivery area at bytes 8 to 1,024,
   always), `vera.actions` (the named actions and their keys, when there are any) and,
   for a simulation module, `vera.vars` and `vera.notify` from `vera.json`.
7. Prints the output path, its size and the named actions.

It does not run the engine's other load checks; `asset wasm-check` does.

### Conventions

- **Errors.** A call that writes returns a `Status`; a call that reads or creates
  returns the value, which is the empty value on failure (`0`, `NO_HANDLE` or `NO_ID`),
  and `lastStatus()` then gives the reason.

  | `Status` | Meaning |
  | --- | --- |
  | `Ok` | It worked |
  | `Gone` | The reference is 0 or the object no longer exists |
  | `NotFound` | No such name or ID, or a missing waypoint |
  | `Refused` | The game refused: an occupied cell, a value it cannot write now, a full quota, a builder over its limit |
  | `Unsupported` | Not available in this match or this build, or an entry of tier `later` |

  A wrong call is not a status: an invalid argument (a scope that does not match the
  variable, a negative random range, a write in a decision handler, ...) stops the
  module.
- **References are `i64`.** `Techno`, `House`, `Cell` and the other reference types are
  aliases of `i64`, read and written through namespace functions that take the reference
  first (`Techno.health(t)`). `NO_HANDLE` (0) is no object.
- **Views** (`KilledEvent`, `TakeDamageDecision`, `ClickEvent`, ...) read the engine's
  record in place and are valid only during their callback. Each has `record` (the whole
  `EventRecord`); decision views also have `request` (the `DecideRecord`).
- **Every registration runs.** `onRules`, `onScenarioStart`, `onMessage`, `onStart`,
  `onRender`, `onFrame` and `onClientEvent` add a function each time they are called;
  all of them run, in registration order.
- **Language.** The code is AssemblyScript-compatible TypeScript; the
  [tutorial](ts-tutorial.md#6-how-this-typescript-differs) lists what that
  rules out (closures over locals, `for…of`, `try`, unions, untyped object literals).
- Names marked `@internal` in the source (`check`, `utf8`, `OUT`, `keyOf`, `run*`, ...)
  are exported for the SDK's own use; don't rely on them.

---

# Simulation modules

A simulation module (`rules.wasm`) imports `vera:sim@1.0` and runs on every machine in
step with the match. Everything below is exported by `@vera20k/sdk`.

## Callbacks

Register callbacks in top-level code. The build keeps the engine export each one needs.

| Function | Export | When the engine calls it |
| --- | --- | --- |
| Top-level code | `vera_init` | Once, before anything else: at the rules stage for a module that also uses `onRules`, otherwise when the match is installed. The only place for `Ini.*` registrations |
| `onRules(fn: () => void)` | `on_rules` | Once, while the rules load (after `rulesmd.ini`, before they are processed). `Rules.set` writes any key here |
| `onScenarioStart(fn: () => void)` | `on_scenario_start` | Once, when the match starts, after the module's INI records |
| `onFrame(every: Every, fn: () => void)` | `on_frame` | On every frame divisible by `every.frames`, at the end of the module phase. Call it once per function; each has its own interval |
| `new Every(frames: i32)` | | The interval for `onFrame` (15 frames are one second at normal speed; below 1 counts as 1) |
| `onMessage(fn: (sender: House, data: Uint8Array) => void)` | `on_message` | In the module phase before the events, once per message a client module of the same source sent. `data` is a copy of the message's bytes |
| `namedAction<T>(name: string, fn: (e: T) => void): i32` | `on_events` (`on_decide` when `fn` takes a decision view), section `vera.actions` | Names `fn` for INI (`Action=Wasm:name`; `Decide=Wasm:name` for a handler that takes a decision view) and returns its dispatch key. `name` must be a string literal |
| `actionKey(name: string): i32` | | The dispatch key of a named action (FNV-1a over the lowercased name, bit 30 set), the same the build writes |

Handlers given to `Trigger.enqueue`, `Trigger.decideSync`, `ActionDef.module`, `Ini.key`
and `Ini.registry` get dispatch keys of their own; you never write keys by hand. `T` is
the record view the handler takes: an event view, a decision view, `EventRecord`,
`DecideRecord` or an INI record.

The order at the start of a match: the mod's `vera_init` and `on_rules`, the map's rules,
the map's `vera_init` and `on_rules`; INI triggers are read; then, per module, its INI
records and its `on_scenario_start`. The mod's module runs before the map's every frame.

## Basic calls

| Function | Does |
| --- | --- |
| `frame(): i32` | The current frame |
| `random(lo: i32, hi: i32): i32` | A number in `lo..hi` inclusive from the module's own random stream, the same on every machine. `lo > hi` stops the module. In a decision handler it draws from a substream and leaves the main stream alone |
| `log(line: string)` | A line in the chat list (8 a frame at most, 160 characters shown) and in `logs/ra2.log` |
| `fail(why: string)` | Logs `why` and stops the module for good on every machine. For a value you cannot use, such as a malformed registered INI key; name the section and key |
| `lookup(kind: NameKind, name: string): i32` | The ID of `name`, or `NO_ID`. Look names up once and keep the IDs |
| `nameOf(kind: NameKind, id: i32): string` | The name of `id`, or `""` |
| `text(id: i32): string` | Engine-held text by string ID: a module action's parameter text, an INI value, a string property. ID 0 is `""` |
| `hasKind(table: i32, id: i32): bool` | Whether this engine implements entry `id` of `table` (0 event kind, 1 action kind, 2 property, 3 name kind, 4 search scope) |
| `lastStatus(): Status` | The status of the latest SDK call that returned a value |

`NameKind`: `TechnoType`, `Weapon`, `Warhead`, `Projectile`, `Anim`, `VoxelAnim`,
`ParticleSystem`, `SuperWeapon`, `Armor`, `Overlay`, `Sound`, `Country`, `TeamType`,
`ScriptType`, `AiTrigger`, `Variable`, `Notify`, `ModuleAction`, `String`, `Property`,
`EventKind`, `ActionKind`, `Hotkey`, `ModuleEvent`. This engine answers `Unsupported`
for `Armor`, `Overlay`, `Sound`, `ScriptType`, `AiTrigger` and `EventKind`.

## References

| Type (`i64`) | What | Make one with |
| --- | --- | --- |
| `Ref` | Any reference | What `get`, `set` and `Find` take |
| `Techno` | A unit or building (its creation serial) | A search, an event, `Action.createObject` |
| `Projectile` | A flying bullet (preview) | A `Launch` or `ProjectileFrame` decision |
| `House` | A house (player or computer side) | `Techno.owner(t)`, a search, `House.fromIndex(i)` |
| `Cell` | A map cell | `Cell.at(x, y)` |
| `Waypoint` | A map waypoint | `Waypoint.number(n)` |
| `TypeRef` | A rules type | `Rules.technoType("E1")` and the other `Rules.*` lookups, `TypeRef.make(kind, id)` |
| `Modifier` | A modifier | `Modifier.*`, `Action.modify` |
| `TriggerRef` | A trigger | `Trigger...done()` |

| Function | Does |
| --- | --- |
| `NO_HANDLE` | 0: no object |
| `NO_ID` | −1: no ID |
| `TypeRef.make(kind: NameKind, id: i32)` | The type reference of `id` in `kind`; no host call |
| `TypeRef.id(t)`, `TypeRef.kind(t)` | Its ID and name kind; no host call |
| `TypeRef.any(kind)` | The wildcard of a name kind, for `Find.types` only |
| `Cell.at(x, y)`, `Cell.x(c)`, `Cell.y(c)` | A cell from its coordinates and back; no host call |
| `Waypoint.number(n)`, `Waypoint.cell(n)` | Waypoint `n` (the same function under two names); functions taking cells resolve it |
| `House.fromIndex(index)` | The house at that array index. Provisional: the engine encodes houses as bit 47 over the index (RFC open question 5) |
| `Global.REF` | The global rules reference (appendix 7.8) |
| `Rules.anim(name): i32` | An animation's ID, which `Action.spawnAnim`, `attachAnim` and `playAnimAt` take |

## Properties

| Function | Does |
| --- | --- |
| `get(reference: Ref, prop: Prop): i64` | The effective value of `prop`, with modifiers applied |
| `set(reference: Ref, prop: Prop, value: i64): Status` | Writes `prop` through the game's own code |
| `getText(reference, prop): string` | A string property (`get`, then `text`) |
| `getMany(refs: StaticArray<i64>, props: StaticArray<i32>, out: StaticArray<i64>): i32` | Reads `props` of every reference into `out` in one call, row-major (`out[i * props.length + j]`); returns the number of values written. `Refused` when `out` is too small |
| `propNamed(name: string): Prop` | A property by its engine name, or `NO_ID` |

`Prop` is a namespace of constants (`Prop.Health`, `Prop.Money`,
`Prop.FirepowerMultiplier`, ...) and the type `Prop` is `i32`. A `.` in an engine name is
written `_`: `Prop.Passable_Foot` is `Passable.Foot`. The full list with value types,
writability and tiers is [appendix section 7](../rfcs/0001-wasm-extension-modules-api.md#7-properties).

Writable with `set`: `Health` (no kill credit; 0 or less is `Refused`), `Owner`, `Rank`,
`Mission`, `Ammo`, `Online`, a trigger's `Enabled`, `House` and `Repeat`, a house's
`ControlDomains`, a superweapon's `ChargePercent`. The modifiable properties refuse `set`.

Getters (one `get` each) and setters:

| Namespace | Functions |
| --- | --- |
| `Techno` | `health`, `owner`, `typeRef`, `mission` (`Mission`), `ammo`, `layer` (`Layer`), `target` (object or cell reference), `experience`, `storage`, `transport`, `passengerCount`, `transportSpaceLeft`, `disguiseType`, `disguiseOwner`, `moving` (`bool`), `facing`, `turretFacing`, `armor`, `cell` |
| `Techno` (writes) | `setHealth`, `setOwner(t, house)`, `setMission(t, Mission)` (as the owner's player would), `setAmmo`, `setOnline(t, bool)` |
| `Techno.read(list, props): StaticArray<i64>` | `getMany` into a new array; allocates, so in hot code call `getMany` with your own buffer |
| `House` | `money`, `name` (`string`), `isHuman`, `isDefeated` (`bool`); `order` (see [house orders](#missions-and-house-orders)) |
| `Cell` | `landType`, `height`, `overlay` (`TypeRef`), `ore`, `waypoint` (−1 for none), `passable(c, SpeedType)` |
| `Global` | `frame`, `difficulty` (0 easy, 1 normal, 2 hard), `missionTimer`, `missionTimerRunning`, `ambientLight`, `ionStormActive` |

Enums for property values: `Category` (`Infantry`, `Vehicle`, `Aircraft`, `Building`,
`Projectile`), `Layer` (`Underground`, `Surface`, `Ground`, `Air`, `Top`), `Relation`
(`SameHouse`, `Ally`, `Enemy`, `Neutral`), `SpeedType` (`Foot` ... `FloatBeach`;
`SpeedType.passable(speed)` gives its cell property), `Mission` (`Sleep`, `Attack`,
`Move`, ... `Hunt`, ... `SpyplaneOverfly`, numbered as the game numbers them).

**Viewpoint properties.** `Relation`, a cell's `Revealed` and `Placeable`, `Visible`,
`Engage` and a TechnoType's `Buildable` depend on whose view they are read for. A plain
`get` answers `Unsupported`. Read them in a trigger's filter clause (for the trigger's
house) or in a search with `.seenBy(house)`.

## Modifiers

A modifier changes a property of an object for a while and goes away by itself. Modifiers
from different modules and Phobos AttachEffects stack; without modifiers the game
computes exactly what it did before.

| Function | Does |
| --- | --- |
| `Modifier.multiply(target, prop, factor: i64, frames: i32): Modifier` | Multiplies by a `fixed` factor (`fixed(1.5)`) |
| `Modifier.add(target, prop, value: i64, frames: i32): Modifier` | Adds `value` (for example `WeaponRangeExtra` in leptons) |
| `Modifier.overrideWith(target, prop, value: i64, frames: i32): Modifier` | Replaces the value while it lasts |
| `Modifier.create(target, prop, op: ModifierOp, value, frames, source = 0, group = 0): Modifier` | The general form: `ModifierOp` (`Add`, `Multiply`, `Override`), a source object (or 0) and an exclusion group (0 none; a second modifier of this module in the same group on the same object replaces the first) |
| `Modifier.remove(m): Status` | Removes it early; `Gone` once it expired |
| `fixed(value: f64): i64`, `FIXED_ONE` | Fixed-point values (16 fraction bits): `fixed(1.0) == FIXED_ONE == 65536` |

Each returns the modifier, or `NO_HANDLE` on failure. `frames` below 0 lasts until
removed. Read a modifier with `get(m, Prop.Remaining)` (also `Property`, `Op`, `Value`,
`Source`, `Group`, `Origin`); an expired one reads as `Gone`. `Action.modify` and
`Action.removeModifiers` are the action forms.

| Property | Ops | Tier |
| --- | --- | --- |
| `FirepowerMultiplier`, `ArmorMultiplier` | Add, Multiply, Override | v1 |
| `SpeedMultiplier` (ground units, aircraft, jumpjets, rockets) | Add, Multiply, Override | v1 |
| `ROFMultiplier`, `WeaponRangeMultiplier`, `WeaponRangeExtra` | Add, Multiply, Override | v1 |
| `Cloakable`, `ForceDecloak`, `DisableWeapons` | Override | v1 |
| `ROTMultiplier` (the body's turn rate of vehicles and buildings) | Multiply | preview |
| `ImmunityFlags` (bits 0..6: mind control, radiation, poison, psychic damage, chrono, EMP, berserk) | Override | preview |
| `CritMultiplier`, `CritExtraChance`, `Unkillable` | as listed in the appendix | later; they act only in Phobos builds |

## Variables

| Class or function | Does |
| --- | --- |
| `Var.local(n): GlobalVar` | Red Alert 2 local variable `n` (0..99) |
| `Var.global(n): GlobalVar` | Red Alert 2 global variable `n` (0..49) |
| `new GlobalVar(name)` | A declared global variable: `get(): i64`, `set(value): Status` |
| `new HouseVar(name)` | A declared house variable: `get(house)`, `set(house, value)` |
| `new ObjectVar(name)` | A declared object variable: `get(object)`, `set(object, value)`; it goes with the object |
| `Var` | The base class: `id` (looked up by name on first use, `NO_ID` for an unknown name), `name`, `read(scope)`, `write(scope, value)` with scope 0 for a global, a house or an object |
| `Scope`, `Visibility` | The enums of the declarations (`Global`, `House`, `Object`; `Private`, `Allies`, `InSight`, `Public`) |

Declare a module's variables in `vera.json` ([above](#verajson)); the build generates
`src/generated/vars.ts`, so you rarely write `new HouseVar(...)` yourself:

```ts
import { log } from "@vera20k/sdk";
import { Wave } from "./generated/vars";

function nextWave(): void {
  Wave.set(Wave.get() + 1);
  log("wave " + Wave.get().toString());
}
```

Declared variables are saved, hashed and readable by client modules according to their
visibility: `Private` (the house's own player), `Allies`, `InSight` (object scope only),
`Public`. Another source's variable is found as `mod.Name` or `map.Name`. Filter clauses
(`whereVar`), `Trigger.onVariable` and `ActionDef.setVar` take any `Var`.

## Searches

`Find` builds a query in module memory; nothing reaches the engine until `page` or
`first`.

| Call | Does |
| --- | --- |
| `Find.objects()` | Units and buildings, by creation order |
| `Find.houses()` | Houses, by index |
| `Find.cells()` | Cells, by cell index; narrow with `near` or `within` |
| `Find.types(t: TypeRef)` | Types matching `t`; `TypeRef.any(kind)` lists every type of the kind |
| `.near(center: Cell, radius: i32)` | Within `radius` leptons of the cell (256 leptons are one cell) |
| `.within(topLeft: Cell, width, height)` | In a rectangle of cells |
| `.ofType(t)`, `.ownedBy(house)` | Type and owner filters |
| `.country(country: TypeRef)` | Entries of a country (`Rules.country(name)`); a `Country` clause |
| `.where(prop, op: Op, value: i64)` | Keeps candidates whose `prop` compares (`Op.Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`) |
| `.seenBy(house)` | Only what `house` sees, and viewpoint properties read for it |
| `.page(out: StaticArray<i64>, after: i64): FindPage` | Fills `out` from sort key `after` (0 first). The page has `count` (entries written) and `next` (where the next page starts, 0 on the last). The returned page object is shared: read it before the next search |
| `.first(): i64` | The first match or `NO_HANDLE`, following pages |

- A `Find` holds at most 16 clauses (`MAX_CLAUSES`); with more, the search answers
  `Refused`.
- One call examines at most 16,384 candidates, and fewer when they would cost more than
  half the call's budget. So a short or even empty page is the last only when `next` is
  0; keep calling while `next != 0`.
- A `Find` can be made once (in top-level code) and run many times.

## Actions

`Action` has one function per action the engine runs. Each is one host call and returns
a `Status` (`createObject` and `modify` return what they make, `NO_HANDLE` on failure).
Invalid parameters stop the module; a game refusal is `Refused`.

**Atomic actions** (appendix 5.3):

| Function | Does | Tier |
| --- | --- | --- |
| `damage(target, amount, warhead: TypeRef, source = NO_HANDLE)` | Damage under armor rules; `source` gets kill credit and experience | v1 |
| `kill(target, source)` | Kills through the damage path, crediting `source` (`NO_HANDLE` for none) | v1 |
| `remove(target)` | Removes without death, survivors or credit | v1 |
| `heal(target, amount)` | Restores health up to Strength, no credit | v1 |
| `createObject(at: Ref, kind: TypeRef, house): Techno` | Creates a unit or building at a cell or waypoint; an occupied cell is `Refused` | v1 |
| `setMission(target, mission, at: Ref = 0)`, `setMissionOn(target, mission, on: Techno)` | A mission aimed at a cell or waypoint (0 for none) or at an object | v1 |
| `teleport(target, to: Ref, blocked: Blocked)` | Chronoshifts a vehicle or infantryman; `Blocked.Nearby` or `Blocked.Fail` when the cell is taken | v1 |
| `clearTarget`, `clearDisguise`, `clearMindControl` | As named | v1 |
| `addExperience(target, points)` | The experience a kill worth `points` credits gives | v1 |
| `addMoney(house, amount)` | Credits; negative takes them | v1 |
| `allowType(house, kind, allow: bool)` | `false` forbids a TechnoType to the house, `true` lifts this module's forbid | v1 |
| `setOwner(target, house)` | Gives the object to the house | v1 |
| `addAmmo(target, amount)` | Within the type's `Ammo=` | v1 |
| `addPassenger(transport, passenger)` | Boards at once if the transport would load it | v1 |
| `setProperty(target, prop, value)` | The action form of `set` | v1 |
| `modify(target, prop, op, value, frames, group): Modifier`, `removeModifiers(target, group, prop = -1)` | Action forms of modifiers; `prop` −1 removes the group's modifiers of every property | v1 |
| `detonateWeapon(at, weapon: TypeRef, source)` | Detonates a weapon at a cell or object without a projectile | v1 |
| `detonateWarhead(at, warhead: TypeRef, damage)` | A warhead's area damage | v1 |
| `fireWeapon(shooter, slot, at = NO_HANDLE, cell = 0)` | Fires weapon slot 0 or 1 now at its target, object `at` or cell `cell`; `Refused` when it cannot fire | v1 |
| `launchSuperWeapon(house, kind: TypeRef, at: Ref)` | Launches the house's superweapon | v1 |
| `spawnAnim(at, anim: i32)` | Plays an animation (`Rules.anim`) | v1 |
| `attachAnim(target, anim: i32, frames: i64 = 0)`, `detachAnim(target, anim: i32 = -1)` | Plays an animation once attached to the object, or keeps it looping for `frames` frames (negative: until `detachAnim`) | preview |
| `endFor(house, outcome: Outcome)` | Ends the match for a house (`Outcome.Win`, `Outcome.Lose`) | v1 |
| `setScore(house, n)`, `addScore(house, n)` | Score | v1 |
| `log(line)` | The action form of `log` | v1 |
| `convert(target, kind: TypeRef, frames = 0)` | Changes the object's type, for `frames` frames or (0) for good | v1 |
| `raiseEvent(event, target, other, value, value2, house = NO_HANDLE)` | Raises a module event ([INI registration](#ini-registration)) on `target` (`Global.REF` for a global event); not in a decision handler | v1 |
| `push(target, toward: Cell, distance, frames)` | Shoves a ground unit `distance` leptons toward a cell (negative: away) over `frames` | preview |
| `setFacing(target, facing)` | Turns the body toward 0..255 | preview |

**Stock actions** (the map trigger actions, appendix 5.2), by what they take:

| Takes | Functions |
| --- | --- |
| Nothing | `revealAll`, `timerStart`, `timerStop`, `growShroud`, `ionStormStop`, `lockInput`, `unlockInput`, `announceWin`, `announceLose`, `endScenario`, `toggleTrainCargo`, `clearSmudges`, `homeCamera` |
| A string | `declareWinner(country)`, `declareLoser(country)`, `showText(label)`, `destroyTrigger(id)`, `playTheme(theme)`, `playSpeech(eva)`, `forceTrigger(id)`, `enableTrigger(id)`, `disableTrigger(id)`, `destroyTag(id)`, `timerText(label)` |
| A number | `playMovie`, `timerExtend(seconds)`, `timerShorten`, `timerSet`, `setGlobal(n)`, `clearGlobal`, `ionStormStart(duration)`, `setLocal(n)`, `clearLocal`, `setAmbientLight`, `wakeGroup`, `playRadarMovie`, `pauseGame`, `setSidebarTab`, `chronoScreenEffect` |
| A TeamType (`Rules.teamType`) | `createTeam(team)`, `destroyTeams(team)`, `reinforce(team)`, `reinforceAt(team, waypoint)`, `reinforceByChrono(team, waypoint)` |
| A waypoint | `dropZoneFlare`, `revealAround`, `revealZone`, `lightningStrike`, `ionCannonStrike`, `nukeStrike`, `chemStrike`, `reshroudAt`, `ironCurtainAt`, `jumpCamera`; `playAnimAt(at, anim)`, `detonateWeaponAt(at, weaponId: i32)`, `centerCamera(at, speed)`, `meteorShower(at, intensity)`, `createCrate(at, kind)`, `lightningStormStrike(at, house)`, `createBuildingAt(at, kind: TypeRef, house)` |
| A type | `setTechLevel(kind, level)`, `flashCameo(kind, frames)` |
| A house | `autoBaseBuilding(house, on)`, `reshroud`, `wakeSleepers`, `wakeHarmless`, `radarBlackout(house, frames)`, `oneTimeSuperWeapon(house, kind)`, `repeatingSuperWeapon(house, kind)`, `superWeaponSetCharge(house, kind, percent)`, `superWeaponReset(house, kind)`, `teleportAll(house, to)`, `setAlternateBase(house, at)`, `clearAlternateBase(house)`, `flashBuildings(house, kind, frames)` |
| An object | `changeHouse(target, house)`, `destroyObject`, `sellBuilding`, `turnOffBuilding`, `turnOnBuilding` |

**Any action.** `Action.runRaw(kind, target, p: usize): i64` runs action number `kind`
with the `ActionParams` at `p` (`params()` clears the SDK's shared one and returns its
address) and returns what it created; `Action.runText(kind, target, text)` runs one that
takes a text. `ActionKind` has the atomic numbers (`ActionKind.FireWeapon` = 1000 ...);
stock actions keep their numbers 0..145.

## Missions and house orders

- **Missions** work on every house, human or computer, as that house's own click would:
  `Techno.setMission(t, Mission.Hunt)` (untargeted) or
  `Action.setMission(t, Mission.Move, Cell.at(60, 40))`.
- **House orders** give a computer house player commands for many units at once and for
  production: `House.order(house, order): Status`. The order runs next frame, checked as
  that house's player input. A human player's house is `Refused`.

| `Order` constructor | Command |
| --- | --- |
| `Order.moveTo(units, cell)`, `Order.moveToObject(units, target)` | Move |
| `Order.stop(units)` | Stop |
| `Order.attack(units, target)`, `Order.attackMove(units, cell)` | Attack |
| `Order.guard(units, target)` | Guard an object, or `NO_HANDLE` to guard in place |
| `Order.deployMcv(units)` | Deploy |
| `Order.setRally(factories, cell)` | Rally point |
| `Order.queueProduction(t)`, `Order.cancelProduction(t)` | Production |
| `Order.placeReadyBuilding(t, cell)` | Place a finished building |
| `Order.sell(buildings)` | Sell |
| `Order.launchSuperWeapon(t, cell)` | Superweapon |
| `Order.takeProduct(kind)` | Take a finished product of a factory kind (2 infantry, 3 vehicle, 4 naval, 5 aircraft) |
| `new Order(command, target, value, units)` | Any `Command` (`Move` ... `TakeProduct`; `EjectBunker` is `later`) |

`units` is a `StaticArray<Techno>`. One order carries at most 64 units (`ORDER_UNITS`);
a module sends at most 256 orders and 4,096 unit entries a frame. Missions don't count
against this. The `Commanded` and `OrderRefused` events report executed and refused
orders.

## Rules

| Function | Does |
| --- | --- |
| `Rules.technoType(name): TypeRef` | A TechnoType, `NO_HANDLE` for an unknown name |
| `Rules.weapon`, `Rules.warhead`, `Rules.projectile`, `Rules.superWeaponType`, `Rules.country`, `Rules.teamType` | The same for the other kinds |
| `Rules.anim(name): i32` | An animation's ID (`NO_ID` for none) |
| `Rules.get(section, key): string` | The merged rules text of `[section] key` (mod and map); `""` with `NotFound` for a missing key |
| `Rules.set(section, key, value): Status` | Writes INI text. Any key in `onRules`, processed like the rest of the rules, the same on every machine. During a match it answers `Refused` |

## Triggers

A trigger is "when this event happens to a bound object, check these clauses, then run
these actions". `Trigger` builds one in module memory; `done()` creates it.

| Builder call | Does |
| --- | --- |
| `Trigger.on(kind: i32)` | A repeating trigger on event `kind` (an `Event`), every difficulty |
| `Trigger.every(interval)` | A `Timer` trigger, every `interval` frames from its creation |
| `Trigger.onProperty(prop, op, value, interval)` | `PropertyPoll`: fires when `prop op value` becomes true, checked every `interval` frames |
| `Trigger.onVariable(v: Var, op, value, interval)` | `VariablePoll` |
| `Trigger.onHousePower(op, value, interval)` | `HousePowerPoll` (output minus drain of the trigger's house) |
| `Trigger.onModifierCount(group, op, count, interval)` | `ModifierCountPoll` |
| `Trigger.onDistance(cell, op, leptons, interval)` | `DistancePoll` |
| `Trigger.onModuleEvent(event)` | A module event ([INI registration](#ini-registration)) |
| `.and(kind)` | Another event; all must occur |
| `.value(v)`, `.id(id)` | The latest event's arguments (a stock event's house index or amount, a type ID) |
| `.where(prop, op, value)`, `.whereOther(...)`, `.whereHouse(...)` | Filter clauses on the latest event: on the subject, the other object, the trigger's house |
| `.whereVar(target: VarTarget, v: Var, op, value)` | A clause on a variable of `VarTarget.Global`, `Subject`, `Other` or `House` |
| `.bind(reference)` | An object, a TechnoType (every object of the type, including later ones), a weapon, warhead or projectile type (for `Fire`), or a cell (stock events 25 and 26) |
| `.house(house)` | The trigger's house, instead of the subject's owner |
| `.difficulty(mask)` | Only on these difficulties: bit 0 easy, 1 normal, 2 hard |
| `.once()` | Fires once per instance |
| `.disabled()` | Starts disabled |
| `.immediate(action: ActionDef)` | Runs an action at once when it fires |
| `.enqueue(fn)` | Delivers the event to `fn` in the module phase |
| `.enqueueWith(fn, target: Target, text)` | The same, with a target and a parameter text (`e.record.target`, `e.record.argsText`) |
| `.done(): TriggerRef` | Creates and binds the trigger; `NO_HANDLE` when none was created (`lastStatus()` says why). A failed binding still returns the trigger, with `lastStatus()` the binding's status |
| `.decideSync(fn): TriggerRef` | Creates it as a decision trigger whose requests reach `fn` synchronously ([decisions](#decisions)); it takes no actions |

- A builder holds 4 events (`MAX_EVENTS`), 4 clauses per event (`MAX_EVENT_CLAUSES`),
  8 actions (`MAX_ACTIONS`) and 8 bindings (`MAX_BINDS`). One more of any makes `done()`
  answer `NO_HANDLE` with `Refused`; bind more with `TriggerRef.bind` afterwards.
- A module has at most 256 triggers, and at most 16 object polls among them.
- Records are delivered in the module phase, at most 1,024 a frame. A backlog over
  16,384 records stops the module.
- Chains: a firing caused by a trigger's own actions nests 8 deep and 256 times a frame;
  the rest waits for the next frame (at most 4,096 waiting).

**`TriggerRef`** (also as static functions of `Trigger`): `bind(t, reference)`,
`unbind(t, reference)`, `remove(t)` (the trigger and every instance),
`setEnabled(t, bool)`, `bindAllTypes(t, kind: NameKind): i32` (binds every type of the
kind, returns the number bound).

**`ActionDef`** (immediate actions, for `.immediate`):

| Function | Does |
| --- | --- |
| `ActionDef.addMoney(amount, house = NO_HANDLE)` | Credits for `house`, or the trigger's house |
| `ActionDef.setVar(target: Target, v: Var, value)` | Writes a variable of the target (a Red Alert 2 variable ignores the target) |
| `ActionDef.heal(target, amount)`, `ActionDef.damage(target, warhead, amount)` | Heal, damage |
| `ActionDef.setProperty(target, prop, value)` | As `set` |
| `ActionDef.log(line)` | A log line |
| `ActionDef.raiseEvent(target, event, value, value2)` | Raises a module event |
| `ActionDef.module(fn, target = Target.Subject, text = "")` | Queues a record for `fn`, as `enqueue` does |
| `ActionDef.raw(kind, target)` and `.id`, `.arg`, `.arg2`, `.value`, `.value2`, `.cell`, `.house`, `.other` | Any action with its raw `ActionParams` |

`Target` says what an action acts on: `Subject`, `Other`, `TagHolder`, `Projectile`,
`House`, `Cell`.

## Events

`Event` holds the event kinds (`Event.Killed`, `Event.TakeDamage`, ...), numbered as the
engine raises them: stock events 1..88, the module events from 1000. A handler takes
`EventRecord` or a view:

**`EventRecord`** (120 bytes): `key`, `kind`, `trigger`, `holder` (the bound object, or
the object or cell whose tag holds the trigger), `subject`, `other`, `house`,
`projectile`, `modifier`, `cell`, `value`, `value2`, `id`, `frame`, `x`, `y`, `z`,
`args` (a string ID), `argsText` (its text), `target` (a module action's `Target`).

| Event | View: fields | Tier |
| --- | --- | --- |
| `TakeDamage` | `TakeDamageEvent`: `victim`, `attacker`, `damage` (after armor), `warhead`, `x`, `y`, `z` | v1 |
| `Killed` | `KilledEvent`: `victim`, `killer`, `x`, `y`, `z`; `KilledByWarheadEvent`: `victim`, `killer`, `warhead` | v1 |
| `FatalDamage` | `FatalDamageEvent`: `victim`, `attacker`, `damage`, `warhead`, `x`, `y`, `z` | v1 |
| `AttackedBy` | `AttackedByEvent`: `target`, `attacker`, `weaponSlot`, `projectile` | v1 |
| `Fire` | `FireEvent`: `shooter`, `target`, `targetCell`, `weaponSlot`, `weapon`, `projectile` | v1 |
| `TargetChanged` | `TargetChangedEvent`: `object`, `target`, `targetCell` | v1 |
| `Detonated` | `DetonatedEvent`: `source`, `projectile`, `warhead`, `cell`, `x`, `y`, `z` | v1 |
| `Crushed` | `CrushedEvent`: `victim`, `crusher`, `x`, `y`, `z` | v1 |
| `Birth` | `BirthEvent`: `object`, `producer` | v1 |
| `TransportEntered`, `TransportExited` | `TransportEnteredEvent`, `TransportExitedEvent`: `passenger`, `transport`, `space`, `garrison` | v1 |
| `PassengerAdded`, `PassengerRemoved` | `PassengerAddedEvent`, `PassengerRemovedEvent`: `transport`, `passenger`, `space`, `garrison` | v1 |
| `HouseTransferred` | `HouseTransferredEvent`: `object`, `oldHouse`, `mindControl` | v1 |
| `ModifierStart`, `ModifierExpire` | `ModifierEvent`: `target`, `modifier`, `property`, `source` | v1 |
| `ModifierStacks` | `ModifierStacksEvent`: `target`, `modifier`, `count` | v1 |
| `Timer` | `TimerEvent`: `lastFrame` | v1 |
| `SuperWeaponLaunched` | `SuperWeaponLaunchedEvent`: `superWeapon`, `house`, `targetCell` | v1 |
| `Deployed` | `DeployedEvent`: `object`, `became`, `deployed` | v1 |
| `Promoted` | `PromotedEvent`: `object`, `rank` (1 veteran, 2 elite) | v1 |
| `ExperienceGained` | `ExperienceGainedEvent`: `object`, `victim`, `gain` | v1 |
| `Cloaked`, `Decloaked` | `CloakEvent`: `object` | v1 |
| `Commanded` | `EventRecord`: `value` the `Command`, `other` and `cell` the target, `value2` a serial per order | v1 |
| `OrderRefused` | `EventRecord`: `subject` (0 for a house-wide order), `house`, `value` the `Command`, `value2` the reason (appendix 8.3 `RefuseReason`) | v1 |
| `ModuleEvent` | `ModuleEventRecord` ([INI registration](#ini-registration)) | v1 |
| `PropertyPoll`, `HousePowerPoll`, `ModifierCountPoll`, `DistancePoll`, `VariablePoll` | `PollEvent`: `subject`, `reading` | v1 |
| Stock events 1..61 (`EnteredBy`, `DestroyedByAnyHouse`, `CreditsExceed`, `ElapsedTime`, `ComesNearWaypoint`, ...) | `StockEvent`: `subject`, `other`, `house`, `boundCell` | v1 |
| Ares events 62..88 | `StockEvent` | only when Ares is on |
| `MissionChanged` | `MissionChangedEvent`: `object`, `mission` | later: not raised |
| `HitByWarhead`, `SpawnFire`, `HitTarget`, `ProjectileIncoming`, `HouseTransfer`, `ProductionCancelled` | — | later: not raised |

`IniEntry` and `IniKey` records are delivered without a trigger
([INI registration](#ini-registration)).

## Decisions

A decision trigger (`.decideSync(fn)`, or `Decide=Wasm:Name` in INI) asks the module
synchronously, at the moment the game decides, and the module may change the answer.

```ts
import { Event, NO_HANDLE, Rules, TakeDamageDecision, Trigger, onScenarioStart } from "@vera20k/sdk";

function halve(d: TakeDamageDecision): void {
  d.setResult(d.previous / 2);
}

onScenarioStart((): void => {
  const gi = Rules.technoType("E1");
  if (gi != NO_HANDLE) Trigger.on(Event.TakeDamage).bind(gi).decideSync(halve);
});
```

- A handler only reads (`get`, `Find`, variable reads, `random`, `log`, ...). Any write
  stops the module: `set`, a modifier, a variable write, any `Action.*` (`raiseEvent`
  and `Action.log` included), creating, binding or removing a trigger, `Rules.set`, a
  notification or a house order. To act on a decision, remember it in module memory and
  act in the next `onFrame`. A handler should not allocate either.
- Every decision view has `native` (the game's own answer), `previous` (the answer of
  the module before, the native one for the first) and `setResult(value)`. Without
  `setResult` the previous answer stands. `request` gives the whole `DecideRecord`:
  `event`, `native`, `previous`, `result`, `setResult`, `handled`, `area` and `areaCap`
  (the result area), `previousLen`.
- Budget: 20,000 operations per request, out of a pool of half the frame budget.
- Several modules can answer one decision: the mod's module first, then the map's.

| Event | View: fields | Result | Tier |
| --- | --- | --- | --- |
| `TakeDamage` | `TakeDamageDecision`: `victim`, `attacker`, `damage`, `warhead`, `x`, `y`, `z` | The final damage (0 blocks the hit) | v1 |
| `ProductionAllowed` (type binding) | `ProductionAllowedDecision`: `house`, `technoType` | 1 allow, 0 disallow until next frame; 1 does not lift the game's own refusal | preview |
| `ProductionQueued` | `ProductionQueuedDecision`: `house`, `technoType` | 1 allow, 0 refuse | v1 |
| `Defeat` | `DefeatDecision`: `house` | 1 defeat, 0 veto this check | v1 |
| `CommandAllowed` (object or type binding) | `CommandAllowedDecision`: `unit`, `command`, `target`, `targetCell` | 1 allow, 0 refuse (also module orders) | preview |
| `DamageIncoming` | `DamageIncomingDecision`: `victim`, `attacker`, `warhead`, `incoming`, `preview`, `x`, `y`, `z` | The incoming damage; 0 or the same sign as `previous` | preview |
| `FireAllowed` | `FireAllowedDecision`: `shooter`, `target`, `targetCell`, `weaponSlot`, `weapon` | 0 vetoes the shot | preview |
| `Retaliate` | `RetaliateDecision`: `victim`, `source`, `warhead` | 1 retaliate, 0 not | preview |
| `DestinationSet` | `DestinationSetDecision`: `object`, `destination`, `destinationCell` | A destination reference, 0 for none | preview |
| `CrushAllowed` | `CrushAllowedDecision`: `victim`, `crusher` | 0 spares the victim | preview |
| `HarvestCell` | `HarvestCellDecision`: `harvester`, `scanned` | A cell reference, 0 for none | preview |
| `DeployAllowed` | `DeployAllowedDecision`: `object` | 0 refuses the deploy | preview |
| `IdleMission` | `IdleMissionDecision`: `unit` | A `Mission` number, 255 for none | preview |
| `MissionStep` | `MissionStepDecision`: `unit`, `mission`, `target`; `step` (the outcome so far, a `MissionStepResult`), `setStep`, `setTarget`, `setDestination`, `setMission`, `setNoMission`, `setDelay` | The Guard, Area Guard, Hunt or Attack body's outcome, partly replaced | preview |
| `MoveFrame` | `MoveFrameDecision`: `unit`, `moving`, `destination`, `destinationCell`, `x`, `y`, `z`; `previousStep`, `newStep()`, `setStep(step)`, `stepBy(dx, dy)`; flags `MOVE`, `FACING`, `STOP` | Moves a ground unit by up to 2,048 leptons a frame instead of its locomotor | preview |
| `PathFound` | `PathFoundDecision`: `unit`, `goal`, `x`, `y`, `z`; `previousCount`, `previousCell(i)`, `cell(i)`, `setCellCount(n)`, `setCells(xy)`, `noRoute()`; `MAX_CELLS` 24 | Replaces the route, each cell next to the one before | preview |
| `ApproachPoint` | `ApproachPointDecision`: `unit`, `chosen`, `target`, `targetCell`, `x`, `y`, `z`; `setCell(cell)` | The cell a unit chasing a target drives to | preview |
| `Fire` (decision form) | `FireDecision`: `shooter`, `target`, `targetCell`, `slot`, `weapon`; `previousCount`, `previousEntry(i)`, `planCapacity`, `planEntry(i)`, `setPlanLength(n)`, `setPlan(plan)` | The shot plan: up to 16 `ProjectileSpec`s launched instead of the native bullet | preview |
| `Launch` (weapon, warhead or projectile type binding) | `LaunchDecision`: `projectile`, `launcher`, `target`, `targetCell`; `setPath(kind, speed, param0, param1, points)`, `setPathTo(kind, speed, param0, param1, x, y, z)` | The bullet's path (`PathKind.Bezier`, `Parabola`, `Spiral`, `Waypoints`) | preview |
| `ProjectileFrame` (projectile type binding) | `ProjectileFrameDecision`: `projectile`, `frames`; `previousStep`, `newStep()`, `setStep(step)` | The bullet's step each frame | preview |
| `FatalDamage` (decision form) | `FatalDamageDecision`: `victim`, `killer`, `damage`, `warhead`, `x`, `y`, `z`; `with(bit, on)`; static `withInfDeath(plan, n)`, `withDeathWeapon(plan, weapon)`; bits `EXPLODE`, `DEATH_WEAPON`, `DEBRIS`, `SURVIVORS`, `CREW`, `ANNOUNCE`, `TIBERIUM_REMAINS` | What a death leaves behind | preview |

**Result structs.** The views with a result area read and write these in place, without
allocating. Each has `static at(ptr)` (a view over module memory), `BYTES`, and getters
and setters for its fields; most have `clear()` and `copyFrom(other)`:

| Struct | Fields |
| --- | --- |
| `ProjectileSpec` (48 bytes) | `projectileType`, `weapon` (−1 the shot's own), `target` (0 the shot's), `data0`, `data1`, `flags` (`ORIGIN_ABSOLUTE`), `originX`, `originY`, `originZ` |
| `ProjectileStep` (56 bytes) | `flags` (`DETONATE`, `SET_VELOCITY`, `SET_POSITION`, `SET_DATA`), `velocityX..Z` (leptons per frame, 16 fraction bits), `positionX..Z`, `data0`, `data1` |
| `MissionStepResult` (24 bytes) | `target`, `destination`, `mission` (−1 none), `delay` |
| `MoveStep` (16 bytes) | `flags`, `facing`, `dx`, `dy` |
| `PathCell` (8 bytes) | `x`, `y`, `cell` |

For a struct of your own (to keep or to copy from), reserve memory once:
`ProjectileStep.at(memory.data(ProjectileStep.BYTES, 8))`. `PATH_HEADER_BYTES` and
`PATH_POINT_BYTES` are the `Launch` path layout's sizes.

## INI registration

A module can give modders INI names of its own: a key on a type section, a registry (a
list section) and module events. The modder writes INI; the module implements it.
Register in top-level code only; anywhere else stops the module.

| Function | INI | The module receives |
| --- | --- | --- |
| `Ini.key(name, section: IniSection, fn): Status` | `name=value` in sections of that kind (`IniSection.TechnoType`, `Weapon`, `Warhead`, `Projectile`, `SuperWeapon`, `Country`, `General`) | One `IniKeyRecord` per section with the key: `typeRef`, `isGeneral`, `value`, `sectionName`, `record` |
| `Ini.registry(name, fn): Status` | A list section `[name]` naming entry sections | One `IniEntryRecord` per entry, in list order: `entryName`, `position`, `record` |
| `Ini.event(name, scope: EventScope): ModuleEvent` | `Event=Wasm:name` in a trigger section | The event's ID (`NO_ID` when refused), for `Trigger.onModuleEvent` and `Action.raiseEvent`. `EventScope.Object` fires triggers bound to the object (or its type), `EventScope.Global` unbound ones |

- Names are 1 to 63 ASCII letters, digits, `.` and `_`, compared ignoring case. A name
  already registered (by any source) or starting with `Vera.` is `Refused`. 4,096
  registrations per module.
- The records arrive before `onScenarioStart`: all `IniEntry` records, then all `IniKey`
  records. They count against `onScenarioStart`'s budget. Loading a save delivers
  nothing again.
- Values are the final merged rules text, not checked by the engine. Read entry sections
  with `Rules.get` and parse with the functions below. Stop with `fail(...)` naming the
  section and key when a value is wrong.
- `ModuleEvent.named(name)` finds an event any source registered (another module's too),
  or `NO_ID`. A raised event arrives as a `ModuleEventRecord`: `event`, `subject`,
  `other`, `value`, `value2`, `house`, `record`.

Parsing (each returns `otherwise` for a malformed value, with `lastStatus()` `NotFound`):

| Function | Does |
| --- | --- |
| `Ini.split(text): string[]` | The comma-separated parts, each without surrounding spaces |
| `Ini.stripComment(text): string` | `text` without a `;` comment or surrounding spaces |
| `Ini.int(text, otherwise = 0): i64` | A decimal integer with an optional sign |
| `Ini.yesNo(text, otherwise = false): bool` | First letter `y`, `t` or `1` true; `n`, `f` or `0` false |
| `Ini.ints(text, out: StaticArray<i64>, otherwise = 0): i32` | The first `out.length` integers into `out`; returns a mask with bit `i` set when part `i` was good |

See the [guide](guide.md#ini-names-a-module-registers) for the INI side and
`sdk/typescript/examples/ini-counter` for a complete mod.

## Notifications and messages

A simulation module and the client modules of the same source (mod or map) talk through
the game, so every machine stays in step:

- **Client to simulation**: a client module's `Client.sendMessage(bytes)` reaches the
  simulation module's `onMessage` handler on every machine after the command delay,
  with the sender's house, which the client cannot forge. The bytes are untrusted:
  check every one.
- **Simulation to client**: `notify.send(house, data)` notifies that house's player;
  `notify.sendObservers(data)` the observers. Each client module there receives a
  `Notify` event.
- **Shared state**: a declared variable the client module reads with `ClientVar`. It is
  saved, so a client module that starts again after a load sees it at once.

| Name | Does |
| --- | --- |
| `new Notify(name)` | A notification name the module declared in `vera.json` (the build generates these in `src/generated/notify.ts`) |
| `notify.id: i32` | Its ID, the name's FNV-1a hash, the same on every machine and in the client world; `NO_ID` for an undeclared name |
| `notify.send(to: House, data = empty): Status` | Notifies the player of `to`; `Refused` over the size or per-frame limit |
| `notify.sendObservers(data = empty): Status` | Notifies every observer |
| `sendNotify(to, id, data): Status` | The raw call; `to` 0 addresses the observers |
| `MAX_MESSAGE_BYTES` | 96: the largest message `onMessage` receives; per player and source 2 a frame and 16 in 15 frames |
| `MAX_NOTIFY_BYTES` | 256: the largest notification; 256 a frame per module |

---

# Client modules

A client module (`ui.wasm`) imports `vera:client@1.0` and runs only on the local machine,
once per render frame. It reads what this player's interface may show, changes the
interface, the camera and the cursor, plays local effects, and reaches the game only
through messages to its simulation module and standard player orders. It may use floats.

Import from `@vera20k/sdk/client` only. The functions of `@vera20k/sdk` (`get`, `log`,
`Var`, `Action`, ...) call `vera:sim` imports, which the engine refuses in a client
module; `vera20k build` refuses them first and names them. `@vera20k/sdk/client`
re-exports what the two share: `Status`, `lastStatus`, `NO_HANDLE`, `NO_ID`, the
reference types (`Ref`, `Techno`, `House`, `Cell`, `TypeRef`, `Waypoint`), `Prop`,
`NameKind`, `Op`, `Category`, `Command`, `Order`, `ORDER_UNITS`, `Find`, `FindPage`,
`MAX_CLAUSES`, `params`, `EventRecord` and `Event`. Of the shared namespaces only the
functions without host calls work there: `Cell.at`, `Cell.x`, `Cell.y`, `TypeRef.make`,
`TypeRef.id`, `TypeRef.kind`, `Waypoint.number` and the `Find` and `Order` builders.
Read properties with `Client.get(t, Prop.Health)`, not `Techno.health(t)`.

```ts
import { ClickEvent, Client, ClientEvent, NO_HANDLE, Node, Ui, onClientEvent, onStart } from "@vera20k/sdk/client";

const BUY = new Uint8Array(1);
BUY[0] = 1;
let button: Node = NO_HANDLE;

onStart((): void => {
  button = Ui.find("BuyReinforcements");
  if (button != NO_HANDLE) Node.setClickable(button, true);
});

onClientEvent(ClientEvent.Click, (e: ClickEvent): void => {
  if (e.node == button) Client.sendMessage(BUY);
});
```

## Client callbacks

| Function | Export | When |
| --- | --- | --- |
| Top-level code | `client_init` | Once after loading. Only `Client.lookup`, `nameOf`, `hasKind` and `log` work there |
| `onStart(fn: () => void)` | `client_on_start` | Once the match interface is built; again after a rejoining player's catch-up and after a save is loaded |
| `onClientEvent<T>(kind: ClientEvent, fn: (e: T) => void)` | `client_on_events` | Each render frame, before `onRender`, once per event of `kind`, in delivery order. `T` is the kind's view or `ClientEventRecord`. Several handlers of one kind run in registration order |
| `onRender(fn: (dt: f32, alpha: f32) => void)` | `client_on_render` | Each render frame, after the frame's events. Both arguments serve presentation only |

A save does not carry client modules: after a load they start afresh (top-level code,
then `onStart`), with their storage as they left it. During catch-up (a rejoin, a watcher
catching up) they don't run.

## Client calls

| Function | Does |
| --- | --- |
| `Client.frame(): i32` | The latest finished simulation frame (0 in `client_init`) |
| `Client.random(lo, hi): i32` | A local number for display only; `lo > hi` stops the module |
| `Client.log(line)` | A line in the chat list (8 a render frame) and the log file |
| `Client.fail(why)` | Logs `why` and stops this client module, on this machine only |
| `Client.hasKind(table, id): bool` | Tables as on the simulation side, plus 5 node kind, 6 client event kind, 7 client action kind |
| `Client.lookup(kind, name): i32`, `Client.nameOf(kind, id): string`, `Client.text(id): string` | As on the simulation side. Variable and notification names are the ones this match's simulation modules declared; hotkey names this module's |

## Reading the game

| Function | Does |
| --- | --- |
| `Client.get(reference, prop): i64` | A property as this player's interface may show it: an object out of sight is `Gone`, a hidden property `Refused`, both 0 |
| `Client.getText(reference, prop): string` | A string property |
| `Client.getMany(refs, props, out): i32` | As `getMany`, each entry under `get`'s visibility |
| `Client.findPage(query: Find, out, after): FindPage` | A `Find` page restricted to what this player sees; the page object is shared |
| `Client.findFirst(query: Find): i64` | The first match this player sees, or `NO_HANDLE` |
| `new ClientVar(name)` | A simulation module's declared variable, looked up on first use: `id`, `name`, `get(scope = NO_HANDLE)` with `NO_HANDLE` for a global variable, a house (a private one only for this player's house) or an object; 0 on failure |
| `Client.varGet(scope, id): i64` | The raw read; scope `NO_HANDLE` for a global variable |

Allies' objects show their `seen` properties only; a spy shows its disguise as the
screen does; spectators read according to the room.

## Nodes

The match interface is a tree of nodes: the parts of the `veraui.ini` layout, the map's
`[VeraUI]` parts and the nodes modules create. `Node` is a reference (`i64`); keep one
between callbacks in a top-level `let`.

| Function | Does |
| --- | --- |
| `Ui.find(name): Node` | A layout part by its name (without the layout prefix: `Buy` for `[Mine.Buy]`), or a created node by its `Name`. This module's source first, then engine, mod and map; `NO_HANDLE` (`NotFound`) without one |
| `Ui.create(kind: NodeKind, parent: Node): Node` | A new node; parent `NO_HANDLE` is the top level (the overlay for a map module) |
| `Ui.createWith(kind, parent, props: NodeProps): Node` | With up to 16 initial properties in one call |
| `Ui.createNamed(kind, parent, name): Node` | Created and named; removed again if the name is refused |
| `Ui.remove(node): Status`, `Node.remove(node)` | Removes it and its children |
| `new NodeProps()` | Initial properties: `.with(prop, value)`, `.at(x, y)`, `.size(width, height)`; at most `NodeProps.MAX` (16), more makes the create `Refused` |

`NodeKind`: `Text`, `Button`, `Row`, `Column`, `List`, `Image`, `Bar` (modules create
these); `CameoGrid` (only layouts make it; a module can find it and write its common
properties).

`Node` functions (each one `Client.set`, `Client.setText` or `Client.get`):

| Function | Does |
| --- | --- |
| `Node.isNode(n): bool` | Whether `n` is a node reference; no host call |
| `Node.get(n, prop)`, `Node.set(n, prop, value)` | Any property |
| `Node.setName(n, name)` | Names a node this module created, for `Ui.find`: once per node, unique among this source's nodes |
| `Node.setLabel(n, text)` | A CSF entry name or literal text (`Text`, `Button`) |
| `Node.setArt(n, name)` | A picture by file name, such as `mybutton.shp` or `panel.pcx` (`Button`, `Image`) |
| `Node.visible(n)`, `Node.setVisible(n, bool)` | Visibility |
| `Node.setEnabled(n, bool)` | `Button`, `List`: drawn disabled and without `Click` at false |
| `Node.setClickable`, `Node.setHoverable`, `Node.setDismissable` | Which events the node sends (`Click`, `Hover`, `Dismiss`) |
| `Node.setPosition(n, x, y)`, `Node.setSize(n, width, height)` | Pixels: sidebar pixels inside the sidebar, screen pixels elsewhere |
| `Node.setFill(n, percent)` | A `Bar`'s fill, 0..100 |
| `Node.setBackdrop(n, bool)` | Whether a `Text` over the battlefield stands on black (the default) |
| `Node.setItems(n, items: string[])` | A `List`'s rows, 1 KiB in all |

Other properties through `Node.set`, `Client.setText` or `Client.setBytes`: `Prop.Hold`
(0 top left, 1 top, 2 top right, 3 left, 4 middle, 5 right, 6 bottom left, 7 bottom,
8 bottom right), `Prop.Gap` (`Row`, `Column`), `Prop.ItemTooltips` (`CameoGrid`).
`ItemProgress`, `Decoration` and `MapMarker` are `later`.

- **Who may write what.** The mod's module reads, writes and removes any node. The map's
  module, under the mod's `[VeraWasm] MapUI=overlay` (the default), finds and reads any
  node but writes only the map's own; under `MapUI=full` it has the mod's rights; under
  `MapUI=no` it is not loaded. A write the module may not make is `Refused`.
- **How many.** Each module holds at most half of the nodes the interface admits from
  others; a create past it is `Refused`. A node created later is drawn over one created
  before.

| Function | Does |
| --- | --- |
| `Client.set(reference, prop, value): Status` | Writes a property of a client-owned reference: a node, the camera, the cursor |
| `Client.setText(reference, prop, value: string): Status` | Writes a string property (`Label`, `Art`, `Items`, ...) |
| `Client.setBytes(reference, prop, data: Uint8Array): Status` | Writes a bytes property |

## The player, camera and cursor

| Function | Does |
| --- | --- |
| `Client.viewer(): House` | This player's house, `NO_HANDLE` for a spectator |
| `Client.isSpectator(): bool` | |
| `Client.sendMessage(data: Uint8Array): Status` | To this source's simulation module's `onMessage` (at most 96 bytes); `Refused` for spectators and over the limits |
| `Client.order(order: Order): Status` | A standard player order for this player, checked like player input. Each unit once per 15 frames, house-wide orders 4 per 15 frames. `Refused` unless the room allows automation (`AutomationAllowed`); the lobby does not offer that switch yet, so it is refused for now |
| `Camera.REF` | The camera's reference |
| `Camera.cell(): Cell` | The cell at the camera's centre |
| `Camera.jumpTo(cell)`, `Camera.centerOn(cell, speed)` | Moves the local camera |
| `Cursor.REF` | The cursor's reference |
| `Cursor.x()`, `Cursor.y()` | Screen pixels, as node `X` and `Y` count |
| `Cursor.cell(): Cell` | The cell under the cursor, `NO_HANDLE` off the map |
| `Cursor.captureMapClicks(capture: bool)` | At true, map clicks reach only this module (`MapClick`), not the game |

## Local actions

Effects on this machine only (`Local`, appendix 5.4):

| Function | Does |
| --- | --- |
| `Local.playSound(id, cell)` | Plays sound `id` (its `[SoundList]` number) at a cell, or not positioned with `NO_HANDLE`. `Client.lookup(NameKind.Sound, ..)` answers `Unsupported` in this build |
| `Local.playSpeech(name)` | EVA speech, such as `"EVA_ReinforcementsHaveArrived"` |
| `Local.select(units: StaticArray<Techno>, append: bool)` | Selects units this player may select (64 at most) |
| `Local.flash(object, frames)` | Flashes an object |
| `Local.showMessage(text, color)` | One line: a CSF entry name or literal text |
| `Local.registerHotkey(name, chord)` | A hotkey with its default chord, such as `"Ctrl+Alt+K"`. `Refused` when a game hotkey has the chord (the hotkey exists without a key). 256 per module; not in top-level code |
| `Hotkey.id(name): i32` | The ID a `Hotkey` event carries for `name`, or `NO_ID` |
| `Local.homeCamera()` | `later`: `Unsupported` for now |
| `Local.run(kind, target, p: usize)` | Any client action (`ClientActionKind`) with the `ActionParams` at `p` (`params()`) |

## Client events

`ClientEvent` kinds, delivered to `onClientEvent` handlers (at most 1,024 a render
frame):

| Kind | View | When | Read |
| --- | --- | --- | --- |
| `Click` | `ClickEvent` | A clickable node was clicked (the click doesn't reach the map) | `node`, `itemIndex` (the `List` row or `CameoGrid` item, −1 otherwise) |
| `Hover` | `HoverEvent` | The pointer entered or left a hoverable node | `node`, `entered` |
| `Hotkey` | `HotkeyEvent` | A registered hotkey was pressed | `id` (compare with `Hotkey.id(name)`) |
| `Dismiss` | `DismissEvent` | A press outside a visible dismissable node, or Esc | `node` |
| `MapClick` | `MapClickEvent` | A map click that was no `Click` or `Dismiss` | `cell`, `right` |
| `SelectionChanged` | `SelectionChangedEvent` | The local selection changed; read it with `Client.findPage` and a `Selected = 1` clause | — |
| `Notify` | `NotifyEvent` | A simulation module's notification for this machine | `id`, `data` (a copy), `dataAt(i)` (in place), `fromMap`, `source`; test with `NotifyName.is(e)` |
| `SimEvent` | `ForwardedEvent` | `Commanded`, `OrderRefused`, `TakeDamage`, `Killed`, `Birth` or `AttackedBy` of this player's objects or house | `simEvent`: the `EventRecord`, or `null` (`other` is 0 when this player cannot see it) |

Every view extends `ClientEventRecord` (32 bytes): `kind`, `source` (0 mod, 1 map),
`ref`, `id`, `value`, `dataPtr`, `dataLen`, and the named getters above.

`new NotifyName(name)` is a notification name some simulation module of this match
declared, looked up on first use: `id` (`NO_ID` when none declared it) and
`is(event): bool`. `ClientEvent`, `NodeKind` and `ClientActionKind` are generated
enums.

## Storage

Each source (the mod, by its name; a map, by `[VeraWasm] StorageId=` or its file name)
has a small key-value store on this machine, kept between matches.

| Function | Does |
| --- | --- |
| `Storage.read(key): Uint8Array` | The value; empty with `NotFound` without one |
| `Storage.readString(key): string` | The value as UTF-8 text, `""` without one |
| `Storage.len(key): i32` | The value's length, −1 without one |
| `Storage.has(key): bool` | Whether the key has a value |
| `Storage.write(key, data: Uint8Array): Status` | Writes it; empty data is `Refused` |
| `Storage.writeString(key, value): Status` | Writes UTF-8 text; `""` is `Refused` |
| `Storage.remove(key): Status` | Deletes it |

Keys are 1 to 64 ASCII bytes; values up to 16 KiB; 256 keys and 256 KiB per store. Over a
limit a write is `Refused` and nothing changes. The store is written to disk at most once
a second. It is local to this machine: never let it change what the simulation does.
