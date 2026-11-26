# Wasm modules for mods and maps

[English](guide.md) | [简体中文](guide.zh-CN.md)

A mod or a map can carry small programs, WebAssembly modules, that add what INI keys
cannot express: a unit that panics when hurt, a reward for the first kill, a wave every
two minutes, a computer opponent with its own plan, a shop button on the screen. This
guide says what modules can do, where they go, how to build and ship them, and what the
limits are.

| Document | For |
| --- | --- |
| This guide | What modules are, where they go, INI triggers, limits, debugging |
| [Tutorial: a simulation module](rust-tutorial.md) ([中文](rust-tutorial.zh-CN.md)) | A first `rules.wasm` step by step, from installing Rust to a bounty mod in a skirmish |
| [Tutorial: a client module](rust-ui-tutorial.md) ([中文](rust-ui-tutorial.zh-CN.md)) | A first `ui.wasm`: a button that buys reinforcements |
| [Rust SDK reference](rust-sdk.md) ([中文](rust-sdk.zh-CN.md)) | Every function, event, action and limit of the Rust SDK |
| [RFC 0001](../rfcs/0001-wasm-extension-modules.md) and its [appendix](../rfcs/0001-wasm-extension-modules-api.md) | The design, and every import, event, action, property and enum with its number |
| [Development status](status.md) | What this build implements, for contributors |

This is a preview. Names and numbers may still change before version 1 of the API; the
game tells you when a module needs a newer API. Module authors can use the [Rust SDK](rust-sdk.md)
or the [TypeScript SDK](ts-sdk.md).

## Two kinds of module

**A simulation module (`rules.wasm`)** runs inside the match on every player's machine,
in step:

- It reads the game through properties (an object's health, a house's money, a cell's
  terrain), searches for objects, houses and cells, and keeps its own variables.
- It changes the game only through the game's own rules: actions (damage, create a unit,
  give money, launch a superweapon), property writes, modifiers (firepower, armor,
  speed), missions for any unit (a player's too), orders for computer houses, and the map
  trigger actions.
- It reacts to events through triggers, made in code or declared in INI, and can change
  the game's decisions as they are made (the damage a hit does, whether a unit may be
  built, where a unit drives).
- It can edit the rules once, while they load (`on_rules`), and give modders INI keys of
  its own.

**A client module (`ui.wasm`)** runs only on the local machine, once per drawn frame. It
shows what this player may see: it adds buttons, texts, pictures, bars and lists to the
match interface and changes the layout's parts, moves the camera, plays sounds and
registers hotkeys. It sends requests to its simulation module, which validates them.
`Client::order` can also send standard player orders when the room allows automation.

Both run in a sandbox: no files, no network, no clock, nothing else on your computer. A
simulation module that crashes, loops or breaks the API is stopped on every machine at
the same moment; the match goes on without it and the players are told which module
stopped and why. A client module that fails is stopped on that machine only, with its
nodes.

## Where modules go

| Module | In a mod | In a map |
| --- | --- | --- |
| `rules.wasm` | `Mods/<Mod>/rules.wasm`, beside the mod's `mod.ini` | Inside the map file, in `[VeraWasmRulesPack]` (written by `asset wasm-embed`) |
| `ui.wasm` | `Mods/<Mod>/ui.wasm` | Inside the map file, in `[VeraWasmUIPack]` (no tool writes it yet; see [a map](#a-map)) |

The mod's modules run first, then the map's. Both see the same game. A client module
talks to the simulation module of its own source: the mod's `ui.wasm` to the mod's
`rules.wasm`, the map's to the map's.

### The mod's `[VeraWasm]`

The mod controls modules with `[VeraWasm]` in its own `rulesmd.ini` (never read from a
map). Every key may be left out:

```ini
[VeraWasm]
Api=1.0            ; the module API the mod's modules were built for
Budget=500000      ; operations per frame for the mod's rules.wasm (default and most: 500000)
MapBudget=100000   ; operations per frame for a map's rules.wasm (default 100000, most 500000)
MapWasm=yes        ; no: maps carrying a rules.wasm cannot start with this mod
MapUI=overlay      ; a map's ui.wasm and [VeraUI] parts: overlay (default), full or no
Controller=no      ; yes: the mod module controls computer seats marked "mod AI" in the launch data (RFC 6i)
SeatBudget=500000  ; operations each "mod AI" seat adds to the mod's budget
```

- A budget below 20,000 refuses the match; one above the most is lowered to it.
- `MapUI=overlay`: the map's client module may change only its own nodes, drawn in an
  overlay above the match interface. `full`: it may change any node, as the mod's can.
  `no`: the map's `ui.wasm` and its `[VeraUI]` parts are left out.
- A player can lower the mod's budget with `[VERA] WasmBudget=` in `VERA.INI`; online,
  the host's value is the room's.
- Every player's copy of `rulesmd.ini` is compared in the lobby, so these keys are the
  same for everyone.

A mod's `rulesmd.ini` replaces the game's whole file. A mod that only adds a module
doesn't need one; a mod that needs `[VeraWasm]` or INI triggers starts from a full copy
(see [modding](../modding.md)).

### A map

- `asset wasm-embed <map> rules.wasm <out map>` writes the module into a copy of the
  map, with `[VeraWasm] Api=` and `RulesSize=`.
- Set `[Basic] VeraMinVersion=` to the VERA20K version you made the map with. A build
  older than that refuses the map with a message instead of playing it without the
  module.
- `[VeraWasm] StorageId=` names the map's client module storage (default: the map's file
  name).
- A map can add interface parts for its own client module in `[VeraUI]`
  ([interface layout](../interface-layout.md#a-maps-own-parts)).
- A map cannot bind INI triggers to types: a `Wasm.Tags` in a map is ignored, and the
  match says so when it starts. A map's module binds its triggers in code.
- A map's `ui.wasm` goes in `[VeraWasmUIPack]` with `[VeraWasm] UISize=`, in the same
  format as the rules pack (base64 of chunked LZO, 70 characters per numbered line). The
  game reads it, but `asset wasm-embed` writes only the rules module, so for now ship a
  client module in a mod.

## Build a module

You need Rust with the WebAssembly target: `rustup target add wasm32-unknown-unknown`.

1. Copy an example from `sdk/examples/` in the VERA20K source: `minimal-rules` or
   `tutorial-bounty` for a simulation module, `ui-reinforcements` for a pair of modules.
   Keep its `.cargo/config.toml`: it targets Wasm and turns off the WebAssembly features
   the game refuses (reference types, SIMD and others).
2. Point its `Cargo.toml` dependency at `sdk/rust/vera20k-sdk`.
3. Write the code. A minimal simulation module:

   ```rust
   #![no_std]
   use vera20k_sdk::prelude::*;

   fn tick() {
       let _ = Action::add_money(House::from_index(0), 100);
   }

   vera20k_sdk::on_frame!(every = 15, tick);
   ```

4. `cargo build --release`. The module is
   `target/wasm32-unknown-unknown/release/<name>.wasm` (a `-` in the crate name becomes
   `_`); rename it `rules.wasm` or `ui.wasm`.
5. Check a `rules.wasm`: `asset wasm-check rules.wasm` runs the game's load checks and
   prints `{"bytes":…,"ok":true}` or why it would be refused.

The `asset` tool is part of VERA20K (`cargo build --release --bin asset` in the source
tree). Its Wasm verbs need no game install:

| Command | Does |
| --- | --- |
| `asset wasm-check <rules.wasm or map>` | Runs the load checks on a simulation module, or on the one a map carries |
| `asset wasm-embed <map> <rules.wasm> <out map>` | Writes the module into a copy of the map |
| `asset wasm-extract <map> <out.wasm>` | Takes a map's simulation module out |

`wasm-check` knows only simulation modules: given a `ui.wasm` it answers that the module
imports `vera:client`. A client module is checked when the match loads it; a problem is
shown in the chat list and the match plays without it.

## Put it in a mod

```text
Mods/MyMod/
├── mod.ini        the mod's name and description
├── rules.wasm     the simulation module (optional)
├── ui.wasm        the client module (optional)
└── rulesmd.ini    only if the mod needs [VeraWasm], INI triggers or other rules changes
```

```ini
; mod.ini
[Mod]
Name=My Mod
Version=1
About=Credits for every infantry kill.
```

The `Mods` folder is in the VERA20K folder: beside `vera20k.exe` on Windows,
`~/Library/Application Support/VERA20k/` on macOS, `~/.local/share/VERA20k/` on Linux.
Choose the mod in **Options → Mods**, or start the game with `vera20k --mod MyMod`. A
mod that failed to start is skipped once on the next start.

Online, every player needs the same mod; the lobby compares it.

## Triggers in INI

A mod can bind triggers to types in its own `rulesmd.ini`, without code, and call the
module's functions from them by name.

```ini
[E1]
Wasm.Tags=GIPanicTag              ; every E1 gets this tag's triggers

[GIPanicTag]
Trigger=GIPanic                   ; one or more trigger sections, comma-separated

[GIPanic]
Event=TakeDamage
Where1=Subject.HealthPercent<30   ; all clauses must hold
Where2=Var(Subject,Panicked)=0
Action1=SetVar(Subject,Panicked,1)
Action2=Wasm:giPanic              ; a function the module names in named_actions!
Repeat=Repeating
```

`Wasm.Tags` goes on a TechnoType, a weapon or a warhead section. On a weapon or warhead,
every event of the trigger must carry one (`Fire`, `Killed`). Projectile sections are
accepted where the game can bind them, and refused with a message otherwise.

Trigger section keys:

| Key | Value |
| --- | --- |
| `Event`, `Event<n>` | `name` or `name(op,interval,id,value)`. All events must hold. `id` is a number; for `PropertyPoll` a property name, for `VariablePoll` a variable name. `Wasm:name` is a module event |
| `Where<n>`, `Event<k>.Where<n>` | `Subject.`, `Other.` or `House.` and a property, or `Var(Global\|Subject\|Other\|House,name)`, then `=`, `!=`, `<`, `<=`, `>` or `>=` and a number. `Where<n>` belongs to event 1 |
| `Action`, `Action<n>` | `Wasm:name` (`Wasm:mod.name`, `Wasm:map.name` for the other source), `Wasm:name(target,text)`, or `name(target,parameters…)` |
| `Decide` | `Wasm:name`, a decision handler |
| `House` | A country name; empty means the subject's owner |
| `Difficulty` | `Easy`, `Normal`, `Hard`, comma-separated |
| `Repeat` | `OneTimeOr` (default), `OneTimeAnd`, `Repeating` |

An action's target is `Subject` (or empty), `Other`, `TagHolder`, `Projectile`, `House`
or `Cell`; with `Cell` a waypoint number comes first. The parameters follow the action's
row in [appendix section 5](../rfcs/0001-wasm-extension-modules-api.md#5-action-kinds), in
order, empty ones 0:

- A type, weapon, warhead, superweapon, TeamType, variable, property, animation, voxel
  animation or particle system takes its name or its number.
- `Mission` takes a mission name (`Hunt`, `Guard`, `Wait`, …), `EndFor` takes `Win` or
  `Lose`, `Teleport` takes `Nearby` or `Fail`, `Modify` takes `Add`, `Multiply` or
  `Override`; numbers work too. `Push(target,cell,distance,frames)` and
  `SetFacing(target,facing)` take numbers.
- A house is a country name; a cell is a waypoint number.
- Sounds take their `[SoundList]` number.

A mistake stops the match from starting, with a message that names the section and key.

## INI names a module registers

A module can give modders INI keys of its own: the modder writes INI, the module
implements it ([RFC 0001 section 6h](../rfcs/0001-wasm-extension-modules.md#6h-ini-registration)).

```ini
[HTNK]
Kratos.Shield=HeavyShield          ; a key on a type
Wasm.Tags=ShieldMedicTag

[Kratos.ShieldTypes]               ; a registry: a list of entry sections
1=HeavyShield

[HeavyShield]                      ; an entry section, read by the module
Strength=300

[ShieldMedicTag]
Trigger=ShieldMedic

[ShieldMedic]
Event=Wasm:Kratos.ShieldBroken     ; a module event
Action1=Wasm:restoreShield(Subject,50)   ; a module action with target and text
```

| Registration (Rust) | INI | The module receives |
| --- | --- | --- |
| `ini::key(name, IniSection::TechnoType, KEY)` | `name=value` in sections of that kind (`TechnoType`, `Weapon`, `Warhead`, `Projectile`, `SuperWeapon`, `Country`, `General`) | One `IniKeyRecord` per section holding the key |
| `ini::registry(name, KEY)` | A list section `[name]` of entry section names | One `IniEntryRecord` per entry, in list order |
| `ini::event(name, EventScope::Object)` | `Event=Wasm:name` in a trigger | The event's ID, raised with `Action::raise_event` or INI `RaiseEvent(target,name,value,value2)` |

```rust
const SHIELD_TYPES: i32 = 10;
const SHIELD: i32 = 11;
static SHIELD_BROKEN: ini::EventSlot = ini::EventSlot::new();

fn init() {
    let _ = ini::registry("Kratos.ShieldTypes", SHIELD_TYPES);
    let _ = ini::key("Kratos.Shield", IniSection::TechnoType, SHIELD);
    if let Ok(event) = ini::event("Kratos.ShieldBroken", EventScope::Object) {
        SHIELD_BROKEN.set(event);
    }
}

fn load_shield_type(r: IniEntryRecord) {
    let (mut name, mut value) = ([0u8; 64], [0u8; 32]);
    let Ok(section) = r.entry_name(&mut name) else { return };
    let _strength = Rules::get(section, "Strength", &mut value).ok().and_then(ini::int);
    // keep it in module memory
}

fn attach_shield(r: IniKeyRecord) {
    let mut value = [0u8; 64];
    let _shield = r.value(&mut value); // "HeavyShield", for the type r.type_ref()
}

vera20k_sdk::init!(init);
vera20k_sdk::on_events!(SHIELD_TYPES => load_shield_type, SHIELD => attach_shield);
```

- Register only in the module's `init`; anywhere else stops the module. A name is 1 to 63
  ASCII letters, digits, `.` and `_`, compared ignoring case. A name the mod or map
  already registered, or one starting with `Vera.`, is `Err(Refused)`.
- Order: the mod's `init` and `on_rules`, the map's rules, the map's `init` and
  `on_rules`, then INI triggers are read (an unregistered `Event=Wasm:name` keeps the
  match from starting). Before each module's `on_scenario_start`, it receives all its
  `IniEntry` records, then all its `IniKey` records. Values are the final merged rules
  text (mod and map), not checked by the engine: parse them yourself and stop with
  `fail(...)`, naming the section and key, when one is wrong.
- Loading a save delivers nothing again: what the handlers built is in the module's
  memory, which the save keeps.

`sdk/examples/ini-shield`, `ini-aura`, `ini-counter` and `ini-academy` are complete
mods configured this way, each with a `rulesmd.ini` and a README in Chinese.

## Debugging

**Logs.** `log("...")` (`client::log` in a client module) writes to the chat list (8 lines
a frame, 160 characters each) and to `logs/ra2.log` in the VERA20K folder, under target
`ext_module`. Write numbers into a byte buffer yourself: `core::fmt` cannot be used (see
below).

**A module that stopped.** The chat list shows a red line such as
`Map module stopped in on_frame: ...`, naming the function and the reason:

| Reason | Meaning | What to do |
| --- | --- | --- |
| `exceeded its operation budget` | One call used more than its budget | Spread the work over frames (`on_frame!(every = N, …)`, a part of the objects each frame); read many values with `get_many` |
| `trapped: …` | The code crashed: out-of-bounds index, `panic!`, a failed `unwrap` | Handle every `Err` with `if let` or `match` |
| `made an invalid call: …` | An argument out of range, a variable of the wrong scope, a write in a decision handler, a registration outside `init` | Check the call against the [SDK reference](rust-sdk.md) |
| `cannot keep up: … event records are waiting` | More trigger records than the module handles | Bind fewer objects, add filter clauses (`where_prop`) |

**`wasm-check` refuses the module.**

| Message | Cause |
| --- | --- |
| An unsupported feature (reference types, SIMD, …) | `.cargo/config.toml` is missing or changed; copy it again from an example |
| Imports `env` or `wasi_*` | The standard library or a crate that needs the system. Use `#![no_std]` and crates without system calls |
| Imports `vera:client@1.0` | A `ui.wasm`; `wasm-check` checks only simulation modules |
| A `start` section | Move start-up code into `init!` |
| `zero byte expected` | `core::fmt` (`write!`, `format_args!`, `{:?}`) was used; its precompiled code carries an encoding the game refuses |

**A match that won't start.** Usually an INI trigger mistake, a budget below 20,000, a
map that needs a newer VERA20K, or a map module under a mod with `MapWasm=no`. The
message names the section and key.

## Limits

Simulation modules:

| What | Limit |
| --- | --- |
| Module size | 8 MiB |
| Memory | 32 MiB for a mod's module (8 MiB more per "mod AI" seat), 16 MiB for a map's |
| Work per call | The budget: 500,000 operations by default for a mod's module, 100,000 for a map's (`[VeraWasm] Budget`, `MapBudget`; at most 500,000, at least 20,000) |
| Work per frame | Further event batches start only while the frame's use is below the budget; `on_frame` always gets a whole budget; only a single call over its budget stops the module. A frame stays below three budgets plus decisions |
| Decisions | 20,000 operations each, out of a pool of half the frame budget (`ProductionAllowed` the frame budget, `Defeat` a quarter) |
| Start-up calls (`init`, `on_rules`, `on_scenario_start`) | 5,000,000 operations each |
| Functions | 20,000 functions, 1 MiB per function body |
| Searches | 16 clauses per query in the engine, 8 in the Rust SDK's `Find`; 16,384 candidates per call |
| Triggers | 256 per module, 16 of them object polls; chains 8 deep and 256 firings per frame, the rest waits (at most 4,096) |
| Deliveries | 1,024 records a frame; a backlog of more than 16,384 records stops the module |
| House orders | 256 orders and 4,096 unit entries per module per frame, 64 units per order, computer houses only. Missions do not count here and work on every house |
| Notifications | 256 bytes each, 256 a frame per module |
| Messages from client modules | 96 bytes each; per player and source 2 a frame and 16 in 15 frames |
| INI registrations | 4,096 per module |

Client modules:

| What | Limit |
| --- | --- |
| Module size, memory | 8 MiB; 32 MiB for a mod's, 16 MiB for a map's |
| Work per render frame | 100,000 operations; `client_init` and `client_on_start` 5,000,000 each |
| Events | 1,024 a render frame |
| Nodes | Half of what the interface admits from others, per module |
| Hotkeys | 256 per module |
| Orders (`Client::order`) | Each unit once per 15 frames, house-wide orders 4 per 15 frames; only where the room allows automation |
| Storage | Keys of 1 to 64 ASCII bytes, values up to 16 KiB, 256 keys and 256 KiB per mod or map |

Hot paths should look names up once, read many values with `get_many`, and stagger work
over frames.

## Saves and online play

- Saves keep each simulation module's memory, variables, modifiers and triggers; loading
  never runs module code again, and the module continues where it was. Client modules
  are not saved: after a load they start afresh and read the saved variables.
- Online, every player runs the same module bytes in step. The game checks the modules'
  memory along with the rest of the match, a fifteenth of it each frame, so a difference
  shows within 15 frames.
- Determinism: use `random(lo, hi)` (the module's own stream, the same on every machine)
  and `frame()` (15 frames a second at normal speed); there is no clock. Floats work, with
  NaN made the same everywhere, but prefer integers or `fixed()` for game values. Client-side
  animation and random effects run locally.
- A browser build runs the same modules with the browser's own WebAssembly engine and
  plays in the same rooms.

## Examples

All in `sdk/examples/` of the VERA20K source; each builds with `cargo build --release`.

| Example | Shows |
| --- | --- |
| `minimal-rules` | The smallest module: money and a variable |
| `tutorial-bounty` | The [tutorial](rust-tutorial.md)'s bounty mod |
| `playtest` | Pay every 30 seconds, half damage through a decision |
| `panic-triggers` | A trigger made in code, numbers without `core::fmt` |
| `modifier-vars` | Modifiers and declared variables |
| `actions-demo` | Many actions |
| `orders-rules` | Rules edits in `on_rules` and orders for a computer house |
| `decide-damage` | A decision |
| `veteran-fury` | Kill and damage events, object and global variables, modifiers, money, healing and created units, orders |
| `siege-doctrine` | `on_rules` and the production, queue, command and defeat decisions |
| `meteor-storm` | Cell searches and properties, a `Timer` trigger, warheads and animations on cells, `random`, the mission timer, lightning storms |
| `trigger-lab` | `Birth`, `Fire` on a weapon, `PassengerAdded`, the polls, immediate actions, `once`, enabling and disabling triggers, house variables, `get_many` |
| `ini-shield`, `ini-aura`, `ini-counter`, `ini-academy` | Mods configured only in INI through registered names |
| `ai-takeover` | A module that takes over a computer house and plays it with orders (RFC 6i) |
| `control-bench` | A benchmark mod that reaches nearly every simulation control point (README in Chinese) |
| `ui-ping` | The smallest pair: a `ui.wasm` sends a message, the `rules.wasm` answers with a notification |
| `ui-reinforcements` | A button that buys a tank: message, check, variable, notification ([client tutorial](rust-ui-tutorial.md)) |
| `muster-field` | A small auto-battler campaign with a full interface on panels; text in Simplified Chinese |

`muster-field` shows two patterns worth copying. Player orders become input through
`CommandAllowed`: the handler may not write, so it refuses the order and queues it in
module memory for the next `on_frame`. And the simulation publishes the camp as private
house variables, written when they change, so its `ui.wasm`, which restarts after a
load, shows the camp at once. Each button stands for one of the camp's commands (hire,
keep, redraw, ...) and sends a two-byte message that the simulation turns into that
command. It needs one human player, a computer opponent and
Bases on.
