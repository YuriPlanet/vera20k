# Wasm modules for mods and maps

A mod or a map can carry a small program, a WebAssembly module, that adds game rules
the INI keys cannot express: a unit that panics when hurt, a reward for the first
kill, a wave every two minutes, a computer opponent with its own plan. This guide
shows how to make one with the Rust SDK and ship it. The design is
[RFC 0001](rfcs/0001-wasm-extension-modules.md); every function, event, action and
property is listed in its [appendix](rfcs/0001-wasm-extension-modules-api.md).

This is a preview. Names and numbers may still change before the API is released
(RFC open question 5); the game tells you when a module needs a newer API.

## What a module can and cannot do

A simulation module (`rules.wasm`) runs inside the game on every player's machine,
in step with the match:

- It reads the game through properties (an object's health, a house's money, a
  cell's terrain), finds objects, houses and cells, and keeps its own variables.
- It changes the game only through the game's own rules: actions (damage, create a
  unit, give money, launch a superweapon), property writes, modifiers (firepower,
  armor, speed), orders for computer houses, and the map trigger actions.
- It reacts to events through triggers, made in code or declared in INI.
- It can edit the rules once, while they load (`on_rules`).

It runs in a sandbox. It has no files, no network and no clock, and it cannot reach
the rest of your computer. A module that crashes, loops or breaks the API is stopped
on every machine at the same moment; the match goes on without it and the players
are told which module stopped and why.

## Where modules go

| Source | File | Policy |
| --- | --- | --- |
| A mod | `Mods/MyMod/rules.wasm`, beside the mod's `rulesmd.ini` | `[VeraWasm]` in the mod's `rulesmd.ini` |
| A map | Inside the map, in `[VeraWasmRulesPack]` (written by `asset wasm-embed`) | The selected mod's `[VeraWasm]` |

The mod's module runs first, then the map's. Both see the same game.

### The mod's `[VeraWasm]`

Read from the mod's own `rulesmd.ini` only, never from a map:

```ini
[VeraWasm]
Api=1.0          ; the API the mod's module was built for
Budget=200000    ; operations per frame for the mod's module (at most 200000)
MapBudget=50000  ; operations per frame for a map's module under this mod
MapWasm=yes      ; no: maps carrying a module cannot start with this mod
MapUI=overlay    ; for interface modules (later): overlay, full or no
```

Every player's copy of `rulesmd.ini` is compared in the lobby, so these keys are the
same for everyone.

### A map

A map with a module needs `[Basic] VeraMinVersion=` set to the VERA20k version it was
made for; older versions then refuse it with a message instead of playing without the
module. `asset wasm-embed` writes `[VeraWasm] Api=` and `RulesSize=` for you.

## Build a module with the Rust SDK

You need Rust with the `wasm32-unknown-unknown` target
(`rustup target add wasm32-unknown-unknown`).

1. Copy an example from `sdk/examples/` in the VERA20k source, such as
   `minimal-rules` (pays a house every 15 frames) or `panic-triggers` (a trigger made
   in code). Keep its `.cargo/config.toml`: it targets Wasm and turns off the
   WebAssembly features the game refuses (reference types, SIMD and others; RFC
   section 3).
2. Point its `Cargo.toml` dependency at `sdk/rust/vera20k-sdk`.
3. Write your code. A minimal module:

   ```rust
   #![no_std]
   use vera20k_sdk::prelude::*;

   fn tick() {
       if frame() % 15 == 0 {
           let _ = Action::add_money(House::from_index(0), 100);
       }
   }

   vera20k_sdk::on_frame!(tick);
   ```

4. Build: `cargo build --release`. The module is
   `target/wasm32-unknown-unknown/release/<name>.wasm`; rename it `rules.wasm`.
5. Check it: `asset wasm-check rules.wasm` runs the game's load checks and prints
   what it found or why it would be refused.

The `asset` tool is part of VERA20k (`cargo run --release --bin asset -- <verb> …` in
the source tree). Its Wasm verbs need no game install:

| Command | Does |
| --- | --- |
| `asset wasm-check <module.wasm or map>` | Runs the load checks |
| `asset wasm-embed <map> <rules.wasm> <out map>` | Writes the module into a copy of the map |
| `asset wasm-extract <map> <out.wasm>` | Takes a map's module out |

### What the SDK gives you

- Exports: `on_scenario_start!`, `on_frame!` (`every = N` for every Nth frame),
  `on_events!` for trigger deliveries, `on_rules!`, `init!`; `named_actions!` names
  the functions INI triggers call with `Wasm:name`.
- References: `Techno`, `House`, `Cell`, `Waypoint`, `TypeRef`, `Global`. Look names
  up once, at start: `TypeRef::techno_type("E1")`, `TypeRef::weapon(..)`,
  `anim_id("MyAnim")`.
- Reads: `get(reference, Prop::Health)` for any property, or the getters such as
  `Techno::health`, `Techno::experience`, `Techno::transport`, `Cell::passable`,
  `Global::mission_timer`. `get_many` reads many in one call.
- Searches: `Find::objects()`, `Find::houses()`, `Find::cells()`, narrowed with
  `near`, `within`, `of_type`, `owned_by` and `where_prop`, paged with `page`.
- Changes: `Action::*` (one function per action), `set`, `Modifier::multiply` and
  friends, `Var` and the `vars!` declarations, `House::order` for computer houses.
- Triggers: `Trigger::on(Event::TAKE_DAMAGE).bind(..).where_prop(..).enqueue(..)`.

### Viewpoint properties

`Relation` (self, ally, enemy or neutral), a cell's `Revealed` and `Placeable` depend
on whose view they are read for. They answer `Unsupported` to a plain `get`. In a
trigger's filter clause they are read for the trigger's house; in a search, for the
house given with `Find::seen_by`. `Placeable` works only in a `Find::cells()` search
with a building type (`of_type`): it keeps the cells where that house could place the
building with its top-left corner there.

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
Action2=Wasm:giPanic              ; a function the module names in vera.actions
Repeat=Repeating
```

`Wasm.Tags` goes on a TechnoType, a weapon or a warhead section. On a weapon or
warhead, every event of the trigger must carry one (`Fire`, `Killed`). Projectile
sections are accepted where the game can bind them, and refused with a message
otherwise.

Trigger section keys:

| Key | Value |
| --- | --- |
| `Event`, `Event<n>` | `name` or `name(op,interval,id,value)`. All events must hold. `id` is a number; for `PropertyPoll` a property name, for `VariablePoll` a variable name |
| `Where<n>`, `Event<k>.Where<n>` | `Subject.`, `Other.` or `House.` and a property, or `Var(Global\|Subject\|Other\|House,name)`, then `=`, `!=`, `<`, `<=`, `>` or `>=` and a number. `Where<n>` belongs to event 1 |
| `Action`, `Action<n>` | `Wasm:name` (`Wasm:mod.name`, `Wasm:map.name` for the other source), or `name(target,parameters…)` |
| `Decide` | `Wasm:name`, a decision handler |
| `House` | A country name; empty means the subject's owner |
| `Difficulty` | `Easy`, `Normal`, `Hard`, comma-separated |
| `Repeat` | `OneTimeOr` (default), `OneTimeAnd`, `Repeating` |

An action's target is `Subject` (or empty), `Other`, `TagHolder`, `Projectile`,
`House` or `Cell`; with `Cell` a waypoint number comes first. The parameters follow
the action's row in appendix section 5, in order, empty ones 0:

- A type, weapon, warhead, superweapon, TeamType, variable, property, animation,
  voxel animation or particle system takes its name or its number.
- `Mission` takes a mission name (`Hunt`, `Guard`, `Wait`, …), `EndFor` takes `Win`
  or `Lose`, `Teleport` takes `Nearby` or `Fail`, `Modify` takes `Add`, `Multiply`
  or `Override`; numbers work too.
- A house is a country name; a cell is a waypoint number.
- Sounds take their `[SoundList]` number.

A mistake stops the match from starting, with a message that names the section and
key.

A map cannot bind types: a `Wasm.Tags` in a map is ignored, and the match says so
when it starts. A map's module binds its triggers in code (`Trigger::on(..).bind(..)`).

## Limits

| What | Limit |
| --- | --- |
| Module size | 8 MiB |
| Memory | 32 MiB for a mod's module, 16 MiB for a map's |
| Work per frame | 200,000 operations by default and at most; the mod sets its own and the map's |
| Start-up calls (`init`, `on_rules`, `on_scenario_start`) | 5,000,000 operations each |
| Functions, function size | 20,000 functions, 1 MiB per function body |
| Search | 16 clauses per query |
| Trigger chains | 8 deep and 256 firings per frame; the rest wait for the next frame (at most 4096) |
| Deliveries | A module that leaves records waiting for more than 16 frames is stopped |
| Orders | 64 orders and 4096 units per module per frame, computer houses only |

Hot paths should look names up once, read many values with `get_many`, and stagger
work over frames (`on_frame!(every = 15, …)`, or by object).

## Saves and online play

Saves keep each module's memory and state; loading never runs module code again.
Online, every player runs the same module bytes in step, and the game checks the
modules' memory along with the rest of the match, a fifteenth of it each frame, so a
difference shows within 15 frames.

## More

- [RFC 0001](rfcs/0001-wasm-extension-modules.md): the design, in particular
  sections 2 (mods and maps), 3 (loading and errors), 6 (triggers, actions,
  properties, variables, decisions, orders) and 15 (performance).
- [The appendix](rfcs/0001-wasm-extension-modules-api.md): every import, event,
  action, property and enum, with the INI keys.
- [Development status](rfcs/0001-wasm-extension-modules-status.md): what this build
  implements so far.
- `sdk/examples/` in the source: `minimal-rules`, `modifier-vars`, `panic-triggers`,
  `actions-demo`, `orders-rules`, `decide-damage`.
