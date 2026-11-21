# Wasm extension modules: development status

The current checkpoint for implementing [RFC 0001](0001-wasm-extension-modules.md)
(API in the [appendix](0001-wasm-extension-modules-api.md)). Replace this file's
content as work lands; do not append a diary.

## Where the work is

- Branch `feature/wasm-extension-modules` on `libi/vera20k`, PR
  [YuriPlanet/vera-mod-engine#7](https://github.com/YuriPlanet/vera-mod-engine/pull/7)
  (the RFC's experiment repository; the earlier work merged there as #5).
- Scope: RFC phase 1 (`rules.wasm`) first, then phase 2 (`ui.wasm`), then phase 3.
  The TypeScript SDK is deferred by the maintainer; validate with the Rust SDK.

## Map of the code

| Piece | Where |
| --- | --- |
| Sandbox: Wasmtime engine, load checks, fuel, imports, `on_events` delivery, saves of memory/globals | `wasm-ext/src/native.rs` (desktop); `browser.rs` is a stub that refuses modules |
| ABI types: import table, `Status`, `SimHost` trait, `PROPERTIES`, `Query`/`Clause` parsing | `wasm-ext/src/lib.rs` |
| Action kind names and numbers (stock 0..149, atomic from 1000), `lookup`/`name_of(ActionKind)` | `wasm-ext/src/action_kinds.rs` |
| `TriggerDef`/`EventDef`/`ActionDef` parsing, `EventRecord`, event and action numbers | `wasm-ext/src/triggers.rs` |
| Custom sections `vera.vars`, `vera.actions`, `vera.delivery` (one reader) | `wasm-ext/src/sections.rs` |
| Sandbox tests with a fake host | `wasm-ext/src/sandbox_tests.rs`, `trigger_tests.rs`, `sections_tests.rs`, `sdk_props_tests.rs` |
| Simulation side: slots, module phase, host (`Host` holds `&mut Simulation`, the overlay registry and the frame effects during a call), saves, hash, checksum | `src/sim/ext_modules.rs`, tests in `ext_modules_tests.rs` and `ext_modules_trigger_tests.rs` (XMP03T4, retail) |
| Modifier store and the per-object cache the combat and movement owners read | `src/sim/ext_modules/modifiers.rs` |
| Declared variables (`vera.vars` values, IDs from 1000) | `src/sim/ext_modules/vars.rs` |
| Module triggers: raising events, clauses, firing, delivery | `src/sim/ext_modules/triggers.rs`; registry `src/sim/trigger_runtime/module_triggers.rs` |
| The `action` import and trigger actions: `run_action` (atomic actions through their owners, stock actions mapped onto the map trigger executor) | `src/sim/ext_modules/actions.rs`; executor without a live trigger `src/sim/trigger_runtime/untriggered.rs` |
| House orders: `order` checks and queues per module slot; `take_due_orders` turns them into player commands | `src/sim/ext_modules/orders.rs`; executed by `Simulation::run_ext_module_orders` (`src/sim/world/command_schedule.rs`) from `run_network_command_tail` in `master_frame.rs` |
| Cell and global properties, `find` over cells | `src/sim/ext_modules/cells.rs` |
| Rules stage: `[VeraWasm]` policy, `VeraMinVersion`, `PreparedExtModules` (the `ExtRulesHook`), `RulesHost` for `vera_init`/`on_rules`, `rules_get`/`rules_set` | `src/sim/ext_modules/rules.rs`; hook trait `ExtRulesHook` and the extra pass in `src/rules/native_processing.rs`; entry `NativeRulesProcessOwner::load_scenario_with_ext_rules` (`process_owner.rs`) |
| Build version and `version_serves` (mods and maps) | `src/util/version.rs` (`GAME_VERSION`) |
| Module phase hook | end of the Triggers arm in `src/sim/world/master_frame.rs` |
| State hash / MP checksum / save restore | `world_hash.rs`, `multiplayer_checksum.rs`, `snapshot/simulation_restore.rs` |
| Player notice when a module stops | `TriggerEffect::ExtModuleStopped`, handled in `src/app/match_runtime/sim_tick.rs` |
| Map embedding (`[VeraWasm]`, `[VeraWasmRulesPack]`) | `src/map/ext_module_pack.rs` |
| Loading | `headless_scenario.rs`, `app/loading/init.rs`: `PreparedExtModules::load` (mod folder `rules.wasm`, then map) before the rules load, `install_scenario_ext_modules` after the simulation exists |
| `asset wasm-check / wasm-embed / wasm-extract` | `src/asset_tools/verb_wasm.rs` |
| Rust SDK | `sdk/rust/vera20k-sdk` (no_std; `props.rs`, `vars.rs`, `triggers.rs`); examples `sdk/examples/minimal-rules`, `modifier-vars`, `panic-triggers`, `actions-demo`, `orders-rules` |

Build an example: `cd sdk/examples/<name> && cargo build --release` (its
`.cargo/config.toml` targets wasm32 with the allowlisted features). The engine tests
`a_rust_built_module_runs`, `a_rust_built_modifier_module_runs`,
`a_rust_built_trigger_module_receives_its_records`,
`a_rust_built_actions_module_runs` and `a_rust_built_orders_module_runs` run them
when `VERA20K_TEST_RULES_WASM`, `VERA20K_TEST_MODIFIER_WASM`,
`VERA20K_TEST_TRIGGERS_WASM`, `VERA20K_TEST_ACTIONS_WASM` and
`VERA20K_TEST_ORDERS_WASM` name the built `.wasm` files. The examples' `.cargo/config.toml`
is ignored by the root `.gitignore`; add a new one with `git add -f`.

## Done (phase 1)

- Loading and validation, operation budgets, deterministic NaN, module random stream,
  disable-on-fault with an in-game notice, saves, state hash, MP checksum.
- Map embedding, mod-folder `rules.wasm`, `asset` verbs.
- Imports: `frame`, `random`, `log`, `has_kind`, `lookup`, `name_of`, `get`,
  `get_many`, `set`, `modify`, `unmodify`, `var_get`, `var_set`, `find`, `action`,
  `trigger_create/remove/bind/unbind`, `rules_get`, `rules_set`, `order`; exports
  `on_events` and `on_rules`.
- `action` and trigger actions share one entry, `run_action`. Atomic actions:
  `LaunchSuperWeapon` (Fire_SW `0x004FAE50`), `Remove`, `Kill`, `Damage`,
  `CreateObject`, `ClearTarget` (Assign_Target `0x006FCDB0`), `AddMoney`, `SetOwner`
  (ChangeOwner), `SetVar` (the `var_set` writer), `DetonateWarhead` (Apply_area_damage
  `0x00489280`), `EndFor`, `Log`. Stock actions 0..149 run through
  TActionClass::Execute `0x006DD8B0` with no live trigger, operands mapped as
  `TActionClass::Read` stores them (table in `actions.rs`). `has_kind(Action)` answers
  what runs; invalid parameters disable the module.
- One host path: `on_frame`, `on_scenario_start`, `vera_init` and `on_events`
  delivery all run through `Simulation::with_ext_module` with the full `Host`
  (registry, frame effects, the slot's order queue and the merged rules text);
  event sites reach the same function for clause reads and immediate actions. The
  one exception is a module that exports `on_rules`: its `vera_init` and `on_rules`
  run on the rules stage's `RulesHost` (below). `has_kind` and the rules-backed
  name kinds are shared by both hosts.
- Rules access and `on_rules` (RFC 8): the loader builds `PreparedExtModules` (both
  modules loaded, policy read) before the scenario rules load and hands it to
  `load_scenario_with_ext_rules` as the `ExtRulesHook`. The mod's modules run after
  `RULESMD.INI`, `LANGRULE.INI` and the mode layer, the map's after the map layer
  (both inside the Scenario layer of `RulesLayerStack::process_internal`). Only a
  module exporting `on_rules` runs there: `vera_init` once, then `on_rules`, which
  sees the rules processed so far (`get` on type references, `rules_get` on the merged
  text plus its own writes). `rules_set` collects INI text that the rules owner
  processes as one more pass (native readers, clamps, postprocessing); the writes are
  folded into the rules content hash only when there are any. A fault refuses the
  scenario (`RulesError::ExtModule`). Initialization there resolves rules type names,
  declared variables (IDs as the match numbers them) and `has_kind`. Modules without
  `on_rules` still run `vera_init` at install, after every module's variables are
  declared. In the match, `rules_get` reads the merged rules text and `rules_set`
  answers `Refused` (empty allowlist).
- House orders (6g): `order` for a computer house that is not defeated and not the
  local player's; the command table (appendix 8.3) checks target and value kinds,
  units must be object references, at most 64 orders and 4096 unit entries per module
  per frame. Orders queue in the module's slot (saved, hashed, cleared when the module
  stops) and execute in the next frame's command step, after the player commands
  (`run_ext_module_orders` in `run_network_command_tail`), as the player commands the
  house's player would send, through the same receiver (`apply_one_due_command`).
  Units that died or changed owner drop out; a defeated house's orders are dropped.
  Orders never enter the network stream.
- Mod policy (`[VeraWasm]` in the mod's own `rulesmd.ini`): `Budget`, `MapBudget`
  (clamped to 1..=200000 operations per `on_frame`), `MapWasm=no` refusing maps with
  modules, `MapUI`, `Api`. A map's `[Basic] VeraMinVersion=` above this build refuses
  the map with a message; `version_serves` moved to `util::version` and serves mods
  and maps.
- Modifiers: Add/Multiply/Override with duration, source, group (same origin, object
  and group replaces) and origin; expiry at the start of the module phase; saved,
  hashed, dropped with their object; `get` reads the effective value; modifier
  properties (7.3) and `find` scope `Modifiers`. Wired: `FirepowerMultiplier`
  (FireAt's Phobos firepower stage `0x006FE352`, also EstimateDamage and bullet
  init), `ArmorMultiplier` (Phobos receive armor divide `0x00701966`, also
  EstimateDamage), `SpeedMultiplier` (Phobos GetCurrentSpeed stage `0x4DB218`). Each
  composes onto the Phobos AttachEffect product; without modifiers the owners
  compute what they did before.
- Variables: `vera.vars` parsed and checked at load; global, house and object scope
  with defaults; wrong scope disables the module; `lookup(Variable)` with `mod.`/`map.`;
  object values go with their object; saved and hashed; `find` variable clauses on the
  candidate; trigger `Where` variable clauses (Global/Subject/Other/House target,
  checked against the declared scope at `trigger_create`) and the immediate `SetVar`
  action (object scope writes the action's target object, house scope the house
  target, global ignores the target) read and write declared variables through
  `VarStore`, the write through the host's `var_set`.
- Writes through owners: `Health` (no kill credit; 0 or less refused), `Owner`
  (ChangeOwner, as the stock change-house action), `Rank` (SetRookie/Veteran/Elite).
- Runtime triggers: the registry lives in the trigger runtime, saved with it, hashed and
  checksummed only when non-empty, checked on restore, dropped with an object's
  storage. Events `TakeDamage` (after the Strength write, Object5F5390), `Killed` (end
  of a terminal `record_the_kill`, Techno702D40), `Fire` (after a FireAt that launched
  its bullet, Techno6FDD50), `Timer` (start of the module phase). Object and TechnoType
  bindings; filter clauses on Subject/Other/House properties and variables; immediate
  actions are every kind `run_action` implements, with the trigger house for house 0
  (`trigger_create` checks the kind and SetVar's scope); `Module` actions queue 112-byte `EventRecord`s delivered to
  `on_events` in subject order under the frame budget, dropped for disabled or removed
  triggers or an ended game; a backlog past 16 frames stops the module
  (`Fault::Backlog`). Trigger `Enabled`/`House`/`Repeat` via `get`/`set`.
- Custom sections: `vera.vars`, `vera.actions` and `vera.delivery` are read and
  checked at load by one parser; `lookup(ModuleAction)` answers from `vera.actions`.
- Readable properties: objects `Type`, `Owner`, `Category`, `Health`, `Strength`,
  `HealthPercent`, `X`, `Y`, `Z`, `Cell`, `Rank`, the three wired multipliers; houses
  `Index`, `Money`, `PowerOutput`, `PowerDrain`, `LowPower`, `IsDefeated`, `IsHuman`;
  TechnoType `Strength`, `Cost`; cells (by cell or waypoint reference) `LandType`,
  `Height`, `Overlay`, `Ore` (Get_Tiberium_Value `0x00485020`), `Waypoint`, `CellTag`;
  global `Frame`, `PlayerCount`, `GameSpeed`.
- `lookup`/`name_of`: TechnoType, Weapon, Warhead, SuperWeapon, Country, TeamType,
  Variable, ModuleAction, Property, ActionKind. `find`: Objects, Houses, Cells (cell
  index order, map, circle or rectangle), Modifiers.
- One reference decoder (`decode_ref`): waypoints (kind 3) and the global reference
  (kind 4, payload 0) decode there; `run_action` uses it too.
- Rust SDK: raw bindings for every import, wrappers for the above (`Order` and
  `House::order`, `Rules::get/set`, `Global`, `Cell` properties, `Find::cells`,
  `TypeRef::super_weapon`), export macros including `on_rules!`,
  `Prop::<Name>` constants for every property (a wasm-ext test checks them against
  `PROPERTIES`), `Modifier`, `vars!`, the `Trigger` builder, `ActionDef`
  constructors, event views, `on_events!` and `named_actions!`, one `Action` function
  per implemented atomic kind (`Action::*` holds the kind numbers `ActionDef` uses).

## Provisional numbering (open question 5)

- Property IDs: section 7 names in first-appearance order, duplicates merged
  (`vera20k_wasm_ext::PROPERTIES`).
- Delivered events from 1000 in appendix 4.3 order: `TakeDamage` = 1000,
  `Killed` = 1004, `Fire` = 1005, `Timer` = 1023.
- Atomic actions from 1000 in appendix 5.3 order (`ATOMIC_ACTIONS`): `LaunchSuperWeapon`
  = 1001, `Remove` = 1004, `Kill` = 1005, `Damage` = 1006, `CreateObject` = 1008,
  `ClearTarget` = 1011, `AddMoney` = 1015, `SetOwner` = 1017, `SetVar` = 1024,
  `DetonateWarhead` = 1026, `EndFor` = 1030, `Log` = 1033, `Module` = 1036; stock
  actions keep their numbers 0..149. A wasm-ext test checks the trigger constants
  against the table.
- TechnoType ID: position in the rules object registry; Weapon/Warhead ID: position
  in the rules' weapon/warhead registries (`RuleSet::weapon_index`/`warhead_index`);
  Country ID: `CountryIdx`; TeamType ID: map definition ordinal; superweapon ID:
  `[SuperWeaponTypes]` order; overlay ID: overlay type index; waypoint reference kind 3,
  global reference kind 4.
- References: object = entity `stable_id`; house = bit 47 over the house index (the RFC
  wants houses on the shared creation counter); type `(1<<48)|(kind<<32)|id`;
  cell `(2<<48)|(x<<16)|y`; global `4<<48`.
- `Command` numbers (appendix 8.3) 0..25 as the SDK's `Command` lists them; the
  `Order` layout is a 24-byte header (kind, count, target, value) and the unit handles.
- Modifier and trigger handles: serials from the objects' shared creation counter
  (`allocate_stable_id`), as the RFC asks.
- Variable IDs: 1000 on, mod module's declarations then the map's, in declaration order.

## Remaining in phase 1

1. Triggers, rest: more event kinds (stock 0..61, the other 4.3 events, polling 4.4),
   weapon/warhead/projectile and cell bindings, stock and other atomic actions in
   triggers, deferred chain triggers and their caps (open question 17), `Killed`
   lethal warhead, map `Action=Wasm:name`.
2. INI bindings: `Wasm.Tags` on TechnoType/Weapon/Warhead/Projectile, tag and trigger
   sections (appendix "INI keys").
3. Modifiers: the other 7.2 properties (ROF, range, crit, `Unkillable`, cloak, ...),
   house modifiers (`PowerOutage`), `ModifierStart`/`Stacks`/`Expire` events, Phobos
   AttachEffect through the same owner, stack limits (open question 13).
4. Variables: client visibility (phase 2); a write resetting `VariablePoll` timers.
5. Property writes `Mission`, `Ammo`, `Tag`, `Online` and a credit-free kill by
   `Health`; more properties (7.1, the rest of 7.6 and 7.8, 7.7 types and rules beyond
   TechnoType `Strength`/`Cost`).
6. Actions: the atomic and stock kinds listed under residuals.
7. Orders and rules, rest: the order and rules residuals below; AI definitions written
   by `on_rules` into the AI document; a non-empty in-match `rules_set` allowlist (open
   question 11).
8. Two or three decidable events and `on_decide` (open question 10).
9. SDK: property constants generated from `PROPERTIES`, wrappers for each new import.

## Residuals

- Policy call for the maintainer: the `Seldom` size ceiling in
  `object_turn_lookup_tests.rs` went from 824 to 832 bytes for the modifier cache
  pointer.
- Modifiers and declared variables are in the state hash but not the MP checksum
  (the checksum covers module memory and globals, and module triggers when present).
- Hash and checksum read all module memory each frame, not the 1/15 page rotation.
- The stopped-module notice is English only.
- `Mission` and `Ammo` writes answer `Unsupported` (no owner operation for a module);
  `Health` writes of 0 or less answer `Refused`.
- Module modifiers are clamped to `[0, 32768]` (armor at least 2^-16); `Override`
  replaces the Phobos product too. Modifiers on a gone object linger in the store only
  until its storage is freed (`finalize`), and none of them applies meanwhile.
- `ModifierOp::Multiply` folds onto the Phobos product in handle order; mixed with
  `Add` the sum scales the Phobos product first.
- A declared variable's ID shifts if a mod module is installed after a map module
  (only tests do that; scenario loading installs the mod first).
- Rules stage: in `vera_init` and `on_rules`, `random` writes 0 with `Ok` (the ABI's
  `random` has no status the host can set; the RFC wants `Unsupported`). `set` on a
  type reference in `on_rules` answers `Unsupported` instead of writing the value's INI
  text. TeamType names answer `Unsupported` in an `on_rules` module's `vera_init` (the
  map's teams are not loaded yet); other calls answer `Unsupported` there as appendix
  1.1 says. A loader that cannot run the rules stage refuses a module exporting
  `on_rules`.
- The merged rules text `rules_get` reads is kept whole in `ExtModules` and saved with
  every save that has modules (one copy, not per slot): large saves.
- `MapUI` is read but not saved: a loaded match reads the default (`Overlay`).
- Lobby and save checks do not compare `on_rules` results (every machine reproduces
  them from the module bytes); the lobby does not compare the mod's `Api=` with a
  module's import version.
- Orders: with custom control grants (shared houses) the house-wide commands
  (production, placement, superweapons) need a granted sender, which a module order
  has not, so the receiver refuses them. `IgnoreOrders` is not honoured (no unit
  ignores a module order). `SuspendProduction` (needs a type's production category),
  `EjectBunker` (no player input sends it) and `Enter` (no single player command)
  answer `Unsupported`. The per-frame order and unit limits are provisional (open
  question 7).
- Cells: `CellTag` reads 0 without a tag and answers `Unsupported` on a tagged cell
  (tags have no references yet); `Passable.<SpeedType>`, `Revealed` and `Placeable`
  are not read; global `Difficulty`, `GameMode`, `MissionTimer`,
  `MissionTimerRunning`, `AmbientLight` and `IonStormActive` answer `Unsupported`.
  A `find` over cells with a type or owner answers `Unsupported`.
- A declared-variable write does not reset `VariablePoll` timers (none are raised yet).
- Clause reads and immediate trigger actions at an event site get empty frame effects;
  none of them reaches a lent owner today.
- A type name shared by two categories resolves to the first.
- Houses queries with area, type or owner answer `Unsupported`.
- Event sites check a per-kind flag, then a map lookup by subject, not an entity side
  column (RFC 15.1 item 1).
- Unsupported atomic actions (`(0, Unsupported)`, `has_kind` 0): `FireWeapon`,
  `SpawnAnim`, `AttachAnim`, `Heal`, `SetMission`, `Teleport`, `ClearDisguise`,
  `ClearMindControl`, `AddExperience`, `AllowType`, `AddAmmo`, `AddPassenger`,
  `RemovePassenger`, `SetProperty`, `Modify`, `RemoveModifiers`, `DetonateWeapon`,
  the projectile and team actions, `SetScore`, `AddScore`, `Convert`, `Push`.
  `SetProperty`/`Modify`/`RemoveModifiers` would route to the `set` and modifier
  owners; not wired yet.
- Unsupported stock actions: those whose house or objects a live trigger selects (3, 6,
  9, 13, 36..38, 52, 74, 75, 81, 91, 111, 113, 119..124, 126, 130, Ares 146, 147),
  sound IDs (19, 98, 99), 40, 15, 39, 49, 50, Ares actions without Ares. A stock
  waypoint operand must be a waypoint reference (a cell or object answers
  `Unsupported`).
- Trigger actions get an empty `text` (the definition keeps no string), so `Log` and
  stock text actions in a trigger log nothing or answer `NotFound`.
- No chain triggers (open question 17): events raised by a trigger's immediate actions
  (a `Kill` raising `Killed`) reach no module trigger. Event-site reads lend no frame
  effects (they reach no lent owner); immediate actions get the event site's effects.
- A disabled module's triggers are removed with it; a trigger mixing `Timer` with
  object events answers `Unsupported`; `Killed` carries no lethal warhead (`id` −1).
- The Rust SDK's delivery area is fixed at address 8 (9 records): `link_section`
  statics cannot hold an address. A stack within 1 KiB of its 1 MiB limit during
  `on_events` would overwrite unread records. One `named_actions!` per module.

## Validation commands

```
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- test -p vera20k-wasm-ext --lib
VERA20K_MIN_FREE_GIB=4 RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 \
  VERA20K_TEST_RULES_WASM=<minimal_rules.wasm> VERA20K_TEST_MODIFIER_WASM=<modifier_vars.wasm> \
  VERA20K_TEST_TRIGGERS_WASM=<panic_triggers.wasm> VERA20K_TEST_ACTIONS_WASM=<actions_demo.wasm> \
  VERA20K_TEST_ORDERS_WASM=<orders_rules.wasm> \
  python3 -m tools.cargo_run -- test -p vera20k-core --lib -- sim::ext_modules:: sim::trigger_runtime::
RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 python3 -m tools.cargo_run -- \
  test -p vera20k-core --lib -- rules:: sim::world::command_schedule:: headless_scenario::
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- test -p vera20k-core --lib sim::world::object_turn::
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- check -p vera20k --lib --bins
VERA20K_SNAPSHOT_LAYOUT=1 VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- \
  check -p vera20k-core --lib --target wasm32-unknown-unknown --features wasm
python3 -m tools.group_tests check
```

Last run (orders, rules access and `on_rules`, policy keys, cell/global/type
properties): wasm-ext 17/17; `sim::ext_modules::` with `sim::trigger_runtime::` 354
passed, 5 ignored, on retail XMP03T4 with all five Rust examples built; `rules::` 1276
passed, 1 failed (`campaign_mission_names_use_physical_mission_and_csf_inputs`: the
local retail install is Chinese, unrelated); `sim::world::command_schedule::` with
`headless_scenario::` 40 passed, 21 ignored; client check, wasm32 check and
`group_tests check` Finished. `sim::world::object_turn::` was not rerun.
