# Tutorial: a simulation module in TypeScript (`rules.wasm`)

[English](ts-tutorial.md) | [简体中文](ts-tutorial.zh-CN.md)

This tutorial takes you from nothing to a Wasm module for VERA20K written in TypeScript,
tried in a game. At the end you have a "bounty" mod: whoever kills a GI or a conscript
gets 200 credits.

- Who it is for: modders who edit `rulesmd.ini` and want gameplay INI cannot express. A
  little JavaScript or TypeScript is enough.
- Reference: where modules go and the limits are in the [guide](guide.md); every
  SDK function is in the [TypeScript SDK reference](ts-sdk.md); the design
  is [RFC 0001](../rfcs/0001-wasm-extension-modules.md) and every number is in its
  [appendix](../rfcs/0001-wasm-extension-modules-api.md).
- Want an interface (buttons, panels)? Read this first, then the
  [client module tutorial](ts-ui-tutorial.md).
- Prefer Rust? The [Rust tutorial](rust-tutorial.md) makes the same mod.
- Preview: the API is still a `0.x` preview; names and numbers may change before
  version 1. The game tells you when a module needs a newer API.

## Contents

1. [What a module is](#1-what-a-module-is)
2. [Set up](#2-set-up)
3. [A first module: a line in the chat](#3-a-first-module-a-line-in-the-chat)
4. [Try it in the game](#4-try-it-in-the-game)
5. [Real gameplay: a kill bounty](#5-real-gameplay-a-kill-bounty)
6. [How this TypeScript differs](#6-how-this-typescript-differs)
7. [Quick reference](#7-quick-reference)
8. [Without code: triggers in INI](#8-without-code-triggers-in-ini)
9. [Debugging](#9-debugging)
10. [Rules and limits](#10-rules-and-limits)

## 1. What a module is

A module is a compiled WebAssembly file. This tutorial makes a **simulation module**,
called `rules.wasm` in the game: it runs with the match, and every player's machine runs
the same copy, so online games stay in step. (The other kind is the **client module**
`ui.wasm`, which runs only on your machine and does the interface; see the
[client module tutorial](ts-ui-tutorial.md).)

Your TypeScript never reaches the game. The SDK compiles it to WebAssembly ahead of time
with [AssemblyScript](https://www.assemblyscript.org), and the game loads the result like
a module written in Rust.

What a module can do:

- **Read the game**: a unit's health, position and owner, a house's money and power, a
  cell's terrain, values from the rules...
- **Change the game**, only through the game's own rules: actions (damage, create a
  unit, give money, launch a superweapon), property writes, modifiers (firepower, armor,
  speed), missions for any house's units (the players' too), orders for computer houses,
  and the map trigger actions.
- **React to events**: triggers listen for damage, kills, shots, births and more, made in
  code or declared in INI.
- **Change the game's decisions**: the damage a hit does, whether a unit may be built,
  whether a command is accepted ("decisions").
- **Edit the rules while they load**, once (`onRules`).

What a module cannot do:

- Read or write files, use the network or read the clock. It runs in a sandbox and
  cannot reach your computer. There is no browser or Node.js API either: no `console`,
  `fetch`, `setTimeout` or `Date`.
- Crash the game. A module that fails, loops or breaks the API is stopped on every
  machine at the same moment; the match goes on and the chat says which module stopped
  and why.

Modules come from two places, which can be used together. The mod's module runs first,
then the map's.

| Source | Where | Good for |
| --- | --- | --- |
| A mod | `rules.wasm` in the mod's folder | Gameplay for the whole mod |
| A map | Inside the map file (with `asset wasm-embed`) | One map's mission or scenario logic |

## 2. Set up

### 2.1 Install Node.js

Install Node.js 20 or later from <https://nodejs.org> (the LTS version is fine). After
that, `node` and `npm` run in a terminal. VS Code is a good editor: it reads the SDK's
types and shows completions and errors as you type.

### 2.2 Get the SDK

The SDK is in the VERA20K source, in `sdk/typescript`. Until the `@vera20k/sdk` package
is published on npm, use it from a copy of the source:

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k/sdk/typescript
npm install          # the SDK's own compiler, AssemblyScript 0.28.20
cd ../..
```

You will use:

| Path | What |
| --- | --- |
| `sdk/typescript` | The TypeScript SDK and its `vera20k` command |
| `sdk/typescript/examples/` | Example modules; this tutorial's complete code is `tutorial-bounty` |

### 2.3 The `asset` tool (optional)

The `asset` tool runs the game's load checks on a module and puts modules into maps. You
need it only for those two jobs; `vera20k build` already refuses the most common
mistakes. It is part of the VERA20K source and needs Rust to build once
(<https://rustup.rs>):

```sh
cargo build --release --bin asset
# the program is target/release/asset (asset.exe on Windows)
```

## 3. A first module: a line in the chat

### 3.1 Make the project

From the VERA20K folder, make a new project with `vera20k init` and install it:

```sh
node sdk/typescript/bin/vera20k.js init my-bounty
cd my-bounty
npm install
```

The project can be anywhere; `init` makes it depend on the SDK in your copy of the
source. Once `@vera20k/sdk` is published, `npx -p @vera20k/sdk vera20k init my-bounty`
will do the same without a copy of the source.

The project:

```text
my-bounty/
├── package.json     the project's name, its dependencies, and build / watch / check
├── tsconfig.json    the type-check settings from the SDK; keep it
├── vera.json        what to build: the kind, the entry file, variables, notifications
├── .gitignore
└── src/index.ts     the module's code
```

`vera.json` says what `vera20k build` makes:

```json
{
  "kind": "rules",
  "entry": "src/index.ts",
  "vars": [],
  "notify": []
}
```

`kind` is `rules` for a simulation module (`ui` for a client module); `entry` is the
file the build compiles. `vars` and `notify` declare the module's variables and
notification names; [section 7](#variables) uses them.

### 3.2 Write the code

`src/index.ts` already holds the smallest module:

```ts
import { log, onScenarioStart } from "@vera20k/sdk";

// Runs once when the match starts.
onScenarioStart((): void => {
  log("Hello from my first module!");
});
```

What `log` writes appears in the game's chat list and in the game's log file.

Note the `: void` on the arrow function. This TypeScript is compiled by AssemblyScript,
which wants types written out; [section 6](#6-how-this-typescript-differs) lists the
differences from everyday TypeScript.

### 3.3 Build and check

```sh
npm run build        # writes build/rules.wasm
npm run check        # type-checks src/ with tsc
```

The result is `build/rules.wasm`, already the name the game wants. The build prints its
size; it fails with a message when the code does not compile or calls something the game
would refuse. `npm run check` writes `src/generated/` from `vera.json` (`vera20k generate`)
and runs the TypeScript type checker, whose messages are often easier to read than the
compiler's. It needs no build first.

While you work, `npm run watch` builds again each time you save a file in `src/` or
`vera.json`.

With the `asset` tool you can also run the game's own load checks:

```sh
asset wasm-check build/rules.wasm
# on success: {"bytes":…,"ok":true}
```

## 4. Try it in the game

### 4.1 Find the Mods folder

| System | VERA20K folder |
| --- | --- |
| Windows | The folder VERA20K is installed in, where `vera20k.exe` is |
| macOS | `~/Library/Application Support/VERA20k/` |
| Linux | `~/.local/share/VERA20k/` |

Mods go in `Mods` in that folder, one subfolder per mod.

### 4.2 Make the mod folder

```text
Mods/MyBounty/
├── mod.ini       the mod's name and description
└── rules.wasm    your module, copied from build/
```

`mod.ini`:

```ini
[Mod]
Name=My Bounty
Version=1
About=Credits for every infantry kill.
```

If you only add a module, don't put a `rulesmd.ini` in the mod: a file in a mod replaces
the game's file of the same name entirely. To change rules, see
[section 8](#8-without-code-triggers-in-ini).

To skip the copying, build straight into the mod folder. Arguments after `--` go to
`vera20k build`:

```sh
npm run build -- --out "<VERA20K folder>/Mods/MyBounty/rules.wasm"
```

or set `"out"` in `vera.json` to that path.

### 4.3 Start

Either:

- open **Options → Mods** in the game and choose your mod; the game restarts into it, or
- start it from the command line: `vera20k --mod MyBounty`.

Then play a skirmish. When it starts, the chat shows `Hello from my first module!`.

## 5. Real gameplay: a kill bounty

The goal: whoever kills a GI (`E1`) or a conscript (`E2`) gets 200 credits. Replace
`src/index.ts` with this; the same code is
`sdk/typescript/examples/tutorial-bounty/src/index.ts`:

```ts
import {
  Action, Event, KilledEvent, NO_HANDLE, Rules, Status, Techno, Trigger,
  log, namedAction, onScenarioStart,
} from "@vera20k/sdk";

// ⑤ One kill: pay the killer's house
function onKill(e: KilledEvent): void {
  // The killer may have died in the same frame
  const house = Techno.owner(e.killer);
  if (house == NO_HANDLE) return;
  if (Action.addMoney(house, 200) == Status.Ok) {
    log("Bounty: +200 credits");
  }
}

// ① Name the handler: INI triggers call it as Wasm:OnKill
namedAction("OnKill", onKill);

onScenarioStart((): void => {
  log("Bounty: 200 credits for every GI or conscript killed.");
  // ② A trigger on the "killed" event
  const trigger = Trigger.on(Event.Killed);
  // ③ Bind it to unit types: every unit of the type, also those built later
  const names: string[] = ["E1", "E2"];
  for (let i = 0; i < names.length; i++) {
    const infantry = Rules.technoType(names[i]);
    if (infantry != NO_HANDLE) trigger.bind(infantry);
  }
  // ④ When it fires, hand the event to onKill; done() creates it
  if (trigger.enqueue(onKill).done() == NO_HANDLE) {
    log("Bounty: the trigger was refused");
  }
});
```

Part by part:

1. **`namedAction("OnKill", onKill)`** gives the handler a name, so INI triggers can call
   it as `Wasm:OnKill`. Write the name as a string literal: the build copies it into the
   module's `vera.actions` section without running the module. A handler only code uses
   needs no name; `enqueue` takes any function.
2. **`Trigger.on(Event.Killed)`** starts a trigger: "when this happens, do that", as in
   the map editor.
3. **`bind(...)`** says which objects the trigger watches. Bound to a type
   (`Rules.technoType("E1")`), it covers every unit of the type, including those built
   later; it can also be bound to one unit. `Rules.technoType` answers `NO_HANDLE` for a
   name the rules don't have, so check before binding.
4. **`enqueue(onKill)`** hands the event record to `onKill` when the trigger fires.
   Records are delivered together in each frame's module phase. **`done()`** creates the
   trigger; it answers the trigger, or `NO_HANDLE` when the game refused it.
5. **The event view** `KilledEvent` gives the record's fields names: `e.victim`,
   `e.killer`, and the position `e.x`, `e.y`, `e.z`. A view is valid only while its
   handler runs; keep the values, not the view. `Techno.owner(t)` reads a unit's house:
   references are plain numbers, not objects, so it is `Techno.owner(t)` and not
   `t.owner` ([section 6](#6-how-this-typescript-differs)).

Build, copy (or build with `--out`) and play again: for every GI or conscript killed, the
chat shows `Bounty: +200 credits` and the money goes up.

> The Rust version of this mod, `sdk/examples/tutorial-bounty`, makes the same calls to
> the game. If what you see in a match differs, please report it.

### Want more?

- **Pay every 30 seconds**: the game runs 15 frames a second, so 450 frames are 30
  seconds. `onFrame` runs a function on every frame divisible by the number:

  ```ts
  import { Action, Every, Find, Op, Prop, onFrame } from "@vera20k/sdk";

  // One page of houses, made once and reused
  const PAGE = new StaticArray<i64>(16);

  function pay(): void {
    const humans = Find.houses().where(Prop.IsHuman, Op.Eq, 1);
    let after: i64 = 0;
    do {
      const page = humans.page(PAGE, after);
      for (let i = 0; i < page.count; i++) Action.addMoney(PAGE[i], 500);
      after = page.next;
    } while (after != 0);
  }

  // 15 frames a second: 450 frames are 30 seconds
  onFrame(new Every(450), pay);
  ```

- **Reward only human players**: in `onKill`, check `House.isHuman(house)` first.
- **Half damage for a badly hurt unit**: use a decision; see [section 7](#decisions).

## 6. How this TypeScript differs

AssemblyScript compiles a strict subset of TypeScript. The SDK uses only what both
TypeScript and AssemblyScript accept, so the same code passes `tsc` (`npm run check`) and
compiles (`npm run build`). Your code has to do the same:

| Not available | Write instead |
| --- | --- |
| A function without types, such as `(e) => {}` | Types on every parameter, and on returns: `(e: KilledEvent): void => {}` |
| `any`, `unknown`, union types other than `T \| null` | A concrete type |
| An untyped object literal `{ x: 1 }` | A class: `class Point { x: i32 = 0; }`, then `new Point()` |
| A closure: a callback using a local variable of the function around it | A top-level variable, or a named function |
| `for…of`, `for…in` | `for (let i = 0; i < list.length; i++)` |
| `try` / `catch` | Check return values: `Status.Ok`, `NO_HANDLE` (a `throw` stops the module) |
| `"hp=" + hp` with a number | `"hp=" + hp.toString()`, or a template literal `` `hp=${hp}` `` |
| `console.log`, `Math.random()`, `Date.now()` | `log(...)`, `random(lo, hi)`, `frame()` |
| An npm package written for JavaScript | Only packages written for AssemblyScript that need nothing from a host |

`tsc` does not catch all of these (it accepts `"hp=" + hp`, for example); the build does.
Each is a compile error with a clear message (`Not implemented: Closures`,
`Not implemented: Iterators`, ...), except the host calls: for those the build says
which import the module would need and refuses it, as in
`imports env.seed` for `Math.random()`.

A few ideas to keep in mind:

- **Number types.** Write `i32` (32-bit integer), `i64` (64-bit integer), `f32`, `f64`,
  `u8`, `bool`. A literal like `0` is an `i32`, so a variable that must hold a 64-bit
  value says so: `let after: i64 = 0`. Integer division stays integer: `7 / 2` is `3`.
  Game values and references are `i64`.
- **References are numbers, not objects.** `Techno`, `House`, `Cell`, `TypeRef` and the
  others are names for `i64`. Read and write them with namespace functions that take the
  reference first: `Techno.health(t)`, `Techno.setMission(t, Mission.Hunt)`. No object is
  made per reference, which keeps the module small and fast. `NO_HANDLE` (0) is "none".
- **Top-level code is the module's initialization.** Statements at the top level of your
  files run once, when the module loads. Register your callbacks there: `onScenarioStart`,
  `onFrame`, `namedAction`, `Ini.key`... Each of them may be called several times: every
  function you register runs, in registration order.
- **Errors are values.** A call that writes (an action, a property write) returns a
  `Status`: `Status.Ok`, or the reason, such as `Gone` (the object no longer exists),
  `NotFound` (no such name), `Refused` (the game refused) or `Unsupported` (not in this
  version). A call that reads or creates returns the value, which is the empty value on
  failure (`0`, `NO_HANDLE`, `NO_ID`); `lastStatus()` then gives the reason.
- **Views live only during their callback.** Event and decision views (`KilledEvent`,
  `TakeDamageDecision`, ...) read the engine's record in place. Copy the fields you need
  into your own variables.

## 7. Quick reference

Each block below is a complete file that compiles; take the lines you need. The
[SDK reference](ts-sdk.md) has every function.

### Lifecycle

```ts
import {
  Event, Every, House, KilledEvent, NO_HANDLE, Rules, TakeDamageDecision, Trigger,
  log, namedAction, onFrame, onMessage, onRules, onScenarioStart,
} from "@vera20k/sdk";

// Top-level code runs once, when the module loads
log("loaded");

function tick(): void {}
function onKill(e: KilledEvent): void {}
function halve(d: TakeDamageDecision): void {}

onRules((): void => {});                  // once, while the rules load; may change them
onScenarioStart((): void => {             // once, when the match starts
  const gi = Rules.technoType("E1");
  if (gi == NO_HANDLE) return;
  Trigger.on(Event.Killed).bind(gi).enqueue(onKill).done();    // events, in the module phase
  Trigger.on(Event.TakeDamage).bind(gi).decideSync(halve);     // decisions, at once
});
onFrame(new Every(1), tick);              // every frame
onFrame(new Every(15), tick);             // every 15 frames (one second)
onMessage((sender: House, data: Uint8Array): void => {});     // messages from client modules
namedAction("OnKill", onKill);            // INI calls it as Wasm:OnKill
```

### Names and references

A module refers to things in the game through references: `Techno` (a unit or
building), `House`, `Cell`, `TypeRef` (a type), `Waypoint`. Look names up once, at start,
and keep the result in a top-level variable:

```ts
import { Cell, NO_HANDLE, Rules, TypeRef, Waypoint, log, onScenarioStart } from "@vera20k/sdk";

// Look names up once and keep them in top-level variables
let GI: TypeRef = NO_HANDLE;
let SUPER: TypeRef = NO_HANDLE;

onScenarioStart((): void => {
  GI = Rules.technoType("E1");       // a unit type; NO_HANDLE for an unknown name
  SUPER = Rules.warhead("Super");    // a warhead
  const cell = Cell.at(50, 60);      // a cell by its coordinates, no call to the game
  const drop = Waypoint.number(3);   // waypoint 3
  if (GI == NO_HANDLE) log("these rules have no E1");
});
```

### Reading properties

```ts
import { Cell, House, Prop, Techno, get, log } from "@vera20k/sdk";

function report(unit: Techno, house: House, cell: Cell): void {
  const hp = Techno.health(unit);
  const owner = Techno.owner(unit);
  const money = House.money(house);
  const land = Cell.landType(cell);
  const name = House.name(house);                  // a string property gives a string
  const percent = get(unit, Prop.HealthPercent);   // any property by its Prop
  log(name + ": " + money.toString() + " credits, unit at " + percent.toString() + "%");
}

// Many objects at once: one call instead of one per value
const PROPS: StaticArray<i32> = [Prop.Health, Prop.Owner];

function readAll(units: StaticArray<Techno>): void {
  const values = Techno.read(units, PROPS);
  for (let i = 0; i < units.length; i++) {
    const health = values[i * 2];
    const owner = values[i * 2 + 1];
  }
}
```

Every property is in [appendix section 7](../rfcs/0001-wasm-extension-modules-api.md#7-properties);
in the SDK they are constants of `Prop`, with `_` for a `.` in the name
(`Prop.Passable_Foot` is `Passable.Foot`).

### Searching

```ts
import { Action, Find, House, Op, Prop, Techno, TypeRef } from "@vera20k/sdk";

// The first GI of a house, NO_HANDLE when it has none
function firstGi(house: House, gi: TypeRef): Techno {
  return Find.objects().ownedBy(house).ofType(gi).first();
}

// Every human house, a page at a time
const PAGE = new StaticArray<i64>(16);

function payHumans(): void {
  const humans = Find.houses().where(Prop.IsHuman, Op.Eq, 1);
  let after: i64 = 0;
  do {
    const page = humans.page(PAGE, after);
    for (let i = 0; i < page.count; i++) Action.addMoney(PAGE[i], 1000);
    after = page.next;
  } while (after != 0);
}
```

`near(cell, radius)` (the radius in leptons, 256 to a cell) and `within(cell, width,
height)` narrow a search. Results come in creation order. Keep asking while `next` is not
0: a page can be short, even empty, and still not be the last. A `Find` holds at most 16
filter clauses.

### Actions

```ts
import { Action, Cell, House, Mission, NO_HANDLE, Rules, Techno, Waypoint } from "@vera20k/sdk";

function examples(unit: Techno, house: House): void {
  const warhead = Rules.warhead("Super");
  const rhino = Rules.technoType("HTNK");
  Action.addMoney(house, 500);                    // credits (negative takes them)
  Action.damage(unit, 50, warhead);               // damage with a warhead
  Action.heal(unit, 50);                          // heal
  Action.kill(unit, NO_HANDLE);                   // kill; the second argument gets the credit
  const tank = Action.createObject(Cell.at(60, 40), rhino, house);  // NO_HANDLE on failure
  Techno.setMission(tank, Mission.Hunt);          // change its mission
  Action.nukeStrike(Waypoint.number(3));          // the map trigger action "nuke strike"
}
```

The atomic actions and the map trigger actions are listed in
[appendix section 5](../rfcs/0001-wasm-extension-modules-api.md#5-action-kinds).

### Modifiers (temporary multipliers)

```ts
import { Modifier, Prop, Techno, fixed } from "@vera20k/sdk";

function boost(unit: Techno): void {
  // Firepower ×2 for 150 frames (10 seconds); negative frames: until removed
  const m = Modifier.multiply(unit, Prop.FirepowerMultiplier, fixed(2.0), 150);
  Modifier.remove(m);   // remove it early
}
```

Firepower, armor, speed, rate of fire, range multiplier and extra range work, and
cloakable, forced decloak and disabled weapons; the [SDK reference](ts-sdk.md#modifiers)
lists the rest. Modifiers stack with Phobos AttachEffects; without modifiers the game
behaves exactly as before.

### Variables

Red Alert 2's local and global variables are `Var.local(n)` and `Var.global(n)`. The
module's own variables are declared in `vera.json`:

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

The build writes them into the module's `vera.vars` section and generates
`src/generated/vars.ts` with one constant per variable, which you import (`npm run check`
and `vera20k generate` write the file too, without compiling):

```ts
import { Techno, Var } from "@vera20k/sdk";
import { Panicked, Wave } from "./generated/vars";

function example(unit: Techno): void {
  Var.local(3).set(1);                // Red Alert 2 local variable 3
  const on = Var.global(5).get();     // Red Alert 2 global variable 5
  Wave.set(Wave.get() + 1);           // declared in vera.json
  Panicked.set(unit, 1);
  const panicked = Panicked.get(unit);
}
```

`scope` is `global` (one value per match), `house` (one per house) or `object` (one per
unit or building); it is required. `visibility` says which client modules may read it: `private` (the
house's own player), `allies`, `insight` (object scope), `public`. Variables are saved
with the game. `notify` declares notification names for client modules (generated into
`src/generated/notify.ts`); the [client tutorial](ts-ui-tutorial.md) uses
one.

### Filter clauses and immediate actions

```ts
import {
  ActionDef, Event, NO_HANDLE, Op, Prop, Rules, TakeDamageEvent, Target, Trigger, VarTarget,
  log, onScenarioStart,
} from "@vera20k/sdk";
import { Panicked } from "./generated/vars";

function giPanic(e: TakeDamageEvent): void {
  log("A GI panics");
}

onScenarioStart((): void => {
  const gi = Rules.technoType("E1");
  if (gi == NO_HANDLE) return;
  Trigger.on(Event.TakeDamage)
    .bind(gi)
    .where(Prop.HealthPercent, Op.Lt, 30)                      // only below 30% health
    .whereVar(VarTarget.Subject, Panicked, Op.Eq, 0)           // and not panicked yet
    .immediate(ActionDef.setVar(Target.Subject, Panicked, 1))  // the game runs it at once
    .enqueue(giPanic)                                          // then the module hears it
    .done();
});
```

The game checks filter clauses itself and does not call the module when they fail, which
is much cheaper than checking in the handler. `.once()` makes a trigger fire once.

### Decisions

The game asks the module at the moment it decides, and the module may change the answer.
For example, half damage for GIs:

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

`d.native` is the game's own answer, `d.previous` the answer so far (another module may
have answered first), and `d.setResult(value)` gives yours; without it the previous answer
stands. Modules can decide the damage a hit does, whether a type may be built or queued,
whether a house is defeated, whether a player command is accepted, and also shots,
retaliation, destinations, crushing, harvesting, deploying, the idle mission, movement
and paths; the [SDK reference](ts-sdk.md#decisions) lists them all. A
decision handler **only reads** (`log` works): an action, a property or variable write
there stops the module. It should not allocate either (no `new`, no string building).

### Missions and orders for computer houses

```ts
import { Action, Cell, House, Mission, Order, Rules, Techno } from "@vera20k/sdk";

function command(computer: House, tank: Techno): void {
  const units: StaticArray<Techno> = [tank];
  House.order(computer, Order.moveTo(units, Cell.at(60, 40)));
  House.order(computer, Order.queueProduction(Rules.technoType("HTNK")));
}

function anyHouse(tank: Techno): void {
  Techno.setMission(tank, Mission.Hunt);                    // a property write, no target
  Action.setMission(tank, Mission.Move, Cell.at(60, 40));   // an action, with a target cell
}
```

`House.order` works only for computer houses. The order runs next frame, checked like a
player's click; one order carries at most 64 units. To command units of **any** house, a
player's too, use missions: they act in the name of the unit's house, as if its player
had clicked.

### Changing the rules as they load

```ts
import { Rules, log, onRules } from "@vera20k/sdk";

onRules((): void => {
  Rules.set("E1", "Strength", "250");            // GIs get 250 health
  log("E1 Strength is now " + Rules.get("E1", "Strength"));
});
```

`onRules` runs once while the rules load; the change holds for the whole match, the same
on every machine. Once the match runs, `Rules.set` is refused.

### Other

```ts
import { NO_HANDLE, Rules, Status, frame, lastStatus, log, random } from "@vera20k/sdk";

function misc(): void {
  const now = frame();             // the current frame, 15 a second
  const roll = random(1, 100);     // deterministic: the same on every machine
  const rhino = Rules.technoType("HTNK");
  if (rhino == NO_HANDLE && lastStatus() == Status.NotFound) log("no HTNK");
  log("frame " + now.toString() + ", roll " + roll.toString());
}
```

## 8. Without code: triggers in INI

When you only need stock actions and handlers the module already has, declare triggers
in the mod's `rulesmd.ini`, without changing the code.

**Prepare the mod's `rulesmd.ini` first**: a mod's `rulesmd.ini` replaces the game's
entirely, so start from a full copy. **Options → Mods → New Mod** in the game makes a mod
with the original files; see [modding](../modding.md).

Example: when a GI drops below 30% health it heals 50, once per GI.

```ini
[E1]
Wasm.Tags=GIHealTag        ; put the tag on the GI type

[GIHealTag]
Trigger=GIHeal             ; the tag's triggers, comma-separated

[GIHeal]
Event=TakeDamage
Where1=Subject.HealthPercent<30
Action1=Heal(Subject,50)
Repeat=OneTimeOr
```

To call the module's handler, write `Action1=Wasm:OnKill`, the name given to
`namedAction("OnKill", onKill)`. `Decide=Wasm:Name` makes the trigger a decision trigger
whose requests go to the handler of that name; that handler takes a decision view, such
as `TakeDamageDecision`.

Trigger section keys:

| Key | Value |
| --- | --- |
| `Event`, `Event<n>` | An event name, or `name(op,interval,id,value)`; several events must all hold |
| `Where<n>` | `Subject.`, `Other.` or `House.` and a property, or `Var(scope,name)`, then a comparison (`=`, `!=`, `<`, `<=`, `>`, `>=`) and a number |
| `Action`, `Action<n>` | `Wasm:name`, `Wasm:name(target,text)`, or `action(target,parameters…)` |
| `Decide` | `Wasm:name`, a decision handler |
| `House` | A country name; empty means the subject's own house |
| `Difficulty` | `Easy`, `Normal`, `Hard`, comma-separated |
| `Repeat` | `OneTimeOr` (default), `OneTimeAnd`, `Repeating` |

A mistake keeps the match from starting, with a message naming the section and key. The
full syntax is in the [guide](guide.md#triggers-in-ini). A map cannot bind types
with `Wasm.Tags` (it is ignored there, and the match says so); a map's module binds in
code.

`Wasm:name(target,text)` passes a target and a text to the handler: read them in the
handler as `e.record.target` and `e.record.argsText`.

### INI keys of your own

A module can also give modders INI names of its own: a key on type sections, a list
section (a registry) and module events. The modder writes INI; the module implements it.
Register in top-level code; anywhere else stops the module.

```ts
import {
  EventScope, Ini, IniEntryRecord, IniKeyRecord, IniSection, NO_ID, Rules, fail, log,
} from "@vera20k/sdk";

// One entry of [Demo.ShieldTypes]: read and check its section
function loadShieldType(r: IniEntryRecord): void {
  const strength = Ini.int(Ini.stripComment(Rules.get(r.entryName, "Strength")), -1);
  if (strength < 1) fail("[" + r.entryName + "] Strength: needs a number of 1 or more");
}

// One section that says Demo.Shield=...
function attachShield(r: IniKeyRecord): void {
  log("[" + r.sectionName + "] Demo.Shield=" + r.value);
}

// Top-level code: the only place to register INI names
Ini.registry("Demo.ShieldTypes", loadShieldType);
Ini.key("Demo.Shield", IniSection.TechnoType, attachShield);
const SHIELD_BROKEN = Ini.event("Demo.ShieldBroken", EventScope.Object);
if (SHIELD_BROKEN == NO_ID) log("another module registered Demo.ShieldBroken");
```

With this INI in the mod, the module receives one `IniEntryRecord` for `HeavyShield` and
one `IniKeyRecord` for `[HTNK]`, before `onScenarioStart`:

```ini
[Demo.ShieldTypes]
1=HeavyShield

[HeavyShield]
Strength=300

[HTNK]
Demo.Shield=HeavyShield
```

INI triggers use the module event as `Event=Wasm:Demo.ShieldBroken`; code raises it with
`Action.raiseEvent` and listens with `Trigger.onModuleEvent`. Values are the merged rules
text, unchecked: parse them with `Ini.int`, `Ini.ints`, `Ini.yesNo`, `Ini.split`, and stop
with `fail(...)` naming the section and key when one is wrong. See the
[guide](guide.md#ini-names-a-module-registers) and
`sdk/typescript/examples/ini-counter` for a complete mod.

### Put the module in a map

A map can carry its own simulation module. With the `asset` tool:

```sh
asset wasm-embed original.map build/rules.wasm new.map   # write the module in
asset wasm-check new.map                                 # check the map's module
```

`wasm-embed` writes `[VeraWasm] Api=` and `RulesSize=` for you. Also put
`VeraMinVersion=` in the map's `[Basic]`, set to the VERA20K version you made it with.
Size matters more in a map: keep a map's module to some tens of KB. `npm run build` is
already a release build tuned for size.

## 9. Debugging

**Logs**

- `log("...")` shows in the chat list, at most 8 lines a frame and 160 characters a
  line. Turn numbers into text with `.toString()`.
- Every log line goes in full to the game's log file `logs/ra2.log`, in the VERA20K
  folder of section 4.1.

**When a module stops**

The chat shows a red line such as `Mod module stopped in on_frame: ...`, with the
function and the reason. Common reasons:

| Reason in the line | Meaning | What to do |
| --- | --- | --- |
| `exceeded its operation budget` | Too much work in one call | Spread it over frames (`new Every(N)`), read in batches with `Techno.read` or `getMany` |
| `trapped: …` | The code crashed: an index out of bounds, a `null` used as an object, a `throw`, a `fail(...)`, memory used up | Check lengths before indexing, `if (x != null)` before use. For an index out of range, a `null` dereference or a `throw`, the module's log line just before reads `abort: <message> at <file>:<line>:<column>`, the place in your source |
| `made an invalid call: …` | The API was used wrongly: an argument out of range, a variable of the wrong scope, a write in a decision handler, a registration outside the top level; the reason follows | Check the call against the [SDK reference](ts-sdk.md) |
| `cannot keep up: … event records are waiting` | More events than the module can handle | Bind fewer objects or add filter clauses with `.where(...)` |

**The build fails**

| Message | Cause |
| --- | --- |
| `ERROR TS…` or `ERROR AS…` with a source line | A compile error; [section 6](#6-how-this-typescript-differs) lists the usual ones (`Not implemented: Closures`, `Iterators`, `Exceptions`, `union types`) |
| `a simulation module may import only vera:sim@1.0, but this one imports env.…` | Something needs the host: `console` (`env.console.log`), `Math.random` (`env.seed`), `Date` (`env.Date.now`), or a JavaScript library |
| `namedAction's name must be a string literal` | `namedAction(NAME, fn)` with a variable; write the name in quotes |
| `named actions "A" and "B" share dispatch key …` | Two names hash alike; rename one |
| `variable "…" repeats "…" (names ignore case)` | Two declarations in `vera.json` differ only in case |
| `vera.json: variable … scope must be global, house or object` | A `vars` entry without a valid `scope` |
| `Cannot find module './generated/vars'` (from `tsc`) | `src/generated/` is missing: run `npm run check` or `vera20k generate`, which write it from `vera.json`. With no `vars` in `vera.json` there is no `vars.ts` to import |

The build also prints `Last converge was suboptimal.` now and then. That is the optimizer
talking; the module is fine.

`npm run build -- --debug` builds without the size optimizations and with debug names,
which makes a module easier to read in a disassembler.

**The match won't start**

Usually an INI trigger mistake, a budget in `[VeraWasm]` below 20,000, a map that needs a
newer game, or a map with a module under a mod that says `MapWasm=no`. The message at the
start names the section and key.

## 10. Rules and limits

**Determinism**

Online, every machine runs the same module, so the module must give the same result on
every machine:

- For random numbers use the SDK's `random(lo, hi)`. It uses the module's own random
  stream, the same sequence on every machine. `Math.random()` does not build.
- Floats work, and NaN is made the same value everywhere. Still, prefer integers or
  `fixed()` for game values.
- There is no clock. Count time with `frame()`; the game runs 15 frames a second.

**Resource limits**

| What | Limit |
| --- | --- |
| Module size | 8 MiB |
| Memory | 32 MiB for a mod's module, 16 MiB for a map's |
| Operations per call (the budget) | 500,000 for a mod's module by default and at most, 100,000 for a map's by default; the mod sets them with `Budget` and `MapBudget` in `[VeraWasm]` |
| Start-up calls (top-level code, `onRules`, `onScenarioStart`) | 5,000,000 each |
| Triggers per module | 256 |
| Filter clauses per search | 16 |
| Orders for computer houses per frame | 256, with 4,096 units in all; at most 64 units an order |

**Saves**

A save keeps the module's memory, variables, modifiers and triggers. Loading runs no
module code again, not even the top-level code; the module continues from the moment of
the save, with its top-level variables as they were.

**Performance**

- Look names up once at start and keep them in top-level variables.
- Read many values at once with `Techno.read` or `getMany`.
- Spread work over the whole map across frames, for example a part of the objects each
  frame.
- Allocate little in code that runs every frame: each `new`, array and joined string
  costs operations and work for the garbage collector. Reuse top-level `StaticArray`s, as
  the search examples above do.

## More

- [TypeScript SDK reference](ts-sdk.md): every function, `vera.json` and the
  build command
- [Client module tutorial](ts-ui-tutorial.md): a button on the screen
  (`ui.wasm`)
- [Guide](guide.md): where modules go, INI syntax, limits and debugging
- [Rust tutorial](rust-tutorial.md): the same mod in Rust
- [RFC 0001](../rfcs/0001-wasm-extension-modules.md): the design (the TypeScript SDK is
  section 5a); the [appendix](../rfcs/0001-wasm-extension-modules-api.md): every import,
  event, action and property
- Examples in `sdk/typescript/examples/`: ports of the Rust examples; `tutorial-bounty` is
  this tutorial's.
