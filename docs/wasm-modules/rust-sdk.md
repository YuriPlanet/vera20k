# Rust SDK reference

[English](rust-sdk.md) | [简体中文](rust-sdk.zh-CN.md)

Every type, function and macro of the Rust SDK for Wasm modules, `vera20k-sdk`
(`sdk/rust/vera20k-sdk` in the VERA20K source). The first part covers simulation
modules (`rules.wasm`), the second client modules (`ui.wasm`). The
[guide](guide.md) says where modules go and what the limits are; the tutorials
walk through a [simulation module](rust-tutorial.md) and a
[client module](rust-ui-tutorial.md).

This reference follows the SDK source. Numbers (property, event and action IDs) are the
ones the SDK and this engine use; the [RFC appendix](../rfcs/0001-wasm-extension-modules-api.md)
lists the same entries with their full semantics.

**Tiers.** The API is a preview (crate version 0.1.0). The RFC sorts every entry into a
tier: `v1` (a candidate for version 1), `preview` (works now, may change without notice)
and `later` (not in version 1; the engine answers `has_kind` 0 and use returns
`Unsupported`). Where an entry below is not `v1`, it says so. Everything else is `v1`,
which is still subject to change until version 1 is frozen.

## Contents

- [Setup](#setup)
- Simulation modules: [exports](#exports), [basic calls](#basic-calls),
  [references](#references), [properties](#properties), [modifiers](#modifiers),
  [variables](#variables), [searches](#searches), [actions](#actions),
  [missions and house orders](#missions-and-house-orders), [rules](#rules),
  [triggers](#triggers), [events](#events), [decisions](#decisions),
  [INI registration](#ini-registration), [notifications and messages](#notifications-and-messages)
- Client modules: [overview](#client-modules), [client exports](#client-exports),
  [client calls](#client-calls), [reading the game](#reading-the-game),
  [nodes](#nodes), [the player, camera and cursor](#the-player-camera-and-cursor),
  [local actions](#local-actions), [client events](#client-events),
  [storage](#storage)

## Setup

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
vera20k-sdk = { path = "../../rust/vera20k-sdk" }   # adjust to where the SDK is

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
strip = true
```

Keep the examples' `.cargo/config.toml`: it builds for `wasm32-unknown-unknown` with
only the WebAssembly features the game accepts (the MVP plus mutable globals, sign
extension, non-trapping float-to-int, bulk memory and multi-value).

- The crate is `#![no_std]`. Its default feature `panic-handler` supplies a panic
  handler that traps, which stops only your module. Turn the feature off if your module
  brings its own.
- `use vera20k_sdk::prelude::*;` brings in what a simulation module usually needs;
  `use vera20k_sdk::client::prelude::*;` the same for a client module.
- Most calls return `Result<T>`, that is `core::result::Result<T, Status>`:

  | `Status` | Meaning |
  | --- | --- |
  | `Gone` | The reference is 0 or the object no longer exists |
  | `NotFound` | No such name or ID, or a missing waypoint |
  | `Refused` | The game refused: an occupied cell, a value it cannot write now, a full quota, a buffer too small |
  | `Unsupported` | Not available in this match or this build, or an entry of tier `later` |

  A wrong call is not an `Err`: an invalid argument (a scope that does not match the
  variable, a negative random range, a write in a decision handler, ...) stops the
  module.
- Don't use `core::fmt` (`write!`, `format_args!`, `{:?}`) in a module: the precompiled
  `core` brings an instruction encoding the load checks refuse (`zero byte expected`).
  Write numbers into a byte buffer yourself, as `write_decimal` in
  `sdk/examples/panic-triggers` does.

---

# Simulation modules

A simulation module (`rules.wasm`) imports `vera:sim@1.0` and runs on every machine in
step with the match. All functions below are in the crate root (`vera20k_sdk::...`)
unless a module path is shown.

## Exports

Each macro exports one function the engine calls. A module uses each macro at most once.

| Macro | Exports | When the engine calls it |
| --- | --- | --- |
| `init!(f)` | `vera_init` | Once, before anything else: at the rules stage for a module that also exports `on_rules`, otherwise when the match is installed. The only place for `ini::*` registrations |
| `on_rules!(f)` | `on_rules` | Once, while the rules load (after `rulesmd.ini`, before they are processed). `Rules::set` writes any key here |
| `on_scenario_start!(f)` | `on_scenario_start` | Once, when the match starts, after the module's INI records |
| `on_frame!(f)` / `on_frame!(every = N, f)` | `on_frame` | Every frame, or on frames divisible by `N` (15 frames are one second at normal speed) |
| `on_events!(KEY => f, ...)` | `on_events` | In the module phase, with the records its triggers queued. Each handler takes `&EventRecord`, an event view (`KilledEvent`, ...) or an INI record view |
| `on_decide!(KEY => f, ...)` | `on_decide` | Synchronously, at the engine's decision point, for decision triggers. Each handler takes `&mut DecideRecord` or a decision view |
| `on_message!(f)` | `on_message` | In the module phase before `on_events`, once per message a client module of the same source sent; `f(sender: House, data: &[u8])` |
| `named_actions! { NAME = "Name" => key, ... }` | section `vera.actions` | Declares the module's named actions: INI triggers call them as `Wasm:Name`; the record arrives in `on_events` with dispatch key `key`. Also defines `pub const NAME: i32 = key` |
| `vars! { static X: GlobalVar = ("Name", Visibility, default); ... }` | section `vera.vars` | Declares the module's variables ([variables](#variables)) |
| `notify_names! { pub static X: Notify = "Name"; ... }` | section `vera.notify` | Declares the notification names the module sends ([notifications](#notifications-and-messages)) |

- `on_events!`, `on_decide!` and `on_message!` each declare the module's delivery area
  (section `vera.delivery`) as well. A module that uses more than one of them writes the
  later ones with `shared_delivery:`, so the area is declared once:

  ```rust
  vera20k_sdk::on_events!(ON_KILL => on_kill);
  vera20k_sdk::on_decide!(shared_delivery: HALVE => halve);
  vera20k_sdk::on_message!(shared_delivery: buy);
  ```

- `named_actions!`, `vars!` and `notify_names!` each write one custom section; a second
  call of the same macro appends a second blob, and the engine refuses the module. List
  everything in one call.
- Dispatch keys are your own numbers. Keep the keys of `named_actions!`,
  `Trigger::enqueue`, `Trigger::decide_sync` and the `ini::*` registrations distinct.
- Don't name your handler `on_message` (or any other export's name): the macro already
  exports that symbol.

The order at the start of a match: the mod's `vera_init` and `on_rules`, the map's rules,
the map's `vera_init` and `on_rules`; INI triggers are read; then, per module, its INI
records (`on_events`) and its `on_scenario_start`. The mod's module runs before the
map's every frame.

## Basic calls

| Function | Does |
| --- | --- |
| `frame() -> i32` | The current frame |
| `random(lo, hi) -> i32` | A number in `lo..=hi` from the module's own random stream, the same on every machine. `lo > hi` stops the module. In a decision handler it draws from a substream and leaves the main stream alone |
| `log(line: &str)` | A line in the chat list (8 a frame at most, 160 characters shown) and in `logs/ra2.log` (target `ext_module`) |
| `fail(why: &str) -> !` | Logs `why` and stops the module for good on every machine. For a value you cannot use, such as a malformed registered INI key; name the section and key |
| `lookup(kind: NameKind, name) -> Result<i32>` | The ID of `name`. Look names up once, at start, and keep the IDs |
| `name_of(kind, id, buf) -> Result<&str>` | The name of `id`, written into `buf`; `Err(Refused)` when `buf` is too small |
| `text(id, buf) -> Result<&str>` | Engine-held text by string ID (`NameKind::String`): a module action's parameter text, an INI value. ID 0 is `""` |
| `sys::has_kind(table, id) -> i32` | 1 when this engine implements entry `id` of `table` (0 event kind, 1 action kind, 2 property, 3 name kind, 4 search scope). Raw binding; the SDK has no wrapper on the simulation side |

`NameKind` values: `TechnoType`, `Weapon`, `Warhead`, `Projectile`, `Anim`, `VoxelAnim`,
`ParticleSystem`, `SuperWeapon`, `Country`, `TeamType`, `Variable`, `ModuleAction`,
`String`, `Property`, `ActionKind`, `ModuleEvent` (v1); `Notify`, `Hotkey` (with client
modules); `Armor`, `Overlay`, `Sound`, `ScriptType`, `AiTrigger`, `EventKind` are
declared but this engine answers `Unsupported` for them.

## References

A reference is an `i64` handle. The SDK wraps each kind in a type that implements the
`Ref` trait, which `get`, `set` and `find` take. `NO_HANDLE` (0) is no object; every
reference type has `is_none()`.

| Type | What | Make one with |
| --- | --- | --- |
| `Techno` | A unit or building | From a search, an event or `Action::create_object` |
| `Projectile` | A flying bullet (preview, 7.4) | From a `Launch` or `ProjectileFrame` decision |
| `House` | A house (player or computer side) | `House::from_index(i)` (house array index), `techno.owner()`, a search |
| `Cell` | A map cell | `Cell::at(x, y)`; `cell.x()`, `cell.y()` |
| `Waypoint` | A map waypoint | `Waypoint::number(n)` |
| `TypeRef` | A rules type | `TypeRef::techno_type("E1")`, `::weapon`, `::warhead`, `::projectile`, `::super_weapon`; `TypeRef::new(kind, id)`; `TypeRef::any(kind)` (a wildcard for `Find::types`); `ty.id()` |
| `Global` | The global rules reference (7.8) | `Global` |
| `Modifier` | A modifier | `Modifier::new` and friends |
| `TriggerRef` | A trigger | `Trigger::...done()` |

`anim_id(name) -> Result<i32>` gives an animation's ID (its position in the rules'
animation list), which `Action::spawn_anim` and `Action::attach_anim` take.

`House::from_index` is provisional: the engine encodes houses as bit 47 over the house
index (RFC open question 5).

## Properties

| Function | Does |
| --- | --- |
| `get(reference, prop) -> Result<i64>` | The effective value of `prop` (with modifiers applied) |
| `set(reference, prop, value) -> Result<()>` | Writes `prop` through the game's own code |
| `get_many(refs: &[i64], props: &[i32], out: &mut [i64]) -> Result<usize>` | Reads `props` of every reference, row-major (`out[i * props.len() + j]`), in one call. Returns the first failing status, the empty values written |
| `Prop::named(name) -> Result<Prop>` | A property by name |

Every property is a constant: `Prop::Health`, `Prop::Money`, `Prop::FirepowerMultiplier`, ...
A `.` in a name is written `_` (`Prop::Passable_Foot` is `Passable.Foot`,
`Prop::Factory_Vehicle_Ready` is `Factory.Vehicle.Ready`). The full list with value
types, writability and tiers is [appendix section 7](../rfcs/0001-wasm-extension-modules-api.md#7-properties);
`sdk/rust/vera20k-sdk/src/props.rs` has the numbers.

Writable with `set`: `Health` (no kill credit; 0 or less is `Refused`), `Owner`, `Rank`,
`Mission`, `Ammo`, `Online`, a trigger's `Enabled`, `House` and `Repeat`, a house's
`ControlDomains`, a superweapon's `ChargePercent`. The modifiable properties
([modifiers](#modifiers)) refuse `set`.

Getters that make one `get` each:

| Type | Getters |
| --- | --- |
| `Techno` | `health`, `type_ref`, `owner`, `layer` (`Layer`), `target` (object or cell reference), `experience` (percent: 100 veteran, 200 elite), `storage` (bales), `transport`, `passenger_count`, `transport_space_left`, `disguise_type`, `disguise_owner`, `moving`, `facing` (0..255), `turret_facing`, `armor`, `mission`, `ammo` |
| `Techno` (setters) | `set_health`, `set_owner(house)`, `set_mission(Mission)`, `set_ammo` (clamped to `Ammo=`), `set_online(bool)` (buildings) |
| `House` | `money` |
| `Cell` | `land_type`, `height`, `overlay` (`TypeRef`), `ore` (credits), `waypoint` (-1 for none), `passable(SpeedType)` |
| `Global` | `Global::frame`, `difficulty` (0 easy, 1 normal, 2 hard), `mission_timer`, `mission_timer_running`, `ambient_light`, `ion_storm_active` |

Enums for property values: `Category` (`Infantry`, `Vehicle`, `Aircraft`, `Building`,
`Projectile`), `Layer`, `Relation` (`SameHouse`, `Ally`, `Enemy`, `Neutral`), `SpeedType`
(`Foot` ... `FloatBeach`, with `passable()` giving its cell property), `Mission`.

**Viewpoint properties.** `Relation`, a cell's `Revealed` and `Placeable`, `Visible`,
`Engage` and a TechnoType's `Buildable` depend on whose view they are read for. A plain
`get` answers `Unsupported`. Read them in a trigger's filter clause (for the trigger's
house) or in a search with `Find::seen_by(house)`. `Placeable` works only in a
`Find::cells()` search with a building type (`of_type`).

## Modifiers

A modifier changes a property of an object for a while and goes away by itself. Modifiers
from different modules and Phobos AttachEffects stack; without modifiers the game
computes exactly what it did before.

| Function | Does |
| --- | --- |
| `Modifier::multiply(target, prop, factor, frames)` | Multiplies by a fixed-point `factor` (`fixed(1.5)`) |
| `Modifier::add(target, prop, value, frames)` | Adds `value` (for example `WeaponRangeExtra` in leptons) |
| `Modifier::override_(target, prop, value, frames)` | Replaces the value while it lasts |
| `Modifier::new(target, prop, op, value, frames, source, group)` | The general form: `ModifierOp` (`Add`, `Multiply`, `Override`), a source object (or 0) and an exclusion group (0 none; a second modifier of this module in the same group on the same object replaces the first) |
| `modifier.remove()` | Removes it early; `Err(Gone)` once it expired |
| `fixed(x) -> i64`, `FIXED_ONE` | Fixed-point values (16 fraction bits): `fixed(1.0) == FIXED_ONE == 65536` |

`frames` below 0 lasts until removed. Read a modifier with `get(modifier, Prop::Remaining)`
(also `Property`, `Op`, `Value`, `Source`, `Group`, `Origin`); an expired one reads as
`Gone`. `Action::modify` and `Action::remove_modifiers` are the action forms.

The modifiable properties that work in this build:

| Property | Ops | Tier |
| --- | --- | --- |
| `FirepowerMultiplier`, `ArmorMultiplier` | Add, Multiply, Override | v1 |
| `SpeedMultiplier` (ground units, aircraft, jumpjets, rockets) | Add, Multiply, Override | v1 |
| `ROFMultiplier`, `WeaponRangeMultiplier`, `WeaponRangeExtra` | Add, Multiply, Override | v1 |
| `Cloakable`, `ForceDecloak`, `DisableWeapons` | Override | v1 |
| `ROTMultiplier` (the body's turn rate of vehicles and buildings, not turrets or aircraft) | Multiply | preview |
| `ImmunityFlags` (bits 0..6: mind control, radiation, poison, psychic damage, chrono, EMP, berserk) | Override | preview |
| `CritMultiplier`, `CritExtraChance`, `Unkillable` | as listed in the appendix | later; they act only in Phobos builds |

The other modifiable properties of appendix 7.2 are `later`.

## Variables

**Red Alert 2 variables**: `Var::local(n)` (local 0..99) and `Var::global(n)` (global
0..49). `var.get() -> i64`, `var.set(value) -> Result<()>`.

**Declared variables** belong to the module. They are saved, hashed and can be read by
client modules according to their visibility. Declare them all in one `vars!`:

```rust
vera20k_sdk::vars! {
    pub static WAVE: GlobalVar = ("Wave", Public, 0);
    pub static SCORE: HouseVar = ("Score", Allies, 0);
    pub static PANICKED: ObjectVar = ("Panicked", Private, 0);
}

fn example(house: House, unit: Techno) -> vera20k_sdk::Result<()> {
    WAVE.set(WAVE.get()? + 1)?;
    SCORE.set(house, 10)?;
    PANICKED.set(unit, 1)
}
```

| Type | Calls | Scope |
| --- | --- | --- |
| `GlobalVar` | `get()`, `set(value)` | One value per match |
| `HouseVar` | `get(house)`, `set(house, value)` | One value per house |
| `ObjectVar` | `get(object)`, `set(object, value)` | One value per object; it goes with the object |

Visibility (`vars::Visibility`) says which client modules may read it: `Private` (the
house's own player), `Allies`, `InSight` (object scope only), `Public`. The name is ASCII;
another source's variable is found as `mod.Name` or `map.Name`. Each accessor looks up
its ID on first use.

## Searches

`Find` builds a query; nothing reaches the engine until `page` or `first`.

| Call | Does |
| --- | --- |
| `Find::objects()` | Units and buildings, by creation order |
| `Find::houses()` | Houses, by index (no area, type or owner filter) |
| `Find::cells()` | Cells, by cell index (y, then x); narrow with `near` or `within` |
| `Find::types(ty)` | Types (TechnoTypes, Weapons, Warheads): `TypeRef::any(kind)` lists every type of the kind |
| `.near(center: Cell, radius)` | Within `radius` leptons of the cell (256 leptons are one cell) |
| `.within(top_left, width, height)` | In a cell rectangle |
| `.of_type(ty)`, `.owned_by(house)` | Type and owner filters |
| `.where_prop(prop, op, value)` | Keeps candidates whose `prop` compares (`Op::Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`) |
| `.seen_by(house)` | Reads viewpoint properties for `house` |
| `.page(&mut out, after) -> Result<(count, next)>` | Fills `out` from position `after` (0 first); `next` is where the next page starts, 0 on the last |
| `.first() -> i64` | The first match or `NO_HANDLE`, following pages |

```rust
// Every human house, one page of 16.
let mut humans = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1);
let mut page = [0i64; 16];
let (count, _) = humans.page(&mut page, 0)?;
for house in &page[..count] {
    let _ = Action::add_money(House(*house), 1000);
}
```

- A `Find` holds at most 8 clauses (`MAX_CLAUSES`); more are dropped. The engine itself
  accepts 16.
- One call examines at most 16,384 candidates, and fewer when they would cost more than
  half the call's budget. So a short or even empty page is the last only when `next` is
  0; keep calling while `next != 0`.
- Cost: 2 operations per candidate, 6 per clause on each candidate (60 more for
  `Engage`, `NearbyEnemyCount`, `TargetInRange`, `CanFireAtTarget`), 10 per result.

## Actions

`Action` has one function per action the engine runs. Each is one host call and returns
`Result<()>` (`create_object` returns the new `Techno`). Invalid parameters stop the
module; a game refusal is `Err(Refused)`.

**Atomic actions** (appendix 5.3):

| Function | Does | Tier |
| --- | --- | --- |
| `damage(target, amount, warhead)`, `damage_from(target, amount, warhead, source)` | Damage under armor rules; `source` gets kill credit and experience | v1 |
| `kill(target, source)` | Kills through the damage path, crediting `source` (`Techno(NO_HANDLE)` for none) | v1 |
| `remove(target)` | Removes without death, survivors or credit | v1 |
| `heal(target, amount)` | Restores health up to Strength, no credit | v1 |
| `create_object(at, kind, house) -> Techno` | Creates a unit or building at a cell or waypoint; an occupied cell is `Refused` | v1 |
| `set_mission(target, mission, at)`, `set_mission_on(target, mission, on)` | Gives a mission aimed at a cell/waypoint (`Cell(NO_HANDLE)` for none) or an object | v1 |
| `teleport(target, to, Blocked::Nearby \| Blocked::Fail)` | Chronoshifts a vehicle or infantryman | v1 |
| `clear_target`, `clear_disguise`, `clear_mind_control` | As named | v1 |
| `add_experience(target, points)` | The experience a kill worth `points` credits gives | v1 |
| `add_money(house, amount)` | Credits; negative takes them | v1 |
| `allow_type(house, kind, allow)` | `false` forbids a TechnoType to the house, `true` lifts this module's forbid | v1 |
| `set_owner(target, house)` | Gives the object to the house | v1 |
| `add_ammo(target, amount)` | Within the type's `Ammo=` | v1 |
| `add_passenger(transport, passenger)` | Boards at once if the transport would load it | v1 |
| `set_property(target, prop, value)` | The action form of `set` | v1 |
| `modify(target, prop, op, value, frames, group) -> Modifier`, `remove_modifiers(target, group, prop)` | Action forms of modifiers | v1 |
| `detonate_weapon(at, weapon, source)` | Detonates a weapon at a cell or object without a projectile | v1 |
| `detonate_warhead(at, warhead, damage)` | A warhead's area damage | v1 |
| `fire_weapon(shooter, slot)`, `fire_weapon_at(shooter, slot, at)`, `fire_weapon_at_cell(shooter, slot, at)` | Fires weapon slot 0 or 1 now; `Refused` when it cannot fire (reloading, out of range) | v1 |
| `launch_super_weapon(house, kind, at)` | Launches the house's superweapon | v1 |
| `spawn_anim(at, anim)` | Plays an animation (`anim_id`) | v1 |
| `attach_anim(target, anim)`, `attach_anim_for(target, anim, frames)`, `detach_anim(target, anim)` | Plays an animation once attached to the object, or keeps it looping for `frames` frames (negative: until `detach_anim`); `detach_anim` with -1 removes all this module keeps there | preview |
| `end_for(house, Outcome::Win \| Outcome::Lose)` | Ends the match for a house | v1 |
| `set_score(house, n)`, `add_score(house, n)` | Score | v1 |
| `log(line)` | The action form of `log` | v1 |
| `convert(target, kind)`, `convert_for(target, kind, frames)` | Changes the object's type, for good or for a while | v1 |
| `raise_event(event, target, other, value, value2)`, `raise_event_with_house(...)` | Raises a module event ([INI registration](#ini-registration)); not in a decision handler | v1 |
| `push(target, toward: Cell, distance, frames)` | Shoves a ground unit `distance` leptons toward a cell (negative: away) over `frames` (0: one per 64 leptons), on top of its movement; 0 ends a push | preview |
| `set_facing(target, facing)` | Turns the body toward 0..255 | preview |

**Stock actions** (the map trigger actions, appendix 5.2) have functions named after
what they do: `reveal_all`, `play_speech("EVA...")`, `show_text(csf)`,
`play_anim_at(waypoint, anim)`, `create_team(team)`, `reinforce_at(team, waypoint)`,
`nuke_strike(waypoint)`, `lightning_storm_strike(waypoint, house)`,
`create_building_at(waypoint, kind, house)`, `set_tech_level(kind, level)`,
`super_weapon_set_charge(house, kind, percent)`, `change_house(target, house)`,
`sell_building(target)`, `timer_set(seconds)`, `set_global(n)`, `force_trigger(id)`, and
about 70 more; each function's doc comment names its stock action number. A TeamType
comes from `TypeRef::new(NameKind::TeamType, lookup(NameKind::TeamType, name)?)`.

`Action::run(kind, target, &params)` and `Action::run_text(kind, target, text)` run any
action number with raw `sys::ActionParams`; `Action::FIRE_WEAPON` ... `Action::SET_FACING`
are the atomic numbers.

## Missions and house orders

Two ways to command units:

- **Missions** work on every house, human or computer, as that house's own click would:
  `techno.set_mission(Mission::Hunt)` (untargeted) or
  `Action::set_mission(unit, Mission::Move, Cell::at(60, 40))`. They pass through the
  house's `CommandAllowed` decisions, except a module's own.
- **House orders** give a computer house player commands for many units at once and for
  production: `house.order(&Order::...)`. The order runs next frame, checked as that
  house's player input. A human player's house is `Refused`.

```rust
let units = [tank];
computer.order(&Order::attack_move(&units, Cell::at(60, 40)))?;
computer.order(&Order::queue_production(TypeRef::techno_type("HTNK")?))?;
```

`Order` constructors: `move_to`, `move_to_object`, `stop`, `attack`, `attack_move`,
`guard` (an object, or `Techno(NO_HANDLE)` to guard in place), `deploy_mcv`,
`set_rally`, `queue_production(ty)`, `cancel_production(ty)`,
`place_ready_building(ty, cell)`, `sell`, `launch_super_weapon(ty, cell)`,
`take_product(kind)` (2 infantry, 3 vehicle, 4 naval, 5 aircraft), and
`Order::new(command, target, value, units)` for any `Command`. `EjectBunker` is `later`.

- One `Order` carries at most 64 units (`ORDER_UNITS`); a module sends at most 256 orders
  and 4,096 unit entries a frame. Missions don't count against this.
- Refused and executed orders can be watched with the `Commanded` and `OrderRefused`
  events.

## Rules

| Function | Does |
| --- | --- |
| `Rules::get(section, key, buf) -> Result<&str>` | The merged rules text of `[section] key` (mod and map); `NotFound` for a missing key |
| `Rules::set(section, key, value)` | Writes INI text. Any key in `on_rules`, processed like the rest of the rules, the same on every machine. During a match it answers `Refused` |

```rust
fn rules() {
    let _ = Rules::set("E1", "Strength", "250");
}
vera20k_sdk::on_rules!(rules);
```

## Triggers

A trigger is "when this event happens to a bound object, check these clauses, then run
these actions". `Trigger` builds one in module memory; `done()` creates it.

```rust
vera20k_sdk::named_actions! { ON_KILL = "OnKill" => 1 }

fn start() {
    let mut trigger = Trigger::on(Event::KILLED);
    for name in ["E1", "E2"] {
        if let Ok(infantry) = TypeRef::techno_type(name) {
            trigger = trigger.bind(infantry);
        }
    }
    if trigger.enqueue(ON_KILL).done().is_err() {
        log("the trigger was refused");
    }
}
```

| Builder call | Does |
| --- | --- |
| `Trigger::on(kind)` | A repeating trigger on event `kind`, every difficulty |
| `Trigger::every(frames)` | A `Timer` trigger, every `frames` from its creation |
| `Trigger::on_property(prop, op, value, interval)` | `PropertyPoll`: fires when `prop op value` becomes true, checked every `interval` frames |
| `Trigger::on_variable(var, op, value, interval)` | `VariablePoll` |
| `Trigger::on_house_power(op, value, interval)` | `HousePowerPoll` (output minus drain of the trigger's house) |
| `Trigger::on_modifier_count(group, op, count, interval)` | `ModifierCountPoll` |
| `Trigger::on_distance(cell, op, leptons, interval)` | `DistancePoll` |
| `Trigger::on_module_event(event)` | A module event ([INI registration](#ini-registration)) |
| `.and(kind)` | Another event; all must occur |
| `.value(v)`, `.id(id)` | The latest event's arguments (a stock event's house index or amount, a type ID) |
| `.where_prop(prop, op, value)`, `.where_other(..)`, `.where_house(..)`, `.where_var(var, op, value)` | Filter clauses on the latest event: on the subject, the other object, the trigger's house, a global variable |
| `.bind(reference)` | An object, a TechnoType (every object of the type, including later ones), a weapon, warhead or projectile type (for `Fire`), or a cell (stock events 25 and 26) |
| `.house(house)` | The trigger's house, instead of the subject's owner |
| `.once()` | Fires once per instance |
| `.disabled()` | Starts disabled |
| `.immediate(ActionDef)` | Runs an action at once when it fires |
| `.enqueue(key)`, `.enqueue_with(key, target, text)` | Queues a record for `on_events` with dispatch key `key` |
| `.done() -> Result<TriggerRef>` | Creates and binds the trigger |
| `.decide_sync(key) -> Result<TriggerRef>` | Creates it as a decision trigger ([decisions](#decisions)); it takes no actions |

- A builder holds 4 events, 4 clauses per event, 8 actions and 8 bindings. One more of
  any makes `done()` answer `Refused` without creating anything; bind more with
  `TriggerRef::bind` afterwards.
- A module has at most 256 triggers, and at most 16 object polls among them.
- Records are delivered in the module phase, at most 16 per `on_events` call and 1,024 a
  frame. A backlog over 16,384 records stops the module.
- Chains: a firing caused by a trigger's own actions nests 8 deep and 256 times a frame;
  the rest waits for the next frame (at most 4,096 waiting).

`TriggerRef`: `bind(reference)`, `unbind(reference)`, `remove()`, `set_enabled(bool)`.

**`ActionDef`** (immediate actions): `add_money(amount)` (the trigger's house),
`add_money_to(house, amount)`, `set_var(var, value)`, `heal(target, amount)`,
`set_property(target, prop, value)`, `log(line)`, `raise_event(target, event, value,
value2)`, `module(key)`, `module_with(key, target, text)`, and
`ActionDef::new(kind, target, params)` for any action. `Target` says what the action acts
on: `Subject`, `Other`, `TagHolder`, `Projectile`, `House`, `Cell`.

## Events

Event kinds are constants of `Event`. Delivered events reach `on_events` as an
`EventRecord`; a view reads it with named fields.

`EventRecord` methods: `trigger()`, `subject()`, `other()`, `holder()`, `house()`,
`args()`, `args_text(buf)`, `target()`; raw fields `key`, `kind`, `projectile`,
`modifier`, `cell`, `value`, `value2`, `id`, `frame`, `x`, `y`, `z`.

| Event | View: fields | Tier |
| --- | --- | --- |
| `TAKE_DAMAGE` | `TakeDamageEvent`: `victim`, `attacker`, `damage` (after armor), `warhead`, `position` | v1 |
| `KILLED` | `KilledEvent`: `victim`, `killer`, `position`; `KilledByWarheadEvent` adds `warhead` | v1 |
| `FATAL_DAMAGE` | `FatalDamageEvent`: `victim`, `attacker`, `damage`, `warhead`, `position` | v1 |
| `ATTACKED_BY` | `AttackedByEvent`: `target`, `attacker`, `weapon_slot`, `projectile` | v1 |
| `FIRE` | `FireEvent`: `shooter`, `target`, `target_cell`, `weapon_slot`, `weapon`, `projectile` | v1 |
| `TARGET_CHANGED` | `TargetChangedEvent`: `object`, `target`, `target_cell` | v1 |
| `DETONATED` | `DetonatedEvent`: `source`, `projectile`, `warhead`, `cell`, `position` | v1 |
| `CRUSHED` | `CrushedEvent`: `victim`, `crusher`, `position` | v1 |
| `BIRTH` | `BirthEvent`: `object`, `producer` | v1 |
| `TRANSPORT_ENTERED`, `TRANSPORT_EXITED` | `TransportEnteredEvent`, `TransportExitedEvent`: `passenger`, `transport`, `space`, `garrison` | v1 |
| `PASSENGER_ADDED`, `PASSENGER_REMOVED` | `PassengerAddedEvent`, `PassengerRemovedEvent`: `transport`, `passenger`, `space`, `garrison` | v1 |
| `HOUSE_TRANSFERRED` | `HouseTransferredEvent`: `object`, `old_house`, `mind_control` | v1 |
| `MODIFIER_START`, `MODIFIER_EXPIRE` | `ModifierEvent`: `target`, `modifier`, `property`, `source` | v1 |
| `MODIFIER_STACKS` | `ModifierStacksEvent`: `target`, `modifier`, `count` | v1 |
| `TIMER` | `TimerEvent`: `last_frame` | v1 |
| `SUPER_WEAPON_LAUNCHED` | `SuperWeaponLaunchedEvent`: `super_weapon`, `house`, `target_cell` | v1 |
| `DEPLOYED` | `DeployedEvent`: `object`, `became`, `deployed` (1 deployed, 0 undeployed) | v1 |
| `PROMOTED` | `PromotedEvent`: `object`, `rank` (1 veteran, 2 elite) | v1 |
| `EXPERIENCE_GAINED` | `ExperienceGainedEvent`: `object`, `victim`, `gain` | v1 |
| `CLOAKED`, `DECLOAKED` | `CloakEvent`: `object` | v1 |
| `COMMANDED` | `&EventRecord`: `value` the `Command`, `other`/`cell` the target, `value2` a serial per order | v1 |
| `ORDER_REFUSED` | `&EventRecord`: `subject` (0 for a house-wide order), `house`, `value` the `Command`, `value2` the reason (appendix 8.3 `RefuseReason`) | v1 |
| `MODULE_EVENT` | `ModuleEventRecord` | v1 |
| `PROPERTY_POLL`, `HOUSE_POWER_POLL`, `MODIFIER_COUNT_POLL`, `DISTANCE_POLL`, `VARIABLE_POLL` | `PollEvent`: `subject`, `reading` | v1 |
| Stock events 1..61 (`ENTERED_BY`, `DESTROYED_BY_ANY_HOUSE`, `CREDITS_EXCEED`, `ELAPSED_TIME`, `COMES_NEAR_WAYPOINT`, ...) | `StockEvent`: `subject`, `other`, `house`, `bound_cell` | v1 |
| Ares events 62..88 | `StockEvent` | only when Ares is on |
| `MISSION_CHANGED` | `MissionChangedEvent` | later: not raised (`has_kind` 0) |
| `HIT_BY_WARHEAD`, `SPAWN_FIRE`, `HIT_TARGET`, `PROJECTILE_INCOMING`, `HOUSE_TRANSFER`, `PRODUCTION_CANCELLED` | — | later: not raised |

The INI-only records `INI_ENTRY` and `INI_KEY` are delivered without a trigger
([INI registration](#ini-registration)).

## Decisions

A decision trigger asks the module synchronously, at the moment the game decides, and
the module may change the answer. Each request reaches the `on_decide!` handler of the
trigger's key.

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

- A handler only reads (`get`, `find`, `var_get`, `random`, `log`, ...). Any write stops
  the module: `set`, a modifier, a variable write, any `Action::*` (`raise_event` and the
  action `Action::log` included), creating, binding or removing a trigger, `Rules::set`,
  a notification or a house order. To act on a decision, remember it in module memory and
  act in the next `on_frame`.
- Every decision has `native()` (the game's own answer), `previous()` (the answer of the
  module before, the native one for the first) and `set_result(value)`. Without
  `set_result` the previous answer stands. `request()` gives the whole `DecideRecord`
  (`record()`, `area()`).
- Budget: 20,000 operations per request, out of a pool of half the frame budget
  (`ProductionAllowed` uses the frame budget, `Defeat` a quarter of its own).
- Several modules can answer one decision: the mod's module first, then the map's.

| Event | View: fields | Result | Tier |
| --- | --- | --- | --- |
| `TAKE_DAMAGE` | `TakeDamageDecision`: `victim`, `attacker`, `damage`, `warhead`, `position` | The final damage (0 blocks the hit) | v1 |
| `PRODUCTION_ALLOWED` (type binding) | `ProductionAllowedDecision`: `house`, `techno_type` | 1 allow, 0 disallow until next frame; 1 does not lift the game's own refusal | preview |
| `PRODUCTION_QUEUED` | `ProductionQueuedDecision`: `house`, `techno_type` | 1 allow, 0 refuse | v1 |
| `DEFEAT` | `DefeatDecision`: `house` | 1 defeat, 0 veto this check | v1 |
| `COMMAND_ALLOWED` (object or type binding) | `CommandAllowedDecision`: `unit`, `command`, `target`, `target_cell` | 1 allow, 0 refuse (also module orders) | preview |
| `DAMAGE_INCOMING` | `DamageIncomingDecision`: `victim`, `attacker`, `warhead`, `incoming`, `preview`, `position` | The incoming damage; 0 or the same sign | preview |
| `FIRE_ALLOWED` | `FireAllowedDecision`: `shooter`, `target`, `target_cell`, `weapon_slot`, `weapon` | 0 vetoes the shot | preview |
| `RETALIATE` | `RetaliateDecision`: `victim`, `source`, `warhead` | 1 retaliate, 0 not | preview |
| `DESTINATION_SET` | `DestinationSetDecision`: `object`, `destination`, `destination_cell` | A destination reference, 0 for none | preview |
| `CRUSH_ALLOWED` | `CrushAllowedDecision`: `victim`, `crusher` | 0 spares the victim | preview |
| `HARVEST_CELL` | `HarvestCellDecision`: `harvester`, `scanned` | A cell reference, 0 for none | preview |
| `DEPLOY_ALLOWED` | `DeployAllowedDecision`: `object` | 0 refuses the deploy | preview |
| `IDLE_MISSION` | `IdleMissionDecision`: `unit` | A `Mission` number, 255 for none | preview |
| `MISSION_STEP` | `MissionStepDecision`: `unit`, `mission`, `target`; `step()`, `set_target`, `set_destination`, `set_mission(Option<Mission>)`, `set_delay`, `set_step` | What the Guard, Area Guard, Hunt or Attack body decided, partly replaced. The body already ran | preview |
| `MOVE_FRAME` | `MoveFrameDecision`: `unit`, `moving`, `destination`, `destination_cell`, `position`; `step_by(dx, dy)`, `set_step(&MoveStep)` (`MOVE`, `FACING`, `STOP` flags), `previous_step()` | Moves a ground unit (Drive, Ship, Walk, Mech, Hover) by up to 2,048 leptons a frame instead of its locomotor | preview |
| `PATH_FOUND` | `PathFoundDecision`: `unit`, `goal`, `position`; `previous_cell(i)`, `set_cells(&[PathCell])` (up to 24, each next to the one before), `no_route()` | Replaces the route | preview |
| `APPROACH_POINT` | `ApproachPointDecision`: `unit`, `chosen`, `target`, `target_cell`, `position`; `set_cell(cell)` | The cell a unit chasing an out-of-range target drives to | preview |
| `FIRE` (decision form) | `FireDecision`: `shooter`, `target`, `target_cell`, `slot`, `weapon`; `previous_entry(i)`, `set_plan(&[ProjectileSpec])` (up to 16) | The shot plan: the projectiles launched instead of the native one | preview in the SDK |
| `LAUNCH` (weapon, warhead or projectile type binding) | `LaunchDecision`: `projectile`, `launcher`, `target`, `target_cell`; `set_path(PathKind, speed, param, points)` | The bullet's path (`Bezier`, `Parabola`, `Spiral`, `Waypoints`) | preview |
| `PROJECTILE_FRAME` (projectile type binding) | `ProjectileFrameDecision`: `projectile`, `frames`; `set_step(&ProjectileStep)`, `previous_step()` | The bullet's step each frame | preview |
| `FATAL_DAMAGE` (decision form) | `FatalDamageDecision`: the death plan bits `EXPLODE`, `DEATH_WEAPON`, `DEBRIS`, `SURVIVORS`, `CREW`, `ANNOUNCE`, `TIBERIUM_REMAINS`; `with(bit, on)`, `with_inf_death`, `with_death_weapon` | What a death leaves behind | preview in the SDK |

The result layouts (`ProjectileSpec`, `ProjectileStep`, `MoveStep`, `PathCell`,
`MissionStepResult`, `PathSpecHeader`) are in `vera20k_sdk::sys`.

## INI registration

A module can give modders INI names of its own: a key on a type section, a registry (a
list section) and module events. The modder writes INI; the module implements it.
Register in `init!` only; anywhere else stops the module.

| Function | INI | The module receives |
| --- | --- | --- |
| `ini::key(name, IniSection::TechnoType, KEY)` | `name=value` in sections of that kind (`TechnoType`, `Weapon`, `Warhead`, `Projectile`, `SuperWeapon`, `Country`, `General`) | One `IniKeyRecord` per section with the key: `type_ref()`, `is_general()`, `value(buf)`, `section_name(buf)` |
| `ini::registry(name, KEY)` | A list section `[name]` naming entry sections | One `IniEntryRecord` per entry, in list order: `entry_name(buf)`, `position()` |
| `ini::event(name, EventScope::Object \| Global) -> ModuleEvent` | `Event=Wasm:name` in a trigger section | The event's ID, for `Trigger::on_module_event` and `Action::raise_event` |

- Names are 1 to 63 ASCII letters, digits, `.` and `_`, compared ignoring case. A name
  already registered (by any source) or starting with `Vera.` is `Err(Refused)`. 4,096
  registrations per module.
- The records arrive through `on_events!` with the key you gave, before
  `on_scenario_start`: all `IniEntry` records, then all `IniKey` records. They count
  against `on_scenario_start`'s budget. Loading a save delivers nothing again.
- Values are the final merged rules text, not checked by the engine. Read entry sections
  with `Rules::get` and parse with `ini::split`, `ini::int`, `ini::ints::<N>`,
  `ini::yes_no`. Stop with `fail(...)` naming the section and key when a value is wrong.
- `ini::EventSlot` keeps an event ID in a `static` (`set`, `get`); `ModuleEvent::named(name)`
  finds an event any source registered.
- A module action's record carries its target and text: `record.target()`,
  `record.args_text(buf)` (INI `Wasm:name(target,text)`, or `ActionDef::module_with`).

See the [guide](guide.md#ini-names-a-module-registers) for the INI side and
`sdk/examples/ini-shield`, `ini-aura`, `ini-counter` and `ini-academy` for complete mods.

## Notifications and messages

A simulation module and the client modules of the same source (mod or map) talk through
the game, so every machine stays in step:

- **Client to simulation**: a client module's `Client::send_message(&bytes)` reaches the
  simulation module's `on_message!` handler on every machine after the command delay,
  with the sender's house, which the client cannot forge. The bytes are untrusted:
  check every one.
- **Simulation to client**: `NOTIFY.send(house, &bytes)` notifies that house's player;
  `send_observers(&bytes)` the observers. Each client module there receives a `Notify`
  event.
- **Shared state**: a declared variable the client module reads with `ClientVar`. It is
  saved, so a client module that starts again after a load sees it at once.

```rust
vera20k_sdk::notify_names! {
    pub static PONG: Notify = "Pong";
}

fn ping(sender: House, data: &[u8]) {
    if data != [7] {
        return;
    }
    let _ = Action::add_money(sender, 1);
    let _ = PONG.send(sender, &[]);
}

vera20k_sdk::on_message!(ping);
```

| Limit | Value |
| --- | --- |
| Message | 96 bytes (`notify::MAX_MESSAGE_BYTES`); per player and source 2 a frame and 16 in 15 frames; observers can't send |
| Notification | 256 bytes (`notify::MAX_NOTIFY_BYTES`); 256 a frame per module; more is `Refused` |

`Notify::id()` is the name's FNV-1a hash, the same on every machine;
`notify::send_raw(to, id, data)` is the raw call. A module that sends notifications must
declare every name with `notify_names!`.

---

# Client modules

A client module (`ui.wasm`) imports `vera:client@1.0` and runs only on the local machine,
once per render frame. It reads what this player's interface may show, changes the
interface, the camera and the cursor, plays local effects, and reaches the game only
through messages to its simulation module and standard player orders. It may use floats.

Use only `vera20k_sdk::client::*` in a client module. The crate-root functions (`get`,
`log`, `Var`, `Action`, ...) import `vera:sim` functions, and the game refuses a client
module that imports them. The reference types (`House`, `Cell`, `Techno`, `Prop`) and
`Find` and `Order` are shared.

```rust
#![no_std]

use vera20k_sdk::client;
use vera20k_sdk::client::prelude::*;

static BUY: NodeSlot = NodeSlot::new();

fn start() {
    if let Ok(button) = Ui::find("BuyReinforcements") {
        let _ = button.set_clickable(true);
        BUY.set(button);
    }
}

fn on_click(e: &ClientEventRecord) {
    if e.node() == BUY.get() && Client::send_message(&[1]).is_err() {
        client::log("the message was refused");
    }
}

vera20k_sdk::on_start!(start);
vera20k_sdk::on_client_event!(ClientEvent::Click => on_click);
```

## Client exports

| Macro | Exports | When |
| --- | --- | --- |
| `client_init!(f)` | `client_init` | Once after loading. Only `client::lookup`, `name_of`, `has_kind` and `log` work there |
| `on_start!(f)` | `client_on_start` | Once the match interface is built; again after a rejoining player's catch-up and after a save is loaded |
| `on_client_event!(ClientEvent::Kind => f, ...)` | `client_on_events` | Each render frame, before `on_render`, with the frame's events; `f(&ClientEventRecord)` |
| `on_render!(f)` | `client_on_render` | Each render frame; `f(dt: f32, alpha: f32)` |

A save does not carry client modules: after a load they start afresh (`client_init`,
then `client_on_start`), with their storage as they left it. During catch-up (a rejoin,
a watcher catching up) they don't run.

## Client calls

| Function | Does |
| --- | --- |
| `client::frame() -> i32` | The latest finished simulation frame (0 in `client_init`) |
| `client::random(lo, hi)` | A local number for display only; `lo > hi` stops the module |
| `client::log(line)` | A line in the chat list (8 a render frame) and the log file |
| `client::has_kind(table, id) -> bool` | Tables as on the simulation side, plus 5 node kind, 6 client event kind, 7 client action kind |
| `client::lookup(kind, name)`, `client::name_of(kind, id, buf)` | As on the simulation side. Variable and notification names are the ones this match's simulation modules declared; hotkey names this module's |

## Reading the game

| Function | Does |
| --- | --- |
| `client::get(reference, prop)` | A property as this player's interface may show it: an object out of sight is `Err(Gone)`, a hidden property `Err(Refused)` |
| `client::get_many(refs, props, out)` | As `get_many`, each entry under `get`'s visibility |
| `client::find_page(&mut find, &mut out, after)` | A `Find` page restricted to what this player sees |
| `ClientVar::new("Name")` | A simulation module's declared variable, read under its visibility: `get()` (global), `get_house(house)` (a private one only for this player's house), `get_object(object)` |
| `client::var_get(scope, var)` | The raw read; scope 0 for a global variable |

Allies' objects show their `seen` properties only; a spy shows its disguise as the
screen does; spectators read according to the room.

## Nodes

The match interface is a tree of nodes: the parts of the `veraui.ini` layout, the map's
`[VeraUI]` parts and the nodes modules create.

| Function | Does |
| --- | --- |
| `Ui::find(name) -> Result<Node>` | A layout part by its name (without the layout prefix: `Buy` for `[Mine.Buy]`), or a created node by its `Name`. This module's source first, then engine, mod and map |
| `Ui::create(kind, parent)` | A new node; `Node(NO_HANDLE)` as parent is the top level |
| `Ui::create_with(kind, parent, &[(Prop, i64)])` | With up to 16 initial properties in one call |
| `Ui::create_named(kind, parent, name)` | Created and named; removed again if the name is refused |
| `Ui::remove(node)`, `node.remove()` | Removes it and its children |
| `NodeSlot` | Keeps a node in a `static` between callbacks: `set(node)`, `get()` |

`NodeKind`: `Text`, `Button`, `Row`, `Column`, `List`, `Image`, `Bar` (modules create
these); `CameoGrid` (only layouts make it; a module can find it and write its common
properties and `ItemTooltips`).

Node methods: `get(prop)`, `set(prop, value)`, `set_name`, `set_label` (a CSF entry name
or literal text), `set_art` (a picture file by name, such as `mybutton.shp` or
`panel.pcx`), `visible`, `set_visible`, `set_enabled`, `set_clickable`,
`set_hoverable`, `set_dismissable`, `set_position(x, y)`, `set_size(w, h)`,
`set_fill(0..100)` (`Bar`), `set_backdrop(bool)` (a `Text` over the battlefield stands
on black unless off), `set_items(&[&str])` (`List` rows, 1 KiB in all). Other properties
through `set`/`client::set_bytes`: `Hold` (0 `TopLeft`, 1 `Top`, 2 `TopRight`,
3 `Left`, 4 `Middle`, 5 `Right`, 6 `BottomLeft`, 7 `Bottom`, 8 `BottomRight`), `Gap`
(`Row`, `Column`), `ItemTooltips` (`CameoGrid`). `ItemProgress`, `Decoration` and
`MapMarker` are `later`.

- **Who may write what.** The mod's module reads, writes and removes any node. The map's
  module, under the mod's `[VeraWasm] MapUI=overlay` (the default), finds and reads any
  node but writes only the map's own; under `MapUI=full` it has the mod's rights; under
  `MapUI=no` it is not loaded. A write the module may not make is `Err(Refused)`.
- **Where.** A module's top-level nodes are drawn in the panels layer; the map's under
  `MapUI=overlay` in the overlay above the match interface. Coordinates are sidebar
  pixels inside the sidebar and screen pixels elsewhere.
- **How many.** Each module holds at most half of the nodes the interface admits from
  others; a create past it is `Refused`.
- A node created later is drawn over one created before.

The [interface layout](../interface-layout.md#parts-for-interface-modules) guide shows the
layout parts made for modules (a button without `Command`, `Image`, `Bar`, `List`).

## The player, camera and cursor

| Function | Does |
| --- | --- |
| `Client::viewer() -> House` | This player's house, `House(NO_HANDLE)` for a spectator |
| `Client::is_spectator()` | |
| `Client::send_message(&bytes)` | To this source's simulation module's `on_message` ([limits](#notifications-and-messages)); `Refused` for spectators and over the limits |
| `Client::order(&Order)` | A standard player order for this player, checked like player input. Each unit once per 15 frames, house-wide orders 4 per 15 frames. `Refused` unless the room allows automation (global `AutomationAllowed`); the lobby does not offer that switch yet, so it is refused for now |
| `Camera::cell()` | The cell at the camera's centre |
| `Camera::jump_to(cell)`, `Camera::center_on(cell, speed)` | Moves the local camera |
| `Cursor::position() -> (x, y)` | Screen pixels, as node `X` and `Y` count |
| `Cursor::cell()` | The cell under the cursor, `Cell(NO_HANDLE)` off the map |
| `Cursor::capture_map_clicks(bool)` | At true, map clicks reach only this module (`MapClick`), not the game |

## Local actions

Effects on this machine only (`Local`, appendix 5.4):

| Function | Does |
| --- | --- |
| `Local::play_sound(id, cell)` | Plays sound `id` (its `[SoundList]` number) at a cell, or not positioned with `Cell(NO_HANDLE)`. `lookup(Sound, ..)` answers `Unsupported` in this build |
| `Local::play_speech(name)` | EVA speech, such as `"EVA_ReinforcementsHaveArrived"` |
| `Local::select(&units, append)` | Selects units this player may select (64 at most) |
| `Local::flash(object, frames)` | Flashes an object |
| `Local::show_message(text, color)` | One line: a CSF entry name or literal text |
| `Local::register_hotkey(name, "Ctrl+Alt+K")` | A hotkey with its default chord; `Hotkey::id(name)` gives the ID its events carry. `Refused` when a game hotkey has the chord (the hotkey exists without a key). 256 per module |
| `Local::run(kind, target, &params)` | Any client action; `HOME_CAMERA` is `later` |

## Client events

`ClientEvent` kinds, delivered to `on_client_event!` (at most 1,024 a render frame):

| Kind | When | Read |
| --- | --- | --- |
| `Click` | A clickable node was clicked (the click doesn't reach the map) | `node()`, `value` the `List` row or `CameoGrid` item, -1 otherwise |
| `Hover` | The pointer entered (`value` 1) or left (0) a hoverable node | `node()` |
| `Hotkey` | A registered hotkey was pressed | `id` |
| `Dismiss` | A press outside a visible dismissable node, or Esc | `node()` |
| `MapClick` | A map click that was no `Click` or `Dismiss` | `cell()`, `value` 0 left, 1 right |
| `SelectionChanged` | The local selection changed; read it with `find_page` and a `Selected = 1` clause | — |
| `Notify` | A simulation module's notification for this machine | `id`, `data()`, `from_map()`; compare with `NotifyName::is(event)` |
| `SimEvent` | `Commanded`, `OrderRefused`, `TakeDamage`, `Killed`, `Birth` or `AttackedBy` of this player's objects or house | `sim_event() -> Option<&EventRecord>` (`other` is 0 when this player cannot see it) |

`ClientEventRecord` dereferences to `sys::ClientEventRecord` (`kind`, `source`,
`reference`, `id`, `value`, `data`, `data_len`). `NotifyName::new("Name")` resolves a
notification name a simulation module declared; `is(event)` checks an event against it.

## Storage

Each source (the mod, by its name; a map, by `[VeraWasm] StorageId=` or its file name)
has a small key-value store on this machine, kept between matches.

| Function | Does |
| --- | --- |
| `Storage::read(key, &mut buf) -> Result<&[u8]>` | The value; `NotFound` without one, `Refused` when `buf` is too small |
| `Storage::len(key)` | The value's length |
| `Storage::write(key, data)` | Writes it; empty data is `Refused` |
| `Storage::remove(key)` | Deletes it |

Keys are 1 to 64 ASCII bytes; values up to 16 KiB; 256 keys and 256 KiB per store. Over a
limit a write is `Refused` and nothing changes. The store is written to disk at most once
a second. It is local to this machine: never let it change what the simulation does.
