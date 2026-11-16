# RFC 0001 appendix: API catalog

**English** · [简体中文](0001-wasm-extension-modules-api.zh-CN.md)

This appendix is part of [RFC 0001](0001-wasm-extension-modules.md) and carries
its version (0.0.6). The two files change in the same commit. Where this
catalog and the RFC text differ, the RFC text applies. The two language
versions of this appendix change together; where they differ, the English text
applies.

The catalog lists every function, event, condition, effect and override the
API would offer, the engine code that owns each one, and where each idea comes
from. It does not decide what ships in version 1: that is open question 13 of
the RFC. Entries that came from the research are marked with the proposal they
depend on and whether RFC 0.0.5 adopted it; see
[Proposed RFC changes](#proposed-rfc-changes).

All file paths are relative to the repository root. Line numbers are left out
on purpose: they drift. Code facts come from `main` at `16f18f4b7`.

## Contents

- [Conventions](#conventions)
- [1. Lifecycle exports](#1-lifecycle-exports)
- [2. Events](#2-events)
- [3. Conditions](#3-conditions)
- [4. Actions](#4-actions)
- [5. Attached effects](#5-attached-effects)
- [6. Marks](#6-marks)
- [7. Queries](#7-queries)
- [8. Overrides](#8-overrides)
- [9. AI](#9-ai)
- [10. Rules access](#10-rules-access)
- [11. Client API](#11-client-api)
- [Gaps in the engine](#gaps-in-the-engine)
- [Proposed RFC changes](#proposed-rfc-changes)

## Conventions

### Signatures

Signatures are written in TypeScript that also compiles with AssemblyScript
(RFC 5a). The Rust SDK carries the same functions in snake case. Package and
class names are placeholders, as in the RFC.

The common subset rules apply to every signature:

- no `any`;
- no union types other than `T | null`;
- no callbacks that capture outer variables: every callback is a top-level
  function;
- no `try`/`catch`: errors are return values (see [Errors](#errors));
- parameters that group several values are classes, not object shapes, built
  with `new` and their constructor; fields with defaults may be set
  afterwards.

`T | null` works only for classes in AssemblyScript; a missing handle is
`NO_HANDLE` (see [Handles](#handles)). A missing integer is written as `-1` (or another documented sentinel), as the community
spreadsheet also does ("negative means none"). The `value` and `value2`
fields of `ActionParams` and `AtomicParams` have no missing value: a kind
that uses them always reads them as given (the tables say which kinds do),
and a kind that does not use them ignores them, so their default `0` is not
a sentinel.

### Value types

| Type | Meaning | Notes |
| --- | --- | --- |
| `i32`, `u32`, `bool` | Plain integers and flags | The simulation API has no floating-point values (RFC 6a). |
| `Fixed` | A fixed-point number as its raw `i32` | The engine's `SimFixed` is `I16F16` (`src/util/fixed_math.rs`): `65536` is `1.0`. Used for multipliers. |
| `Coord` | `x`, `y`, `z` in leptons | One cell is 256 leptons. |
| `Cell` | `x`, `y` in cells | `Cell.waypoint(n)` gives a cell that carries waypoint number `n` (section 7.3). |
| `Frames` | A count of logic frames | 15 logic frames per game second (`RA2_LOGIC_FRAMES_PER_SECOND`, `src/util/fixed_math.rs`). |
| `string` | A name | Section names and keys, setup lookups, trigger and export names, CSF labels, client text. Compared without regard to case (RFC 4). Calls made during play take IDs instead (see [IDs](#ids)). |
| `Techno`, `House`, ... | A generational handle | An `i64` value; see [Handles](#handles). |
| `TechnoTypeId`, `MarkId`, ... | The integer ID of a named thing | An `i32` value; see [IDs](#ids). |
| `Uint8Array` | Bytes | Messages, module data. |

```ts
type Fixed = i32;      // I16F16 raw value
type Frames = i32;

class Cell  { x: i32; y: i32; waypoint: i32 = -1; }   // waypoint: a number to resolve, -1 none
class Coord { x: i32; y: i32; z: i32; }   // leptons
class Rect  { x: i32; y: i32; width: i32; height: i32; }   // cells
```

### Handles

Object references are generational handles (RFC 5). A handle whose object is
gone reads as gone and never as a reused slot. The bit layout is not part of
the API and depends on open question 2 (ABI), except that every handle value
is below 2^53, so an SDK built on a JavaScript engine can hold it in a number
(RFC 5a).

Handles are values. In the TypeScript SDK each handle type is an alias of
`i64`, one of the SDK's portable integer types that both `tsc` and
AssemblyScript accept; in the Rust SDK each is a newtype. Passing, storing or
receiving a handle allocates nothing (RFC 15). Because a handle is a plain
integer, it has no methods: in the TypeScript SDK a call on a handle is a
namespace function that takes the handle first (`Techno.kill(t, ...)`,
`House.addMoney(h, 500)`), and the Rust SDK may keep methods on its
newtypes. For brevity the tables write such a call as `techno.kill(...)` or
`house.addMoney(...)`; code examples use the namespace form. Handles are
never nullable: `NO_HANDLE` (`0`) means "none" and is never a live object.

| Handle | Refers to | Gone when |
| --- | --- | --- |
| `Techno` | any unit or building | the object is removed from the game |
| `Building`, `Infantry`, `Vehicle`, `Aircraft` | a `Techno` of that category | as `Techno` |
| `Projectile` | a bullet in flight | it detonates or is removed |
| `House` | a player or computer house | never during a scenario |
| `Team` | a live AI team | the team is disbanded |
| `Trigger` | a live trigger | it is deleted |
| `Tag` | a live tag | it is deleted |
| `Effect` | one attached effect instance | it expires or is removed |
| `SuperWeapon` | one house's superweapon of one type | the house no longer holds it |
| `Node` | a node of the client interface tree (section 11) | it is removed or the match screen closes |

```ts
type Techno     = i64;   // any unit or building
type Building   = i64;   // a Techno of that category
type Infantry   = i64;
type Vehicle    = i64;
type Aircraft   = i64;
type Projectile = i64;
type House      = i64;
type Team       = i64;
type Trigger    = i64;
type Tag        = i64;
type Effect     = i64;
type SuperWeapon = i64;
type Node       = i64;   // client
const NO_HANDLE: i64 = 0;

// namespace Techno (the handle comes first):
isAlive(t: Techno): bool;
asBuilding(t: Techno): Building;   // NO_HANDLE when not a building
asInfantry(t: Techno): Infantry;   // NO_HANDLE when not infantry
asVehicle(t: Techno): Vehicle;     // NO_HANDLE when not a vehicle
asAircraft(t: Techno): Aircraft;   // NO_HANDLE when not an aircraft

// a handle that may be absent is compared with NO_HANDLE (in an action):
if (e.other != NO_HANDLE) {
  ctx.log("attacked");
}
```

A signature, field or result that may hold no object has the plain handle
type and a note that it may be `NO_HANDLE`. Event payload fields that may be
absent (an attacker, a projectile) are `NO_HANDLE` when absent. `T | null` is
kept for classes such as `Path` and `DisguiseInfo` (see
[Signatures](#signatures)).

Types and the other named kinds listed under [IDs](#ids) are named by their
ID string in INI and in setup calls; module code holds the integer ID a lookup
returns.

### IDs

Named things a module uses during play have integer IDs. A module resolves
each name once, usually in `onRules` or `onScenarioStart`, keeps the ID in its
memory and passes the ID to every other call (RFC 15). The SDKs cache the IDs
they resolve. In the TypeScript SDK each ID type is an alias of `i32`; in the
Rust SDK each is a newtype. Like handles, IDs are values and allocate nothing.

| ID type | Names | Lookup |
| --- | --- | --- |
| `TechnoTypeId` | infantry, vehicle, aircraft and building types | `ctx.typeId(name)` |
| `WeaponTypeId` | weapons | `ctx.weaponTypeId(name)` |
| `WarheadTypeId` | warheads | `ctx.warheadTypeId(name)` |
| `ProjectileTypeId` | projectiles (BulletTypes) | `ctx.projectileTypeId(name)` |
| `AnimTypeId` | animations | `ctx.animTypeId(name)` |
| `VoxelAnimTypeId` | voxel animations | `ctx.voxelAnimTypeId(name)` |
| `ParticleSystemTypeId` | particle systems | `ctx.particleSystemTypeId(name)` |
| `SuperWeaponTypeId` | superweapons | `ctx.superWeaponTypeId(name)` |
| `EffectTypeId` | attached effect types (`[AttachEffectTypes]`) | `ctx.effectTypeId(name)` |
| `EffectGroupId` | effect type groups (`Groups`) | `ctx.effectGroupId(name)` |
| `ArmorId` | armor names | `ctx.armorId(name)` |
| `OverlayTypeId` | overlays | `ctx.overlayTypeId(name)` |
| `SoundId` | sounds in the simulation API | `ctx.soundId(name)` |
| `MarkId` | marks | `Mark.id(name)`: registers an unknown name in simulation code; only looks up in client code |
| `NotifyId` | simulation-to-client notifications, declared in the sending simulation module's `vera.notify` custom section | `Notify.id(name)`: the 32-bit FNV-1a hash of the name's UTF-8 bytes after ASCII case folding, the same on every machine and on both sides; the names simulation modules declare are checked for collisions across the match at load; an undeclared name returns `NO_ID` (in a simulation module, a name it did not declare; in a client module, a name no simulation module of the match declared) |
| `TeamTypeId`, `ScriptTypeId`, `AiTriggerId` | AI team types, team scripts, AI triggers | `ai.teamTypeId(name)`, `ai.scriptTypeId(name)`, `ai.aiTriggerId(name)` |

```ts
type TechnoTypeId = i32;   // every ID type above is an alias of i32
type WarheadTypeId = i32;
type MarkId = i32;
const NO_ID: i32 = -1;

// once, in onScenarioStart:
const E1 = ctx.typeId("E1");          // NO_ID and NotFound when nothing matches
const PANICKED = Mark.id("Panicked");
// during play:
Techno.addMark(techno, PANICKED, 0, Visibility.Allies);
```

A lookup compares names without regard to case (RFC 4). A name that matches
nothing returns `NO_ID` and sets `ctx.lastStatus()` to `NotFound`: an expected
outcome under the [error model](#errors), because names come from data a mod
may change. A call given `NO_ID` returns `NotFound` as well; an ID out of
range for its kind is an invalid argument. In simulation code `Mark.id`
registers a name it has not seen, so it always returns an ID; the registry of
mark names is simulation state, saved and hashed (G-M1). In a client module
`Mark.id` only looks up: a name the simulation has not registered returns
`NO_ID` and sets `NotFound`, so a client can never change the registry.
Notification names are declared ahead of
time by the simulation modules that send them: the SDK build writes the names
a simulation module sends, from a list in the module's project configuration,
into a Wasm custom section `vera.notify`. The format of that list is SDK
documentation, not engine API. Client modules declare nothing, and a declared
name that no client module receives is not an error. `Notify.id` returns an
ID for every declared name: when it loads the modules, the engine checks the
declared names of all simulation modules loaded in the match, the mod's and
the map's together, for hash collisions, and refuses modules whose names
collide, with a message (RFC 3, step 6; RFC 10). In a simulation module,
`Notify.id` of a name the module did not declare returns `NO_ID` and sets
`NotFound`. In a client module it returns an ID only for a name that some
simulation module of the match declared, because the engine keeps the table
of declared names; any other name returns `NO_ID` and sets `NotFound`, so a
client never matches an undeclared name by hash.
Types cannot be added during a match (RFC 8), and an ID never changes once
assigned: a later rules layer may add types, which get new IDs, but never
invalidates an existing one (RFC 2). An ID resolved in the mod's `onRules`
therefore stays valid for the whole scenario and across a save and its load.

Names stay in setup and rare calls: the lookups above, `ctx.house(name)`,
`onRules` writes and `rules.set` (section 10), rules reads, `Trigger.create`
with the action names of `Action.wasm`, `Trigger.find` and `Tag.find`, AI
definitions (9.2), rare scenario actions that name a country, theme, EVA line
or CSF label, and in the client API `find` at start, hotkey names, sounds, CSF
labels and text (section 11).

### Event delivery

The engine does not call a module once per event or per action. When a trigger
springs, its original actions and atomic actions run at once, as original
actions do in the original, and each of its module actions (`Action.wasm`)
writes one fixed-size record into the module's queue, carrying the payload of
the event that sprang the trigger. A module receives events only through such
triggers: there are no stand-alone event records, and every record is one
queued module action. In the module phase the engine hands the module the
whole array in one call (RFC 5, 15): `onEvents(ctx, batch)`,
generated by the SDK, which runs the matching action function for each record.
The records are ordered by the subject's creation serial, then by a sequence
number per object; records without a subject object (house, logic and cell
triggers) come first, in the order the trigger runtime springs them. This
order never depends on the engine's loop order, and events raised inside
parallel engine code are collected and queued after that code (RFC 5). Each
module's queue holds at most a fixed number of records per frame; records
beyond the cap are kept in order and delivered first in the next frame, and
they are saved and hashed. The module's frame budget grows by a fixed
allowance per delivered record (RFC 5; open question 8).
A queued action is dropped if the game has ended, or its trigger was disabled
or removed, before the module phase. A record holds the module action, the
event kind, the subject handle, the other handle, the house, a projectile and
an effect handle, a type ID and a few integer payload fields (`EventRecord`,
[section 2](#2-events)). Each event's payload in this catalog names the record
fields it fills; fields an event does not list are `NO_HANDLE` or `-1`. The
layout of each record belongs to the interface definition (RFC 12). The SDK
reads records in place and allocates nothing per event.

Every event VERA adds is raised at a safe point, a moment that is not inside
any calculation: after a calculation has completed (for example `TakeDamage`
after the damage has been committed) or before one begins (for example
`HouseTransfer` before the transfer begins). The trigger's original and atomic
actions run at once from that point (RFC 5). Because they run at once, they can
chain within a frame: damage raises `TakeDamage`, its trigger deals damage
with an atomic action, which raises `TakeDamage` again. When a chain includes
a module trigger (a trigger that has at least one `Action.wasm` action, was
created by a module, or is bound through `Wasm.Tags`; a map trigger that
carries a module action counts), an event VERA adds or an atomic action, the
engine limits its depth per frame; a trigger that would exceed the
limit fires in the next frame instead, so no such chain loops forever (gap
G-T11). It springs at the start of the next frame's module phase, before that
phase's queued actions, with the payload it had; its module actions join that
frame's batch, and pending deferred triggers are saved and hashed. The limit
is open question 18 of the RFC. A chain made only of original events and
original actions is never deferred and runs exactly as in the original (RFC
Goal 5).

### Query results

A query writes its results into a buffer the module provides and returns how
many it wrote; the engine allocates nothing per query (RFC 6c, 15). Results
are sorted by the creation serial the engine assigns to every object and
projectile when it is created, never by storage position; the sort costs in
proportion to the size of the result. When more results exist than fit, the
query sets `out.cursor`, and a second call with the same buffer continues
after the last result written; `out.cursor` is `0` once the last page has
been written. A paged query's result set is fixed when its first page is
read: later pages within the same module call continue that set even if the
world changes in between. A cursor is valid only within the module call that
started the query.

```ts
class HandleBuffer {         // allocated once by the module and reused
  constructor(capacity: i32);
  items: i64[];              // handles, from index 0
  cursor: i64;               // paging: 0 starts a query, non-zero continues it
}
class IdBuffer {             // the same for IDs
  constructor(capacity: i32);
  items: i32[];
  cursor: i64;
}
// objects.inRadius(at: Coord, leptons: i32, f: Filter, out: HandleBuffer): i32
```

### Side tables

State that only some objects use lives in side tables keyed by handle, not in
every object: marks (section 6), type tag bindings and other subscriptions
(`Wasm.Tags`, RFC 7), and the 16 bytes of module data per projectile (RFC 9).
An object that uses none of them costs nothing. Each object, weapon, warhead
and projectile type carries one flag that says whether a module binds it; an
event site checks that flag and does nothing more when it is clear (RFC 15). Side tables are saved and hashed with
the simulation state.

### Enumerations

| Enum | Values | Source |
| --- | --- | --- |
| `Mission` | 0 `Sleep`, 1 `Attack`, 2 `Move`, 3 `QMove`, 4 `Retreat`, 5 `Guard`, 6 `Sticky`, 7 `Enter`, 8 `Capture`, 9 `Eaten`, 10 `Harvest`, 11 `AreaGuard`, 12 `Return`, 13 `Stop`, 14 `Ambush`, 15 `Hunt`, 16 `Unload`, 17 `Sabotage`, 18 `Construction`, 19 `Selling`, 20 `Repair`, 21 `Rescue`, 22 `Missile`, 23 `Harmless`, 24 `Open`, 25 `Patrol`, 26 `ParadropApproach`, 27 `ParadropOverfly`, 28 `Deliberate` (INI name `Wait`), 29 `AttackMove`, 30 `SpyplaneApproach`, 31 `SpyplaneOverfly`; `None` = 255 | `MissionType`, `src/rules/mission_data.rs`. Ids are the map ids. `QMove` and `Ambush` are dormant in YR; `Rescue` is AI-only; `AttackMove` has no dispatcher case. Whether `setMission` should accept those four is unverified. |
| `Relation` | `Self`, `Ally`, `Enemy`, `Neutral` | RFC 6a. Owner of alliances: `src/sim/house_diplomacy.rs`. "Neutral" means a civilian or passive house; exact rule unverified. |
| `Category` | `Infantry`, `Vehicle`, `Aircraft`, `Building`, `Projectile` | RFC 6a; `ObjectCategory` in `src/rules/object_type.rs` covers the first four. |
| `Layer` | `Underground`, `Surface`, `Ground`, `Air`, `Top` | Engine layer names unverified; RFC 6a uses air, ground, water. Water is a land type, not a layer. |
| `LandType` | `Clear`, `Road`, `Water`, `Rock`, `Wall`, `Tiberium`, `Beach`, `Rough`, `Ice`, `Railroad`, `Tunnel`, `Weeds` | The land sections read by `src/rules/terrain_rules.rs`. |
| `SpeedType` / movement zone | as `Locomotor`/`SpeedType`/`MovementZone` readers | `src/rules/locomotor_type.rs`; exact value list unverified here. |
| `Compare` | 0 `Less`, 1 `LessEqual`, 2 `Equal`, 3 `GreaterEqual`, 4 `Greater`, 5 `NotEqual` | Same order as the AI trigger comparator (`compare`, `src/sim/ai_team_creation.rs`). |
| `Difficulty` | `Easy`, `Normal`, `Hard`, `All` | Trigger difficulty flags (`TriggerType.difficulty`). |
| `TagRepeat` | 0 `OneTimeOr`, 1 `OneTimeAnd`, 2 `Repeating` | `TagType.repeat`; names are conventional, behavior from `dispatch_live_tag_event`. |
| `Visibility` | 0 `Own`, 1 `Allies`, 2 `InSight`, 3 `Everyone` | Who sees a mark (section 6): its own house, allies, players who can see the object, everyone. RFC 6b. |
| `SuperWeaponKind` | 0 `MultiMissile` … 11 `PsychicReveal`; Ares kinds | `SuperWeaponKind`, `src/rules/superweapon_type.rs`. |

### Errors

The SDK has no exceptions. Two kinds of failure exist:

1. **Expected outcomes** return a `Status` (or `NO_HANDLE` for a function that
   returns a handle). The module decides what to do. A handle that is
   `NO_HANDLE` or whose object is gone gives `Gone`, a name that matches
   nothing gives `NotFound`, and a refusal by an owner, including an atomic
   action's target that turns out not to fit when its trigger runs, gives
   `Refused`.
2. **Programming errors** (a trap, the operation budget or call depth
   exceeded, an invalid argument) disable the module at the same frame on
   every machine, as RFC 3 says. A failure of the host itself (out of memory,
   an internal engine error) is not a module error but a match error (RFC 3).

```ts
enum Status {
  Ok = 0,
  Gone = 1,         // the handle is NO_HANDLE or its object no longer exists
  NotFound = 2,     // a name matched no type, section, trigger or node
  Refused = 3,      // the owner declined: cell blocked, cannot attach, not allowed
  Unsupported = 4,  // the feature is off in this game (for example an Ares action without Ares)
}
```

What counts as an invalid argument (and so disables the module) is listed per
function: out-of-range indexes (global 0..49, local 0..99), negative counts,
an enum value outside its list. `Trigger.create` checks the arguments of each
`Action.atomic` in the same way, including a target kind that can never fit
the action ([4.1](#41-using-actions)). A name that matches nothing returns
`NotFound`, because names come from data that a mod may change. So does a
waypoint the map does not have: a cell that carries a waypoint number
(`Cell.waypoint(n)`) is resolved by the call that takes it, and when the map
has no such waypoint, `Trigger.create` returns `NO_HANDLE` with `NotFound`,
another call returns `NotFound`, and neither disables the module
([4.1](#41-using-actions)). A target that
turns out not to fit an atomic action when its trigger runs is a data error as
well and gives `Refused` ([4.1](#41-using-actions)). The split
between the two kinds is **adopted in RFC 0.0.5** (proposal P18).

A call on a handle that is `NO_HANDLE` or whose object is gone does nothing.
It returns `Gone` where it returns a `Status`, and sets
`ctx.lastStatus()` (in a client module, `ui.lastStatus()`) to `Gone`.
This holds for every call on a handle, including calls that return
`void`, such as the `Node` functions (section 11.1). It never disables the module
(RFC 3).

An atomic action run by a trigger (`Action.atomic`, [4.1](#41-using-actions))
has no module code to receive its status. When it fails, its error status
(`Refused`, `Gone` or `NotFound`) is written to the in-game log and the action
is skipped; nothing else happens (RFC 3).

`ctx.lastStatus(): Status` repeats the status of the last call for functions
that return a handle, an ID or a value, and for calls on a missing or gone
handle as above.

### Random draws

| Column value | Meaning |
| --- | --- |
| `module` | Draws from the module's own stream, derived from the scenario seed (RFC 4). |
| `native` | Runs native code that draws from the native streams exactly as that code does. The draw list is the Rust port's. |
| `none` | Draws nothing. |

```ts
class Random {
  next(): u32;                    // module stream
  range(low: i32, high: i32): i32; // inclusive; module stream
}
// ctx.random: Random
```

The draws of each native action are those of its Rust port. A per-action
audit of draw counts has not been done; such entries say `native (unverified)`.

### Entry marks

Every entry carries these columns or fields:

| Field | Values |
| --- | --- |
| **Tier** | `v1` (P1 or P2 of the RFC's plan), `v1?` (a candidate under open question 13, or for overrides a v1 candidate under open question 11), `later` |
| **World** | `sim` (`rules.wasm`), `client` (`ui.wasm`), `both` |
| **Owner** | the Rust `file::function` that does the work, or **no owner yet**: then the [gap list](#gaps-in-the-engine) says what must be added |
| **Sources** | where the entry comes from (abbreviations below) |

Source abbreviations:

| Abbreviation | Meaning |
| --- | --- |
| `E12` | original map trigger event 12 |
| `A42` | original map trigger action 42 |
| `S46` | AI team script action 46 |
| `Ar:` | an Ares event, action or key |
| `Ph:` | a Phobos key |
| `T5`, `F12`, `C8` | community spreadsheet: sheet 触发器 (triggers) row 5, sheet 效果器 (effects) row 12, sheet 约束条件 (conditions) row 8 |
| `Rc:` | a Recoil (Spring) analogue, from the research notes |
| `RFC 6b` | the RFC section that names the entry |

`TR` means `src/sim/trigger_runtime.rs`, `TRA` means
`src/sim/trigger_runtime_ares.rs`, `TSV` means `src/sim/team_script_vm/`.

Names of original events and actions in this catalog are API names. Where the
code has no name, the conventional FinalAlert2 name was the starting point;
the number is what identifies the event or action.

## 1. Lifecycle exports

A module exports only what it uses. An export that is absent costs nothing
(RFC 5).

### Simulation module (`rules.wasm`)

| Export | Signature | When | Tier | Owner | Sources |
| --- | --- | --- | --- | --- | --- |
| `onRules` | `onRules(rules: RulesWriter): void` | once, at the module's place in the rules layers (mod: after `RULESMD.INI`, `LANGRULE.INI` and the mode layer; map: after the map layer); a failure stops the match from starting | v1 | `src/rules/native_processing.rs::RulesLayerStack::push`/`process`; module layer: **no owner yet** | RFC 2, 5, 8; Rc: defs post-processing stage |
| `onScenarioStart` | `onScenarioStart(ctx: Ctx): void` | once, at the start of the scenario | v1 | **no owner yet** (module host) | RFC 5; Rc: `GameStart` |
| `onFrame` | `onFrame(ctx: Ctx): void`, generated by the SDK; functions are registered with `onFrame(new Every(N), fn)` | once per frame in the module phase, after `onEvents`, when exported; it runs each registration in the frames where `frame % N == 0`, in registration order | v1 | **no owner yet** (module phase in `src/sim/world/master_frame.rs`) | RFC 5; T27 `HoldDuration=yes`; Rc: `GameFrame` |
| actions | `(ctx: Ctx, e: EventRecord): void`, registered with `action(name, fn)`; the SDK build lists the names in the custom section `vera.actions`, which gives each an index | queued when a trigger with this action springs; runs in the module phase, from the frame's batch (`onEvents`) | v1 | TR `spring_live_trigger` → event record: **no owner yet** | RFC 5, 6a, 12 |
| `onEvents` | `onEvents(ctx: Ctx, batch: EventBatch): void`, generated by the SDK; dispatches on `EventRecord.action` | once per frame per module with queued records, in the module phase, after `onMessage` and before `onFrame` | v1 | **no owner yet** (batched event writer, G-T9) | RFC 5, 15 |
| overrides | see [section 8](#8-overrides) | at a fixed step of the frame when the engine reaches the decision, in creation-serial order | v1? (two or three ship in v1, open question 11) | per override | RFC 5, 6 L3 |
| `onMessage` | `onMessage(ctx: Ctx, sender: House, data: Uint8Array): void` | when a client message has reached the simulation, in the module phase, before `onEvents`; spectators cannot send | v1 (P2) | `src/sim/command.rs::Command`: **no module variant yet** | RFC 5, 10; Rc: `RecvLuaMsg(msg, playerID)` |
| `onLaunch` | `onLaunch(ctx: LaunchCtx, p: Projectile): Path \| null` | once per projectile of a type bound to the module path | later | `ProjectileTrajectory` module arm: **no owner yet** | RFC 9 level 3 |
| `onTrajectoryFrame` | `onTrajectoryFrame(ctx: Ctx, type: ProjectileTypeId, bullets: BulletBatch): void` | once per projectile type per frame | later (L4) | **no owner yet** | RFC 6 L4, 9 level 4 |
| `onLoad` | — | not defined: module memory is restored with the snapshot, and `onScenarioStart` does not run again | **adopted in RFC 0.0.5** (P2) | — | Rc: `Load`, notes 6.10 |

`ctx` is the one entry point to the simulation API:

```ts
class Ctx {
  frame(): i32;
  random: Random;
  scenario: Scenario;   // section 7
  objects: Objects;     // section 7
  map: MapQuery;        // section 7
  rules: RulesReader;   // section 10
  ai: Ai;               // section 9
  house(name: string): House;   // lookup, NO_HANDLE when not found; ID lookups: see IDs
  global(index: i32): Variable;  // 0..49
  local(index: i32): Variable;   // 0..99
  log(text: string): void;
  notify(to: House, id: NotifyId, data: Uint8Array): Status;  // section 11
  notifySpectators(id: NotifyId, data: Uint8Array): Status;   // every spectator's client
  lastStatus(): Status;
}
class Variable { get(): bool; set(value: bool): Status; }
```

`ctx.local(i)` and `ctx.global(i)` go through
`TR::Simulation::set_trigger_variable`, the single writer, which also resets
the timers of triggers waiting on that variable (events 27/28/36/37).
Sources: A28, A29, A56, A57, S12, S38, S39, S40.

### Client module (`ui.wasm`)

| Export | Signature | When | Tier | Owner | Sources |
| --- | --- | --- | --- | --- | --- |
| `onStart` | `onStart(ui: Ui): void` | once, when the match screen is built, after the INI layout loads | v1 (P2) | **no owner yet** (client host in `src/ui/`) | RFC 5, 10 |
| `onRender` | `onRender(ui: Ui, dt: f32, alpha: f32): void` | once per render frame, within the client time budget (RFC 3) | v1 (P2) | **no owner yet** | RFC 5; Rc: `Update(dt)` |
| `onClick` | `onClick(ui: Ui, node: Node): void` | a node with a click handler was clicked | v1 (P2) | **no owner yet** | RFC 10; T39 `OnButtonClick` |
| `onHover` | `onHover(ui: Ui, node: Node, inside: bool): void` | the cursor entered or left a node | v1 (P2) | **no owner yet** | RFC 10 |
| `onHotkey` | `onHotkey(ui: Ui, name: string): void` | a hotkey the module registered was pressed | v1 (P2) | **no owner yet** | RFC 10 |
| `onSelect` | `onSelect(ui: Ui): void`; the selection is read with `client.selection(out)` | the local selection changed | v1 (P2) | selection owner in `src/app/input/` (unverified) | RFC 10; T38 `OnSelect` |
| `onNotify` | `onNotify(ui: Ui, id: NotifyId, data: Uint8Array): void` | a simulation notification for the local player, or for spectators on a spectator's machine, arrived | v1 (P2) | **no owner yet** | RFC 1, 11 ("local notifications"); export name **adopted in RFC 0.0.5** (P19) |
| `onPeerMessage` | `onPeerMessage(ui: Ui, from: House, data: Uint8Array): void` | an ally's or spectator's client sent a message | later | **no owner yet** | **not adopted in RFC 0.0.5** (P5); Rc: `SendLuaUIMsg` |

Client floating point is allowed: the client world is not deterministic
(RFC 1). Every client call runs within a time budget per render frame,
measured by the local clock: an overrun skips that frame's call, and a module
that overruns persistently or traps is disabled on that machine only (RFC 3).

## 2. Events

### How events are used

```ts
enum EventKind { Delivered, Polled }

class EventParam {
  value: i32 = -1;          // number parameter (house index, waypoint, seconds, ...)
  name: string | null = null;   // type name or TeamType name
  compare: Compare = Compare.GreaterEqual;   // polled events
  interval: Frames = 15;    // polled events: how often the engine checks
}

class EventDef {
  constructor(event: i32);   // an Event value
  event: i32;
  param: EventParam = new EventParam();
}

class TriggerDef {
  constructor(events: EventDef[], actions: Action[]);
  events: EventDef[];        // one or more; all must hold (AND), as in the original
  actions: Action[];         // one or more
  house: House = NO_HANDLE;  // NO_HANDLE: the subject's owner at each spring
  conditions: Cond[] = [];
  difficulty: Difficulty = Difficulty.All;
  allowWin: bool = false;    // the A15 marker
  transferOnCapture: bool = false;   // TriggerType token 8
}

class EventRecord {          // fixed size; read in place from the batch (RFC 15)
  action: i32;               // index of the queued module action in vera.actions; always set
  kind: i32;                 // Event value
  subject: Techno;           // the object the trigger is attached to; may be NO_HANDLE
  other: Techno;             // attacker, target, transport, producer, ...; may be NO_HANDLE
  house: House;              // house involved (retained house for E1/E24/E25/E26/E59); may be NO_HANDLE
  projectile: Projectile;    // may be NO_HANDLE
  effect: Effect;            // may be NO_HANDLE
  id: i32;                   // type ID where listed: weapon, warhead, effect type, superweapon type
  value: i32;                // damage, weapon index, stack count, frame, ...
  value2: i32;               // second integer field where listed
  cellX: i32;                // "cell" in the tables; -1 when none
  cellY: i32;
}
class EventBatch {           // one per module per frame
  count: i32;
  at(i: i32): EventRecord;   // a view into module memory, not a copy
}
```

`EventRecord` fields not listed for an event are `NO_HANDLE` or `-1`. Records
come in the order of [Event delivery](#event-delivery) (subject creation
serial, then a sequence per object); each is one queued module action with the payload of the
event that sprang its trigger, and there are no stand-alone event records. The
layout belongs to the interface definition (see
[Event delivery](#event-delivery)). RFC 6a: every event names the object it is
attached to and, where there is one, the other object
involved.

Original events evaluate in the existing trigger runtime (`TR::has_occurred`).
A trigger's original and atomic actions run when it springs, also in a trigger
with module actions; only its module actions wait for the module phase
(section 4.1, RFC 5). Original events raised by House tags are polled every
8th frame (`TR::raise_house_tags`), Logic tags every frame
(`TR::advance_triggers`). All events of one trigger must hold (AND), as in the
original, and `TagRepeat` keeps the original repeat types.

A trigger is attached through a tag (`Tag.create`, `Techno.setTag`,
`cell.setTag`) or bound to types. A binding in code acts like `Wasm.Tags=` in
INI; mod and map modules may both use it, and it applies at once to every
object of the type (G-T3):

```ts
Trigger.bindType(t: Trigger, type: TechnoTypeId): Status;
Trigger.bindWeaponType(t: Trigger, type: WeaponTypeId): Status;     // weapon events
Trigger.bindWarheadType(t: Trigger, type: WarheadTypeId): Status;
Trigger.bindProjectileType(t: Trigger, type: ProjectileTypeId): Status;
```

Each bound object runs a bound trigger as its own instance, with its own
repeat state, so a one-time trigger fires once per object; `Trigger.disable`
and `Trigger.destroy` act on every instance. For a bound trigger the
trigger's house is the subject's owner at the moment it springs, and the tag
holder (`ActionTarget.TagHolder`) is the object that raised the event.

### 2.1 Original events 0..61

Kind: `R` raised at a moment (delivered), `H` House-tag poll, `L` Logic-tag
poll. Payload: `S` subject, `O` other (the actor or source), `Hs` house. All
original events are `v1`, `sim`; owner of evaluation is `TR::has_occurred`
unless the row says otherwise.

| # | API name | Parameter | Kind | Payload | Raised by (owner) | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | — | — | — | — | none | Not exposed: never true. |
| 1 | `EnteredBy` | `value` house index (-1 any) | R | S, O, Hs retained | `src/sim/building_entry.rs`, `c4_plant.rs`, `world/world_orders.rs`, `movement/per_cell.rs`, `vehicle_theft.rs`, `TR::dispatch_foot_cell_tag_events` | Writes persistence. |
| 2 | `SpiedBy` | ignored | R | S, O | **no raise site found** | Never fires today. |
| 3 | `ThievedBy` | `value` house index | R | S, Hs | **no raise site found** | Never fires today. |
| 4 | `DiscoveredByPlayer` | ignored | R | S, O | `world/lifecycle.rs` discovery; `combat/record_kill.rs::record_kill_tag_prefix` | |
| 5 | `HouseDiscovered` | `value` house | H | Hs | `TR::raise_house_tags` | |
| 6 | `AttackedByAnyHouse` | ignored | R | S, O | `TR::raise_house_attacked_tags`; `combat/object_health.rs`; `capture_manager.rs`; `record_kill.rs` | T4 `OnAttackedBy` (coarse form). |
| 7 | `DestroyedByAnyHouse` | ignored | R | S, O | `combat/record_kill.rs` | Needs a kill source. |
| 8 | `AnyEvent` | — | H | S | `TR::raise_house_tags` (every 8 frames) | |
| 9 | `AllUnitsDestroyed` | `value` house | H | Hs | House-tag poll | |
| 10 | `AllBuildingsDestroyed` | `value` house | H | Hs | House-tag poll | |
| 11 | `AllDestroyed` | `value` house | H | Hs | House-tag poll | |
| 12 | `CreditsExceed` | `value` credits | H | Hs | House-tag poll | T31 `OnPlayerMoney`. |
| 13 | `ElapsedTime` | `value` seconds (×15 frames) | L | — | `TR::advance_triggers` | Timer reset by `reset_trigger_timers`. T5 `OnTimer`. |
| 14 | `MissionTimerExpired` | — | L | — | `TR::advance_triggers` | |
| 15 | `BuildingsDestroyedCount` | `value` count | H | Hs | House-tag poll | |
| 16 | `UnitsDestroyedCount` | `value` count | H | Hs | House-tag poll | |
| 17 | `NoFactoriesLeft` | — | H | Hs | House-tag poll | |
| 18 | `CiviliansEvacuated` | — | H | Hs | House-tag poll | |
| 19 | `BuildBuildingType` | `value` BuildingType index | H | Hs | House-tag poll | Writes persistence. API also accepts `name`. |
| 20 | `BuildUnitType` | `value` VehicleType index | H | Hs | House-tag poll | As 19. |
| 21 | `BuildInfantryType` | `value` InfantryType index | H | Hs | House-tag poll | As 19. |
| 22 | `BuildAircraftType` | `value` AircraftType index | H | Hs | House-tag poll | As 19. |
| 23 | `TeamLeavesMap` | `name` TeamType | R | — | `TR::raise_team_left_map_tags` ← `TSV/team_ai.rs` | |
| 24 | `ZoneEntryBy` | `value` house (-1 any) | R | S, O, Hs retained | `TR::dispatch_foot_cell_tag_events` over map tags | |
| 25 | `CrossesHorizontalLine` | as 24 | R | O, Hs retained | same, line flag 0x100000 | |
| 26 | `CrossesVerticalLine` | as 24 | R | O, Hs retained | same, line flag 0x200000 | |
| 27 | `GlobalIsSet` | `value` 0..49 | L | — | `TR::evaluate_event` | |
| 28 | `GlobalIsClear` | `value` 0..49 | L | — | `TR::evaluate_event` | |
| 29 | `DestroyedByAnythingNotInfiltrate` | ignored | R | S, O | `combat/record_kill.rs` | |
| 30 | `LowPower` | `value` house | H | Hs | House-tag poll | T32 `OnPlayerPower`. |
| 31 | `BridgeDestroyed` | ignored | R | cell | `TR::dispatch_bridge_cell_tag_event` | |
| 32 | `BuildingExists` | `value` BuildingType index | H | Hs | House-tag poll | Writes persistence. |
| 33 | `SelectedByPlayer` | ignored | R | S | `world/world_commands.rs` | Not raised in network games. T38 (client form in 11). |
| 34 | `ComesNearWaypoint` | `value` waypoint | R | S | `movement/per_cell.rs` | Within 0x500 leptons. |
| 35 | `EnemyInSpotlight` | ignored | R | S | `world/spotlight.rs` | |
| 36 | `LocalIsSet` | `value` 0..99 | L | — | `TR::evaluate_event` | |
| 37 | `LocalIsClear` | `value` 0..99 | L | — | `TR::evaluate_event` | |
| 38 | `FirstDamagedCombat` | ignored | R | S, O | `combat/object_health.rs` | Source present. |
| 39 | `HalfHealthCombat` | ignored | R | S, O | `combat/object_health.rs` | Yellow health. |
| 40 | `QuarterHealthCombat` | ignored | R | S, O | `combat/object_health.rs` | Red health. |
| 41 | `FirstDamagedAny` | ignored | R | S, O (may be `NO_HANDLE`) | `combat/object_health.rs`; `world/crash.rs` | |
| 42 | `HalfHealthAny` | ignored | R | S | `combat/object_health.rs` | |
| 43 | `QuarterHealthAny` | ignored | R | S | `combat/object_health.rs` | |
| 44 | `AttackedByHouse` | `value` house array index | R | S, O | `combat/object_health.rs`; `capture_manager.rs` | Ares converts Player@X. |
| 45 | `AmbientLightBelow` | `value` | L | — | `TR::advance_triggers` when ambient changed | |
| 46 | `AmbientLightAbove` | `value` | L | — | same | |
| 47 | `ElapsedScenarioTime` | `value` seconds | L | — | `TR::evaluate_event` | |
| 48 | `DestroyedByAnything` | ignored | R | S, O (may be `NO_HANDLE`) | `record_kill.rs`; `building_entry.rs`; `world/world_orders.rs`; `movement/per_cell.rs`; `vehicle_theft.rs`; `world/crash.rs` | T9 `OnKilled` (coarse form). |
| 49 | `PickupCrate` | ignored | R | S | `crates/pickup.rs` | |
| 50 | `PickupCrateAny` | ignored | L | — | `TR::advance_triggers` when a crate was picked up | |
| 51 | `RandomDelay` | `value` seconds | L | — | `TR::advance_triggers` | Draws: native (`scenario_rng`, in `reset_trigger_timers`). |
| 52 | `CreditsBelow` | `value` credits | H | Hs | House-tag poll | |
| 53 | `SpyAsHouse` | `value` house | R | S, Hs | `TR::dispatch_foot_cell_tag_events` (Agent infantry) | Writes persistence. |
| 54 | `SpyAsInfantry` | `value` InfantryType index | R | S | same | Writes persistence. |
| 55 | `NavalUnitsDestroyed` | `value` house | H | Hs | House-tag poll | |
| 56 | `LandUnitsDestroyed` | `value` house | H | Hs | House-tag poll | |
| 57 | `BuildingDoesNotExist` | `value` BuildingType index | H | Hs | House-tag poll | |
| 58 | `PowerFull` | `value` house | H | Hs | House-tag poll | |
| 59 | `EnteredOrOverflownBy` | as 1 | R | S, O, Hs retained | `TR::dispatch_foot_cell_tag_events`; `TR::dispatch_airborne_cell_tag_event` | |
| 60 | `TechTypeExists` | `name` type, `value` count | L | — | `TR::evaluate_event`; Ares: `TRA::ares_has_occurred` | |
| 61 | `TechTypeDoesNotExist` | `name` type | L | — | same | |

Original events by number stay usable in module-created triggers as
`new EventDef(n)`; the `Event` enum gives their names.

### 2.2 Ares events 62..88

Available only when the game reads the Ares way (`rules.ares_form()`);
otherwise `Trigger.create` returns `NO_HANDLE` with `Unsupported`. Triggers
follow `ares_form()` rather than `rules.ares()` alone on purpose: trigger data
read the stock way keeps its stock form until the rules are read the Ares
way, and the Ares arms would misread it (`RuleSet::ares_form`,
`src/rules/ruleset.rs`). Evaluated in
`TRA::ares_has_occurred`. All `v1`, `sim`, sources `Ar:` event of that number.

| # | API name | Parameter | Kind | Raised by |
| --- | --- | --- | --- | --- |
| 62 | `UnderEmp` | — | R | `src/sim/game_entity/emp.rs` |
| 63 | `UnderEmpByHouse` | `value` house | R | `emp.rs` |
| 64 | `RemovedFromEmp` | — | R | `emp.rs` |
| 65 | `RemovedFromEmpByHouse` | `value` house | R | `emp.rs` |
| 66 | `InSpotlight` | — | R | `world/spotlight.rs` |
| 67 | `DriverKilled` | — | R | `vehicle_theft/driver_kill.rs` |
| 68 | `DriverKilledByHouse` | `value` house | R | `driver_kill.rs` |
| 69 | `VehicleTaken` | — | R | `vehicle_theft.rs` |
| 70 | `VehicleTakenByHouse` | `value` house | R | `vehicle_theft.rs` |
| 71 | `Abducted` | — | R | `abduction.rs` |
| 72 | `AbductedByHouse` | `value` house | R | `abduction.rs` |
| 73 | `AbductsSomething` | — | R | `abduction.rs` |
| 74 | `AbductsSomethingOfHouse` | `value` house | R | `abduction.rs` (`TRA::dispatch_object_ares_event`) |
| 75 | `SuperWeaponActivated` | `value` SW type index | H | `superweapon/mod.rs` (`TRA::raise_ares_super_event`) |
| 76 | `SuperWeaponDeactivated` | `value` SW type index | H | `superweapon/mod.rs` |
| 77 | `SuperWeaponNearWaypoint` | `name` SW type, `value` waypoint (5 cells) | H | `superweapon/mod.rs` |
| 78 | `HouseReverseEngineeredType` | `name` type | H | House-tag poll |
| 79 | `SomethingReverseEngineered` | — | R | `reverse_engineering.rs` |
| 80 | `TypeReverseEngineered` | `name` type | R | `reverse_engineering.rs` |
| 81 | `HouseOwnsAtLeast` | `name`, `value` | H | House-tag poll |
| 82 | `HouseOwnsAtMost` | `name`, `value` | H | House-tag poll |
| 83 | `AttackedOrDestroyedByAnybody` | — | R | `combat/object_health.rs` |
| 84 | `AttackedOrDestroyedByHouse` | `value` house | R | `combat/object_health.rs` |
| 85 | `DestroyedByHouse` | `value` house | R | `combat/record_kill.rs` |
| 86 | `TypeCountAtMost` | `name`, `value` | L | Logic-tag poll |
| 87 | `HouseNoKeepAliveObjects` | `value` house | H | House-tag poll |
| 88 | `HouseNoKeepAliveBuildings` | `value` house | H | House-tag poll |

### 2.3 New delivered events (RFC 6a)

New event numbers come from a range free of Ares and Phobos (open question 6);
none is assigned here. New delivered events are raised only for objects whose
type binds a tag with that event (`Wasm.Tags`, or `Trigger.bindType` in code) or whose own tag holds such a
trigger, never for every object (RFC 5, Goal 6). The weapon events (`Fire`,
which carries the projectile handle, `HitTarget` and `Detonated`, RFC 6a) can
also be bound on a weapon, warhead or projectile type (`Wasm.Tags=` in its
section): such a binding fires for every shooter that uses the type, whatever
the shooter's own type, and the shooter is the subject. Each raise site checks
one flag of the object's type, or of the weapon, warhead or projectile type,
first (see [Side tables](#side-tables)); `Cond.isWeaponType` tests the weapon.
Each event is raised at a safe point, a moment that is not inside any
calculation: after the calculation that produced it has completed, or, for
`HouseTransfer`, before the transfer begins (see
[Event delivery](#event-delivery)); the "Raised at" column names the owner of
that moment and says where the exact safe point is not known. After
`HouseTransfer` the engine re-checks that the transfer is still valid (the
object still exists and its owner has not changed) before doing it, and
skips the transfer, without `HouseTransferred`, when an action has changed
that. `FatalDamage` is raised after the fatal damage is committed; the death
chain that follows checks that the object still exists, and a delivered event
cannot cancel the death (RFC 5).

| Event | Payload (`EventRecord`) | Raised at (owner of the moment) | Tier | Sources |
| --- | --- | --- | --- | --- |
| `TakeDamage` | `other` source (may be `NO_HANDLE`), `value` damage after armor | `src/sim/combat/world_receiver.rs::commit_entities`, after the damage is committed; raise hook **no owner yet** | v1? | RFC 6a; T2 `OnTakeDamage` (history fields ③④ are module memory, RFC 6a); Rc: `UnitDamaged` |
| `HitByWarhead` | `id` warhead (`WarheadTypeId`), `value` final damage, `other` source | `combat/damage/receive.rs::receive_damage`, after it completes (exact point unverified); hook **no owner yet** | v1? | RFC 6a; T3 `OnHitBy` (immune hits do not count) |
| `AttackedBy` | `other` attacker, `value` weapon index (-1 none), `projectile` (may be `NO_HANDLE`) | fire step in `src/sim/combat/world_fire.rs` (raised on the target), after the step completes (exact point unverified); hook **no owner yet** | v1? | RFC 6a; T4 |
| `FatalDamage` | `other` attacker (may be `NO_HANDLE`), `value` damage | `combat/world_receiver.rs`, after the fatal damage is committed and before the death chain (whether this is a safe point is unverified); hook **no owner yet** | v1? | RFC 6a; T22 `OnFatalDamage` |
| `Killed` | `other` killer (may be `NO_HANDLE`) | `src/sim/combat/record_kill.rs`, after the kill is recorded (exact point unverified); hook **no owner yet** | v1? | RFC 6a; T9; Rc: `UnitDestroyed` |
| `Fire` | `other` target (may be `NO_HANDLE`), `value` weapon index, `id` weapon (`WeaponTypeId`), `projectile` (may be `NO_HANDLE`) | `combat/world_fire.rs` FireAt steps; `src/sim/aircraft/fire_at.rs::fire_at`; after the shot is fired (exact point unverified); hook **no owner yet** | v1? | RFC 6a, 9 level 2; T10 `OnFire` |
| `SpawnFire` | `other` spawned unit, `value` weapon index, `projectile`; target in the record's `cell` or via the spawn's target | `src/sim/spawn_manager.rs` (after the spawn's shot; exact point unverified); hook **no owner yet** | v1? | RFC 6a; T11 `OnSpawnFire` |
| `HitTarget` | `other` target, `value` damage, `id` weapon (`WeaponTypeId`), `projectile` | `combat/world_receiver.rs` (raised on the firer), after the damage is committed; hook **no owner yet** | v1? | RFC 6a; T21 `OnHitTarget` |
| `TargetChanged` | `other` new target (`NO_HANDLE` when the target is cleared) | target assignment and clearing (`src/sim/world/techno_ai/scan_batch.rs` and order paths; unverified), after the target is set or cleared; hook **no owner yet** | v1? | RFC 6a, 6 (P13); T6 `OnAimed` |
| `ProjectileIncoming` | `projectile`, `other` launcher (may be `NO_HANDLE`) | projectile admission `src/sim/combat/projectile_delivery.rs::admit_fired_bullet`, after the projectile is admitted; hook **no owner yet** | v1? | RFC 6a; T7 `OnProjectileComing` |
| `Detonated` | `projectile`, `cell`, `other` source (may be `NO_HANDLE`), `id` warhead (`WarheadTypeId`) | detonation admission in `src/sim/projectile`, after the detonation (exact point unverified); hook **no owner yet** | v1? | RFC 6 L2 ("projectile detonated"), 9 level 2; T36; Rc: `ProjectileDestroyed`, `Explosion` |
| `Birth` | `other` producer (may be `NO_HANDLE`), `effect` (may be `NO_HANDLE`), `value` build time (-1 none) | `src/sim/world/world_spawn.rs`, production exit, reinforcements, after the object is placed (exact point unverified); hook **no owner yet** | v1? | RFC 6a; T8 `OnBirth`; Rc: `UnitCreated` |
| `TransportEntered` | raised on the passenger (`subject`); `other` transport or building, `value` occupied size, `value2` 1 garrison / 0 transport; passengers by query | `src/sim/building_entry.rs`, transport boarding (`src/sim/passenger.rs` callers), after the entry completes; hook **no owner yet** | v1? | RFC 6a; T13 `OnEntry`; Rc: `UnitLoaded` |
| `TransportExited` | as `TransportEntered`, after leaving | unload paths, after the exit completes; hook **no owner yet** | v1? | RFC 6a; T14 `OnExit`; Rc: `UnitUnloaded` |
| `PassengerAdded` | raised on the transport or building (`subject`); `other` passenger, `value` occupied size, `value2` garrison flag | as `TransportEntered` | v1? | RFC 6a; T15 `OnEntryBy` |
| `PassengerRemoved` | as `PassengerAdded` | as `TransportExited` | v1? | RFC 6a; T16 `OnExitBy` |
| `Crushed` | `other` crusher | crush step in movement (file unverified), after the crush; hook **no owner yet** | v1? | RFC 6a; T25 `OnCrush` |
| `HouseTransfer` | `house` new house, `value` 1 if by mind control; raised before the transfer | `src/sim/world/mod.rs::change_owner_with_rules`; `src/sim/capture_manager.rs`; before the transfer begins; hook **no owner yet** | v1? | RFC 6a; T28 `OnHouseTransfer`; Rc: `UnitTaken` |
| `HouseTransferred` | `house` previous house, `value` 1 if by mind control; raised after the transfer | `change_owner_with_rules`, after the transfer completes; hook **no owner yet** | v1? | RFC 6a (P10); Rc: `UnitGiven` (two-phase transfer) |
| `EffectStart` | `effect`, `value` stack count | `src/sim/attach_effect.rs::AttachEffectList::attach`, after the effect is attached; hook **no owner yet** | v1? | RFC 6a; T17 `OnBehaviorStart` |
| `EffectExpire` | `effect` (reads gone after delivery), `id` effect type (`EffectTypeId`) | `attach_effect.rs` update/removal, after the effect is removed; hook **no owner yet** | v1? | RFC 6a; T19 `OnBehaviorExpire` |
| `EffectStacks` | `effect`, `value` new stack count | `attach_effect.rs` cumulative attach, after the stack is added; hook **no owner yet** | v1? | RFC 6a; T20 `OnBehaviorStack` |
| `Timer` | `value` frame of the previous firing; fires every `param.interval` frames, the period the trigger sets | module trigger timer: **no owner yet** (E13 timers reuse `CdTimer`) | v1? | RFC 6a; T5 `OnTimer` |
| `SuperWeaponLaunched` | `id` SW type (`SuperWeaponTypeId`), `house`, `cell` target | `src/sim/superweapon/fire.rs::fire_super_weapon`, after the superweapon has fired (exact point unverified); hook **no owner yet** | v1? | RFC 6a |

T18 `OnBehavior` (effect running, every interval) is the polled
`EffectStacksPoll` event below, or a condition `hasEffect` on a `Timer` trigger.

### 2.4 New polled events (RFC 6a)

The engine checks the state natively every `param.interval` frames and fires
on the edge: at the check where the state has become true, and not again until
it has been false at a check. An object is checked in the frames where
`frame % interval == serial % interval`, with `serial` its creation serial, so
the checks of different objects are spread over different frames.
Comparison: `param.compare` against `param.value`. Owner of the
poll: **no owner yet** (a module poll list beside the Logic tags in
`TR::advance_triggers`). Each reads an existing owner.

| Event | Reads | Read owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `HealthPoll` | health (`value`), or percent when `name` is `"%"` | `src/sim/combat/object_health.rs::Health` | v1? | RFC 6a; T26 |
| `AmmoPoll` | ammo count | `src/sim/ammo.rs` | v1? | RFC 6a; T29 |
| `PassengersPoll` | passenger count | `src/sim/passenger.rs::PassengerCargo::count` | v1? | RFC 6a; T33 |
| `TransportSpacePoll` | free size | `PassengerCargo::total_size`, `capacity` | v1? | RFC 6a; T23 |
| `StoragePoll` | ore load | miner storage (`Miner`, `src/sim/world/world_spawn/construction.rs` creates it) | v1? | RFC 6a; T35 |
| `MissionPoll` | current mission equals `value` | `src/sim/mission/state.rs` | v1? | RFC 6a; T12 `OnMission` |
| `MovingPoll` | `value` 1 moving, 0 stopped | locomotion state (unverified getter) | v1? | RFC 6a; T34 `OnStartMoving` |
| `LandTypePoll` | land type of the object's cell equals `value` | map cell land (`src/rules/terrain_rules.rs` semantics) | v1? | RFC 6a; T24 |
| `HouseMoneyPoll` | the trigger house's available money | `src/sim/economy.rs::Economy` (`available_money`) | v1? | RFC 6a; T31 |
| `HousePowerPoll` | output minus drain | `src/sim/power_system.rs::PowerState::total_output`, `total_drain` | v1? | RFC 6a; T32 |
| `EffectStacksPoll` | stacks of effect type `name` | `attach_effect.rs` (`active_count`) | v1? | RFC 6a; T18, T20 |
| `DistancePoll` | distance in leptons to the record's `other` | coordinates of both objects | v1? | RFC 6a; T37 `Compare` (distance only; other comparisons are module code) |

### 2.5 Further candidate events (from the research)

RFC 0.0.5 added these to its further candidate events for open question 13
(proposal P11, RFC 6a). `Stunned`/`Unstunned` and `Decloaked` are catalog
additions that the RFC's list does not name. `HouseTransferred` is in the
RFC's main table and listed in 2.3.

| Event | Payload | Where it would be raised | Sources |
| --- | --- | --- | --- |
| `Built` | `other` builder | construction complete (buildup end) | Rc: `UnitFinished` |
| `Idle` | — | mission ends with nothing queued (mission owner `src/sim/mission/`) | Rc: `UnitIdle` |
| `MoveFailed` / `Arrived` | `cell` destination | movement owner | Rc: `UnitMoveFailed`, `UnitArrivedAtGoal` |
| `Stunned` / `Unstunned` | `other` source | EMP (`game_entity/emp.rs`); Ares 62/64 cover the Ares case | Rc: `UnitStunned` |
| `Promoted` | `value` new rank | `src/sim/combat/veterancy.rs` | Rc: `UnitExperience` |
| `Cloaked` / `Decloaked` | — | `src/sim/cloak_disguise.rs` cloak tick | Rc: `UnitCloaked` |
| `Spotted` / `Lost` | `house` the house that now sees or lost it | `src/sim/vision/mod.rs` | Rc: `UnitEnteredLos`, `UnitLeftLos` |
| `AmmoChanged` | `value` old, `value2` new | `src/sim/ammo.rs` | Rc: `StockpileChanged` |
| `MissionChanged` | `value` mission | `src/sim/mission/` | Rc: `UnitCommand` |
| `HouseDefeated` | `house` | `src/sim/world/house_defeat.rs` | Rc: `TeamDied` |
| `PlayerLeft` | `house` | `HouseState.left_game` writer | Rc: `PlayerRemoved` |
| `StorageFull` | `house` | `src/sim/economy.rs` (Ares storage) | Rc: `ResourceExcess` |
| `MotionEnded` | `value` reason | the motion effect (section 5) | Rc: `MoveCtrlNotify` |

### 2.6 Spreadsheet triggers not taken as events

| Row | Trigger | Where it goes |
| --- | --- | --- |
| T27 | `HoldDuration=yes` (unconditional, every frame) | `onFrame` |
| T30 | `OnCounterCondition` | not in the engine (RFC 6a): module memory |
| T37 | `Compare` beyond distance | module code |
| T38 | `OnSelect` | client `onSelect` (section 11); E33 in the simulation |
| T39 | `OnButtonClick` | client `onClick` |
| T40 | `RecorderCondition` | not in the engine (RFC 6a) |

The spreadsheet gives damage as floating point (T2, T3, T21, T22). The API
gives integers, as the RFC requires.

## 3. Conditions

A condition filters a trigger natively before module code runs (RFC 6a). It
tests the subject by default, or `EventRecord.other` when built with `.onOther()`.
`Cond.not(c)` negates. All conditions are `sim`. Evaluation owner: **no owner
yet** (a condition list on the TriggerType, checked in `TR` before springing).

```ts
class Cond {
  static not(c: Cond): Cond;
  onOther(): Cond;
}
```

| Function | Tests | Read owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `Cond.relation(r: Relation)` | relation of the tested object's house to the trigger house | `src/sim/house_diplomacy.rs` | v1 | RFC 6a; C5 |
| `Cond.category(c: Category)` | object category | `ObjectType` category / projectile | v1 | RFC 6a; C3, C14 |
| `Cond.layer(l: Layer)` | object layer | entity layer (unverified getter) | v1 | RFC 6a; C4 |
| `Cond.isType(type: TechnoTypeId)`; `Cond.isProjectileType(type: ProjectileTypeId)` | type equals `type` | `ObjectType` id; projectile type | v1 | RFC 6a; C12, C13 (negated), C15 (projectile type) |
| `Cond.isWeaponType(type: WeaponTypeId)` | the weapon of a weapon event (`Fire`, `HitTarget`) equals `type` | the record's `id` | v1 | RFC 6a (weapon, warhead and projectile bindings) |
| `Cond.armor(armor: ArmorId)` | armor | `src/rules/armor.rs::armor_index`; Ares `ares_armor.rs` | v1 | RFC 6a; C18 |
| `Cond.civilianHouse()` | owner is a civilian house | `HouseState` side/country (rule unverified) | v1 | RFC 6a; C16 |
| `Cond.health(cmp: Compare, value: i32)` | health points | `combat/object_health.rs::Health` | v1 | RFC 6a; C19 |
| `Cond.healthPercent(cmp: Compare, percent: i32)` | health percent | `Health` | v1 | RFC 6a; C19 |
| `Cond.healthBelowPercent(p: i32)` | shorthand of the above | `Health` | v1 | RFC example |
| `Cond.landType(t: LandType)` | land type of the object's cell | map cell land | v1 | RFC 6a; C2 |
| `Cond.housePower(state: PowerLevel)` | `Full` or `Low` | `src/sim/power_system.rs::PowerState::is_low_power` | v1 | RFC 6a; C6 (red/yellow/green: a third level is unverified in the engine) |
| `Cond.houseMoney(cmp: Compare, value: i32)` | the tested object's house money | `src/sim/economy.rs` | v1 | RFC 6a; C7 |
| `Cond.hasEffect(type: EffectTypeId, minStacks: i32)` | effect present with at least `minStacks` | `attach_effect.rs` `active_count` | v1 | RFC 6a; C8, C9 (negated) |
| `Cond.hasMark(m: MarkId)` | mark present | marks: **no owner yet** | v1 | RFC 6a; C10, C11 (negated) |
| `Cond.effectSource(type: EffectTypeId, r: Relation)` | the source of effect `type` has relation `r` | `AttachEffectInstance` source (`is_self_owned`) | v1? | RFC 6a; C21 |
| `Cond.inWeaponRange(weaponIndex: i32)` | `other` is within the subject's weapon range | range owner `attach_effect.rs::range_with_attach_effects` and weapon `Range` | v1? | RFC 6a; C20 |
| `Cond.isStandIn()` | object is a "stand" (替身) | not in the RFC; no engine concept | — (not taken) | C17 |

## 4. Actions

### 4.1 Using actions

Actions run in two ways:

- inside a trigger: `TriggerDef.actions` holds `Action.original(n, params)`,
  `Action.wasm(name)` or `Action.atomic(kind, target, params)`;
- directly from module code, through the functions below.

Inside a trigger, original actions (`Action.original`) and atomic actions
(`Action.atomic`) run when the trigger springs, as original actions do in the original,
also when the trigger contains module actions. Only `Action.wasm` actions are
queued, each with the payload of the event that sprang the trigger; they run
in the module phase as one batch per module per frame, in creation-serial order (RFC 5;
[Event delivery](#event-delivery)): a trigger with the actions [original A,
module B, atomic C] runs A and C when it springs and B later in the same
frame. A queued module action is dropped if the game has ended, or its trigger
was disabled or removed, before the module phase. An author who needs a strict
order writes all of the trigger's actions as module code and calls original
actions through the functions below. Called directly from module code, an
action runs at once.

In a trigger, an atomic action is `Action.atomic(kind, target, params)`:
`kind` names one atomic action of [4.3](#43-atomic-actions-rfc-6b-and-spreadsheet-instant-effects)
(`AtomicKind`), `params` carries its parameters (`AtomicParams`), and `target`
is an explicit target parameter (`ActionTarget`). An atomic action covers an
action whose parameters fit: one target, one ID, up to two integers, one cell
and one house. `Log` is the one accepted exception: it also carries its line
of text. An action that needs more, such as `map.createProjectile`, is written
as a module action. The targets: `Subject` (the default) is the record's
subject; `Other` the record's other object; `TagHolder` the objects whose tag
holds the springing trigger, as the "live" rows of 4.2 act on, or the cell
when the trigger is held by a CellTag, or, for a trigger bound to a type or a
weapon, warhead or projectile type, the object that raised the event;
`Projectile` the event's projectile; `House` the house in `params.house`, or
the trigger's house when that is `NO_HANDLE`; `Cell` the cell in
`params.cell`. A waypoint is given as a cell that carries its number:
`Cell.waypoint(n)` returns such a cell, and `Trigger.create` resolves it to
the waypoint's cell, since waypoints do not move. A waypoint the map does not
have is a map data error, not a module error: `Trigger.create` returns
`NO_HANDLE` with `NotFound`, creates no trigger, writes the error to the
in-game log and leaves the module enabled; in INI it is reported in the lobby
at load. A kind that uses no target (`SetTechLevel`,
`Log`) ignores `target`. Fixed rules cover what the parameters cannot say:

- an explicit house (the `House` target, the new owner of `SetOwner`, the
  owner of objects the action creates) is `params.house`; when it is
  `NO_HANDLE`, the trigger's house is used, which for a bound trigger is the
  subject's owner at that moment;
- a source (of damage, a kill or a detonation: `Kill`, `Damage`,
  `DetonateWeapon`, `DetonateWarhead`) is the record's `other` and is
  optional: when the event has none, the action runs with no source and gives
  no kill credit;
- a second object the action acts on (a passenger) is the record's `other`;
  when the event has none, the action gets `Gone` and is skipped, and the
  status is logged;
- `FireWeapon` aims at, in order of precedence: `params.cell` if given, else
  the record's `other`, else the shooter's current target; with none of them
  it gets `Gone` and is skipped;
- the cell of an object target is the cell the object is in; a `Projectile`
  target counts as an object target, and its cell is the one under the
  projectile;
- a coordinate is the center of the given cell, or of the target's cell,
  except for a `Projectile` target, whose coordinate is the projectile's
  center.

`Trigger.create` checks the arguments of each `Action.atomic`: an invalid one
is an invalid argument from module code and disables the module (see
[Errors](#errors); RFC 3). This includes a target kind that can never fit the
action, that is, one the Target column below does not list for the kind (for
example `Cell` for `Kill`). Atomic actions written in INI are checked at load
and reported in the lobby; their INI form is open question 5 of the RFC. A
target that turns out not to fit when the trigger runs (for example a
`TagHolder` that is a cell, for `Kill`) is a data error, not a programming
error: the action gets `Refused` and the module stays enabled. No module code receives the status of
an atomic action run by a trigger: when it fails, its error status
(`Refused`, `Gone` or `NotFound`) is written to the in-game log and the action
is skipped; nothing else happens (see [Errors](#errors); RFC 3, 6b). Original and atomic actions that run at once
can raise further events and chain within a frame; a chain that includes a
module trigger, an event VERA adds or an atomic action is limited to the
per-frame depth (see [Event delivery](#event-delivery)).

```ts
class ActionParams {
  value: i32 = 0;
  value2: i32 = 0;           // auxiliary value (paramType 11)
  name: string | null = null; // type, CSF label, sound, theme, EVA name
  team: string | null = null;
  trigger: string | null = null;
  tag: string | null = null;
  cell: Cell | null = null;   // replaces the waypoint token
}
class Action {
  static original(n: i32, p: ActionParams): Action;
  static wasm(name: string): Action;   // a name in the module's vera.actions table
  static atomic(kind: AtomicKind, target: ActionTarget, params: AtomicParams): Action;
}
enum ActionTarget { Subject, Other, TagHolder, Projectile, House, Cell }   // target of an atomic action in a trigger; Subject is the default; House is params.house, or the trigger's house when that is NO_HANDLE
enum AtomicKind {            // the atomic actions of 4.3 whose parameters fit, named after their functions
  FireWeapon, LaunchSuperWeapon, SpawnAnim, Remove, Kill, Damage, SetHealth, Heal,
  CreateObject, SetMission, Teleport, ClearTarget, ClearDisguise, PlaySound,
  AddExperience, SetRank, AddMoney, SetTechLevel, AllowType, SetOwner,
  ClearMindControl, SetAmmo, AddAmmo, AddPassenger, RemovePassenger,
  AddEffect, RemoveEffect, AddMark, RemoveMark,
  DetonateWeapon, DetonateWarhead, DetonateProjectile, RemoveProjectile,
  EndFor, SetScore, AddScore, Log, Convert,
}
class AtomicParams {         // the parameters of an atomic action other than its target
  id: i32 = NO_ID;           // type ID: weapon, warhead, superweapon, animation, techno, sound, effect, mark
  value: i32 = 0;            // amount, damage, points, weapon index, mission, rank, ammo, enum value
  value2: i32 = 0;           // second value where the action needs one
  cell: Cell | null = null;  // a cell, where the action takes one; a waypoint as Cell.waypoint(n)
  house: House = NO_HANDLE;  // House target, SetOwner's new owner, owner of created objects; NO_HANDLE: the trigger's house
  text: string | null = null; // Log only: the line to write
}
```

`AtomicKind` lists the actions of 4.3 whose parameters fit; the area search (a
query) and `map.createProjectile` are not among them. The table lists the
target and the fields each kind uses; how they map onto each function's
parameters belongs to the interface definition (RFC 12). *Object* means
`Subject`, `Other` or a `TagHolder` that holds objects; *cell* means `Cell` or
the cell of an object target, including `Projectile` (the cell under the
projectile; its center for a coordinate); `—` means the kind uses no target
(and ignores it) or no field; `other` is the second object of the rules
above, and "optional" marks a source the action can do without.

| `AtomicKind` | Target | Fields and second object |
| --- | --- | --- |
| `FireWeapon` | object (the shooter) | `id` weapon or `value` weapon slot; aims at `cell` if given, else `other`, else the shooter's current target |
| `LaunchSuperWeapon` | `House` | `id` superweapon type, `cell` |
| `SpawnAnim` | cell | `id` animation |
| `Remove` | object | — |
| `Kill` | object | `other` as the source (optional) |
| `Damage` | object | `id` warhead, `value` damage; `other` as the source (optional) |
| `SetHealth`, `Heal` | object | `value` points |
| `CreateObject` | cell | `id` type; `house` the owner |
| `SetMission` | object | `value` mission, `cell` |
| `Teleport` | object | `cell`, `value` blocked-cell rule (`Blocked`) |
| `ClearTarget`, `ClearDisguise`, `ClearMindControl` | object | — |
| `PlaySound` | cell | `id` sound |
| `AddExperience` | object | `value` points |
| `SetRank` | object | `value` rank |
| `AddMoney` | `House` | `value` amount |
| `SetTechLevel` | — | `id` type, `value` level |
| `AllowType` | `House` | `id` type, `value` 1 allowed, 0 not |
| `SetOwner` | object | `house` the new owner |
| `SetAmmo`, `AddAmmo` | object | `value` ammo |
| `AddPassenger`, `RemovePassenger` | object (the transport) | `other` as the passenger |
| `AddEffect`, `RemoveEffect` | object | `id` effect type |
| `AddMark`, `RemoveMark` | object or `House` | `id` mark; for `AddMark`, `value` the mark's value and `value2` its `Visibility` |
| `DetonateWeapon` | cell | `id` weapon; `other` as the source (optional) |
| `DetonateWarhead` | cell | `id` warhead, `value` damage; `other` as the source (optional) |
| `DetonateProjectile`, `RemoveProjectile` | `Projectile` | — |
| `EndFor` | `House` | `value` outcome |
| `SetScore`, `AddScore` | `House` | `value` points |
| `Log` | — | `text` |
| `Convert` | object | `id` type |

`Action.original(n, …)` runs the existing arm of
`TR::execute_live_trigger_action`; with Ares, `TRA::ares_execute_action`
first. The named functions in 4.2 call the same owners. Where the original
takes a waypoint, the API takes a `Cell`; `Cell.waypoint(n)` gives a
waypoint's cell. Owners that read a waypoint index today need a cell entry
(gap G-A1). Rows marked "live" act on the objects whose tag holds the
springing trigger; their direct form takes the object instead.

Every row is `v1`, `sim`, unless stated. Draws: `native (unverified)` for
every row that runs native code, except where noted.

### 4.2 Original actions 0..145

| # | API function | Parameters | Owner | Notes |
| --- | --- | --- | --- | --- |
| 0 | — | — | no-op | Only as `Action.original(0)`. |
| 1 | `scenario.declareWinner(country: string)` | country | `src/sim/world/house_defeat.rs::request_player_outcome` | Acts on the player, not the trigger house. |
| 2 | `scenario.declareLoser(country: string)` | country | same | |
| 3 | `house.beginProduction()` | — | `HouseState::enable_scripted_production` | |
| 4 | `ai.createTeam(teamType: TeamTypeId, house: House): Team` | TeamType | `TSV/reinforcements.rs::trigger_create_team` | Max gate applies. S0–S68 run the team. `house` may be `NO_HANDLE`; returns `NO_HANDLE` when no team is created. |
| 5 | `ai.destroyTeams(teamType: TeamTypeId)` | TeamType | `TSV/membership.rs::destroy_teams_of_type` | |
| 6 | `house.allToHunt()` | — | `house_strategy::all_to_hunt` | |
| 7 | `ai.reinforce(teamType: TeamTypeId)` | TeamType (its own waypoint) | `TSV/reinforcements.rs::spawn_team_reinforcements` | |
| 8 | `map.dropZoneFlare(cell: Cell)` | cell | `src/sim/anim_class.rs::trigger_drop_zone_flare` | G-A1. |
| 9 | `house.fireSale()` | — | `strategy_emergency_mut().set_state_four()` | |
| 10 | `scenario.playMovie(index: i32)` | movie index | `TriggerEffect::FullscreenMovie` | Presentation effect for every player. |
| 11 | `scenario.showText(csfLabel: string)` | CSF label | `TriggerEffect::LocalizedMessage` | |
| 12 | `trigger.destroy()`; `Trigger.destroyType(id: string)` | trigger | TR (deferred delete of live instances) | |
| 13 | `house.beginAutocreate()` | — | `HouseState::enable_scripted_autocreate` | |
| 14 | live; direct form `techno.setOwner(house)` (4.3) | house | `src/sim/world/mod.rs::change_owner_with_rules` | |
| 15 | `TriggerDef.allowWin = true` | — | `TR::classify_map_triggers` (`register_allow_win_tag_type`) | Marker only; no executable arm. |
| 16 | `map.revealAll()` | — | `src/sim/world/shroud_refresh.rs::trigger_reveal_entire_map` | |
| 17 | `map.revealAround(cell: Cell)` | cell | `trigger_reveal_at_waypoint` | G-A1. |
| 18 | `map.revealZone(cell: Cell)` | cell | `trigger_reveal_zone_at_waypoint` | G-A1. |
| 19 | `sound.play(sound: SoundId)` | sound | `SimSoundEvent::VocCentered` | |
| 20 | `scenario.playTheme(name: string)` | theme | `TriggerEffect::MusicTheme` | |
| 21 | `scenario.playSpeech(eva: string)` | EVA name | `SimSoundEvent::TriggerEva` | |
| 22 | `trigger.force()`; `Trigger.forceType(id: string)` | trigger | `TR::apply_live_trigger_action` | Springs with no actor and a zero cell. |
| 23 | `scenario.timer.start()` | — | `scenario_countdown.rs` (`change_scenario_timer(Start)`) | |
| 24 | `scenario.timer.stop()` | — | same (`Stop`) | |
| 25 | `scenario.timer.extend(seconds: i32)` | seconds | same (`Extend`) | |
| 26 | `scenario.timer.shorten(seconds: i32)` | seconds | same (`Shorten`) | |
| 27 | `scenario.timer.set(seconds: i32)` | seconds | same (`Set`) | |
| 28 | `ctx.global(i).set(true)` | 0..49 | `TR::set_trigger_variable` | Out of range: invalid argument. |
| 29 | `ctx.global(i).set(false)` | 0..49 | same | |
| 30 | `house.setAutoBaseBuilding(on: bool)` | flag | `src/sim/world/house_base.rs::trigger_auto_base_building` | |
| 31 | `map.growShroud()` | — | `grow_shroud` | |
| 32 | live; direct form `techno.kill(KillMode.Normal, NO_HANDLE)` (4.3) | — | C4 direct damage + `apply_damage_to_cell` (bridge) in TR | Ares survivor rule applies. |
| 33 | `house.grantSuperWeapon(type: SuperWeaponTypeId, oneTime: bool)` | SW type | `src/sim/superweapon/mod.rs::trigger_grant_super` | `oneTime = true`. |
| 34 | same, `oneTime = false` | SW type | same | |
| 35 | `house.setPreferredTargetKind(value: i32)` | value | `set_preferred_super_weapon_target_kind` | |
| 36 | `house.transferAllTo(to: House)` | house | `trigger_change_all_house_objects` | |
| 37 | `house.makeAlly(other: House)` | house | `src/sim/house_diplomacy.rs::make_ally` | Both ways. |
| 38 | `house.makeEnemy(other: House)` | house | `house_diplomacy.rs::make_enemy` | Both ways. |
| 39 | — | — | no arm (returns false) | Not exposed. |
| 40 | `scenario.setVisibleMapArea(r: Rect)` | rectangle | `change_visible_map_area` | |
| 41 | `map.playAnim(anim: AnimTypeId, cell: Cell)` | AnimType | `anim_class.rs::trigger_play_anim_at_waypoint` | G-A1. Ares `MakeInfantry=` owner rule. |
| 42 | `map.detonateWeapon(weapon, at, NO_HANDLE)` (4.3), `at` the cell's center | WeaponType | `src/sim/world/trigger_explosion.rs::trigger_explosion_at_waypoint` | G-A1. One function with the 4.3 entry. |
| 43 | `map.playVoxelAnim(type: VoxelAnimTypeId, cell: Cell)` | VoxelAnimType | `trigger_play_voxel_anim_at_waypoint` | G-A1. |
| 44 | `weather.startIonStorm(duration: i32)` | duration | `superweapon/lightning_storm::start` | Refused if raging. |
| 45 | `weather.stopIonStorm()` | — | `lightning_storm::request_end` | Refused if not raging. |
| 46 | `scenario.lockInput()` | — | `TriggerEffect::InputLockChanged` | |
| 47 | `scenario.unlockInput()` | — | `unlock_input` | |
| 48 | `scenario.centerCamera(cell: Cell, speed: i32)` | cell, speed | `TriggerEffect::CameraAtCoord` | Every player's camera. Client form in 11. |
| 49 | — | — | no arm (dormant residual) | Zoom in. Not exposed. |
| 50 | — | — | no arm | Zoom out. Not exposed. |
| 51 | `house.reshroud()` | — | `trigger_reshroud_map` | |
| 52 | live; direct form `building.setSpotlightBehavior(v: i32)` | value | `set_spotlight_behavior` | |
| 53 | `trigger.enable()` | trigger | TR (enable, difficulty-admitted, reset timers) | |
| 54 | `trigger.disable()` | trigger | TR | |
| 55 | `scenario.radarEvent(cell: Cell, kind: i32)` | cell, `RadarEventType` | `SimSoundEvent::RadarEvent` | |
| 56 | `ctx.local(i).set(true)` | 0..99 | `TR::set_trigger_variable` | |
| 57 | `ctx.local(i).set(false)` | 0..99 | same | |
| 58 | `map.meteorShower(cell: Cell, intensity: i32)` | cell, intensity | `anim_class.rs::trigger_meteor_shower` | G-A1. |
| 59 | `map.reduceOre(cell: Cell)` | cell | `src/sim/world/mod.rs::trigger_reduce_tiberium_at_waypoint` | G-A1. |
| 60 | live; direct form `building.sell()` | — | `production::sell_back` | |
| 61 | live; direct form `building.setOnline(false)` | — | `src/sim/building_power.rs::go_building_offline` | |
| 62 | live; direct form `building.setOnline(true)` | — | `building_power.rs::go_building_online` | |
| 63 | `map.damageBridgeAt(cell: Cell)` | cell | `bridge_orchestrator::damage_at_waypoint` | G-A1. |
| 64 | `map.lightFlash(cell: Cell, size: FlashSize.Small)` | cell | C4 `CombatLightRequest::Impact`, damage 50 | |
| 65 | same, `FlashSize.Medium` | cell | damage 100 | |
| 66 | same, `FlashSize.Large` | cell | damage 300 | |
| 67 | `scenario.announceWin()` | — | `PlayerOutcomeRequest::Flag{Victory}` | |
| 68 | `scenario.announceLose()` | — | `Flag{Defeat}` | |
| 69 | `scenario.endScenario()` | — | `EndScenario` | |
| 70 | `tag.destroy()`; `Tag.destroyType(id: string)` | tag | `TR::delete_live_tag` | Stops the running dispatch. |
| 71 | `lighting.setAmbientStep(v: i32)` | value | `set_ambient_change_input(false, v)` | |
| 72 | `lighting.setAmbientRate(v: i32)` | value | `set_ambient_change_input(true, v)` | |
| 73 | `lighting.setAmbient(v: i32)` | value | `set_ambient_light` | |
| 74 | `house.setAiTriggersActive(true)` | — | `src/sim/house_state.rs::HouseState::set_ai_triggers_active` | |
| 75 | `house.setAiTriggersActive(false)` | — | same | |
| 76 | `house.setTeamRatio(TeamRatio.AiTriggers, v)` | value | `src/sim/ai_team_creation.rs::HouseTeamCreation::set_trigger_ratio` | |
| 77 | `house.setTeamRatio(TeamRatio.Aircraft, v)` | value | `set_aircraft_team_ratio` | |
| 78 | `house.setTeamRatio(TeamRatio.Infantry, v)` | value | `set_infantry_team_ratio` | |
| 79 | `house.setTeamRatio(TeamRatio.Units, v)` | value | `set_unit_team_ratio` | |
| 80 | `ai.reinforceAt(teamType: TeamTypeId, cell: Cell)` | TeamType, cell | `spawn_team_reinforcements(waypoint)` | G-A1. |
| 81 | live; direct form `techno.wakeUp()` | — | Guard for sleeping/harmless (TR) | |
| 82 | `house.wakeSleepers()` | — | TR (Sleep foots not owned by the house) | |
| 83 | `house.wakeHarmless()` | — | TR (Harmless foots owned by the house) | |
| 84 | `scenario.wakeGroup(group: i32)` | group | TR | |
| 85 | `map.setVeinGrowth(v: i32)` | value | `session.set_vein_growth` | |
| 86 | `map.setOreGrowth(v: i32)` | value | `ore_growth_config.set_enabled_from_trigger` | |
| 87 | `map.setIceGrowth(v: i32)` | value | `session.set_ice_growth` | |
| 88 | `map.createParticleSystem(type: ParticleSystemTypeId, cell: Cell)` | ParticleSystem | `particles/spawn.rs::trigger_create_particle_system_at_waypoint` | G-A1. |
| 89 | `map.removeParticleSystems(cell: Cell)` | cell | `trigger_mark_particle_systems_done_at_waypoint` | G-A1. |
| 90 | `weather.lightningStrike(cell: Cell)` | cell | `lightning_storm::create_cloud_bolt` | G-A1. |
| 91 | live; direct form `infantry.goBerserk()` | — | `make_psycho` | |
| 92 | `house.setFirestorm(true)` | — | Ares: `src/sim/ares_firestorm.rs::ares_set_firestorm_state` | `Unsupported` without Ares. |
| 93 | `house.setFirestorm(false)` | — | same | |
| 94 | `map.ionCannonStrike(cell: Cell)` | cell | `src/sim/world/ion_blast.rs::trigger_ion_blast` | G-A1. |
| 95 | `map.nukeStrike(cell: Cell)` | cell | `src/sim/world/trigger_strikes.rs::trigger_nuke_strike` | G-A1. |
| 96 | `map.chemStrike(cell: Cell)` | cell | `trigger_chemical_strike` | G-A1. |
| 97 | `scenario.toggleTrainCargo()` | — | `session.toggle_train_crate` | |
| 98 | `sound.playAtRandomWaypoint(sound: SoundId)` | sound | `src/sim/world/object_sound.rs::trigger_play_sound_at_random_waypoint` | Ares variant; draws native. |
| 99 | `sound.playAt(sound, at)` (4.3), `at` the cell's center | sound, cell | `trigger_play_sound_at_waypoint` | G-A1. One function with the 4.3 entry. |
| 100 | `scenario.playRadarMovie(index: i32, pause: bool)` | movie | `TriggerEffect::RadarMovie{pause:false}` | |
| 101 | `map.reshroudAt(cell: Cell)` | cell | `trigger_conceal_at_waypoint` | G-A1. |
| 102 | `weather.lightningStormStrike(cell: Cell)` | cell | `lightning_storm::start` with rules duration | Uses the trigger house. |
| 103 | `scenario.timer.setText(csfLabel: string)` | label | `set_scenario_timer_caption` | |
| 104 | `team.flash(frames: i32)`; `ai.flashTeams(teamType, frames)` | TeamType, frames | `TSV/actions.rs::flash_teams_of_type` | |
| 105 | `scenario.talkBubble(team: Team, frame: i32)` | team, frame | `talk_bubble_set_object_and_frame` | `team` may be `NO_HANDLE`. |
| 106 | `rules.setTechLevel(type: TechnoTypeId, level: i32)` | type, level | `src/sim/world/type_tech_levels.rs::set_object_type_tech_level` | Also section 10. |
| 107 | `ai.reinforceByChrono(teamType: TeamTypeId, cell: Cell)` | TeamType, cell | `spawn_chrono_team_reinforcements` | G-A1. |
| 108 | `map.createCrate(kind: i32, cell: Cell)` | kind (20 random) | `crates::place_crate` | Draws native when random. G-A1. |
| 109 | `map.ironCurtainAt(cell: Cell)` | cell | `superweapon::iron_curtain::at_waypoint` | G-A1. |
| 110 | `scenario.pause(duration: i32)` | duration | `session.pause_scenario` | |
| 111 | live; direct form `building.evictOccupants()` | — | `garrison_eject::sell_building_occupants` | |
| 112 | `scenario.jumpCamera(cell: Cell)` | cell | `TriggerEffect::CameraAtCoord{velocity:None}` | |
| 113 | `house.cheer()` | — | `house_cheer` | |
| 114 | `scenario.setSidebarTab(tab: i32)` | 0..3 | `TriggerEffect::SelectSidebarTab` | |
| 115 | `scenario.flashCameo(type: TechnoTypeId, duration: i32)` | type | `TriggerEffect::FlashSidebarCameo` | |
| 116 | `sound.stopAt(cell: Cell)` | cell | `trigger_stop_sound_at_waypoint` | G-A1. |
| 117 | `scenario.playRadarMovie(index, true)` | movie | `RadarMovie{pause:true}` | |
| 118 | `map.clearSmudges()` | — | `src/sim/world/time_warp.rs::trigger_clear_all_smudges` | |
| 119 | `house.destroyAll(Destroy.All)` | — | `house_blowup(All)` | |
| 120 | `house.destroyAll(Destroy.Buildings)` | — | `house_blowup(Buildings)` | |
| 121 | `house.destroyAll(Destroy.Land)` | — | `house_blowup(Land)` | |
| 122 | `house.destroyAll(Destroy.Naval)` | — | `house_blowup(Naval)` | |
| 123 | `house.mindControlBase(of: House)` | house | `src/sim/capture_manager.rs::control_house_buildings(.., false)` | |
| 124 | `house.restoreMindControlledBase(of: House)` | house | `control_house_buildings(.., true)` | |
| 125 | `map.createObject(type, cell, owner, new CreateParams())` (4.3) with a building type | type, cell | `TR::trigger_create_building_at_waypoint` (the building path of `createObject`) | G-A1. Returns `NO_HANDLE` when no building is created. |
| 126 | `house.restoreStarting(buildingsOnly: false)` | — | `restore_initial_house_technos(.., true)` | |
| 127 | `scenario.chronoScreenEffect(duration: i32)` | duration | `start_chrono_screen_effect` | |
| 128 | `house.teleportAll(cell: Cell)` | cell | `src/sim/world/trigger_chrono.rs::chronoshift_house_to` | |
| 129 | `house.superWeapon(type).setChargePercent(p: i32)` | SW, percent | `SuperWeaponInstance::set_charge_percentage` | |
| 130 | `house.restoreStarting(buildingsOnly: true)` | — | `restore_initial_house_technos(.., false)` | |
| 131 | `house.flashBuildings(type: TechnoTypeId, duration: i32)` | type | `entity.flashing.start` | |
| 132 | `house.superWeapon(type).setRechargeTime(frames: i32)` | SW, time | `SuperWeaponInstance::set_recharge_time` | |
| 133 | `house.superWeapon(type).resetRechargeTime()` | SW | `reset_recharge_time` | |
| 134 | `house.superWeapon(type).reset()` | SW | `superweapon::reset_super` | |
| 135 | `house.setPreferredTargetCell(cell: Cell)` | cell | `set_preferred_super_weapon_target` | |
| 136 | `house.clearPreferredTargetCell()` | — | `clear_preferred_super_weapon_target` | |
| 137 | `house.setAlternateBase(cell: Cell)` | cell | `HouseState::set_alternate_base` | |
| 138 | `house.clearAlternateBase()` | — | `clear_alternate_base` | |
| 139 | `house.radarBlackout(frames: i32)` | frames | `src/sim/power_system.rs::PowerState::start_radar_outage` | |
| 140 | `house.setDefensiveTargetCell(cell: Cell)` | cell | `alert_super_weapon_defense` | |
| 141 | `house.clearDefensiveTargetCell()` | — | `clear_super_weapon_defense` | |
| 142 | `lighting.retint(Color.Red, v: i32)` | value | `retint_normal_component(0)` | |
| 143 | `lighting.retint(Color.Green, v)` | value | component 1 | |
| 144 | `lighting.retint(Color.Blue, v)` | value | component 2 | |
| 145 | `scenario.homeCamera()` | — | `TriggerEffect::HomeCamera` | |

Ares actions (only with Ares; otherwise `Unsupported`; owner `TRA::ares_execute_action`):

| # | API function | Owner | Sources |
| --- | --- | --- | --- |
| 146 | `house.addAuxPower(power: i32)` | `PowerState::add_aux_power` | Ar: 146 |
| 147 | `house.killDrivers()` | `TRA::ares_trigger_kill_drivers` | Ar: 147 |
| 148 | `scenario.setEvaVoice(v: i32)` | `TriggerEffect::EvaVoice` | Ar: 148 |
| 149 | `techno.setGroup(group: i32)` | `assign_object_group` | Ar: 149 |

### 4.3 Atomic actions (RFC 6b) and spreadsheet instant effects

One function is one operation with one owner (RFC 6 granularity). In a
trigger, each one that `AtomicKind` lists is written `Action.atomic` with the
kind of the same name and takes the target parameter of
[4.1](#41-using-actions).

| Function | Semantics | Draws | Owner | Tier | Sources |
| --- | --- | --- | --- | --- | --- |
| `techno.fireWeapon(w: WeaponSel, at: Target): Status` | Fire the weapon in slot `w.index` or of type `w.weapon` at a cell, an object or the current target, through FireAt. Refused when the weapon cannot fire now | native | **no owner yet** (FireAt is reached only from the fire update in `src/sim/combat/world_fire.rs::visit_fire`) | v1? | RFC 6b; F4 `LaunchWeapon`; Rc: `UnitWeaponFire` |
| `house.launchSuperWeapon(type: SuperWeaponTypeId, cell: Cell): Status` | Fire a superweapon the house holds | native | `src/sim/superweapon/fire.rs::fire_super_weapon` | v1? | RFC 6b; F5 |
| `map.spawnAnim(anim: AnimTypeId, at: Coord): Status` | Create an animation | native | `src/sim/anim_class.rs` (coordinate entry; G-A1) | v1? | RFC 6b; F6 |
| `techno.remove(): Status` | Remove with no death, no survivors, no credit | none | `src/sim/world/lifecycle.rs::Simulation::uninit_with_context` | v1? | RFC 6b; F7 `Remove`; Rc: `DestroyUnit(reclaimed)` |
| `techno.kill(mode: KillMode, source: Techno): Status` | Kill through the damage path with damage = health and `ignore_defenses` (death animation, survivors, kill credit to `source`); `source` may be `NO_HANDLE` | native | `src/sim/world/mod.rs::commit_direct_damage_receiver` (`EntityDamageEvent::direct_receiver`) | v1? | RFC 6b; F8 `Suicide` |
| `techno.damage(amount: i32, warhead: WarheadTypeId, source: Techno): Status` | Deal damage through warhead and armor, with credit and experience; `source` may be `NO_HANDLE` | native | `commit_direct_damage_receiver`; math `combat/damage/receive.rs::receive_damage` | v1? | RFC 6b "change health (add)"; F9; Rc: `AddUnitDamage`; **adopted in RFC 0.0.5** (P9: add goes through the warhead path) |
| `techno.setHealth(points: i32): Status` / `heal(points: i32)` | Set or heal health, no credit | none | `combat/object_health.rs::Health::set`, `heal` (no Simulation-level owner) | v1? | RFC 6b; F9 `ModifyHealth` |
| `map.createObject(type: TechnoTypeId, at: Cell, owner: House, c: CreateParams): Techno` | Create a finished object; blocked cell refuses (`NO_HANDLE`) | native | `src/sim/world/world_spawn.rs::spawn_object_at_height`, `place_building_with_cleared_foundation`, `place_foot_with_cleared_cell` (doc says "tests, tools, debug") | v1? | RFC 6b; F10 `CreateUnit` (inheriting triggers/effects: `CreateParams.inheritTag`) |
| `techno.setMission(m: Mission, target: Target \| null): Status` | Queue a mission as a player order would | none | `src/sim/mission/retask.rs::queue_order_mission`, `queue_megamission` | v1 | RFC 6b, example; F16 `Mission`; F13 `CommandMoveTo` (= `Move` with a cell) |
| `techno.teleport(cell: Cell, rule: Blocked): Status` | Chronosphere-style move; `Blocked.Nearby` picks a nearby cell, `Blocked.Fail` refuses | native | **no owner yet** (`superweapon/chronosphere.rs::warp_object` is private; `trigger_chrono.rs::chronoshift_house_to` is house-wide) | v1? | RFC 6b; F12 `ForceMoveTo` |
| `techno.clearTarget(): Status` | Drop the current target | none | **no owner yet** (only per-locomotor `clear_target` traits) | v1? | RFC 6b; F15 |
| `techno.clearDisguise(): Status` | Remove a spy or Mirage disguise | none | `src/sim/cloak_disguise.rs` (`DisguiseRuntime::clear_unit`; no transaction owner) | v1? | RFC 6b; F14 |
| `objects.inRadius(...)` | Search an area (a query) | none | section 7 | v1 | RFC 6b; F17 `SearchArea` |
| `sound.playAt(sound: SoundId, at: Coord)` | Play a sound | none | `SimSoundEvent` | v1? | RFC 6b; F18 `PlayAudio` |
| `techno.addExperience(points: i32)` / `setRank(r: Rank)` | Add experience or set a rank | none | `src/sim/combat/veterancy.rs::Veterancy` (`set_elite`, `set_veteran`, `set_rookie`, `set_from_percent`) | v1? | RFC 6b; F19 `GetEXP` |
| `house.addMoney(amount: i32): Status` | Add or take money | none | `src/sim/economy.rs::Economy::add_credits`, `spend_money` | v1? | RFC 6b; F20 `GetMoney` |
| `rules.setTechLevel(type, level)` / `house.allowType(type, allowed)` | Change the tech tree | none | `set_object_type_tech_level` exists; per-house prerequisite change: **no owner yet** | v1? | RFC 6b; F21 `TechModifier` |
| `techno.setOwner(house: House): Status` | Transfer the owner | none | `change_owner_with_rules` | v1? | RFC 6b; F22 |
| `techno.clearMindControl(): Status` | Free a mind-controlled object to its original owner | none | `src/sim/capture_manager.rs` (original-owner records exist; no release function confirmed) | v1? | RFC 6b; F23 |
| `techno.setAmmo(n: i32)` / `addAmmo(n: i32)` | Change ammo | none | `src/sim/ammo.rs` (`add`, `decrease_by`) | v1? | RFC 6b; F24 |
| `transport.addPassenger(p: Techno)` / `removePassenger(p: Techno)` | Board or eject a passenger | native | **no owner yet** (`PassengerCargo::board_forced_chain`, `disembark` are list operations without limbo/unlimbo) | v1? | RFC 6b; F25 |
| `techno.addEffect(...)` / `removeEffect(...)` | section 5 | — | section 5 | v1 | RFC 6b; F2, F3 |
| `techno.addMark(...)` / `removeMark(...)` / `setMarkValue(...)`; the same on `House` | section 6 | — | section 6 | v1 | RFC 6b |
| `map.createProjectile(p: ProjectileParams): Projectile` | Create a projectile: type, start, target object or coordinate, source for kill credit and experience, 16 bytes of initial module data; `NO_HANDLE` on failure. Not an `AtomicKind`: it needs more parameters than an atomic action has (4.1) | native | `src/sim/world/mod.rs::Simulation::admit_projectile` | v1? | RFC 6b, 9; Rc: `SpawnProjectile` |
| `map.detonateWeapon(weapon: WeaponTypeId, at: Coord, source: Techno): Status` | Detonate a weapon at a coordinate with no bullet; `source` may be `NO_HANDLE` | native | `src/sim/world/trigger_explosion.rs::trigger_explosion_at_waypoint` (waypoint only; G-A1) | v1? | A42 with a coordinate (RFC 6: coordinates are parameters) |
| `map.detonateWarhead(warhead: WarheadTypeId, damage: i32, at: Coord, source: Techno): Status` | Area damage of a warhead with a given damage and source; `source` may be `NO_HANDLE` | native | `src/sim/combat/world_receiver.rs::apply_area_damage` | v1? | **adopted in RFC 0.0.5** (P7); Rc: `SpawnExplosion` |
| `projectile.detonate(): Status` | Detonate a projectile now | native | `src/sim/projectile` detonation admission (no public entry) | later | **adopted in RFC 0.0.5** (P8); Rc: `SetProjectileCollision` |
| `projectile.remove(): Status` | Remove a projectile without detonation | none | **no owner yet** | later | **adopted in RFC 0.0.5** (P8); Rc: `DeleteProjectile` |
| `scenario.endFor(house: House, outcome: Outcome): Status` | End the game for one house, won or lost | none | `house_defeat.rs::request_player_outcome` | v1? | RFC 6b |
| `house.setScore(points: i32)` / `addScore` | Set scores | none | `Economy::add_score` (no setter) | v1? | RFC 6b |
| `ctx.log(text: string)` | Write a line to the in-game log | none | **no owner yet** | v1 | RFC 6b, 10 |
| `techno.convert(type: TechnoTypeId): Status` | Change type (as Ares conversion) | native | `src/sim/game_entity/type_conversion.rs::convert_techno_type` | v1? | RFC 8 (per-object type change); S68 |

```ts
class WeaponSel { index: i32 = -1; weapon: WeaponTypeId = NO_ID; }
class Target { techno: Techno = NO_HANDLE; cell: Cell | null = null; current: bool = false; }
enum KillMode { Normal }          // death animation, survivors, credit
enum Blocked { Nearby, Fail }
class CreateParams { inheritTag: Techno = NO_HANDLE; mission: Mission = Mission.None; }
class ProjectileParams {
  type: ProjectileTypeId;
  weapon: WeaponTypeId = NO_ID;   // supplies damage and warhead
  from: Coord;
  toTechno: Techno;          // may be NO_HANDLE
  toCoord: Coord | null;
  source: Techno;            // kill credit and experience; may be NO_HANDLE
  data: Uint8Array | null;   // up to 16 bytes, saved and hashed
}
```

## 5. Attached effects

Owner: `src/sim/attach_effect.rs` (`AttachEffectList`, `AttachEffectInstance`),
type reader `src/rules/attach_effect.rs` (`[AttachEffectTypes]`). Much of both
is behind `cfg(feature = "phobos")` today; RFC 8 requires moving it to a run-time
switch first (gap G-E1). The effects modules use are available whenever the
module feature is on, independent of the Phobos switch, and the VERA-only
effect kinds (G-E3) switch on the same way (RFC 8, 14). The only full attach
transaction is the warhead path
`Simulation::apply_warhead_attach_effects`; there is no
"attach this effect type to this object" Simulation function (gap G-E2).

### 5.1 Functions

```ts
class EffectParams {
  duration: Frames = -1;      // -1: the type's Duration
  delay: Frames = -1;         // -1: none
  source: Techno = NO_HANDLE;
  sourceHouse: House = NO_HANDLE;
}
// namespace Techno (the handle comes first):
addEffect(t: Techno, type: EffectTypeId, p: EffectParams): Effect;   // NO_HANDLE + Refused when stacking refuses
removeEffect(t: Techno, e: Effect): Status;
removeEffects(t: Techno, type: EffectTypeId): i32;    // returns how many
removeEffectGroup(t: Techno, group: EffectGroupId): i32;
effects(t: Techno, out: HandleBuffer): i32;           // deterministic order: attach order
// namespace Effect:
type(e: Effect): EffectTypeId; remaining(e: Effect): Frames; stacks(e: Effect): i32;
source(e: Effect): Techno;   // may be NO_HANDLE
```

| Function | Owner | Tier | Sources |
| --- | --- | --- | --- |
| `addEffect` | `AttachEffectList::attach(type_index, AttachEffectParams, &AttachEffectType, AttachPolicy, source)` + publication: **no owner yet** | v1 | RFC 6b; F2 `AddBehavior`; Ph: `AttachEffect.AttachTypes` |
| `removeEffect`, `removeEffects` | `remove_attach_effect_type` (private) | v1 | RFC 6b; F3; Ph: `AttachEffect.RemoveTypes` |
| `removeEffectGroup` | same | v1 | Ph: `AttachEffect.RemoveGroups` |
| `effects`, `remaining`, `stacks` | `AttachEffectList::instances`, `AttachEffectInstance::remaining`, `active_count` | v1 | RFC 6c |

Effect types are defined in INI (`[AttachEffectTypes]`, Phobos reader) or by
`onRules` writing those sections. New properties use the same reader (RFC 6b).

### 5.2 Effect properties

| Property | Phobos key (reader `src/rules/attach_effect.rs`) | Meaning | Sources |
| --- | --- | --- | --- |
| Duration | `Duration`; warhead side `AttachEffect.Durations`, `AttachEffect.DurationOverrides` | frames; negative means unlimited (unverified) | RFC 6b; C3 |
| Delay | `AttachEffect.Delays`, `AttachEffect.InitialDelays` (attaching side) | frames before the effect starts | RFC 6b; C12 |
| Interval | no Phobos key found | frames between firings | RFC 6b; C13; **no reader yet** |
| Cooldown before reapplying | `AttachEffect.RecreationDelays` (attaching side) | frames after expiry before it can attach again | RFC 6b; C11 |
| Maximum firings | no Phobos key found (`DiscardOn.Firing.Count` discards after firings; not the same) | | RFC 6b; C8; **no reader yet** |
| Source | `AttachEffect.SameSourceOnly`, `AttachEffect.IgnoreFromSameSource`, `AttachEffect.CumulativeSourceMaxCount` | rules for the effect's source | RFC 6b; C5 |
| Inherited on transform | `AllowTransfer.Convert` | kept when the object converts | RFC 6b; C6 |
| Inherited on creation | `AllowTransfer`; `AttachEffects.AttachOnOwnerChange` (unverified semantics) | | RFC 6b; C6 |
| Ignores the Iron Curtain | `PenetratesIronCurtain` | | RFC 6b; C7 |
| Stacking | `Cumulative`, `Cumulative.MaxCount` (negative: no cap); `AttachEffect.ReplaceLongerDuration`; `AttachEffect.CumulativeRefreshAll`, `.OnAttach`, `AttachEffect.CumulativeRefreshSameSourceOnly` | see 5.3 | RFC 6b; C2, C4 |
| Type group | `Groups` | names used by `RemoveGroups`, `RequiredGroups`, `DisallowedGroups` | C9 |
| Tag text | — | marks (section 6) | C10 |
| Counter | — | not in the engine (RFC 6a) | C17 |
| Powered | `Powered` | runs only while the object has power | Ph |
| Discard | `DiscardOn`, `DiscardOn.*` | ends on move, firing, damage, mission, land type, ... | Ph |
| Expire weapon | `ExpireWeapon`, `ExpireWeapon.TriggerOn`, `.CumulativeOnlyOnce`, `.UseInvokerAsOwner` | fire a weapon at expiry | Ph |

### 5.3 Stacking and exclusive groups

The RFC requires each effect type to declare how it stacks (open question 14).
Proposed mapping onto the Phobos reader:

| Rule | Phobos keys |
| --- | --- |
| Stack up to a limit | `Cumulative=yes`, `Cumulative.MaxCount=N` |
| Replace the older one | `Cumulative=no` (refreshes the existing instance; exact refresh rule from `should_refresh`, unverified) |
| Refuse the newer one | **no key yet** (new property, same reader) |
| Exclusive group | **no key yet**: proposed `ExclusiveGroup=` (new property; name unverified) |

The RFC puts transform, teleport and motion in one exclusive group; the owner
keeps one at a time.

### 5.4 Effect kinds

| Effect (RFC 6b) | Phobos key on `[AttachEffectType]` | Stacking default | Exclusive group | Owner | Tier | Sources |
| --- | --- | --- | --- | --- | --- | --- |
| Tint | `Tint.Color`, `Tint.Intensity`, `Tint.VisibleToHouses`, `Tint.Cumulative` | replace | — | `attach_effect.rs` + render | v1 | RFC 6b; F30 `Paintball` |
| Attached animation | `Animation`, `CumulativeAnimations`, `Animation.OfflineAction`, `Animation.TemporalAction`, `Animation.UseInvokerAsOwner`, `Animation.HideIfAttachedWith` | replace | — | `attach_effect.rs` | v1 | RFC 6b; F31 |
| Prevent death | `Unkillable` | replace | — | `attach_effect.rs::apply_unkillable_clamp` | v1 | RFC 6b; F32 |
| Transform into a type | no key | refuse | motion group | `type_conversion.rs::convert_techno_type`; effect: **no owner yet** | v1? | RFC 6b; F33 |
| Replace a weapon | no key | replace | — | **no owner yet** | v1? | RFC 6b; F34 `OverrideWeapon` |
| Motion (thrown, pushed) | no key | refuse | motion group | **no owner yet** (added to movement, never written as coordinates) | later | RFC 6b; F35 `Force`; Rc: MoveCtrl |
| Teleport | no key | refuse | motion group | **no owner yet** | v1? | RFC 6b; F36 |
| Damage taken multiplier | `ArmorMultiplier` (+ `.AllowWarheads`, `.DisallowWarheads`, `.Chance`, `.Delay`, `.AffectsHouse`, `.HitAnim`) | stack | — | `attach_effect.rs::attach_effect_armor_multiplier` | v1 | RFC 6b; F37 `Status` |
| Firepower multiplier | `FirepowerMultiplier` | stack | — | `stat_multipliers` | v1 | RFC 6b; F37 |
| Speed multiplier | `SpeedMultiplier` | stack | — | `stat_multipliers` | v1 | RFC 6b; F37 |
| Rate of fire multiplier | `ROFMultiplier`, `ROFMultiplier.ApplyOnCurrentTimer` | stack | — | `stat_multipliers` | v1 | RFC 6b; F37 |
| Range | `WeaponRange.Multiplier`, `WeaponRange.ExtraRange`, `.AllowWeapons`, `.DisallowWeapons` | stack | — | `range_with_attach_effects` | v1 | RFC 6b; F37 |
| Target priority multiplier | no key | stack | — | **no owner yet** (target scan `src/sim/world/techno_ai/scan_batch.rs`) | v1? | RFC 6, 6b (P13); paired with `TargetChanged` (2.3) |
| Cloak | `Cloakable`; `ForceDecloak` | replace | — | `attach_effect.rs` + `cloak_disguise.rs` | v1 | RFC 6b; F37 |
| Immunity to a warhead or attack type | partly `ArmorMultiplier.AllowWarheads` with multiplier 0 (unverified); no immunity key | replace | — | **no owner yet** | v1? | RFC 6b; F40 `ImmuneTo` |
| Cannot fire | `DisableWeapons` | replace | — | `weapon_attach_effects_allow` | v1 | RFC 6b; F41 `Silence` |
| No target search | no key | replace | — | **no owner yet** | v1? | RFC 6b; F42 |
| Damage modifier (fixed add, forced value) | no key (`ReflectDamage.*` reflects, not modifies) | stack | — | **no owner yet** | v1? | RFC 6b; F43 |
| Cannot be selected | no key | replace | — | **no owner yet** (client and command validation) | v1? | RFC 6b; F45 |
| Ignores player orders | no key | replace | — | **no owner yet** (command admission `src/sim/command.rs`) | v1? | RFC 6b; F47 |
| Not a legal target | no key | replace | — | **no owner yet** (target scan) | v1? | RFC 6b; F48 |
| House power off | no key | replace | — | **no owner yet** (`PowerState::start_blackout` is house-wide and timed) | v1? | RFC 6b; F49 |
| Critical hits | `Crit.Multiplier`, `Crit.ExtraChance`, `Crit.AllowWarheads`, `Crit.DisallowWarheads` | stack | — | `attach_effect.rs` | v1 | Ph |
| Reflect damage | `ReflectDamage`, `ReflectDamage.*` | stack | — | `apply_attach_effect_reflect_damage` | v1 | Ph |
| Revenge weapon | `RevengeWeapon`, `RevengeWeapon.*` | stack | — | `apply_attach_effect_revenge_weapons` | v1 | Ph |
| Laser trail | `LaserTrail.Type` | replace | — | `attach_effect.rs` | v1 | Ph |
| Penetrates force shield | `PenetratesForceShield` | replace | — | `attach_effect.rs` | v1 | Ph |

Spreadsheet lasting effects not taken as effects:

| Row | Effect | Where it goes |
| --- | --- | --- |
| F38 `Info`, F44 `ShowCSF`, F50 `ShowButton` | text, values, buttons over an object | client display properties (RFC 6b, 10; section 11) |
| F39 `Stand` | stand-in object | not in the RFC; module code with create/remove |
| F46 `SearchOnAllies` | search targets among allies | not in the RFC; candidate for `later` |
| F51 `Recorder`, F52 `Operator`, F53 recorder use | recorders and operators | not in the engine (RFC 6a) |
| F54 `Mark.Names` | marks | section 6 |

## 6. Marks

A mark is a text marker an object or a house carries; either may carry any
number (RFC 6b, Terminology). House marks use the same calls and tier as
object marks. Actions and effects add and remove them; conditions test
them. Owner: **no owner yet** (gap G-M1): a side table keyed by handle
(RFC 15), saved and hashed, cleared by the object's removal. Calls take a
`MarkId` from `Mark.id(name)` (see [IDs](#ids)); only simulation code
registers mark names, and the registry is simulation state, saved and
hashed.

```ts
// namespace Techno, and the same in namespace House (the handle comes first):
addMark(t: Techno, m: MarkId, value: i32, visibility: Visibility): Status;
removeMark(t: Techno, m: MarkId): Status;
hasMark(t: Techno, m: MarkId): bool;
markValue(t: Techno, m: MarkId): i32;                // NotFound when absent
setMarkValue(t: Techno, m: MarkId, value: i32): Status;
marks(t: Techno, out: IdBuffer): i32;    // order: the order the marks were added
```

| Entry | Semantics | Tier | Sources |
| --- | --- | --- | --- |
| `addMark` | adds `m` with its value and visibility; adding it again keeps one mark and sets its value and visibility | v1 | RFC 6b; F54 `Mark.Names` |
| `removeMark` | removes `m`; absent is `Ok` | v1 | RFC 6b |
| `hasMark` | test by ID; names compare case-insensitively when resolved | v1 | RFC 6b; C10, C11 |
| `markValue`, `setMarkValue` | read or change the value of a mark the object or house carries; `NotFound` when it carries none | v1 | RFC 6b (P3) |
| `marks` | writes the object's or house's marks into `out` | v1 | RFC 6c ("marks" in object reads) |
| Effect-carried marks | an effect type lists marks the object carries while the effect is active (new property, e.g. `Marks=`; name unverified) | v1? | F54 "持有带它的行为就获得这个标签" |
| Mark value and visibility | the `value` and `visibility` of `addMark`, with `markValue` and `setMarkValue`; **adopted in RFC 0.0.5** (P3) | v1 | Rc: rules params with access levels |
| House marks | the four entries above on `House`; `House` is also a target of the `AddMark` and `RemoveMark` atomic actions (4.1); **adopted in RFC 0.0.5** (P3) | v1 | Rc: `SetTeamRulesParam` |

Client modules read marks only of objects they can see, and of those only
the marks whose visibility level admits their player (RFC 6b, P3).

## 7. Queries

Queries are read-only (RFC 6c). Results come in object creation order
(**adopted in RFC 0.0.5**, P6), taken from the creation serial the engine
keeps (RFC 15), and are written into a buffer the module provides: a query
returns how many results it wrote, and large results are paged (see
[Query results](#query-results)). In client modules every query is filtered by
what the local player can see (RFC 10).

```ts
class Filter {
  house: House = NO_HANDLE;         // NO_HANDLE: any
  relationTo: House = NO_HANDLE;  relation: i32 = -1;   // Relation, -1 any
  category: i32 = -1;               // Category, -1 any
  type: TechnoTypeId = NO_ID;       // NO_ID: any
  mark: MarkId = NO_ID;             // NO_ID: any
}
// results: HandleBuffer or IdBuffer (Conventions)
```

### 7.1 Objects

| Function | Returns | Read owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `objects.inCell(c: Cell, f: Filter, out: HandleBuffer)` | `i32` count | cell occupancy (owner unverified) | v1 | RFC 6c |
| `objects.inRadius(at: Coord, leptons: i32, f: Filter, out: HandleBuffer)` | `i32` count | spatial index (owner unverified) | v1 | RFC 6c; F17; Rc: `GetUnitsInCylinder` |
| `objects.inRect(r: Rect, f: Filter, out: HandleBuffer)` | `i32` count | same | v1 | RFC 6c; Rc: `GetUnitsInRectangle` |
| `objects.all(f: Filter, out: HandleBuffer)` | `i32` count | entity store; `HouseTracking` for per-house types | v1 | RFC 6c |
| `objects.nearest(at: Coord, leptons: i32, f: Filter)` | `Techno` (`NO_HANDLE` when none) | same | v1? | Rc: `GetUnitNearestEnemy` |
| `objects.projectilesInRadius(at: Coord, leptons: i32, out: HandleBuffer)` | `i32` count | projectile store | v1? | RFC 6c (category projectile) |
| `techno.type()` | `TechnoTypeId` | `ObjectType` id | v1 | RFC 6c |
| `techno.owner()` | `House` | entity owner | v1 | RFC 6c |
| `techno.health()`, `strength()`, `healthPercent()` | `i32` | `combat/object_health.rs::Health`; `Strength` | v1 | RFC 6c, example |
| `techno.coord()`, `cell()` | `Coord`, `Cell` | entity position | v1 | RFC 6c |
| `techno.mission()` | `Mission` | `src/sim/mission/state.rs` | v1 | RFC 6c |
| `techno.target()` | `Target` | target state (owner unverified) | v1 | RFC 6c |
| `techno.rank()`, `experience()` | `Rank`, `i32` | `combat/veterancy.rs` | v1 | RFC 6c |
| `techno.ammo()` | `i32` | `src/sim/ammo.rs` | v1 | RFC 6c |
| `techno.passengers(out: HandleBuffer)`, `transport()` | `i32` count, `Techno` (`NO_HANDLE` when none) | `src/sim/passenger.rs::PassengerCargo::passengers` | v1 | RFC 6c |
| `techno.effects(out: HandleBuffer)`, `marks(out: IdBuffer)` | `i32` counts | sections 5, 6 | v1 | RFC 6c |
| `techno.disguise()` | `DisguiseInfo \| null` (type ID, house) | `src/sim/cloak_disguise.rs::forced_disguise_type`, `disguise_house` | v1 | sim reads the true state |
| `techno.isCloaked()` | `bool` | `cloak_disguise.rs::is_fully_cloaked` | v1? | |
| `techno.layer()`, `landType()` | enums | entity layer; map | v1 | RFC 6a conditions |
| `projectile.type()`, `coord()`, `source()`, `target()`, `data()` | values; `type()` is a `ProjectileTypeId` | `src/sim/projectile` | v1? | RFC 9 |
| `objects.canFireAt(t: Techno, w: i32, at: Target)`, `inRange(...)` | `bool` | **no owner yet** (structured legality probe) | v1? | **adopted in RFC 0.0.5** (P15); RFC 6c probes; Rc: `GetUnitWeaponTestTarget` |

### 7.2 Houses

| Function | Returns | Read owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `house.name()`, `country()`, `side()` | `string`, `string`, `i32` | `src/sim/house_state.rs::HouseState` (`name`, `country`, `side_index`) | v1 | RFC 6c |
| `house.money()` | `i32` | `Economy::available_money` | v1 | RFC 6c |
| `house.powerOutput()`, `powerDrain()`, `isLowPower()` | `i32`, `bool` | `src/sim/power_system.rs::PowerState::total_output`, `total_drain`, `is_low_power` | v1 | RFC 6c |
| `house.buildings(f, out: HandleBuffer)` / `count(type: TechnoTypeId)` | `i32` count, `i32` | `src/sim/house_tracking.rs::HouseTracking` | v1 | RFC 6c |
| `house.canBuild(type: TechnoTypeId)` | `bool` (prerequisites met) | `src/sim/production/can_build.rs::can_build`, `requirements_met`, `owns_prerequisites` | v1 | RFC 6c |
| `house.techLevel()` | `i32` | `HouseState::tech_level` | v1 | RFC 6c |
| `house.isAlly(other)`, `allies(out: HandleBuffer)`, `enemies(out: HandleBuffer)` | `bool`, `i32` counts | `src/sim/house_diplomacy.rs` | v1 | RFC 6c |
| `house.isDefeated()`, `isHuman()` | `bool` | `HouseState.is_defeated`, `is_human` | v1 | RFC 6c |
| `house.superWeapon(type: SuperWeaponTypeId)` | `SuperWeapon` (`NO_HANDLE` when the house does not hold it; reads: ready, charge) | `src/sim/superweapon/mod.rs::HouseSuperWeapons` | v1? | |
| `house.stats()` | lost/destroyed counts | `MatchStatistics` | v1? | E15, E16 |
| `scenario.houses(out: HandleBuffer)` | `i32` count, order: house array order | `src/sim/houses.rs::Houses` | v1 | RFC 6c |

### 7.3 Map

| Function | Returns | Read owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `map.size()` | `Rect` | map | v1 | RFC 6c |
| `map.landType(c: Cell)` | `LandType` | terrain semantics (`src/rules/terrain_rules.rs`) | v1 | RFC 6c |
| `map.height(c: Cell)` | `i32` | cell level | v1 | RFC 6c |
| `map.overlay(c: Cell)`, `ore(c: Cell)` | `OverlayTypeId` (`NO_ID` when none); amount | overlay and tiberium state | v1 | RFC 6c |
| `map.passable(c: Cell, zone: i32)` | `bool` | movement zone check (owner unverified) | v1 | RFC 6c |
| `map.isVisibleTo(c: Cell, h: House)` | `bool` | `src/sim/vision/mod.rs::is_cell_revealed(owner, rx, ry)` | v1 | RFC 6c |
| `Cell.waypoint(n: i32)` | `Cell` that carries waypoint number `n` (`x` and `y` set when the map has it); the call that takes the cell resolves it, and a missing waypoint gives `NotFound` (`Trigger.create`: `NO_HANDLE` with `NotFound`, 4.1); waypoints do not move, so a trigger keeps the cell it was created with | map waypoints | v1 | RFC 6b, 6c |
| `map.cellTag(c: Cell)` | `Tag` (`NO_HANDLE` when none) | `TriggerRuntime.cell_tags` | v1 | RFC 6c |
| `map.canPlace(type: TechnoTypeId, c: Cell, h: House)` | `bool` | `src/sim/build_site.rs::can_place_building_at` | v1? | **adopted in RFC 0.0.5** (P15); Rc: `TestBuildOrder` |
| `map.closestPlaceable(type: TechnoTypeId, near: Cell, h: House)` | `Cell \| null` | **no owner yet** (`src/sim/ai_base_site.rs` has an AI form) | v1? | P15; RFC 6c probes; Rc: `ClosestBuildPos` |

### 7.4 Rules

| Function | Returns | Owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `rules.getString(section: string, key: string)` | `string \| null` | processed rules (`ProcessedRulesLayers::ini()` projection) | v1 | RFC 6c |
| `rules.getInt(section, key, fallback: i32)` | `i32` | `src/rules/ini_value.rs` readers | v1 | RFC 6c |
| `rules.getBool(section, key, fallback: bool)` | `bool` | same | v1 | RFC 6c |
| `rules.getFixed(section, key, fallback: Fixed)` | `Fixed` | same, converted to I16F16 | v1 | RFC 6c |
| `rules.getList(section, key)` | `StringList` | same | v1 | RFC 6c |
| `rules.types(list: string)` | `StringList` (e.g. `"InfantryTypes"`) | `RuleSet` registries | v1 | RFC 6c |

A parsed value is the value the engine uses, including Ares and Phobos keys
when those are on (RFC 8). A key the engine never reads still answers from
the merged text.

### 7.5 Scenario

| Function | Returns | Owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `ctx.frame()` | `i32` | simulation frame | v1 | RFC 6c |
| `scenario.difficulty()` | `Difficulty` | `TR::trigger_difficulty` / scenario | v1 | RFC 6c |
| `scenario.gameMode()` | `string` | session (owner unverified) | v1 | RFC 6c |
| `scenario.playerCount()` | `i32` | `Houses` | v1 | RFC 6c |
| `scenario.timer.remaining()` | `Frames` | `scenario_countdown.rs` | v1 | E14 |
| `ctx.global(i).get()`, `ctx.local(i).get()` | `bool` | `TR::global_variable_is_set`, `local_variable_is_set` | v1 | RFC example |
| `Trigger.find(id)`, `Tag.find(id)` | handles | `TR` definitions | v1 | RFC 7 |

### 7.6 Query as house

```ts
// sim:
ctx.asHouse(h: House): View;   // View has objects, map, houses, filtered like a client of h
```

Runs the same queries with the visibility filter of house `h`, so module AI
can play fair and mission logic can ask "does house H know about X". Default
simulation queries stay full knowledge. Tier `later`, **not adopted in RFC 0.0.5**
(P4). Owner: the client visibility filter (G-C3). Rc: `CallAsTeam`.

### 7.7 Client results

In client modules (RFC 10):

- objects under shroud are absent, and so is an enemy object under fog that
  the player cannot see now: the engine keeps no last-seen copies (RFC 10,
  P17);
- an enemy disguised object reads as its disguise: `type()` returns the
  disguised type and `owner()` the disguised house, using
  `src/sim/cloak_disguise.rs::is_disguised_to`, `disguise_house`,
  `disguise_hides_house_from`. Owner of the filter: **no owner yet** (G-C3).
  Rc: decoy defs returned to non-allies;
- a cloaked enemy is absent unless detected;
- spectators see what the spectator sees, the whole map when the room allows.

## 8. Overrides

Overrides return a result and cannot run actions (RFC 5). They may read state,
keep their own state in module memory and draw from the module's random
stream. They are named after game concepts. Across modules they **chain**:
the mod's module computes a result, the map's module receives it and may
change it, and the engine uses the final value (RFC 2). A disabled module's
override is skipped; for the call in which a module failed, the engine uses
the previous link's value, or the native value (RFC 3). Every row is a v1
candidate: two or three of them ship in version 1 (open question 11).

Overrides run only inside the simulation's phases, never from local code
(the sidebar, the cursor, the interface) and never from parallel code. Each
override is called at a fixed step of the frame, its call site in the
Frequency column: the engine gathers the step's queries, calls the module for
them in order of the subject's creation serial, then a sequence number per
object, and then applies the results (RFC 5). Local code reads the result the
simulation last computed.

Combination (RFC 2, adopted from P1): each link receives the native value and
the previous link's result, a later link may lift an earlier link's veto, and a
value override returns an explicit "handled" flag.

Overrides bind to types, never globally: a weapon, warhead or TechnoType names
the override in INI, and the engine calls the module only for those types
(**adopted in RFC 0.0.5**, P12; the binding key name is open question 5).
Query and result classes (`DamageQuery`, `ShotQuery`, `ShotPlan`, `OrderQuery`,
`DeathQuery`, `DeathResult`, `TargetBatch`) carry handles and IDs (warhead,
weapon, type), never names; batches are written into module memory and read
back from it (RFC 15).

| Override | Signature | Frequency | Batching | Decision owner | Tier | Sources |
| --- | --- | --- | --- | --- | --- | --- |
| `Override.Damage` | `(ctx: Ctx, d: DamageQuery, current: i32): DamageResult` (damage, `handled`) | per damage packet on a bound warhead or receiver type, at the damage step | none (one call per packet; low count expected for bound types) | `src/sim/combat/damage/receive.rs::receive_damage` | v1? | RFC 6 L3; Rc: `UnitPreDamaged`; LUS `HitByWeapon` |
| `Override.TargetChoice` | `(ctx: Ctx, batch: TargetBatch): void` — writes one choice (or "keep native") per searcher | once per bound weapon type per frame, with every searcher of that type and its native candidates, after the parallel target scan | **batched**, never per candidate | target scan `src/sim/world/techno_ai/scan_batch.rs` | v1? | RFC 6 L3; Rc: `AllowWeaponTarget` (per-candidate hot spot, #3087) |
| `Override.ProductionAllowed` | `(ctx: Ctx, house: House, type: TechnoTypeId, current: bool): bool` | once per frame for each house and bound type, at a fixed step of the simulation; the sidebar and the AI read the cached result and never call the module | none | `src/sim/production/can_build.rs::can_build` | v1? | RFC 6 L3; Rc: `AllowUnitCreation` |
| `Override.Fire` | `(ctx: Ctx, shot: ShotQuery, plan: ShotPlan): bool` — `true` takes over the shot: the engine creates the projectiles the module wrote into `plan` (each with its type, module data and parameters) at once, at the moment the native shot would leave, and no native projectile; an empty plan fires nothing; `Fire` is still raised, with `projectile` `NO_HANDLE` | per shot of a bound weapon, at the fire step | none | FireAt steps `src/sim/combat/world_fire.rs`; aircraft `fire_at.rs` | v1? | RFC 6 L3, 9 |
| `onLaunch` | `(ctx: LaunchCtx, p: Projectile): Path \| null` | once per projectile of a bound type | none | module arm of `ProjectileTrajectory` (RFC 9): **no owner yet** | later | RFC 9 level 3 |
| `onTrajectoryFrame` | `(ctx: Ctx, type: ProjectileTypeId, bullets: BulletBatch): void` | once per bound projectile type per frame | **batched** per type (L4) | same | later | RFC 6 L4, 9 level 4 |
| `Override.CommandAllowed` | `(ctx: Ctx, t: Techno, order: OrderQuery, current: bool): bool` | per player or AI order for a bound type, when the simulation executes the order; the cursor's local preview does not call it | none | command admission `src/sim/command.rs` (function unverified) | v1? | **adopted in RFC 0.0.5** (P13); Rc: `AllowCommand` |
| `Override.Death` | `(ctx: Ctx, t: Techno, q: DeathQuery): DeathResult` (death weapon, survivors, wreck) | per death of a bound type, in the death chain | none | death chain `combat/record_kill.rs` and survivor rules (unverified) | v1? | **adopted in RFC 0.0.5** (P13); LUS `Killed -> wreckLevel` |

The overrides of the RFC's examples (target choice, damage, production
permission, taking over a weapon's shot) are covered. "Prefer data plus an
event" for target priority (a priority multiplier as an attached effect and a
`TargetChanged` event) is **adopted in RFC 0.0.5** (P13, RFC 6).

## 9. AI

Computer houses keep their AI (RFC 6d). Modules create task forces and team
types, assign team scripts, start and disband teams, and switch AI triggers,
through the existing owners in `src/sim/ai_team_creation.rs` and `TSV`.
All entries are `sim`.

### 9.1 Functions

| Function | Semantics | Owner | Tier | Sources |
| --- | --- | --- | --- | --- |
| `ai.defineTaskForce(id: string, d: TaskForceDef): Status` | add or replace a task force | `TeamScriptVm::register_task_force` | v1 | RFC 6d |
| `ai.defineScript(id: string, lines: ScriptLine[]): Status` | add or replace a team script (max 50 lines) | `TeamScriptVm::register_script` | v1 | RFC 6d |
| `ai.defineTeamType(id: string, d: TeamTypeDef): Status` | add or replace a team type | `TeamScriptVm::register_team_type` | v1 | RFC 6d |
| `ai.defineAiTrigger(id: string, d: AiTriggerDef): Status` | add or replace an AI trigger (rewrites its running weight to the start weight) | `TeamScriptVm::register_ai_trigger` | v1 | RFC 6d |
| `ai.createTeam(teamType, house)` | start a team (Max gate) | `src/sim/team_script_vm/reinforcements.rs::Simulation::create_team` | v1 | RFC 6d; A4 |
| `ai.createTeamUnlimited(teamType, house)` | start a team without the Max gate | `Simulation::construct_team_unlimited` | v1? | S18 path |
| `team.disband()` | destroy the team, record the AI trigger outcome, release members | `TSV/membership.rs::destroy_team` | v1 | RFC 6d; A5 |
| `team.addMember(t: Techno)` / `removeMember(t: Techno)` | join or leave (runs `team_can_add`) | `team_add_member`, `team_remove_member` | v1? | |
| `team.setScript(script: ScriptTypeId)` | change a live team's script | **no owner yet** (only S17 writes it inline) | v1 | RFC 6d "assign AI team scripts" |
| `team.jumpTo(line: i32)` | move a team's cursor | **no owner yet** (only S6 inline, and test-only helpers) | v1? | |
| `ai.setAiTriggerEnabled(trigger: AiTriggerId, on: bool)` | enable or disable one AI trigger | **no owner yet** (`enabled` is set only at load) | v1 | RFC 6d |
| `ai.setAiTriggerWeights(trigger: AiTriggerId, start, min, max)` | change weights at run time | **no owner yet** (`ai_trigger_records` private) | later | |
| `house.setAiTriggersActive(on)` | house-wide switch | `HouseState::set_ai_triggers_active` | v1 | A74, A75 |
| `house.setTeamRatio(kind, v)` | house ratios | `HouseTeamCreation::set_trigger_ratio` and siblings | v1 | A76–A79 |
| reads: `ai.teams(out: HandleBuffer)`, `team.type()`, `team.script()`, `team.cursor()`, `team.members(out: HandleBuffer)`, `team.succeeded()` | | `TeamScriptVm::teams_in_order`, `TeamScriptState` getters | v1 | |

### 9.2 Definitions

Definitions are setup calls and take names; run-time calls take the IDs of
`ai.teamTypeId`, `ai.scriptTypeId` and `ai.aiTriggerId` (see [IDs](#ids)).

```ts
class TaskForceEntry { count: i32; type: string; }
class TaskForceDef { entries: TaskForceEntry[]; group: i32 = -1; }   // at most 6 entries
class ScriptLine { action: i32; argument: i32; }                     // at most 50 lines
class TeamTypeDef {          // keys of [TeamTypes] sections; defaults from the reader
  name: string | null = null;     // default: the ID
  script: string; taskForce: string;
  priority: i32 = 7; max: i32 = -1; group: i32 = -1;
  house: string | null = null; veteranLevel: i32 = 1;
  waypoint: i32 = -1; transportWaypoint: i32 = -1;
  isBaseDefense: bool = false; suicide: bool = false; aggressive: bool = false;
  useTransportOrigin: bool = false; full: bool = false; whiner: bool = false;
  autocreate: bool = false; areTeamMembersRecruitable: bool = true;
  reinforce: bool = false; avoidThreats: bool = false; onTransOnly: bool = false;
  recruiter: bool = false; annoyance: bool = false; guardSlower: bool = false;
  transportsReturnOnUnload: bool = false; mindControlDecision: i32 = 0;
  droppod: bool = false; onlyTargetHouseEnemy: bool = false; loadable: bool = false;
  tag: string | null = null;
}
class AiTriggerDef {         // the 18 tokens of an [AITriggerTypes] row
  name: string; primaryTeam: string | null; owner: string | null;   // null: <all>
  condition: i32 = -1; objectType: string | null = null;
  amount: i32 = 0; compare: Compare = Compare.Less;   // comparator bytes 0..8
  startWeight: i32; minWeight: i32; maxWeight: i32;
  multiplayer: bool; side: i32 = 0; secondaryTeam: string | null = null;
  easy: bool = true; normal: bool = true; hard: bool = true;
}
```

Fields come from `src/rules/team_ai_ini.rs` and
`TSV/registry_install.rs::resolve_ini_definitions`. `Whiner` is read but has
no consumer. `TechLevel`, `Prebuild`, `LooseRecruit` and `IonImmune` are in
retail AIMD.INI but not read, so they are not fields. AI trigger tokens 3, 11
and 13 are not consumed and are not fields. AI trigger conditions: -1 always,
0 enemy owns N of the type, 1 house owns, 2 enemy power surplus below 100,
3 below 0, 4 enemy money compare, 5 own Iron Curtain nearly ready, 6 own
Chronosphere nearly ready, 7 civilian-side house owns (`condition_holds`).

### 9.3 Team script actions

`ScriptLine.action` takes these numbers. Dispatch:
`TSV/actions.rs::team_run_action`. All `v1` as data. "A" =
`TSV/actions.rs`, "SA" = `super_actions.rs`, "FF" = `follow_friendlies.rs`,
"SH" = `scout_houses.rs`. The atomic analogue column names the module
function a module would call to do the same to one object.

| # | `ScriptAction` | Argument | Rust | Atomic analogue |
| --- | --- | --- | --- | --- |
| 0 | `AttackQuarry` | quarry (2..11) | A `team_action_attack_quarry` | `setMission(Hunt)` |
| 1 | `AttackWaypoint` | waypoint | A `team_action_attack_waypoint` | `setMission(Attack, cell)` |
| 2 | `GoBerserk` (conv.) | — | inline no-op, never finishes | — |
| 3 | `MoveToWaypoint` | waypoint | A `team_action_move_to_waypoint` | `setMission(Move, cell)` |
| 4 | `MoveToCell` | packed cell (x = arg%128, y = arg/128) | inline | `setMission(Move, cell)` |
| 5 | `Guard` | `arg*15` frames | inline | `setMission(Guard)` |
| 6 | `JumpToLine` | 1-based line | inline | `team.jumpTo` |
| 7 | `PlayerWins` | — | inline `request_player_outcome(Victory)` | `scenario.endFor` |
| 8 | `Unload` | 1, 2, 3 | A `team_action_unload` | `removePassenger` |
| 9 | `Deploy` | — | A `team_action_deploy` | — |
| 10 | `FollowFriendlies` | — | FF `team_action_follow_friendlies` | — |
| 11 | `DoMission` | mission id | A `team_action_do_mission` | `setMission` |
| 12 | `SetGlobal` | index | inline `set_trigger_variable` | `ctx.global(i).set(true)` |
| 13 | `IdleAnimation` | anim | inline `foot_play_idle_animation` | — |
| 14 | `LoadOntoTransport` | — | A `team_action_load_onto_transport` | `addPassenger` |
| 15 | `SpyOnBuildingAtWaypoint` | waypoint | A `team_action_spy_waypoint` | — |
| 16 | `PatrolToWaypoint` | waypoint | A `team_action_patrol_waypoint` | `setMission(Patrol)` |
| 17 | `ChangeScript` | ScriptType ordinal | inline | `team.setScript` |
| 18 | `ChangeTeam` | TeamType ordinal | A `team_action_change_team` | — |
| 19 | `Panic` | — | inline `infantry::apply_panic_force` | — |
| 20 | `ChangeHouse` | country index | inline `change_owner_with_rules` | `setOwner` |
| 21 | `Scatter` | — | inline `scatter` | — |
| 22 | `GoToNearbyShroud` | — | inline `nearby_shrouded_cell`; refused outside single-player | — |
| 23 | `PlayerLoses` | — | inline `request_player_outcome(Defeat)` | `scenario.endFor` |
| 24 | `PlaySpeech` (conv.) | — | inline: only advances | — |
| 25 | `PlaySound` | SOUNDMD ordinal | inline `SimSoundEvent::VocCenteredIndex` | `sound.play` |
| 26 | `PlayMovie` | movie | inline `TriggerEffect::FullscreenMovie` | `scenario.playMovie` |
| 27 | `PlayMusic` | theme | inline `TriggerEffect::MusicTheme` | `scenario.playTheme` |
| 28 | `ReduceOre` | — | inline `reduce_tiberium_in_radius` | `map.reduceOre` |
| 29 | `BeginProduction` | — | inline `enable_scripted_production` | `house.beginProduction` |
| 30 | `FireSale` | — | inline `set_state_four` | `house.fireSale` |
| 31 | `SelfDestruct` | — | inline `commit_direct_damage_receiver` | `kill` |
| 32 | `IonStormStart` | duration | inline `lightning_storm::start` | `weather.startIonStorm` |
| 33 | `IonStormEnd` | — | inline `lightning_storm::request_end` | `weather.stopIonStorm` |
| 34 | `CenterCamera` | velocity | inline `TriggerEffect::CameraAtCoord` | `scenario.centerCamera` |
| 35 | `ReshroudMap` | — | inline `reset_shroud_keeping_counters` | `house.reshroud` |
| 36 | `RevealMap` | — | inline `reveal_entire_map` | `map.revealAll` |
| 37 | `DeleteMembers` | — | inline `techno_limbo_with_context` + `uninit_with_context` | `remove` |
| 38 | `ClearGlobal` | index | inline `set_trigger_variable` | `ctx.global(i).set(false)` |
| 39 | `SetLocal` | index | inline | `ctx.local(i).set(true)` |
| 40 | `ClearLocal` | index | inline | `ctx.local(i).set(false)` |
| 41 | `Unpanic` | — | inline `apply_unpanic_force` | — |
| 42 | `ForceFacing` | facing | A `team_action_force_facing` | — |
| 43 | `WaitUntilFullyLoaded` | — | A `team_action_wait_until_loaded` | — |
| 44 | `TruckUnload` | — | inline `store_techno_type` | `convert` |
| 45 | `TruckLoad` | — | inline `store_techno_type` | `convert` |
| 46 | `AttackEnemyBuilding` | type `arg&0xFFFF`, mode `arg>>16` | A `team_action_attack_enemy_building` | — |
| 47 | `MoveToEnemyBuilding` | same | A `team_action_move_to_enemy_building` | — |
| 48 | `ScoutHouses` | — | SH `team_action_scout_houses` | — |
| 49 | `RegisterSuccess` | — | inline (advance, succeeded) | — |
| 50 | `Flash` | frames | inline `flash_members` | — |
| 51 | `PlayAnimation` | anim low16, loops high16 | A `team_action_play_animation` | `map.spawnAnim` |
| 52 | `TalkBubble` | frame | A `team_action_talk_bubble` | — |
| 53 | `GatherAtEnemyBase` | Ares: + `AISafeDistance` | A `team_action_gather_at_enemy_base` | — |
| 54 | `RegroupAtBase` | Ares: + `AIFriendlyDistance` | A `team_action_regroup_at_base` | — |
| 55 | `IronCurtain` | Ares: `SW.Group` | SA `team_action_iron_curtain` | `launchSuperWeapon` |
| 56 | `ChronoshiftToBuilding` | like 46 | SA `team_action_chronoshift(Building)` | `teleport` |
| 57 | `ChronoshiftToQuarry` | quarry | SA `team_action_chronoshift(Quarry)` | `teleport` |
| 58 | `MoveToOwnBuilding` | type, mode | A `team_action_move_to_own_building` | — |
| 59 | `AttackBuildingAtWaypoint` | waypoint | A `team_action_attack_waypoint_building` | — |
| 60 | `EnterGrinder` | — | inline `foot_enter_grinder` | — |
| 61 | `OccupyTankBunker` | — | inline `foot_enter_tank_bunker` | — |
| 62 | `EnterBioReactor` | — | inline `foot_enter_bio_reactor` | — |
| 63 | `OccupyBattleBunker` | — | inline `foot_enter_battle_bunker` | — |
| 64 | `GarrisonNearest` | — | inline `foot_enter_nearest_occupiable` | — |
| 65, 66 | — | — | not in code | — |
| 67 | `AresSeekVehicle` | — | Ares arm shared with 64; unreachable past the step gate | — |
| 68 | `AresConvert` | — | inline `convert_techno_type` (Ares only) | `convert` |

## 10. Rules access

### 10.1 `onRules` writes

`onRules` may change any key and add types. Writes go through the native INI
readers, clamps and post-read passes and are part of the lobby hash (RFC 8).
The writes form one more layer in the layered stack, in this order (RFC 2):
`RULESMD.INI`, `LANGRULE.INI`, the mode layer, the mod module's `onRules`, the
map layer, the map module's `onRules`. Per-pass clamps and find-or-allocate
behave as for INI text, and the engine processes the rules again after each
module layer. A type ID never changes once assigned: a later layer may add
types, with new IDs, but never invalidates an existing one. A failure in
`onRules` stops the match from starting (RFC 3).

```ts
class RulesWriter extends RulesReader {
  set(section: string, key: string, value: string): Status;   // value is INI text
  remove(section: string, key: string): Status;
  addType(list: string, id: string): Status;                  // e.g. ("InfantryTypes", "E7")
}
```

| Function | Owner | Tier | Sources |
| --- | --- | --- | --- |
| `set`, `remove`, `addType` | `src/rules/native_processing.rs::RulesLayerStack::push` then `process`; then `RuleSet::from_processed_rules`. Module layer: **no owner yet** (G-R1) | v1 | RFC 8 |
| reads inside `onRules` | the rules as processed after the earlier layers | v1 | RFC 2 (map sees the mod's result) |

### 10.2 Changes during a match

`rules.set("E1", "Speed", "6")` during a match parses the value like INI text,
takes effect from the next frame, invalidates dependent caches and is saved as
a rules overlay (RFC 8). Today the `RuleSet` is immutable in a match
(`src/sim/runtime.rs::SimResources`); an overlay owner does not exist (G-R2).
Each allowlisted key states what happens to objects already on the map
(RFC 8). The allowlist is open question 12.

**Candidates** (plain values read through `&RuleSet` at use, no derived copy
found):

| Section family | Key | Objects already on the map | Notes |
| --- | --- | --- | --- |
| Weapon | `Damage`, `ROF`, `Burst`, `MinimumRange` | take the new value at the next use | |
| Weapon | `Range` | as above | only when the projectile has `ROT>0`; with `ROT=0` it feeds `Speed` (onRules only) |
| Weapon | `Report`, `Anim` | as above | presentation |
| Warhead | `Verses` | as above | with Ares, check `install_ares_verses` |
| Warhead | `CellSpread`, `PercentAtMax`, `Wall`, `Wood`, `Sparky`, `Fire`, `Conventional`, `Rocker`, `InfDeath`, `AnimList` | as above | |
| TechnoType | `Cost`, `BuildTime`-type multipliers | computed on demand (`cost_of`, `type_cost`, `building_actual_cost`) | |
| TechnoType | `Speed` | converted on demand | check per-locomotor state (`movement/foot_speed.rs`, `drive_locomotion.rs` `raw_type_speed`) |
| TechnoType | `ROT`, `Points`, `Soylent`, `Crushable`, `Selfhealing`, `Explodes`, `VeteranAbilities`, `EliteAbilities` | as above | |
| TechnoType | `Power` | every owning house must be invalidated (`PowerState::invalidate`) | |
| Projectile | `Inaccurate`, `Arcing`, `Shadow` | new projectiles | `Arcing` with care (see onRules-only) |
| Globals | numeric `[General]`, `[CombatDamage]`, `[AudioVisual]` fields of `GeneralRules` | as above | frame-converted values need the conversion re-run |

**Copied into each object at construction** (allowlist only with a stated
rule for existing objects):

| Key | Copy site | Rule needed |
| --- | --- | --- |
| `Strength` | `src/sim/world/world_spawn/construction.rs` (`Health::new`) | keep points or keep ratio (`Health::set_fraction`) |
| `Sight` | `construction.rs` (`GameEntity.sight`) | update each object and its revealed area |
| `Ammo`, `InitialAmmo` | `src/sim/ammo.rs::TechnoAmmo::from_type` | `TechnoAmmo::change_type_capacity` exists |
| `Storage` | `construction.rs` (`Miner::new`) | update capacity |
| `Passengers`, `SizeLimit` | `construction.rs` (`PassengerCargo::new`) | update capacity; what if over |
| Phobos `AttachEffect.*` on the type | `attach_effect.rs::attach_initial_effects` | only new objects |

**onRules only** (derived caches or post-read passes):

| Key | Reason | Reference |
| --- | --- | --- |
| weapon `Range`, `[General] Gravity`, projectile `ROT`, `Floater`, Phobos projectile `Gravity` | rewrite weapon `Speed` for `ROT=0` projectiles | `native_processing.rs::postprocess_weapon_speeds` |
| `Primary`, `Secondary`, `Weapon{n}`, `EliteWeapon{n}`, `OccupyWeapon`, weapon `Warhead`/`Projectile` | resolved to handles stamped with `rules_serial` | `ruleset.rs::resolve_handles`; `src/sim/type_handle_table.rs` |
| `Prerequisite`, `PrerequisiteOverride`, `[General] Prerequisite*`, Ares `Prerequisite.List{n}` | parsed to building array indices | `read_prerequisite_lists`; `prerequisite.rs::parse_prerequisites` |
| `Owner`, `RequiredHouses`, `ForbiddenHouses` | country bitmasks | `object_type.rs::HouseMasks::derive` |
| `Factory` | `factory_map` | `ruleset.rs` |
| `Foundation`, `[AI] AIBaseSpacing`, `BuildConst`, `IsBaseDefense`, `UndeploysInto`, `Spawned`/`MissileSpawn` | geometry and base lists, copied per object by `TypeFacts::stamp` | `ruleset.rs::complete_building_type_ext`; `construction.rs` |
| `Spawns`, `SpawnsNumber`, mind-control and temporal weapon setup | managers built once | `init_capture_manager`, `init_temporal`, `init_spawn_manager` |
| `Armor` (name) | resolved to `ArmorId` | `armor.rs::armor_index`; `ares_armor.rs` |
| `Image`, art keys, `Sequence` | art and animation binding need assets | `install_art_data`, `rebuild_animation_sequences` |
| Ares `PoweredBy`, `PowersUnit`, `PoweredUnit` | cross-type fixups | `ValidateThings` in `ruleset.rs` |
| list sections, any key naming a new type | types are added only while the rules load; a new type gets a new ID and existing IDs never change (RFC 2) | `find_or_allocate`, `EXPLICIT_RULE_TYPE_FAMILIES` |
| `[Colors]`, country `Color`, `Side`, `ParentCountry` | color ramps, country/side projection | `refresh_country_side_projection`, `project_country_side_registry` |
| sound and EVA keys | bound to indexes after load | `bind_type_sound_references`, `bind_type_eva_references` |
| `TechLevel` | use the per-type runtime owner instead | `set_object_type_tech_level` (A106) |

Ares and Phobos keys: the allowlist states for each whether it may change
during a match (RFC 8); none is classified in this catalog yet.

## 11. Client API

All entries are `client` and P2 (`v1`) unless marked. The client never changes
game state directly: it issues standard player orders or sends a message
(RFC 1, 10).

### 11.1 Interface tree

The engine holds the tree, lays it out and draws it. Changes are batched per
render frame (RFC 10). Owner of the tree for modules: **no owner yet** (G-C1);
the existing layout reader is `src/ui/match_layout.rs` and
`src/ui/sidebar/layout_spec.rs` (`veraui.ini`).

```ts
class Ui {
  find(name: string): Node;        // NO_HANDLE when no node has that name
  create(n: NodeSpec): Node;
  remove(n: Node): Status;
  registerHotkey(name: string, defaultKey: string): Status;
  viewer(): House;                 // NO_HANDLE for a spectator
  isSpectator(): bool;
  lastStatus(): Status;            // as ctx.lastStatus(); Gone after a call on a NO_HANDLE or removed node
}
// namespace Node (a handle; it comes first). A call on a NO_HANDLE or removed node does nothing (Errors).
name(n: Node): string;
setLabel(n: Node, csfOrText: string): void;   // Label
setArt(n: Node, name: string): void;          // Art
setPosition(n: Node, x: i32, y: i32): void;
setSize(n: Node, w: i32, h: i32): void;
setVisible(n: Node, v: bool): void;
setItems(n: Node, items: string[]): void;     // List
setFill(n: Node, v: i32): void;               // Bar: how full the bar is
onClick(n: Node, enabled: bool): void;        // routes clicks to the module's onClick
```

| Operation | Tier | Sources |
| --- | --- | --- |
| `find` by name | v1 | RFC 10, example |
| `create`, `remove` | v1 | RFC 10, example |
| `setLabel`, `setArt`, `setPosition`, `setVisible` | v1 | RFC 10 |
| `setItems` | v1 | RFC 10 example |
| `setSize`, `setFill` | v1? | |
| `onClick`, hover, hotkeys | v1 | RFC 10 |
| data binding (a property bound to a published value) | later | Rc: RmlUi data models |

Widget types (open question 15). Client code uses the `veraui.ini` names
(`new Column(Hold.BottomLeft, parts)` mirrors `Type=Column`, `Hold=BottomLeft`,
`Parts=`). The
RFC example uses `Column`, `Text` and `List`; `veraui.ini` today has `Button`,
`Text`, `Row`, `Column`, `CameoGrid` (`docs/interface-layout.md`).

| Type | Source | Tier |
| --- | --- | --- |
| `Text` | RFC 10; `veraui.ini` | v1 |
| `Button` | `veraui.ini` | v1 |
| `Row`, `Column` | `veraui.ini`; RFC 10 example (`new Column`) | v1 |
| `CameoGrid` | `veraui.ini` | v1? |
| `List` | RFC 10 example (`setItems`) | v1? |
| `Image` | RFC 10 (`image`) | v1? |
| `Bar` (progress) | — | later |

### 11.2 INI layout keys

Static layout uses the syntax of `veraui.ini` (RFC 10). Keys that exist today
(`docs/interface-layout.md`):

| Key | Meaning |
| --- | --- |
| `Type` | `Button`, `Text`, `Row`, `Column`, `CameoGrid` |
| `Hold` | `TopLeft`, `Top`, `TopRight`, `Left`, `Middle`, `Right`, `BottomLeft`, `Bottom`, `BottomRight` |
| `X`, `Y`, `Width`, `Height` | position and size |
| `Parts` | children of a row or column |
| `Gap` | pixels between parts |
| `Label` | button words or a CSF label |
| `Art` | an SHP picture |
| `Command` | a keyboard command name (`StopObject`, `GuardObject`, ...) |
| `Value` | `Credits`, `Power`, `Drain`, `Name`, `Time`, `Kills`, `Losses` |
| `Tab`, `Columns`, `Rows` | cameo grids |

RFC 0.0.5 uses these keys in its example and adds the `List` part type and
`Label` for `Text` parts (P16). Where a mod and a map keep
layout sections is open question 15. Client code uses the same names:
`new Text(label)`, `Node.setLabel` and `Node.setArt` (RFC 10). `Value` keeps
only its INI meaning, the data source a part shows (`Credits`, `Power`, ...);
the fill of a `Bar` is set with `node.setFill`, and a number shown over an
object with `display(t).setNumber` (11.3).

### 11.3 Display properties per object

| Property | Owner | Tier | Sources |
| --- | --- | --- | --- |
| `display(t).setArt(name)` | render binding (**no owner yet**) | v1? | RFC 10 |
| `display(t).setPalette(name)` | **no owner yet** | v1? | RFC 10 |
| `display(t).setTint(color, intensity)` | **no owner yet** (sim tint is an effect, section 5) | v1? | RFC 10 |
| `display(t).addAnim(name)` / `removeAnim` | **no owner yet** | v1? | RFC 10 |
| `display(t).setVisible(v)` | **no owner yet** | v1? | RFC 10 |
| `display(t).setLabel(text)`, `setNumber(v)`, `addButton(name)` | **no owner yet** | v1? | RFC 6b, 10; F38 `Info`, F44 `ShowCSF`, F50 `ShowButton` |
| minimap visibility, map markers (point, line, text) | **no owner yet** | later | Rc: `SetUnitNoMinimap`, `MarkerAddPoint` |

Display properties affect only the local picture. Cloaking and other gameplay
properties belong to the simulation module (RFC 10).

### 11.4 Selection and orders

| Function | Owner | Tier | Sources |
| --- | --- | --- | --- |
| `client.selection(out: HandleBuffer)` | selection state in `src/app/input/` (unverified) | v1 | RFC 10 |
| `client.select(list: HandleBuffer, count: i32, append: bool)` | `Command::Select` (`src/sim/command.rs`) | v1 | RFC 10 |
| `client.order(o: OrderSpec)` | `Command` variants: `Move`, `MoveToObject`, `Stop`, `Attack`, `AttackMove`, `Guard`, `DeployMcv`, `UndeployBuilding`, `SetRally`, `QueueProduction`, `SuspendProduction`, `CancelProductionByType`, `PlaceReadyBuilding`, `SellBuilding`, `ToggleRepair`, `EjectBunker`, `UnloadPassengers`, `CaptureBuilding`, `PlantC4`, `LaunchSuperWeapon`, `ToggleInfantryDeploy`, `ForceAttackCell`, `SellWallAtCell` | v1 | RFC 10 (validated like any player input) |
| `client.sendMessage(data: Uint8Array): Status` | command stream: **no module variant yet** (G-C2); `Refused` on a spectator's machine | v1 | RFC 1, 10, 11; Rc: `SendLuaRulesMsg` |
| `client.broadcast(to: Audience, data: Uint8Array)` | **no owner yet** | later | **not adopted in RFC 0.0.5** (P5); Rc: `SendLuaUIMsg` |
| order rewrite hook (`CommandNotify`-style) | **no owner yet** | later | Rc: `CommandNotify`, `DefaultCommand` |

The engine fills in the sender of every message and limits size and rate
(RFC 11).

### 11.5 Camera, sound, storage, messages, log

| Function | Owner | Tier | Sources |
| --- | --- | --- | --- |
| `camera.center(at: Coord, speed: i32)` | app camera (the target of `TriggerEffect::CameraAtCoord`) | v1? | A48; Rc: `SetCameraTarget` |
| `camera.jump(at: Coord)` | same (`velocity: None`) | v1? | A112 |
| `camera.home()` | `TriggerEffect::HomeCamera` target | v1? | A145 |
| `sound.play(name)`, `sound.playAt(name, at)` | `src/audio/` (local) | v1 | RFC 10 |
| `sound.speech(eva)` | EVA playback (local) | v1 | RFC 10 |
| `storage.get(key: string): Uint8Array \| null` | **no owner yet** (G-C4); the store of the module's source: the mod's, or the map's own, also under a mod | v1 | RFC 10 |
| `storage.set(key: string, data: Uint8Array): Status` | **no owner yet**; `Refused` beyond the store's quota | v1 | RFC 10 |
| `messages.show(csfOrText: string, color: i32)` | `src/ui/messages.rs` (unverified entry) | v1 | RFC 10 |
| `ui.log(text: string)` | in-game console: **no owner yet** | v1 | RFC 10 |
| simulation notification receipt (`onNotify`) | **no owner yet** | v1 | RFC 1, 11 |

### 11.6 Client queries

The query functions of section 7 exist in the client with the same names and
the same buffers, filtered as in 7.7. `ui.viewer()` names the local house,
or is `NO_HANDLE` for a spectator (`ui.isSpectator()`). `Mark.id` in a client
module only looks names up ([IDs](#ids)).

## Gaps in the engine

Each gap is an API entry whose owner does not exist yet. The fix is always in
the existing owner's module (project contract); none of these is a second
implementation.

### Module host

- **G-H1** Module loader, validation (shape limits, NaN canonicalization by
  bytecode rewriting, `vera.actions` check), metering with the call-depth
  limit, memory cap reserved at instantiation, and disable path (RFC 3, 4):
  new, in core, outside `sim/` hot loops, reaching the Wasm runtime through
  one interface; the runtime is chosen per platform behind platform build
  features, so the core crate stays browser-compatible (RFC 14).
- **G-H2** Module phase in the simulation frame (open question 9):
  `src/sim/world/master_frame.rs` gains a phase that springs deferred chain
  triggers, then runs per module `onMessage`, `onEvents` and `onFrame`;
  `onFrame` scheduling.
- **G-H3** Module memory in snapshots and in the periodic full state hash,
  not in the per-frame multiplayer checksum (RFC 4, 15): save/load and
  `hash_state` paths.
- **G-H4** `ctx.log`, in-game failure messages and the error status of a
  failed atomic action run by a trigger (RFC 3, 14): a sim-side log line
  published as a `TriggerEffect`-like output, shown by the app.
- **G-H5** Simulation-to-client notifications (`ctx.notify`,
  `ctx.notifySpectators`, `onNotify`), delivered only to the player each is
  for, or to the spectators; the content is the module's own bytes, which the
  engine does not filter (RFC 11).
- **G-H6** The periodic full state hash covers the whole module memory,
  bounded by the memory cap, with the instrumentation counters and the
  module's random stream (RFC 4, 15); its cost is part of open question 4.
- **G-H7** Module call order: override calls and events raised inside
  parallel engine code are collected and issued after it, and override calls
  and delivered records are ordered by the subject's creation serial, then a
  sequence per object, never by loop order (RFC 5).
- **G-H8** Map minimum-version key (`[Basic] VeraMinVersion=`, name not
  final), read by the release before module support, which refuses a map that
  needs a later version with a message (RFC 11).

### Triggers, events and conditions

- **G-T1** Module and atomic trigger actions (`Action.wasm`,
  `Action.atomic`): new action numbers in `TR::execute_live_trigger_action`;
  `Action.wasm` queues a module call, and `Action.atomic` resolves its target
  and runs the 4.3 function at once, logging a failure (G-H4); the arguments
  of `Action.atomic`, including target kinds that can never fit, are checked
  when `Trigger.create` runs, and those of atomic actions written in INI at
  load (RFC 6a, 6b).
- **G-T2** Run-time trigger and tag creation from modules
  (`Trigger.create`, `Tag.create`, `Techno.setTag`, `cell.setTag`): today
  definitions come only from `RuntimeDefinitions::read_map`/`read_teams`; IDs
  that cannot collide with the map's (RFC 7).
- **G-T3** Type tag bindings (`Wasm.Tags`, and `Trigger.bindType`,
  `bindWeaponType`, `bindWarheadType` and `bindProjectileType` from modules)
  kept outside the `AttachedTag` slot as subscriptions in a side table keyed
  by handle (RFC 7, 15), and `Wasm.Tags` on weapon, warhead and projectile
  types for the weapon events (RFC 6a); one trigger instance per bound object
  with its own repeat state, the subject's owner as the trigger's house and
  the subject as tag holder.
- **G-T4** Condition list on a TriggerType, checked natively before the
  spring (RFC 6a).
- **G-T5** Polled event list with an interval per trigger (RFC 6a), beside
  the Logic tags; a minimum interval, edge firing, and checks of different
  objects spread over different frames by creation serial modulo the
  interval (RFC 15).
- **G-T6** Raise hooks for each new delivered event of 2.3, at the owners
  named there; raised only for bound types (G-T10); after `HouseTransfer`,
  the transfer re-checks that it is still valid.
- **G-T7** Event payload carried from the raise site into the module's
  `EventRecord` (section 2); today `dispatch_object_tag_event` carries actor
  and source only.
- **G-T8** Original events 2 (`SpiedBy`) and 3 (`ThievedBy`) have no raise
  site (pre-existing; not caused by modules).
- **G-T9** Batched queue: per module, the frame's array of fixed-size
  `EventRecord`s, one per queued module action with the payload of the event
  that sprang its trigger, written when a trigger with a module action
  springs, ordered by subject creation serial (G-H7), and
  handed over in one call per module per frame, before `onFrame`, up to a
  per-frame cap beyond which records move to the next frame; it drops queued
  actions whose trigger was disabled or removed, and every queued action once
  the game has ended (RFC 5, 15).
- **G-T10** Per-type module flag on object, weapon, warhead and projectile
  types, set when a module binds the type, checked first at each event site
  so that nothing else runs for unbound types (RFC 15).
- **G-T11** Per-frame chain depth counter in `TR`, where a trigger springs:
  counts the depth of a chain of triggers sprung at once within a frame when
  the chain includes a module trigger (one with an `Action.wasm` action, one
  created by a module, or one bound through `Wasm.Tags`), an event VERA adds
  or an atomic action, and moves a trigger that would exceed the limit to
  the start of the next frame's module phase, saved and hashed while it
  waits; a chain of only original events and original actions is not
  limited and runs as in the original (RFC 5, Goal 5; the limit is open
  question 18).

### Actions

- **G-A1** Owners that read a waypoint index (actions 8, 17, 18, 41, 42, 43,
  58, 59, 63, 80, 88, 89, 90, 94–96, 99, 101, 107–109, 116, 125) need a cell
  entry; the waypoint form calls it.
- **G-A2** Fire a weapon from an object now (`techno.fireWeapon`): FireAt is
  reached only from the fire update (`combat/world_fire.rs::visit_fire`).
- **G-A3** Teleport one object to a cell with a blocked-cell rule:
  `chronosphere.rs::warp_object` is private; no general owner.
- **G-A4** Clear target: only per-locomotor trait methods exist.
- **G-A5** Clear disguise as a transaction (publication, visibility):
  `DisguiseRuntime::clear_unit` is a field operation.
- **G-A6** Set health with publication at Simulation level (health category
  events, death at 0): only `Health::set`/`heal`.
- **G-A7** Release mind control for one object: original-owner records exist
  in `capture_manager.rs`; no release function confirmed.
- **G-A8** Add or remove a passenger with limbo and unlimbo:
  `PassengerCargo` has list operations only.
- **G-A9** Per-house prerequisite or buildability change at run time.
- **G-A10** Set a house's score (only `add_score`) and per-house end of game
  from a module beyond `request_player_outcome` (check it covers one house).
- **G-A11** Create object for gameplay (not tests): `spawn_object` is
  documented for tests and tools; a production entry with ownership,
  publication and blocked-cell refusal.
- **G-A12** Remove or force-detonate one projectile (P8, adopted in RFC 0.0.5).
- **G-A13** Detonate a warhead with damage and source at a coordinate
  (P7, adopted in RFC 0.0.5): `apply_area_damage` exists, the API wrapper
  with credit does not.

### Attached effects and marks

- **G-E1** Move `src/sim/attach_effect.rs` and `src/rules/attach_effect.rs`
  from `cfg(feature = "phobos")` to a run-time switch (RFC 8).
- **G-E2** Simulation-level "attach effect type to object" and "remove effect"
  transactions; `remove_attach_effect_type` is private.
- **G-E3** New effect kinds without a Phobos key: transform, replace weapon,
  motion, teleport, immunity, no target search, target priority multiplier,
  damage modifier, cannot be selected, ignores orders, not a legal target,
  house power off.
- **G-E4** New effect properties: interval, maximum firings, "refuse newer"
  stacking, exclusive group, effect-carried marks.
- **G-M1** Marks: a side table keyed by handle (RFC 15), with each mark's
  value and visibility, saved, hashed, cleared on removal; the registry of
  mark names is simulation state, saved and hashed, and only simulation code
  adds to it.

### Queries

- **G-Q1** Creation serial: assigned to every object and projectile at
  creation, saved, and used as the order of query results, override calls and
  delivered records, and as the stagger of polled events (RFC 5, 6c, 15).
- **G-Q2** Query entry points that write handles or IDs into a module buffer
  sorted by creation serial, return the count and page large results, with
  the result set fixed within a module call; nothing is allocated per query
  (RFC 15).

### Overrides

- **G-O1** Call points and type bindings for each override of section 8
  (damage in `receive_damage`, batched target choice in `scan_batch.rs`,
  production in `can_build` with its per-frame result cached for the sidebar
  and the AI, fire in the FireAt steps with the shot plan), called in
  creation-serial order (G-H7).
- **G-O2** Module projectile arm in `ProjectileTrajectory`, 16 bytes of
  module data per projectile in a side table keyed by handle, clamps (RFC 9,
  15).

### AI

- **G-AI1** Set a live team's script (`team.setScript`).
- **G-AI2** Move a team's cursor outside tests (`team.jumpTo`).
- **G-AI3** Enable or disable one AI trigger at run time.
- **G-AI4** Change an AI trigger's weights at run time.

### Rules

- **G-R1** A module rules layer pushed into `RulesLayerStack` after the mod or
  map layer, hashed into the lobby check.
- **G-R2** A mid-match rules overlay: saved, hashed, cache invalidation of
  `rules.serial()`-keyed caches (`movement_tick.rs`, `house_defeat.rs`,
  `scatter.rs`, `movement_commands.rs`), and per-key rules for existing
  objects.
- **G-R3** Name-to-ID lookups for each ID kind (see [IDs](#ids)) over the
  existing registries (`RuleSet` type indexes, sound binding, AI
  definitions), with `NotFound` for an unknown name, a registry of mark
  names for `Mark.id`, and the stable hash of notification names for
  `Notify.id`, read from each simulation module's `vera.notify` custom
  section, with a collision check at load across all simulation modules of
  the match, the mod's and the map's, kept as the table of declared names
  that client modules' `Notify.id` also reads (RFC 3, 10, 15).

### Client

- **G-C1** Module-driven interface tree: create, remove, find, set
  properties, click and hover routing, batched per render frame, on top of the
  `veraui.ini` layout reader.
- **G-C2** A module message variant in `src/sim/command.rs::Command`, with
  sender, size and rate limits, recorded in replays.
- **G-C3** Visibility filter for client queries: shroud, fog rule, cloak,
  disguise (`cloak_disguise.rs` helpers exist).
- **G-C4** Key-value store on the player's machine, one per mod and one per
  map, with a quota.
- **G-C5** Per-object display properties (image, palette, tint, extra
  animations, visibility, text, values, buttons).
- **G-C6** In-game console for `log()`.
- **G-C7** Client module time budget per render frame by the local clock,
  skipping an overrunning call and disabling a module that overruns
  persistently or traps on that machine only (RFC 3).

Total: 55 gaps (H 8, T 11, A 13, E/M 5, Q 2, O 2, AI 4, R 3, C 7). G-T8 is a
pre-existing gap of the original trigger system.

## Proposed RFC changes

Things the research showed that RFC 0.0.4 did not say. RFC 0.0.5 decided each
one: all are adopted except P4 and P5. Entries that depend on them carry the
proposal number.

- **P1 Combination rule per override.** *Adopted in 0.0.5.* RFC 2 says overrides chain. Say per
  override whether the next link receives the native value, the previous
  link's value, or both; whether a boolean veto can be lifted by a later link
  (Recoil ANDs booleans and stops at the first veto); and give each value
  override an explicit "fully handled" result instead of a magic value.
- **P2 `onScenarioStart` and saves.** *Adopted in 0.0.5.* Say that `onScenarioStart` does not run
  after loading a save (module memory is restored), and that there is no
  `onLoad`. Add a test that handles stored in module memory resolve to the same
  objects after load.
- **P3 Mark values and visibility.** *Adopted in 0.0.5.* Let a mark carry an optional integer and a
  visibility level (private, allied, in sight, public), so client modules can
  show states without leaking hidden ones. Add house-level marks. Decide value
  types now and say "no nesting".
- **P4 Query as house.** *Not adopted.* Add `ctx.asHouse(h)` to the simulation API: the same
  queries filtered as house `h` sees them. Default simulation queries stay full
  knowledge.
- **P5 Client channel between allies.** *Not adopted.* Decide whether client modules may send
  rate-limited messages to allies or spectators without the simulation, or
  whether all such traffic goes through `sendMessage`.
- **P6 Query order.** *Adopted in 0.0.5.* Name the deterministic order (for example object
  creation order) so it survives engine refactors and sim-scale reordering.
- **P7 Detonate a warhead at a cell.** *Adopted in 0.0.5.* Add "detonate warhead at a coordinate
  with a damage and a source" to the 6b list (how ion storms, custom
  superweapons and chain reactions are built).
- **P8 Projectile removal.** *Adopted in 0.0.5.* Add "remove a projectile without detonation" and
  "detonate a projectile now" as two separate actions.
- **P9 Damage versus health.** *Adopted in 0.0.5.* Split "change health (add or set)" into "deal
  damage through the warhead and armor path, with a source for credit" and
  "set health, no credit".
- **P10 Two-phase house transfer.** *Adopted in 0.0.5.* RFC 6a raises HouseTransfer before the
  transfer. Add an after-phase event or state which state the handler sees.
- **P11 More candidate events.** *Adopted in 0.0.5.* Add the candidates of 2.5 (Built, Idle,
  MoveFailed, Arrived, Promoted, Cloaked, Spotted, Lost, AmmoChanged,
  MissionChanged, HouseDefeated, PlayerLeft, StorageFull, MotionEnded) to the
  candidate list for open question 13.
- **P12 Type-bound subscriptions only.** *Adopted in 0.0.5.* Say that high-frequency events
  (damage, fire, projectile created or detonated, crush, move failed) and every
  override are raised only for bound types, never through a global "all
  objects" subscription. (Superseded in 0.0.6: the "projectile launched" event
  was removed, and the weapon events are `Fire`, `HitTarget` and `Detonated`.)
- **P13 Data before callbacks for hot decisions.** *Adopted in 0.0.5.* For target priority prefer
  a priority multiplier (an attached effect) plus a `TargetChanged` event, and
  keep per-call overrides for rare decisions. Add `Override.CommandAllowed` and
  `Override.Death` as candidates. Note custom queued missions (a module-defined
  mission the module executes each frame) as a future RFC.
- **P14 Events that cannot be cancelled.** *Adopted in 0.0.5.* State which native results a
  delivered event cannot change (for example cancelling a death), because
  events arrive in the module phase; those need an override or an effect
  (prevent death). (Superseded in 0.0.6: original and atomic actions now run
  when the trigger fires, and only module actions wait for the module phase;
  a delivered event still cannot cancel a native result.)
- **P15 Probe queries.** *Adopted in 0.0.5.* Add "could this weapon fire at that target now",
  "in range", "can type T be placed at cell C for house H" and "closest
  placeable cell", so modules reuse native legality.
- **P16 Layout syntax.** *Adopted in 0.0.5.* The RFC's INI example uses `Anchor=`, `Size=`,
  `Children=` and `Type=Panel`; `veraui.ini` uses `Hold=`, `Width=`/`Height=`
  and `Parts=` and has no `Panel`. Use the existing keys, or say which new ones
  are added.
- **P17 Fog rule for client queries.** *Adopted in 0.0.5.* Say what a client query returns for an
  enemy object under fog (absent or last seen), and whether a client module
  sees allied houses' marks, values and production.
- **P18 Error model.** *Adopted in 0.0.5.* Name which failures return a status (gone handle,
  unknown name, refused by an engine rule) and which disable the module
  (invalid argument, trap, budget).
- **P19 Notification API.** *Adopted in 0.0.5.* Name the simulation-to-client notification
  functions and give notifications a declared name rather than "magic first
  bytes".
- **P20 Fixed point at the boundary.** *Adopted in 0.0.5.* State the fixed-point format of
  multipliers and fractions (`I16F16` raw, as `SimFixed`), since Phobos reads
  multipliers as decimals.
- **P21 Never change units or meaning.** *Adopted in 0.0.5.* Add to section 12: a published
  function never changes its units or meaning; a change is a new function or
  version.

Total: 21 proposals.
