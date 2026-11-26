# Tutorial: a simulation module (`rules.wasm`)

[English](rust-tutorial.md) | [简体中文](rust-tutorial.zh-CN.md)

This tutorial takes you from nothing to a Wasm module for VERA20K written in Rust, tried
in a game. At the end you have a "bounty" mod: whoever kills a GI or a conscript gets 200
credits.

- Who it is for: modders who edit `rulesmd.ini` and want gameplay INI cannot express. A
  little experience with any programming language is enough; the Rust is basic.
- Reference: where modules go and the limits are in the [guide](guide.md); every
  SDK function is in the [Rust SDK reference](rust-sdk.md); the design is
  [RFC 0001](../rfcs/0001-wasm-extension-modules.md) and every number is in its
  [appendix](../rfcs/0001-wasm-extension-modules-api.md).
- Want an interface (buttons, panels)? Read this first, then the
  [client module tutorial](rust-ui-tutorial.md).
- Preview: the API is still a `0.x` preview; names and numbers may change before
  version 1. The game tells you when a module needs a newer API.

## Contents

1. [What a module is](#1-what-a-module-is)
2. [Set up](#2-set-up)
3. [A first module: a line in the chat](#3-a-first-module-a-line-in-the-chat)
4. [Try it in the game](#4-try-it-in-the-game)
5. [Real gameplay: a kill bounty](#5-real-gameplay-a-kill-bounty)
6. [Quick reference](#6-quick-reference)
7. [Without code: triggers in INI](#7-without-code-triggers-in-ini)
8. [Put the module in a map](#8-put-the-module-in-a-map)
9. [Debugging](#9-debugging)
10. [Rules and limits](#10-rules-and-limits)

## 1. What a module is

A module is a compiled WebAssembly file. This tutorial makes a **simulation module**,
called `rules.wasm` in the game: it runs with the match, and every player's machine runs
the same copy, so online games stay in step. (The other kind is the **client module**
`ui.wasm`, which runs only on your machine and does the interface; see the
[client module tutorial](rust-ui-tutorial.md).)

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
- **Edit the rules while they load**, once (`on_rules`).

What a module cannot do:

- Read or write files, use the network or read the clock. It runs in a sandbox and
  cannot reach your computer.
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

### 2.1 Install Rust

Install Rust from <https://rustup.rs>, then add the WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
```

### 2.2 Get the VERA20K source

The SDK and the examples are in the VERA20K source:

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
```

You will use:

| Path | What |
| --- | --- |
| `sdk/rust/vera20k-sdk` | The Rust SDK |
| `sdk/examples/` | Example modules; this tutorial's complete code is `tutorial-bounty` |

### 2.3 Build the `asset` tool

The `asset` tool checks modules and puts them into maps. Its Wasm commands need no game
install:

```sh
cargo build --release --bin asset
# the program is target/release/asset (asset.exe on Windows)
```

## 3. A first module: a line in the chat

### 3.1 Make the project

The easy way is to copy an example. This makes `my-bounty` beside the others in
`sdk/examples`:

```sh
cp -R sdk/examples/tutorial-bounty sdk/examples/my-bounty
```

The project has three files:

```text
my-bounty/
├── .cargo/config.toml   the build target and allowed Wasm features; keep it
├── Cargo.toml           the project's name and its SDK dependency
└── src/lib.rs           the module's code
```

Change `name` in `Cargo.toml` to your own. If the project is not under `sdk/examples`,
change the dependency's path so it points at the SDK:

```toml
[dependencies]
vera20k-sdk = { path = "../../rust/vera20k-sdk" }
```

Keep `.cargo/config.toml`. It makes the compiler emit only the WebAssembly features the
game accepts, without reference types, SIMD and the others the game refuses. Without it,
`wasm-check` reports an unsupported feature.

### 3.2 Write the code

Replace `src/lib.rs` with the smallest module:

```rust
#![no_std]                       // no standard library: small and quick to start

use vera20k_sdk::prelude::*;     // the SDK's common names at once

fn start() {
    log("Hello from my first module!");
}

// Tell the game: call start when the match starts
vera20k_sdk::on_scenario_start!(start);
```

What `log` writes appears in the game's chat list and in the game's log file.

### 3.3 Build and check

```sh
cd sdk/examples/my-bounty
cargo build --release
```

The result is `target/wasm32-unknown-unknown/release/my_bounty.wasm` (a `-` in the
project name becomes `_`). Rename it `rules.wasm` and run the game's load checks on it:

```sh
asset wasm-check rules.wasm
# on success: {"bytes":…,"ok":true}
```

When it fails, it says why the game would not accept the module.

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
└── rules.wasm    your module
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
[section 7](#7-without-code-triggers-in-ini).

### 4.3 Start

Either:

- open **Options → Mods** in the game and choose your mod; the game restarts into it, or
- start it from the command line: `vera20k --mod MyBounty`.

Then play a skirmish. When it starts, the chat shows `Hello from my first module!`.

## 5. Real gameplay: a kill bounty

The goal: whoever kills a GI (`E1`) or a conscript (`E2`) gets 200 credits. The complete
code is `sdk/examples/tutorial-bounty/src/lib.rs`:

```rust
#![no_std]

use vera20k_sdk::prelude::*;

// ① name the module's handler and give it a dispatch key
vera20k_sdk::named_actions! { ON_KILL = "OnKill" => 1 }

fn start() {
    log("Bounty: 200 credits for every GI or conscript killed.");
    // ② make a trigger on the "killed" event
    let mut trigger = Trigger::on(Event::KILLED);
    // ③ bind it to unit types: every unit of the type, also those built later
    for name in ["E1", "E2"] {
        if let Ok(infantry) = TypeRef::techno_type(name) {
            trigger = trigger.bind(infantry);
        }
    }
    // ④ when it fires, hand the event to ON_KILL; done() creates it
    if trigger.enqueue(ON_KILL).done().is_err() {
        log("Bounty: the trigger was refused");
    }
}

// ⑤ one kill: pay the killer's house
fn on_kill(event: KilledEvent) {
    let Ok(house) = event.killer().owner() else {
        return; // the killer may have died in the same frame
    };
    if Action::add_money(house, 200).is_ok() {
        log("Bounty: +200 credits");
    }
}

vera20k_sdk::on_scenario_start!(start);
// ⑥ send ON_KILL's events to on_kill
vera20k_sdk::on_events!(ON_KILL => on_kill);
```

Part by part:

1. **`named_actions!`** declares the module's own actions. Each has a name (`"OnKill"`)
   and a dispatch key (`1`). INI calls it by name as `Wasm:OnKill`; code dispatches
   events by key. A module has one `named_actions!` with all its actions.
2. **`Trigger::on(Event::KILLED)`** starts a trigger: "when this happens, do that", as in
   the map editor.
3. **`bind(...)`** says which objects the trigger watches. Bound to a type
   (`TypeRef::techno_type("E1")`), it covers every unit of the type, including those
   built later; it can also be bound to one unit.
4. **`enqueue(ON_KILL)`** hands the event record to the module when the trigger fires.
   Records are delivered together in each frame's module phase. **`done()`** creates the
   trigger, or returns an error.
5. **The event view** `KilledEvent` gives the record's fields names: `victim()`,
   `killer()`, `position()`. `owner()` reads a unit's house.
6. **`on_events!`** sends each key's events to its function.

Build, rename, check, replace the mod's `rules.wasm` and play again: for every GI or
conscript killed, the chat shows `Bounty: +200 credits` and the money goes up.

> This section's code is built with the SDK and passes `asset wasm-check`. If what you
> see in a match differs, please report it.

### Want more?

- **Pay every 30 seconds**: write a function `pay()` and register it with
  `vera20k_sdk::on_frame!(every = 450, pay);`. The game runs 15 frames a second, so 450
  frames are 30 seconds. See `sdk/examples/playtest`.
- **Reward only human players**: in `on_kill`, read the house's `IsHuman` first:
  `vera20k_sdk::get(house, Prop::IsHuman) == Ok(1)`.
- **Half damage for a badly hurt unit**: use a decision; see the next section.

## 6. Quick reference

Everything below assumes `use vera20k_sdk::prelude::*;`. SDK functions usually return a
`Result`: an `Err(Status)` says why, such as `Gone` (the object no longer exists),
`NotFound` (no such name), `Refused` (the game refused) or `Unsupported` (not in this
version). The [SDK reference](rust-sdk.md) has every function.

### Lifecycle

```rust
vera20k_sdk::init!(f);                  // once, when the module loads
vera20k_sdk::on_rules!(f);              // once, while the rules load; may change them
vera20k_sdk::on_scenario_start!(f);     // once, when the match starts
vera20k_sdk::on_frame!(f);              // every frame
vera20k_sdk::on_frame!(every = 15, f);  // every 15 frames (one second)
vera20k_sdk::on_events!(KEY => f);      // the events triggers deliver
vera20k_sdk::on_decide!(KEY => f);      // decisions
vera20k_sdk::on_message!(f);            // messages from client modules
```

Use each macro at most once in a module.

### Names and references

A module refers to things in the game through references: `Techno` (a unit or
building), `House`, `Cell`, `TypeRef` (a type), `Waypoint`. Look names up once, at
start, and keep the result:

```rust
let gi = TypeRef::techno_type("E1")?;        // a unit type
let warhead = TypeRef::warhead("Super")?;    // a warhead
let cell = Cell::at(50, 60);                 // a cell by its coordinates
```

### Reading properties

```rust
let hp = vera20k_sdk::get(unit, Prop::Health)?;   // any property
let money = vera20k_sdk::get(house, Prop::Money)?;
let hp = unit.health()?;                          // common ones have getters
let owner = unit.owner()?;
```

All properties are in [appendix section 7](../rfcs/0001-wasm-extension-modules-api.md#7-properties).
To read many objects' properties, `get_many` is faster than one read at a time.

### Searching

```rust
// the first GI owned by house
let first = Find::objects().owned_by(house).of_type(gi).first();

// every human player, a page at a time (at most page's length per page)
let mut page = [0i64; 16];
let (count, next) = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).page(&mut page, 0)?;
```

`near(cell, radius)` (the radius in leptons, 256 to a cell) and `within(cell, width,
height)` narrow a search. Results come in creation order; while `next` is not 0 there is
another page (even when this one was not full, or empty). A `Find` holds at most 8
filter clauses.

### Actions

```rust
Action::add_money(house, 500)?;                     // credits (negative takes them)
Action::damage(unit, 50, warhead)?;                 // damage with a warhead
Action::heal(unit, 50)?;                            // heal
Action::kill(unit, Techno(NO_HANDLE))?;             // kill; the second argument gets the credit
let tank = Action::create_object(cell, rhino, house)?;  // a unit on a cell
unit.set_mission(Mission::Hunt)?;                   // change its mission
```

The atomic actions and the map trigger actions are listed in
[appendix section 5](../rfcs/0001-wasm-extension-modules-api.md#5-action-kinds).

### Modifiers (temporary multipliers)

```rust
// firepower ×2 for 150 frames (10 seconds); negative frames: until removed
let boost = Modifier::multiply(unit, Prop::FirepowerMultiplier, fixed(2.0), 150)?;
boost.remove()?;   // remove it early
```

Firepower, armor, speed, rate of fire, range multiplier and extra range work, and
cloakable, forced decloak and disabled weapons; the turn rate (`ROTMultiplier`) and
immunities (`ImmunityFlags`) are `preview`; critical hits and unkillable act only in
builds with Phobos. They stack with Phobos AttachEffects; without modifiers the game
behaves exactly as before.

### Variables

```rust
// Red Alert 2 local and global variables
Var::local(3).set(1)?;
let on = Var::global(5).get();

// the module's own variables: global, house and object scope, saved with the game
vera20k_sdk::vars! {
    static WAVE: GlobalVar = ("Wave", Public, 1);
    static PANICKED: ObjectVar = ("Panicked", Private, 0);
}
WAVE.set(2)?;
PANICKED.set(unit, 1)?;
```

A module has one `vars!` with all its variables.

### Decisions (changing the game's judgement as it is made)

The game asks the module at the moment it decides, and the module may change the answer.
For example, half damage for one unit type:

```rust
const HALVE: i32 = 1;

fn start() {
    if let Ok(gi) = TypeRef::techno_type("E1") {
        let _ = Trigger::on(Event::TAKE_DAMAGE).bind(gi).decide_sync(HALVE);
    }
}

fn halve(mut d: TakeDamageDecision) {
    d.set_result(d.previous() / 2);
}

vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_decide!(HALVE => halve);
```

Modules can decide the damage a hit does, whether a type may be built or queued, whether
a house is defeated, whether a player command is accepted, and also shots, retaliation,
destinations, crushing, which ore cell to harvest, deploying, the idle mission, movement
and paths (most of the later ones `preview`); the [SDK reference](rust-sdk.md#decisions)
lists them all. A decision handler **only reads** (`log` works): an action, a property or
variable write there stops the module.

A module that uses both `on_events!` and `on_decide!` writes the second one with
`shared_delivery:`, as in `vera20k_sdk::on_decide!(shared_delivery: HALVE => halve);`:
both macros declare the delivery area, and a module has only one.

### Missions and orders for computer houses

```rust
let units = [tank];
computer_house.order(&Order::move_to(&units, Cell::at(60, 40)))?;
```

`House::order` works only for computer houses. The order runs next frame, checked like a
player's click. One order carries at most 64 units.

To command units of **any** house, a player's too, use missions: they act in the name of
the unit's house, as if its player had clicked.

```rust
tank.set_mission(Mission::Hunt)?; // a property write, no target
Action::set_mission(tank, Mission::Move, Cell::at(60, 40))?; // an action, with a target cell
```

### Changing the rules as they load

```rust
fn rules() {
    let _ = Rules::set("E1", "Strength", "250");   // GIs get 250 health
}
vera20k_sdk::on_rules!(rules);
```

`on_rules` runs once while the rules load; the change holds for the whole match, the same
on every machine. Once the match runs, `Rules::set` is refused.

## 7. Without code: triggers in INI

When you only need stock actions and functions the module already has, declare triggers
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

To call the module's function, write `Action1=Wasm:OnKill`, the name given in
`named_actions!`.

Trigger section keys:

| Key | Value |
| --- | --- |
| `Event`, `Event<n>` | An event name, or `name(op,interval,id,value)`; several events must all hold |
| `Where<n>` | `Subject.`, `Other.` or `House.` and a property, or `Var(scope,name)`, then a comparison (`=`, `!=`, `<`, `<=`, `>`, `>=`) and a number |
| `Action`, `Action<n>` | `Wasm:name`, or `action(target,parameters…)` |
| `Decide` | `Wasm:name`, a decision handler |
| `House` | A country name; empty means the subject's own house |
| `Difficulty` | `Easy`, `Normal`, `Hard`, comma-separated |
| `Repeat` | `OneTimeOr` (default), `OneTimeAnd`, `Repeating` |

A mistake keeps the match from starting, with a message naming the section and key. The
full syntax is in the [guide](guide.md#triggers-in-ini).

A map cannot bind types with `Wasm.Tags`: in a map it is ignored, and the match says so
when it starts. A map's module binds in code.

## 8. Put the module in a map

A map can carry its own module, loaded when players choose the map:

```sh
asset wasm-embed original.map rules.wasm new.map   # write the module in
asset wasm-check new.map                           # check the map's module
asset wasm-extract new.map extracted.wasm          # take the module out
```

`wasm-embed` writes `[VeraWasm] Api=` and `RulesSize=` for you. Also put
`VeraMinVersion=` in the map's `[Basic]`, set to the VERA20K version you made it with:
older versions then refuse the map clearly instead of starting it without the module.

`wasm-embed` writes only the simulation module (`rules.wasm`). No tool can put a map's
client module (`ui.wasm`) in yet; ship it in a mod for now.

Put the new map in `Maps` in the VERA20K folder and it shows in the map list.

A mod can control map modules in its own `rulesmd.ini`:

```ini
[VeraWasm]
MapWasm=no        ; maps with modules cannot start under this mod
MapBudget=100000  ; operations per frame for a map's module (default 100000, at most 500000)
```

## 9. Debugging

**Logs**

- `log("...")` shows in the chat list, at most 8 lines a frame and 160 characters a line.
- Every log line goes in full to the game's log file `logs/ra2.log`, in the VERA20K
  folder of section 4.1.

**When a module stops**

The chat shows a red line such as `Map module stopped in on_frame: ...`, with the function
and the reason. Common reasons:

| Reason in the line | Meaning | What to do |
| --- | --- | --- |
| `exceeded its operation budget` | Too much work in one call | Spread it over frames (`every = N`), read in batches with `get_many` |
| `trapped: …` | The code crashed (index out of bounds, `panic!`, a failed `unwrap`) | Handle `Err` with `if let` / `match` |
| `made an invalid call: …` | The API was used wrongly: an argument out of range, a variable of the wrong scope, a write in a decision handler; the reason follows | Check the call against the [SDK reference](rust-sdk.md) |
| `cannot keep up: … event records are waiting` | More events than the module can handle | Bind fewer objects or add filter clauses with `where_prop` |

**`wasm-check` refuses the module**

| Message | Cause |
| --- | --- |
| An unsupported feature (reference types, SIMD, …) | `.cargo/config.toml` is missing or changed; copy it again from an example |
| Imports `env` / `wasi_*` | The standard library or a crate that needs the system. Add `#![no_std]` and use crates that don't |
| A `start` section | Don't use code that makes a start function; put start-up code in `init!` |
| `zero byte expected` | `core::fmt` was used (`write!`, `format_args!`, `{:?}`). That part of the precompiled `core` has an instruction encoding the game refuses. Turn numbers into text yourself, as `write_decimal` in `sdk/examples/panic-triggers` does |

**The match won't start**

Usually an INI trigger mistake, a budget in `[VeraWasm]` below 20,000, a map that needs a
newer game, or a map with a module under a mod that says `MapWasm=no`. The message at the
start names the section and key.

## 10. Rules and limits

**Determinism**

Online, every machine runs the same module, so the module must give the same result on
every machine:

- For random numbers use the SDK's `random(lo, hi)`. It uses the module's own random
  stream, the same sequence on every machine.
- Floats work, and NaN is made the same value everywhere. Still, prefer integers or
  `fixed()` for game values.
- There is no clock. Count time with `frame()`; the game runs 15 frames a second.

**Resource limits**

| What | Limit |
| --- | --- |
| Module size | 8 MiB |
| Memory | 32 MiB for a mod's module, 16 MiB for a map's |
| Operations per call (the budget) | 500,000 for a mod's module by default and at most, 100,000 for a map's by default; the mod sets them with `Budget` and `MapBudget` in `[VeraWasm]` (at least 20,000, at most 500,000) |
| Start-up calls (`init`, `on_rules`, `on_scenario_start`) | 5,000,000 each |
| Triggers per module | 256 |
| Filter clauses per search | 16 in the engine, 8 in the Rust SDK's `Find` |
| Orders for computer houses per frame | 256, with 4,096 units in all; at most 64 units an order |

**Saves**

A save keeps the module's memory, variables, modifiers and triggers. Loading runs no
module code again; the module continues from the moment of the save.

**Performance**

- Look names up once at start and keep them in `static`s.
- Read many values at once with `get_many`.
- Spread work over the whole map across frames, for example a part of the objects each
  frame.

## More

- [Guide](guide.md): where modules go, INI syntax, limits and debugging
- [Rust SDK reference](rust-sdk.md): every SDK function, event and action
- [Client module tutorial](rust-ui-tutorial.md): a button on the screen
  (`ui.wasm`)
- [RFC 0001](../rfcs/0001-wasm-extension-modules.md): the design; the
  [appendix](../rfcs/0001-wasm-extension-modules-api.md): every import, event, action and
  property
- [Development status](status.md): what this build implements and what is
  missing
- Examples in `sdk/examples/`: the [guide's list](guide.md#examples) says what each
  shows; `tutorial-bounty` is this tutorial's.
