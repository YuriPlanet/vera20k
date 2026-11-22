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
- Feedback: the RFC draft PR
  [YuriPlanet/vera20k#4653](https://github.com/YuriPlanet/vera20k/pull/4653) carries
  two comments asking for review: the modding tutorial and the four playtest mods.

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
| Event coverage: which kinds are raised (`events::raises`), event scopes, stock-event and poll glue | `src/sim/ext_modules/events.rs`; stock predicates shared with map triggers in `src/sim/trigger_runtime/events.rs` (`house_event_holds`, `near_waypoint`), glue in `trigger_runtime/module_events.rs` |
| Chain caps (depth 8, 256 chained firings per frame, deferred firings) | `src/sim/ext_modules/chains.rs` |
| Decisions: `on_decide`, `DecideRecord`, decision budget and random substreams | `src/sim/ext_modules/decide.rs`; constants `wasm-ext/src/decide.rs` |
| INI-declared triggers (`Wasm.Tags`, tag and trigger sections of the mod's `rulesmd.ini`) | `src/sim/ext_modules/ini_triggers.rs`; event names `wasm-ext/src/event_kinds.rs` |
| Property writes (`Health`, `Owner`, `Rank`, `Mission`, `Ammo`, `Online`) | `src/sim/ext_modules/writes.rs` |
| Object, type and global property reads beyond the basics; viewpoint reads (`read_in`) | `src/sim/ext_modules/props.rs` |
| Action state kept across frames: `AllowType` forbids, timed `Convert` reversions | `src/sim/ext_modules/held.rs` |
| Modder guide | `docs/wasm-modules.md` |
| Tutorials (Chinese) | `docs/wasm-modules-tutorial.zh-CN.md` (Rust, builds and checks); `docs/wasm-modules-tutorial-ts.zh-CN.md` (TypeScript, written ahead of the SDK from RFC 5a and appendix 3 to review the planned API; nothing in it is compiled) |
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
| Rust SDK | `sdk/rust/vera20k-sdk` (no_std; `props.rs`, `vars.rs`, `triggers.rs`); examples `sdk/examples/minimal-rules`, `modifier-vars`, `panic-triggers`, `actions-demo`, `orders-rules`, `decide-damage`; the tutorial's `tutorial-bounty`; playtest mods `playtest`, `veteran-fury`, `siege-doctrine`, `meteor-storm`, `trigger-lab` (these four split the API by area without overlap, see `docs/wasm-modules.md`); the small game `muster-field` |

Build an example: `cd sdk/examples/<name> && cargo build --release` (its
`.cargo/config.toml` targets wasm32 with the allowlisted features). The engine tests
`a_rust_built_module_runs`, `a_rust_built_modifier_module_runs`,
`a_rust_built_trigger_module_receives_its_records`,
`a_rust_built_actions_module_runs`, `a_rust_built_orders_module_runs` and
`a_rust_built_decide_module_halves_damage` run them when `VERA20K_TEST_RULES_WASM`,
`VERA20K_TEST_MODIFIER_WASM`, `VERA20K_TEST_TRIGGERS_WASM`, `VERA20K_TEST_ACTIONS_WASM`,
`VERA20K_TEST_ORDERS_WASM` and `VERA20K_TEST_DECIDE_WASM` name the built `.wasm` files.
`playtest_modules_keep_running` plays 2000 frames of a Battle against one computer
house with each module that `VERA20K_TEST_PLAYTEST_WASM` lists (`:`-separated) and
checks it keeps running with no trigger refused; `--nocapture` prints the module logs.
These tests need `RA2_DIR` (the retail install) or they return early and pass
without running. The examples' `.cargo/config.toml` is ignored by the root
`.gitignore`; add a new one with `git add -f`.

Writing modules: `core::fmt` (`write!`, `format_args!`, `{:?}`) in a `no_std` module
pulls in an encoding the load checks refuse (`zero byte expected`); the examples
format numbers with a small `Line` builder instead. One `on_frame!` export per
module (merge timers into one function).

## Playtest setup

- Fast client: `python3 -m tools.cargo_run -- build --profile fast -p vera20k --bin
  vera20k --bin asset` with `CARGO_TARGET_DIR=/Volumes/扩展盘/vera20k-build/cargo-target/wx-engine`;
  the binaries land in `owned-worktrees/<sha256(worktree path)[:16]>/fast/`. Put a
  `config.toml` beside them (`[paths] ra2_dir = "<retail install>"`), or the client
  finds no `ra2md.mix` and the mod fails to start. `cargo_run`'s cache retention
  deletes the namespaces of removed worktrees, binaries included.
- Mods go in the user data folder (`~/Library/Application Support/VERA20k/Mods/<Name>/`
  on macOS) as `mod.ini` plus `rules.wasm`; start with `vera20k --mod <Name>`. A mod
  that failed to start is skipped once on the next start (crash guard).
- Module `log` lines appear in the chat list (8 per frame) and in `logs/ra2.log`
  under target `ext_module`.

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
  (clamped to 1..=200000 operations per simulation frame, shared by event delivery,
  decisions and `on_frame`), `MapWasm=no` refusing maps with
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
- Runtime triggers: each module may register up to 256 triggers and 16 object
  polling events across those triggers. Creating a trigger beyond either limit
  returns `Refused`. Removing a trigger frees capacity. The trigger runtime
  saves and validates the registry and includes it in hashes and checksums
  when populated. It removes object-bound state when the object leaves play. Events `TakeDamage` (after the Strength write, Object5F5390), `Killed` (end
  of a terminal `record_the_kill`, Techno702D40), `Fire` (after a FireAt that launched
  its bullet, Techno6FDD50), `Timer` (start of the module phase). Object and TechnoType
  bindings; filter clauses on Subject/Other/House properties and variables; immediate
  actions are every kind `run_action` implements, with the trigger house for house 0
  (`trigger_create` checks the kind and SetVar's scope); `Module` actions queue 112-byte `EventRecord`s delivered to
  `on_events` in subject order under the frame budget. Delivery drops records for
  disabled or removed triggers and stops when the game ends. After up to 1024
  records are delivered, a backlog beyond 16384 records stops the module. Enqueueing a record beyond the 17408 pending
  records limit also stops the module before further delivery
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

- Event coverage for module triggers: stock object events 1, 4, 6, 7, 29, 34, 35,
  38..44, 48, 49 (object Tag funnel `0x006E53A0`); cell events 25, 26 (cell binding);
  stock house events 5, 8..12, 15..22, 30, 32, 52, 55..58 (polled every 8th frame) and
  logic events 13, 14, 27, 28, 36, 37, 45..47, 60, 61 (every frame), all through the
  stock predicates map triggers use. New events `Birth` (RecordLastBuilt `0x004FB6B0`),
  `TransportEntered`/`PassengerAdded` (AddPassenger `0x00710670`), `HouseTransferred`
  (ChangeOwner `0x007014A0`), `SuperWeaponLaunched` (SuperClass::Launch `0x006CC390`);
  polls `PropertyPoll`, `VariablePoll` (a variable write restarts its timers, stock and
  declared). Weapon/warhead type bindings (`Fire`), cell bindings; `Killed` carries the
  lethal warhead.
- Chains (open question 17): a firing raised by a module trigger's own actions nests
  at most 8 deep and 256 times per frame; past that it waits, with its record, for the
  next module phase (saved, hashed); more than 4096 waiting stops the module.
- Decisions (6f, open question 10): `TakeDamage` final damage (Object5F5390, before the
  Strength write), `ProductionQueued` (Begin_Production `0x004FA350`, Factory_AI start)
  and `Defeat` (house defeat gate before Blowup_All `0x004FC6D0`). `on_decide` runs
  synchronously, mod links then map links; every write import disables the module;
  20,000 operations per call within the frame budget; `random` uses a substream of
  (seed, source, frame, subject, kind) and leaves the main stream. A decision raised
  inside a module call can ask the other module while its budget lasts.
- INI triggers: the mod's `rulesmd.ini` `Wasm.Tags` on TechnoTypes, tag sections and
  trigger sections (`Event`, `Where`, `Action` as `Wasm:name` or `name(target,…)`,
  `Decide=Wasm:name`, `House`, `Difficulty`, `Repeat`) become module triggers through
  `trigger_create`'s path, bound to the tagged types, saved with the registry. A
  mistake refuses the match with the section and key named.
- More atomic actions through their owners: `Heal`, `SetMission` (the player's
  MEGAMISSION order), `SpawnAnim`/`DetonateWeapon` at a cell (stock 41/42 owners),
  `AddExperience`, `AddAmmo`, `Teleport`, `ClearDisguise`, `ClearMindControl`,
  `Convert`, `SetScore`, `AddScore`; `SetProperty`, `Modify`, `RemoveModifiers` route
  to the `set` and modifier owners. Writes `Mission`, `Ammo`, `Online`. Trigger
  actions keep their text (`Log`, stock text actions). The MP checksum covers
  modifiers and declared variables.
- Rules registries: warheads and projectiles are numbered in a fixed order
  (`[Warheads]` order; projectiles in weapon order). Their HashSet order differed per
  process, so a module's Warhead ID did too.

- Phase-1 finish:
  - Events: Ares events 62..86 when Ares is on (TEventExt::HasOccured `1004F7A0`),
    stock 23 and 50, `AttackedBy`, `FatalDamage`, `TransportExited`,
    `PassengerRemoved`, `ModifierStart`/`Stacks`/`Expire`; polls `HousePowerPoll`,
    `ModifierCountPoll`, `DistancePoll`; projectile type bindings; `Timer` together
    with object events; `Birth` for reinforcements, Chrono reinforcements and
    trigger-made objects; `HouseTransferred` value 1 for mind control.
  - Decisions: `ProductionAllowed` (CanBuild `0x004F7870`, asked once per frame) and
    `CommandAllowed` (EventClass::Execute `0x004C6CB0`, every player command, module
    order and `SetMission`). A decision inside module A's call still asks module B's
    links.
  - Actions: `FireWeapon` (GetFireError `0x006FC0B0`, then the direct fire visit),
    `AttachAnim` (as team script 51), `AllowType` (checked in CanBuild),
    `AddPassenger`, timed `Convert`. Modifiers wired for ROF, weapon range
    (multiplier and extra), crit (Phobos builds), `Unkillable` (Phobos builds),
    `Cloakable`, `ForceDecloak`, `DisableWeapons`.
  - Properties: most of 7.1 (layer, target, experience, storage, transport,
    passengers, disguise, moving, facings, armor, relation with a viewpoint), 7.6
    `Passable.<SpeedType>`, `Revealed`, `Placeable` (cells `find` with a type), 7.7
    TechnoType/Weapon/Warhead keys, 7.8 `Difficulty`, mission timer, ambient light,
    ion storm. `lookup`/`name_of` for Projectile, Anim, VoxelAnim, ParticleSystem.
  - INI: `Wasm.Tags` on weapon, warhead and projectile sections; names for anim and
    enum parameters; a match notice when a map sets `Wasm.Tags`; event names for the
    4.4 and 4.5 kinds.
  - Hash and checksum: module memory is digested per 64 KiB page and each page
    re-digested on frames where `page ≡ frame (mod 15)` (plus grown pages); the hash
    and checksum read the digests (RFC 4/15). Digests are saved.

- The `Seldom` size ceiling in `object_turn_lookup_tests.rs` is 832 bytes (was 824):
  the modifier cache pointer. Approved by the maintainer.

## Provisional numbering (open question 5)

- Property IDs: section 7 names in first-appearance order, duplicates merged
  (`vera20k_wasm_ext::PROPERTIES`).
- New events from 1000 in appendix order, 4.3 then 4.4 then 4.5: `TakeDamage` =
  1000, `Killed` = 1004, `Fire` = 1005, `Birth` = 1011, `TransportEntered` = 1012,
  `PassengerAdded` = 1014, `HouseTransferred` = 1019, `Timer` = 1023,
  `SuperWeaponLaunched` = 1024; polls 1026..1030 (`PropertyPoll`, `HousePowerPoll`,
  `ModifierCountPoll`, `DistancePoll`, `VariablePoll`); decision-only 1031..1036
  (`ProductionAllowed` 1031, `ProductionQueued` 1032, `Defeat` 1033, `CommandAllowed`
  1034; `Launch` and `ProjectileFrame` reserved). Stock events keep 0..88. One name
  table: `wasm-ext/src/event_kinds.rs`.
- `Passable.<SpeedType>` properties are appended to `PROPERTIES` (IDs 173..180).
- Houses stay on the bit-47 reference: putting them on the shared creation counter
  would renumber every object in every game, modules or not (stock state hash, saves,
  replays), and would break the SDK's `const fn House::from_index`. Documented at
  `HOUSE_REF_BIT`.
- Atomic actions from 1000 in appendix 5.3 order (`ATOMIC_ACTIONS`): `LaunchSuperWeapon`
  = 1001, `Remove` = 1004, `Kill` = 1005, `Damage` = 1006, `CreateObject` = 1008,
  `ClearTarget` = 1011, `AddMoney` = 1015, `SetOwner` = 1017, `SetVar` = 1024,
  `DetonateWarhead` = 1026, `EndFor` = 1030, `Log` = 1033, `Module` = 1036; stock
  actions keep their numbers 0..149. A wasm-ext test checks the trigger constants
  against the table.
- TechnoType ID: position in the rules object registry; Weapon/Warhead ID: position
  in the rules' weapon/warhead registries (weapons in rules-processor order, warheads
  in `[Warheads]` order) (`RuleSet::weapon_index`/`warhead_index`);
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

Phase 1 is complete as far as this engine has owners for the RFC's tables; what is
left is listed under residuals. Small follow-ups found by the playtest mods:

- Read the house property `IsCivilian` (`house_property` in `ext_modules.rs`).
- An SDK helper that turns numbers into text without `core::fmt`.
- Build-dependent effects: `Unkillable` and the crit modifiers exist only in Phobos
  builds; a module cannot tell up front (raised on the RFC PR).

Phase 2 (`ui.wasm`) should build on the GUI crate of `feature/gui-crate`
([YuriPlanet/vera20k#4973](https://github.com/YuriPlanet/vera20k/pull/4973), not
merged). Its `docs/gui-library-api.md` already maps the `client_node_*` and
`client_set/get` calls (sections 3 and 15) and its `Gui` tree has create, set, get,
cascading remove, `Refused` and a node cap. Missing for the RFC: the in-match UI
(only out-of-game screens moved), lookup by part name, node owners, the
`veraui.ini` loader, and the RFC v1 kinds `Row`, `Column`, `CameoGrid`, `Image`,
`Bar`, `Decoration` and `MapMarker`. Order: move the in-match UI onto the crate,
add names, owners and those kinds, then a thin `client_*` bridge that checks the
section 10 permissions.

Before version 1 (RFC "Process"): the TypeScript SDK
(deferred by the maintainer), phase 2 (`ui.wasm`), phase 3 (a real mission with
modules, external modders, freeze), and the later items (`Launch`,
`ProjectileFrame`, module trajectories, peer calls).

## Residuals

- The stopped-module notice is translated (`VERA:ModuleStopped`), but the fault
  reason inside it is English only.
- Events not raised: stock 0, 2, 3, 24, 31, 33, 51, 53, 54, 59, Ares 87, 88; 4.3
  `HitByWarhead`, `SpawnFire`, `HitTarget`, `TargetChanged`, `ProjectileIncoming`,
  `Detonated`, `Commanded`, `Crushed`, `HouseTransfer`, `ProductionCancelled`
  (reasons in `events.rs`). `Birth` misses paradrops, spawned aircraft, crate units,
  survivors and starting objects; garrisons and non-departure cargo removal raise no
  passenger events; type-bound timers and polls scan every object; `Timer` with only
  weapon/projectile bindings never fires.
- Decisions: decision triggers cannot have immediate actions; `ProductionAllowed`
  cannot lift CanBuild's own refusal and also asks Neutral/Special; `CommandAllowed`
  asks nothing for commands without an 8.3 number or an actor, nor for the AI's own
  missions; no kind uses a result area; nested decisions get no merged rules text.
- Actions unsupported: `RemovePassenger`, `Push`, team and projectile actions,
  `AttachAnim` with a frame count; house modifiers (`PowerOutage`); stock actions that
  need a live trigger (3, 6, 9, 13, 36..38, 52, 74, 75, 81, 91, 111, 113, 119..124,
  126, 130, Ares 146, 147), sound IDs (19, 98, 99), 40, 15, 39, 49, 50.
  Crit and `Unkillable` modifiers exist only in Phobos builds, as their owners do.
  `ROFMultiplier.ApplyOnCurrentTimer` is not applied; `Cloakable` Override 0 does
  not remove a type's own `Cloakable=`. Timed conversions are in the state hash, not
  the MP checksum.
- Properties not read: `Team`/`Tag` values (no references), `GameMode`, `Name`,
  `Buildable`, `Points`, the house's `IsCivilian` (answers `Unsupported`, so a clause
  on it matches nothing; found by `sdk/examples/veteran-fury`), the `v1?` object
  properties; `lookup(Armor)`,
  `lookup(Sound)`; `Passable` ignores objects and overlays; `PropertyPoll` reads
  without a viewpoint.
- `Health` writes of 0 or less answer `Refused`; an `Ammo` write restarts a running
  reload clock; `AddExperience` uses the base Cost.
- Modifiers are clamped to `[0, 32768]` (armor at least 2^-16); `Override` replaces
  the Phobos product too.
- Rules stage: `random` writes 0 with `Ok`; `set` on a type reference in `on_rules`
  answers `Unsupported`; the merged rules text is saved with every module save;
  `MapUI` is not saved; the lobby compares neither `on_rules` results nor `Api=`.
- Orders: custom control grants refuse house-wide module orders; `IgnoreOrders` is
  not honoured; `SuspendProduction`, `EjectBunker`, `Enter` answer `Unsupported`.
- INI: sounds take numbers only; the map `Wasm.Tags` warning is a match notice, not
  a lobby warning; one module per trigger's `Wasm:` actions.
- A type name shared by two categories resolves to the first. Houses queries with
  area, type or owner answer `Unsupported`.
- The Rust SDK's delivery area is fixed at address 8 (9 records); one
  `named_actions!` and one `vars!` per module.

## Validation commands

```
export CARGO_TARGET_DIR=/Volumes/扩展盘/vera20k-build/cargo-target/wx-engine
python3 -m tools.cargo_run -- test -p vera20k-wasm-ext --lib
# Build each sdk/examples/* with `cargo build --release`, then point the
# VERA20K_TEST_*_WASM variables (RULES, MODIFIER, TRIGGERS, ACTIONS, ORDERS, DECIDE)
# at the .wasm files. VERA20K_TEST_PLAYTEST_WASM takes the four playtest mods
# (veteran-fury, siege-doctrine, meteor-storm, trigger-lab), `:`-separated, and
# plays 2000 frames with each (playtest_modules_keep_running, --nocapture prints
# their logs). VERA20K_TEST_MUSTER_WASM takes muster_field.wasm and plays its first
# wave from player command records (muster_field_plays_a_round_from_player_orders)
# and a campaign lost with an empty company (muster_field_ends_when_the_last_banner_falls).
RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 VERA20K_TEST_..._WASM=... \
  python3 -m tools.cargo_run -- test -p vera20k-core --lib -- sim::ext_modules:: sim::trigger_runtime::
RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 python3 -m tools.cargo_run -- \
  test -p vera20k-core --lib -- rules:: sim::combat:: sim::production:: sim::world:: \
  sim::anim_class:: sim::superweapon:: sim::ammo:: sim::cloak_disguise:: sim::economy:: \
  sim::passenger:: sim::movement::
python3 -m tools.cargo_run -- check -p vera20k --lib --bins
VERA20K_SNAPSHOT_LAYOUT=1 python3 -m tools.cargo_run -- \
  check -p vera20k-core --lib --target wasm32-unknown-unknown --features wasm
python3 -m tools.group_tests check
```

Last run (after merging EncoreAlpha's review changes `4b4ed4243` and upstream main,
HEAD `fc78b8e90`): wasm-ext 21/21; `sim::ext_modules::` with `sim::trigger_runtime::`
395 passed, 5 ignored, all six Rust examples run; `rules::`, `assets::`,
`headless_scenario::`, `sim::attach_effect::`, `sim::capture_manager::`,
`sim::combat::`, `sim::crates::`, `sim::passenger::`, `sim::production::`,
`sim::team_script_vm::`, `sim::world::`, `sim::movement::` 5086 passed, 173 ignored,
2 failed for this machine only (Chinese retail CSF; no unlayered retail `ini/`);
client check, wasm32 check and `group_tests check` pass.

Since then (HEAD `6e8124e67`): `playtest_modules_keep_running` passes with the four
playtest mods. A `--profile fast` client loaded a mod module through the app loader
in a retail skirmish: the module ran, logged to the chat list and paid credits;
the waves of `veteran-fury` failed on `IsCivilian` until the module stopped using
it. The other three mods have run only in the headless test.
