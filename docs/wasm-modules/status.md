# Wasm extension modules: development status

The current checkpoint for implementing [RFC 0001](../rfcs/0001-wasm-extension-modules.md)
(API in the [appendix](../rfcs/0001-wasm-extension-modules-api.md)). Replace this file's
content as work lands; do not append a diary.

## Where the work is

- Repository: `YuriPlanet/vera20k`. Phase 1 (#5183), the control points (phase 2a,
  #5245) and the client modules (phase 2, `ui.wasm`, #5251) are all on `main`.
- Scope: RFC phase 1 (`rules.wasm`), then phase 2 (`ui.wasm`), then phase 3. Module authors can use the
  [Rust SDK](rust-sdk.md) or the [TypeScript SDK](ts-sdk.md).
- Feedback: the RFC draft PR
  [YuriPlanet/vera20k#4653](https://github.com/YuriPlanet/vera20k/pull/4653) carries
  two comments asking for review: the modding tutorial and the four playtest mods.

## Map of the code

| Piece | Where |
| --- | --- |
| Sandbox: load checks, budgets, delivery, saves of memory/globals, one `SimModule` over a runtime | `wasm-ext/src/module.rs` |
| Load-time rewrite every runtime runs: basic-block metering, bulk-memory charges, weighted call depth, NaN canonicalization, float globals stored as integer bits, memory maximum capped at the limit, exported mutable globals | `wasm-ext/src/instrument.rs` |
| The `vera:sim` imports, written once over a `Guest` view and linked into both runtimes. A new import is a function here plus one line in `for_each_import!`; neither runtime file changes | `wasm-ext/src/host.rs` |
| Runtimes: Wasmtime (desktop), wasmi (standalone WASI tools), the browser's own WebAssembly engine through js-sys (browser) | `wasm-ext/src/native.rs`, `wasm-ext/src/browser.rs`; rewrite tests in `rewrite_tests.rs` |
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
| INI registration (RFC 0001 section 6h): registered names, event IDs, string table | `src/sim/ext_modules/ini_names.rs`; start records `ini_start.rs`; `RaiseEvent` in `actions.rs`; `Event=Wasm:`, `Wasm:name(target,text)` in `ini_triggers.rs`; tests `ext_modules_ini_register_tests.rs`, demo smoke `ini_demo_mods_reach_their_marks` |
| Modder guide | `docs/wasm-modules/guide.md` (`.zh-CN.md` beside each English page) |
| Tutorials | `docs/wasm-modules/rust-tutorial.md` (Rust simulation module), `docs/wasm-modules/rust-ui-tutorial.md` (Rust client module); `docs/wasm-modules/ts-tutorial.zh-CN.md` (TypeScript) |
| Rust SDK reference | `docs/wasm-modules/rust-sdk.md` |
| The `action` import and trigger actions: `run_action` (atomic actions through their owners, stock actions mapped onto the map trigger executor) | `src/sim/ext_modules/actions.rs`; executor without a live trigger `src/sim/trigger_runtime/untriggered.rs` |
| House orders: `order` checks and queues per module slot; `take_due_orders` turns them into player commands | `src/sim/ext_modules/orders.rs`; executed by `Simulation::run_ext_module_orders` (`src/sim/world/command_schedule.rs`) from `run_network_command_tail` in `master_frame.rs` |
| Cell and global properties, `find` over cells | `src/sim/ext_modules/cells.rs` |
| House domain claims (RFC 6i): `ControlDomains`, the claim store, "mod AI" seats, `Simulation::module_claims` that the native AI asks | `src/sim/ext_modules/claims.rs` (gates listed in its module doc); seats from `SkirmishLaunchSession::mod_ai_slots` in `initialize_skirmish_launch_houses`; tests `ext_modules_claim_tests.rs` |
| Superweapon references, properties and `find` over superweapons | `src/sim/ext_modules/supers.rs` |
| 6i reads: `Visible` (`Simulation::object_seen_by`, `techno_ai/common_body.rs`), `Engage` (`Simulation::engage_check`, `techno_ai/target_scan.rs`), factories (`FactoryRegistry::owner_view`, `production/factory.rs`); tests | `src/sim/ext_modules/props.rs`; `ext_modules_drive_tests.rs` |
| Rules stage: `[VeraWasm]` policy, `VeraMinVersion`, `PreparedExtModules` (the `ExtRulesHook`), `RulesHost` for `vera_init`/`on_rules`, `rules_get`/`rules_set` | `src/sim/ext_modules/rules.rs`; hook trait `ExtRulesHook` and the extra pass in `src/rules/native_processing.rs`; entry `NativeRulesProcessOwner::load_scenario_with_ext_rules` (`process_owner.rs`) |
| Build version and `version_serves` (mods and maps) | `src/util/version.rs` (`GAME_VERSION`) |
| Module phase hook | end of the Triggers arm in `src/sim/world/master_frame.rs` |
| State hash / MP checksum / save restore | `world_hash.rs`, `multiplayer_checksum.rs`, `snapshot/simulation_restore.rs` |
| Player notice when a module stops | `TriggerEffect::ExtModuleStopped`, handled in `src/app/match_runtime/sim_tick.rs` |
| Map embedding (`[VeraWasm]`, `[VeraWasmRulesPack]`) | `src/map/ext_module_pack.rs` |
| Loading | `headless_scenario.rs`, `app/loading/init.rs`: `PreparedExtModules::load` (mod folder `rules.wasm`, then map) before the rules load, `install_scenario_ext_modules` after the simulation exists |
| `asset wasm-check / wasm-embed / wasm-extract` | `src/asset_tools/verb_wasm.rs` |
| Rust SDK | `sdk/rust/vera20k-sdk` (no_std; `props.rs`, `vars.rs`, `triggers.rs`); examples `sdk/examples/minimal-rules`, `modifier-vars`, `panic-triggers`, `actions-demo`, `orders-rules`, `decide-damage`; the tutorial's `tutorial-bounty`; playtest mods `playtest`, `veteran-fury`, `siege-doctrine`, `meteor-storm`, `trigger-lab` (these four split the API by area without overlap, see `docs/wasm-modules/guide.md`); the small game `muster-field` (with its `ui.wasm`); the benchmark `control-bench` |

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
without running.
The benchmark mod `sdk/examples/control-bench` reaches nearly every phase-1 control
point in one module (222 marks in a rules stage, four areas and a finale, all by
frame ~2900) and runs
through `control_bench_reaches_every_mark` (`VERA20K_TEST_CONTROL_BENCH` the built
module, `VERA20K_TEST_CONTROL_BENCH_FRAMES` the frames, 6000 by default): the test
makes a mod folder as a modder ships it (`rules.wasm`; the retail `rulesmd.ini`
with the bench's merged in, since a mod's `rulesmd.ini` replaces the game's;
`mod.ini`; `ControlBench.map`, XMP03T4 with the bench's `map.ini` TeamTypes) and
loads the Battle against one computer house through that mod
(`headless_scenario::load_with_mod_folder`), so `vera_init` and `on_rules` run
at the rules stage. The finale starts when the areas are done or at frame 4800.
It exercises `SuspendProduction`, vetoes then allows `Defeat`, and calls `EndFor`.
The test requires `DEMO-OK bench.all` and fails on any `BENCH-FAIL` or refusal.
It reports the failures and marks still missing. Its `README.zh-CN.md` lists what
it leaves out and why. The examples' `.cargo/config.toml` is ignored by the root
`.gitignore`; add a new one with `git add -f`.

Writing modules: `core::fmt` (`write!`, `format_args!`, `{:?}`) in a `no_std` module
pulls in an encoding the load checks refuse (`zero byte expected`); the examples
format numbers with a small `Line` builder instead. One `on_frame!` export per
module (merge timers into one function).

## Playtest setup

- Fast client: `python3 -m tools.cargo_run -- build --profile fast -p vera20k --bin
  vera20k --bin asset`, with `CARGO_TARGET_DIR` set to a build directory of your
  choice; the binaries land in `owned-worktrees/<sha256(worktree path)[:16]>/fast/`. Put a
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
- Browser runtime (open question 1): the browser build runs modules with the
  browser's own WebAssembly engine (`WebAssembly.Module`/`Instance` through js-sys;
  imports are wasm-bindgen closures into the engine, `i64` as BigInt). Measured in
  headless Chrome on 2026-11-24 with 200,000-operation calls after the rewrite: per
  call 0.025 ms computation, 0.045 ms calls (fib), 0.22 ms host calls (`get`),
  against 0.9 / 2.4 / 0.4 ms for the wasmi 2 interpreter, which missed the 2 ms bar
  on calls in the browser. Standalone WASI tools use wasmi. All runtimes execute the same rewritten bytes
  (`instrument.rs`, RFC sections 3 and 4): each basic block is charged in full
  before it runs, one operation per original instruction; `memory.copy/fill/init`
  and `table.copy/init` add one per 8 bytes of length; each function adds 4 +
  parameters + declared locals to a depth counter capped at 8,192 slots (2,048
  frames: a Chrome Web Worker held about 10,500 empty frames, the main thread
  20,600); float results that may be NaN are replaced with the canonical NaN;
  mutable float globals live as integer bits, so JavaScript reads them exactly; the
  memory maximum is the module's limit, so `memory.grow` fails at the same size.
  Wasmtime's fuel and Cranelift's NaN canonicalization are off. The instance is a
  JavaScript object of the main thread, so a browser match with modules never lends
  ticks to the simulation worker. Module memory is copied page by page for the
  digests. Played together on 2026-11-24: a desktop client (`--no-default-features
  --features wasm-net`, the build a browser package matches) and a browser page in
  one relay match with VeteranFury and with MeteorStorm (`tools/spawn/run.sh browser`,
  `BROWSERS=1 MOD=…`, one AI), 5,091 frames each, no desync, the page's module log
  equal to the desktop's.
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
  units must be object references, at most 256 orders and 4096 unit entries per module
  per frame. Orders queue in the module's slot (saved, hashed, cleared when the module
  stops) and execute in the next frame's command step, after the player commands
  (`run_ext_module_orders` in `run_network_command_tail`), as the player commands the
  house's player would send, through the same receiver (`apply_one_due_command`).
  Units that died or changed owner drop out; a defeated house's orders are dropped.
  Orders never enter the network stream. Every command of the table but `EjectBunker`
  runs: `Guard` with a cell is the area-guard click, `Harvest` the harvest click,
  `Enter` the entry click's MEGAMISSION (Eaten at a Grinder), `SuspendProduction`
  SUSPEND for the type's production category, `TakeProduct` the mobile PLACE of the
  factory kind (Infantry, Vehicle, Naval, Aircraft). A module order is its house's own
  sender (`CommandSender::HouseOrder`): custom control grants do not refuse its
  house-wide commands, and its `QueueProduction` asks the prerequisites the house's
  player would be asked (`production::buildable_by_player`).
- Order results (6i): `Commanded` (1016, object) on each actor of a command the
  receiver applied, `value` the 8.3 number, `other`/`cell` the target, `value2` a serial
  shared by one module order or by consecutive same-target commands of one house (a
  click); `OrderRefused` (1043, global) for a refused command, once per actor or once
  with subject 0 and `id` the type for a house-wide command, `house` the issuer, `value2`
  the `RefuseReason`. Both for player commands and module orders, raised after the
  receiver in `apply_one_due_command`, and only when a module trigger listens. The
  receiver (`apply_command_with_overlays`) returns `Result<(), CommandRefusal>`:
  MEGAMISSION actor gate `Gone`, token gate `IllegalTarget`; `CommandAllowed` and
  `ProductionQueued` vetoes `Vetoed`; PRODUCE without a factory (or, for a module
  order, without the player's prerequisites) `Prerequisite`, a refused append or a busy
  building factory `QueueFull`; PLACE without a finished product `NotReady`, a refused
  placement or exit `CannotPlace`; a superweapon not granted `Prerequisite`, not charged
  `NotReady`; every other refusal `Other`, read as `Gone` when its actor is no longer
  alive.
- Mod policy (`[VeraWasm]` in the mod's own `rulesmd.ini`): `Budget`, `MapBudget`
  (clamped to 1..=200000 operations: the most one call may use, and the frame's
  use past which no more event batches start), `MapWasm=no` refusing maps with
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
  `on_events` in subject order, at most 16 a call. Each call has the whole
  budget. The first call of a frame always starts, and another starts while the frame's use is below it. Delivery drops records for
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
  20,000 operations per call from a pool of half the frame budget
  (`ProductionAllowed` from the frame budget, keeping its last refusal when that
  is used up; `Defeat` from a quarter of its own, vetoing when that is used up); `random` uses a substream of
  (seed, source, frame, subject, kind) and leaves the main stream. A decision raised
  inside a module call can ask the other module while its pool lasts.
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
    `Cloakable`, `ForceDecloak`, `DisableWeapons`, `ImmunityFlags` (preview, see
    `ext_modules/modifiers.rs`; the modifier `Stat::Immunity` changed the save
    layout).
  - Properties: most of 7.1 (layer, target, experience, storage, transport,
    passengers, disguise, moving, facings, armor, relation with a viewpoint), 7.6
    `Passable.<SpeedType>`, `Revealed`, `Placeable` (cells `find` with a type), 7.7
    TechnoType/Weapon/Warhead keys, 7.8 `Difficulty`, mission timer, ambient light,
    ion storm. `lookup`/`name_of` for Projectile, Anim, VoxelAnim, ParticleSystem.
  - INI: `Wasm.Tags` on weapon, warhead and projectile sections; names for anim and
    enum parameters; a match notice when a map sets `Wasm.Tags`; event names for the
    4.4 and 4.5 kinds.
  - Hash and checksum: module memory is digested per 64 KiB page and each page
    re-digested with XXH32 on frames where `page ≡ frame (mod 15)` (a new page
    counts at once, with digest 0 until its turn); the hash
    and checksum read the digests (RFC 4/15). Digests are saved.

- The `Seldom` size ceiling in `object_turn_lookup_tests.rs` is 832 bytes (was 824):
  the modifier cache pointer. Approved by the maintainer.

## Done (phase 1b: INI registration)

RFC 0001 section 6h (0.0.21): modules give
modders INI names.

- `ini_register` (import 24, `vera_init` only; elsewhere invalid) registers a key on
  a section kind (`IniSection` 0..6), a registry (list section) or a module event
  (object or global). Names 1..63 bytes of `[A-Za-z0-9._]`, unique per kind across
  mod and map (the later one gets `Refused`), `Vera.` refused, 4096 per module. Both
  init paths register: `RulesHost` (modules with `on_rules`, kept in
  `PreparedExtModules::ini` until install) and the install-time `Host`.
- Before each module's `on_scenario_start` the module phase delivers `IniEntry`
  (1041; each distinct entry of its registries, `args` the entry name, `value` its
  position) then `IniKey` (1042; each section of the key's kind with a non-empty
  value, `subject` its type reference, `args` the value) from the merged rules text,
  out of the one-time budget, outside the per-frame record limits.
- `ModuleEvent` (1040, `EventDef.id` the event ID) raised by atomic action
  `RaiseEvent` (1042; 1041 `ApplyEffect` reserved); INI `Event=Wasm:name` and
  `RaiseEvent(target,Name,value,value2)`. Object events go through the type and
  object bindings, global ones to every global trigger with them.
- Module actions carry `args` (parameter text as a string ID, interned once at
  `trigger_create` into the `Module` action's unused `value`) and `target`; INI
  `Wasm:name(target,text)`. Name kind 18 `String` reads string IDs, 24
  `ModuleEvent` looks events up.
- `EventRecord` is 120 bytes (`args` 108, `target` 112), `DecideRecord` 160; the SDK
  delivery area holds 8 records. Registrations and strings are saved
  (`SavedExtModules::ini`) and in the state hash and checksum.
- Demo mods, configured only in INI (`sdk/examples/ini-shield`, `ini-aura`,
  `ini-counter`, `ini-academy`, each with `rulesmd.ini` and a Chinese README):
  `ini_demo_mods_reach_their_marks` plays 3000 frames with each (or the entry's
  optional fourth field) and checks its `DEMO-OK` marks when `VERA20K_TEST_INI_DEMOS`
  lists `module.wasm|rulesmd.ini|marks[|frames]` entries (`;`-separated): shield
  `entry,key,absorb,broken,restore`, aura `entry,key,apply,event,action`, counter
  `entry,count,reward,event`, academy `entry,key,enter,promote,event,action`. All
  pass (2026-11-24). Known gaps they found: `CreateObject` cannot target a factory
  (rewards appear beside it), INI triggers cannot hear a global module event (only
  code triggers), no stock evict action (the academy unloads by mission).
- Rust SDK: `ini::{key, registry, event}`, `IniKeyRecord`, `IniEntryRecord`,
  `ModuleEventRecord`, `Trigger::on_module_event`, `Action::raise_event`,
  `ActionDef::module_with`, args helpers (`ini::split`, `ini::int`, `ini::ints`).

## Done (phase 1c: house domain claims)

RFC 0001 section 6i, the claim part (`OrderRefused`, the read properties and the
seat budgets are separate work).

- House property `ControlDomains` (`Domain` mask, appendix 8.3 enum 25): `get` reads
  it, `set` claims and releases. The value is the set of bits the writer holds on
  that house; bits outside `DOMAIN_ALL` are invalid; `Refused` for a human,
  defeated or local player's house and for a bit the other source holds; leaving a
  bit out releases only the writer's own. A stopped module releases all its bits.
  Saved (`SavedExtModules::claims`), hashed and in the checksum only while any claim
  exists. `ExtModules::claimed_seats(source)` counts a source's seats.
- "Mod AI" seats: `SkirmishLaunchSession::mod_ai_slots` (AI `raw_slot_index`
  values, left out of the launch JSON while empty) names the seats. The modules are
  installed before the launch houses exist, so `initialize_skirmish_launch_houses`
  calls `Simulation::seat_mod_ai_houses`, which claims all four bits for the mod
  module when the mod's `[VeraWasm] Controller=yes` (`WasmPolicy::controller`) and
  its module runs; otherwise the native AI keeps the seat and the log says so.
- Native AI gates (`Simulation::module_claims`, one emptiness check without
  claims):
  - `Production`: the chooser block of `HouseClass::Update`
    (`update_production_choices`), `Factory_AI`'s start of production
    (`factory_ai`), `AI_Manage_Build_Queue` and `Fire_Sale` in the strategy tick,
    the house AI's MCV deploy (`owner_builds_its_own_base`); a refused deploy falls
    back to Guard instead of Hunt (`finish_initial_attempt`), and a claim that newly
    covers `Production` turns an MCV the house AI already sent to Hunt into Guard
    (`mcv_deploy::yield_base_hunts`), so Mission_Hunt's deploy arm does not deploy it.
  - `Forces`: team creation and AI triggers (`update_team_creation`), the house's
    teams in `TeamClass::AI` (`team_ai`, the teams stand still), `All_To_Hunt` in the
    strategy tick, the regroup after an MCV deploys.
  - `Abilities`: the computer-only unit arms: the AI auto-deploy of `Deployer=`
    infantry, `Mission_Hunt`'s computer undeploy, the computer capture arm
    (engineer capture, spy infiltration, garrisoning).
  - `SuperWeapons`: `AI_TryFireSW` (`superweapon::ai_fire::try_fire`).
- Residuals (also in `claims.rs`): the strategy tick's enemy search, anger and timer
  draw keep running; a building already in the yard when `Production` is claimed is
  still placed; the production mode still steps at exits; `Abilities` has no
  house-level entry (house AI ability orders mostly come through team scripts);
  ungarrisoning, the base defense response and a capturer's Hunt after capture are
  not gated; a claimed house's emptied team is not dissolved until the claim goes.
  The lobby does not offer "mod AI" yet (it must send `mod_ai_slots`; an older
  game refuses such a launch, `deny_unknown_fields`).
- Demo mod `sdk/examples/ai-takeover` (`rulesmd.ini` with `Controller=yes`, Chinese
  README): claims every computer house and plays it through orders only (MCV deploy,
  a four-building order placed at `Placeable` cells, infantry and tanks let out with
  `TakeProduct`, `AttackMove` at the player's base), hearing `Commanded`,
  `OrderRefused` and `Birth`; it attacks once its war factory stands. Its
  `ini_demo_mods_reach_their_marks` marks, over 4500 frames (the refinery and war
  factory take about 1260 frames each):
  `claim,deploy,build,place,train,attack,order-rejected,native-idle,base|4500`; the
  house stands all four buildings by frame 2400 and attacks with 40 units. `find`
  also returns the product a factory holds (off the map), so the demo counts a
  building as standing only beyond the one the building factory holds. SDK:
  `Event::ORDER_REFUSED`, `Order::take_product`.

## Done (phase 1c: reads for driving a house)

RFC 0001 section 6i's read surface (claims and orders are separate work):

- Objects: `Destination` (NavCom, cell or object), `WeaponReady` (rearm timer run
  out), `Deployed` (`is_fully_deployed`), `UnderConstruction` (BState 0),
  `Parasite` (ParasiteEatingMe); `ControlGroup` answers `Unsupported` (client only);
  `Armor` was already readable.
- Viewpoint: `Visible` (the viewpoint house's fog as the translucent fog draws,
  current sight under FogOfWar, and the screen GetVisualState `0x00703860` for
  cloak and sensors; `FogState::shows_objects_to` is now the one owner the draw
  plan, pickers and modules share) and `Engage` (`EngageResult`: GetFireError
  without range, the selected Warhead's Verses against the target's armor, InRange).
  `Query.viewpoint` may be an object: `Engage` reads for it, house viewpoint
  properties for its owner; filter clauses keep the trigger house. A clause on a
  "derived, high" property (`HIGH_COST_PROPERTIES`: `Engage`, `NearbyEnemyCount`,
  `TargetInRange`, `CanFireAtTarget`) costs `HIGH_CLAUSE_COST` (60) more per
  candidate.
- Houses: `Factory.<Kind>.Type/Progress/Ready/Queued` for the six kinds, from the
  House slot factory or, for a computer house, the first building-held one.
- Cells: `Zone.<MovementZone>` (MapClass::GetZoneID `0x0056D230`), `Obstructed`
  (a building in the cell, or a wall or impassable overlay).
- Global `AutomationAllowed` reads 0 until the lobby offers the room switch.
- Superweapons: references (bit 46, house index and `[SuperWeaponTypes]` index),
  `Type`, `Owner`, `Ready`, `ChargePercent` (writing runs stock action 129), and
  `find` scope 8 `SuperWeapons` over granted Supers, by house then type.

## Done (phase 2a: RFC 0.0.21 gaps and unit control points, RFC 0.0.22)

Items 1 and 2 of the control-point plan:

- Gaps the RFC already promised: stock `SpiedBy` (2) and `ThievedBy` (3) for module
  triggers (module-only raises; no native site raises them on a Tag); `Birth` for
  deployed buildings, undeployed units, survivors, crew and Ares pilots; garrison
  `TransportExited`/`PassengerRemoved` (`value2` 1) on ejection and on removal inside;
  `FatalDamage` on deaths without a damage commit (every `record_the_kill` terminal
  caller with Strength left; a sale is no death); `DamageIncoming` (preview) at the
  Techno receiver's entry; `SpeedMultiplier` on Fly, Jumpjet and Rocket (module
  modifiers alone on their type speed); `Crushed` and `Detonated` (fired projectiles
  only).
- Control points (`ext_modules::control`): decision-only `FireAllowed`, `Retaliate`,
  `DestinationSet`, `CrushAllowed`, `HarvestCell`, `DeployAllowed` (preview, 1044..1050
  without 1048); delivered `TargetChanged`, `Deployed`, `Promoted`, `ExperienceGained`,
  `Cloaked`, `Decloaked`. No new saved state: every decision completes at its commit
  point and every event is raised there.
- `DeployAllowed` is asked before every deploy: an MCV's (after its placement test), a
  simple deployer's start, and every infantryman's Do_Action(Deploy) through the one
  gate `deploy::infantry_request_deploy` (pending deploy, Mission_Unload, the
  computer's automatic deploy, the Approach arm). A refusal leaves a DeploysInto unit
  queueing Guard, a simple deployer's or infantryman's Unload ending in Guard without
  deploying, the others standing. Undeploys are not asked (RFC 4.5). Tests: a standing
  GI's refusal (`ext_modules_control_tests.rs`); the Control Bench's
  `movement.DeployInfantry`, `movement.DeployMcv`, `movement.DeployAllowed` (chopper).
- Deferred by the maintainer: `MissionChanged` (1048, tier `later`; no raise
  point: mission transitions are written from many callers without rules or frame
  effects). `has_kind` answers 0 for it (`ext_modules_control_tests.rs`).

## Provisional numbering (open question 5)

- Property IDs: section 7 names in first-appearance order, duplicates merged
  (`vera20k_wasm_ext::PROPERTIES`).
- New events from 1000 in appendix order, 4.3 then 4.4 then 4.5: `TakeDamage` =
  1000, `Killed` = 1004, `Fire` = 1005, `Birth` = 1011, `TransportEntered` = 1012,
  `PassengerAdded` = 1014, `HouseTransferred` = 1019, `Timer` = 1023,
  `SuperWeaponLaunched` = 1024, `Commanded` = 1016, `OrderRefused` = 1043; polls 1026..1030 (`PropertyPoll`, `HousePowerPoll`,
  `ModifierCountPoll`, `DistancePoll`, `VariablePoll`); decision-only 1031..1036
  (`ProductionAllowed` 1031, `ProductionQueued` 1032, `Defeat` 1033, `CommandAllowed`
  1034, `Launch` 1035, `ProjectileFrame` 1036); unit-level decision-only events
  from 1060 (`IdleMission` 1060, RFC 100060; `MissionStep` 1062, RFC 100062;
  `MoveFrame` 1063, `PathFound` 1064 and `ApproachPoint` 1065, RFC
  100063..100065; no INI names yet);
  `HarvestCell` is
  1049 (RFC 100049), asked in every ore scan. Stock events keep 0..88. One name table: `wasm-ext/src/event_kinds.rs`.
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
  wants houses on the shared creation counter); superweapon = bit 46 over
  `(house index << 16) | [SuperWeaponTypes] index` (same reason; documented at
  `SUPER_REF_BIT`); type `(1<<48)|(kind<<32)|id`;
  cell `(2<<48)|(x<<16)|y`; global `4<<48`.
- `Command` numbers (appendix 8.3) 0..25 as the SDK's `Command` lists them; the
  `Order` layout is a 24-byte header (kind, count, target, value) and the unit handles.
- Modifier and trigger handles: serials from the objects' shared creation counter
  (`allocate_stable_id`), as the RFC asks.
- Variable IDs: 1000 on, mod module's declarations then the map's, in declaration order.

## Cost without modules (2026-11-24)

`sim-bench` release builds of 52857e028 (the engine before any extension
code: no `wasm-ext`, the same non-extension engine commits) and bad49319b (phase 1,
INI registration, 6i, browser runtime), no modules, retail Death.mmx, 5 interleaved
runs each. Every final state hash is identical: skirmish 9000 ticks
`9d2f3b9e068b45ec`, march 20000 units 150 ticks `fc907cef24f6b40f`, match 1800
ticks `89503a631feffc5c`. Mean frame time, median of 5 (without / with extension):
skirmish 0.61 / 0.60 ms, march 33.85 / 33.67 ms, match 31.74 / 31.42 ms; main-thread
CPU likewise (match 22.05 / 21.74 ms). The machine was loaded (load average 15 to 43)
but the runs were interleaved. Without modules the extension adds only emptiness
checks: `module_claims` (empty claim store), `module_listens` (empty kind list)
before `Commanded`, `OrderRefused`, garrison and other module events, the seat
budget loop over no slots.

## Remaining in phase 1

- RFC 0.0.21 section 6i: the simulation side is implemented (claims and native AI
  gates, "mod AI" seats, `Commanded`/`OrderRefused`, the command table, the read
  properties, budgets 500,000/100,000/`SeatBudget`, `[VERA] WasmBudget` as a personal
  rule, 8 MiB per "mod AI" seat). The client side (`SimEvent`, `AutomationAllowed`,
  the client order rate limit) came with phase 2 (below). Left: the lobby and launcher
  offering "mod AI" seats (`mod_ai_slots`); a random map's seats get no extra memory.

Phase 1 is complete as far as this engine has owners for the RFC's tables; what is
left is listed under residuals. Small follow-ups found by the playtest mods:

- Read the house property `IsCivilian` (`house_property` in `ext_modules.rs`).
- An SDK helper that turns numbers into text without `core::fmt`.
- Build-dependent effects: `Unkillable` and the crit modifiers exist only in Phobos
  builds; a module cannot tell up front (raised on the RFC PR).

Before version 1 (RFC "Process"): phase 3 (a real mission with
modules, external modders, freeze), and the later items (peer calls,
`TargetChanged`'s decision form, `MissionChanged`).

## Done (control points, preview, RFC 0.0.23)

- Result areas in `on_decide` (`wasm-ext/src/decide.rs` `ResultArea`,
  `ProjectileSpec`, `PathSpec`, `ProjectileStep`; SDK views `FireDecision`,
  `LaunchDecision`, `ProjectileFrameDecision`).
- `Fire` decision form, the shot plan: `src/sim/combat/fire_plan.rs`, asked in
  `world_fire::emit_admitted_fire` before `BulletClass::Fire` (`0x006FF014`).
- `Launch` and `ProjectileFrame`, projectile references and properties (7.4),
  per-projectile module data and paths: `src/sim/ext_modules/projectiles.rs`;
  `ProjectileStore::reaches_candidate_step`, `ModuleMotion` in
  `src/sim/projectile.rs`; asked from `combat::projectile_delivery::admit_bullet`
  and `Simulation::visit_logic_bullet`. Tests `ext_modules_flight_tests.rs`.
- `FatalDamage` decision form, the death plan: `src/sim/combat/death_plan.rs`,
  read at the entry of `world_receiver::handle_death`; tests
  `ext_modules_death_tests.rs`. Bit 6 is Ares' TiberiumRemains (the engine has
  no wreck object): the death arm places the remains only from the plan
  (`Simulation::ares_leaves_tiberium_remains` is its native value); its
  round trip is tested in `death_plan.rs` (no Ares scene in the harness).
- `IdleMission`: `src/sim/ext_modules/unit_ai.rs` (`HarvestCell` is in
  `control.rs`, its test with the idle one); the computer
  mission arms the 6i claims had missed are gated (`claims.rs` module doc);
  tests `ext_modules_unit_ai_tests.rs`.
- `MissionStep` (MIS-S3..S6, preview), the outcome of the Guard, Sticky, Area
  Guard, Hunt and Attack bodies of infantry and vehicles: a `MissionStepResult`
  (target, destination, mission to queue, delay) in the result area, asked in
  `mission_handlers::dispatch_foot_mission` after the body and before
  `MissionClass::AI`'s queue and timer write (`0x005B3060`), applied in
  `src/sim/ext_modules/unit_ai.rs` through the class setters. The body runs in
  full first, so its scenario draws are unchanged (list 4.4); what else it did
  (shots, deploys, a mission queued on going idle) stands. No saved state.
  SDK `MissionStepDecision`; tests `ext_modules_unit_ai_tests.rs`.
- Movement (step 6): `MoveFrame` (result `MoveStep`), `Push` and `SetFacing`
  (atomic 1035, 1045) in `src/sim/ext_modules/motion.rs`, raised from
  `Simulation::process_active_locomotor` (`world/object_turn.rs`, FootClass::AI's
  Process call `0x004DA877`) for Drive/Ship/Walk/Mech/Hover movers with no step in
  flight; pushes are saved and hashed (`ExtModules::motions`, save field
  `motions`); standing infantry stay awake while module movement is active
  (`world/idle_sleep.rs`). `PathFound` (result `PathCell`s) in
  `src/sim/ext_modules/paths.rs`, asked in `movement/foot_path.rs`
  `search_foot_path` (Find_Path `0x004D3920`, before the copy at `0x004D3E82`);
  the path batch stays on (the decision is at the adoption, in the Foot's turn).
  SDK `MoveFrameDecision`, `PathFoundDecision`, `Action::push`,
  `Action::set_facing`. Tests `ext_modules_movement_tests.rs`.
- `ApproachPoint` (MOV-S18, preview, a cell reference): in
  `src/sim/ext_modules/paths.rs` (`module_approach_point`), asked in
  `movement/foot_approach.rs` `approach_foot_target` (FootClass::Approach_Target
  `0x004D5690`) once the ring search, a CloseRange= spot or a Jumpjet's first
  candidate chose the cell (`0x004D689A`), before the destination is set. The
  search and its spot draws run in full first. Not asked when the approach ends
  without a chosen cell. SDK `ApproachPointDecision`; test
  `ext_modules_movement_tests.rs`.
- `ROTMultiplier` (MOV-S14, preview), the body facing's turn rate:
  `src/sim/ext_modules/modifiers.rs` (`ModifierStore::apply_rot`,
  `Simulation::set_body_rot`). Event-driven: written to the body's
  `FacingClass` when the object's modifiers change (start, replacement,
  removal, expiry, load) and at the engine's later type-rate writes (Ares
  conversion, Carryall drop); the turning code never reads modifiers. The
  native word is kept in the modifier store while the property holds (save
  field `native_rots`) and restored exactly. Test in
  `ext_modules_movement_tests.rs`.

## Done (properties past the first set)

`src/sim/ext_modules/props.rs` (its module doc has the owner of each):

- Objects: `Cloaked` (CloakState 2), `NearbyEnemyCount` (enemies of the owner
  within `Sight=`), `TargetInRange` and `CanFireAtTarget` against TarCom
  (`Simulation::target_check`: InRange and GetFireError with range of the slot
  SelectWeapon picks), `DoorOpen` (DoorClass not closed; war factories, gates,
  vehicles with `Passengers=`), `Animating` (BState Active). The three searches
  are "derived, high" (`HIGH_COST_PROPERTIES`).
- Types: TechnoType `Points` (`Points=`, parsed into `ObjectType::points`), any
  rules type's `Name` and the global `GameMode` (the session's multiplayer mode
  class, its `mpmodes` section) as string IDs; `get` adds the text to the string
  table (saved and hashed), a filter clause only finds a held one. `Name` reads
  in `on_rules` too.
- TechnoType `Buildable` (viewpoint: `production::buildable_by_player`) and
  `find` scope 3 `Types` (TechnoTypes, Weapons, Warheads; wildcard or one type;
  no area or owner). SDK `Find::types`, `TypeRef::any`.
- Tests: `ext_modules_drive_tests.rs` (the GI duel's reads, a war factory's door,
  names, points, `GameMode`, `Types` with `Buildable`),
  `ext_modules_control_tests.rs` (`Cloaked` under the cloak modifiers). Control
  Bench marks `basics.type-name` .. `basics.door-open`.

## Done (phase 2: client modules, RFC 0.0.24)

PR #5251 (merged). A mod's and a
map's `ui.wasm` run under `vera:client@1.0` on the presentation side.

- Sandbox: `wasm-ext/src/client.rs` (`ClientHost`, `ClientEvent`, records, kinds),
  `client_module.rs` (`ClientModule`: load checks, `client_init`, `client_on_start`,
  `client_on_events` batches in the delivery area, `client_on_render(dt, alpha)`),
  `client_host.rs` (the 20 imports; the reads both worlds share go through
  `host.rs` `Reads`). One budget of 100,000 operations and 1,024 events per render
  frame; `client_init` and `client_on_start` get 5,000,000 each (an assumption by
  analogy with the simulation's one-time calls). Floats are allowed; the rewrite still
  canonicalises NaNs. `client_random` is the sandbox's own xorshift.
- Simulation (`src/sim/ext_modules/`): `Command::ModuleMessage` (VERA opcode `0x34`,
  98-byte body) through the command stream into `on_message` (`messages.rs`: 96 bytes,
  2 per player per source per frame, 16 in 15 frames, observers and ended games
  dropped, saved and hashed only while any wait); `send_notify` and `vera.notify`
  (`sections.rs`, FNV-1a IDs, load-time collision check); the client outbox
  (`client_outbox.rs`: notifications and forwarded `SimEvent`s, never saved or hashed,
  newest 4,096 kept); global `AutomationAllowed` from the room's rules
  (`HostRules`/`RulesSettings`, default off).
- Client reads: `sim::ext_modules::client_reads` (in the core crate, beside the
  simulation's own readers, which it calls behind a visibility gate:
  `object_seen_by`, disguise as the screen shows it, allies' `seen` properties only,
  spectators per the room). The app re-exports it (`src/app/ui_modules/reads.rs`).
- App (`src/app/ui_modules/`): `mod.rs` (loading the mod's and the map's modules
  under `MapUI=`, a fresh load and a save restore alike; the render-frame order;
  catch-up skips the modules and owes `client_on_start`; a fault disables one module
  and takes its nodes), `host.rs` (import routing), `tree.rs` (nodes on the match's
  own `Gui`, owners per RFC 10, names, the cursor, pointer routing, the map's
  `[VeraUI]` parts, button looks), `paint.rs` (the Panels and Overlay layers, pictures
  by name loaded on first use), `local.rs` (client actions, hotkeys, selection,
  `Notify`/`SimEvent` delivery, orders and messages; replays refuse both), `storage.rs`
  (`<settings>/wasm-storage/<key>/store.bin`, write-through at most once a second).
- `veraui.ini`: types `Image`, `Bar`, `List`; a button without `Command`; a text
  with `Label` (`docs/interface-layout.md`).
- SDK: `sdk/rust/vera20k-sdk/src/client.rs`, `notify.rs`; example
  `sdk/examples/ui-reinforcements` (RFC 10's round trip).
- Saves: layout `CLIENT_MESSAGES` `0xF8E3612E` (`snapshot/migration.rs`); older
  worlds hold no waiting message and did not allow automation.
- Verified: the round trip `ui_reinforcements_round_trip` (message to one Rhino and
  one notification; a second purchase refused); no-module `sim-bench` release builds
  of 60d988324 and this branch give the same final hashes (skirmish 9000 ticks
  `9d2f3b9e068b45ec`, match 1800 ticks `89503a631feffc5c`) and the same frame times;
  a mixed relay room (`tools/spawn/run.sh browser`, `BROWSERS=1`, `MOD=` the
  `sdk/examples/ui-ping` mod, desktop built `--no-default-features --features
  wasm-net`): each player's `ui.wasm` sent its message by itself, both machines'
  `rules.wasm` received both messages (each credits its sender), each player got
  only its own `Pong`, no desync.

Not yet: `asset` verbs for client modules (`wasm-embed` writes only
`[VeraWasmRulesPack]`, `wasm-check` loads only `vera:sim` modules, so a map's
`[VeraWasmUIPack]` has no tool); a look at the drawn module nodes in a real client; the lobby hint for
`MapUI=no` and the room switch "allow automation"; `Decoration`, `MapMarker`,
`ItemProgress` (`later`). Choices to review: one `ItemTooltips` value per grid (the
last writer wins); a module's `Art` on a `veraui.ini` part shows only pictures that
`veraui.ini` names; a text button's hover is a quarter shade; module calls skip a
render frame while a lent tick holds the world (its published frame has no objects);
`Storage::read` in the SDK asks the length with `Storage::len` instead of a second
read; a map whose file changed since a save loses its `[VeraUI]` parts on restore
(logged only).

## Done (battlefield visuals, RFC 0.2.0)

- `TintColor` and `TintIntensity` modifiers (preview): `ext_modules/modifiers.rs`
  (`Stat::TintColor`, `Stat::TintIntensity`, `GameEntity::module_tint`); the colour
  takes only an override of `0xRRGGBB`, the intensity only additions. The frame
  record captures the tint and `lighting::custom_tint` ORs its RGB565 word into the
  Phobos shield and AttachEffect tints and adds its intensity, the same for every
  viewer. Tests `ext_modules_control_tests.rs`
  (`tint_modifiers_compose_into_the_module_tint`) and `lighting_tint_tests.rs`.
- `AttachAnim` with `value2` and `DetachAnim` (1046, preview): `ext_modules/held.rs`
  keeps each looping animation (`hold_loops`) with its object, origin and end frame;
  the module phase keeps exactly the wanted ones alive (`maintain_ext_module_anims`):
  destroyed when the time is up or the object left the game, taken off while the
  object is in a transport or fully cloaked, put back after or when the engine
  destroyed it. Saved and hashed with the
  other held state. SDK `attach_anim_for`/`detach_anim`, `attachAnim(.., frames)`/
  `detachAnim`. Test `attach_anim_holds_for_its_frames_or_until_detached`.
- `DoorOpen` write (preview): `ext_modules/held.rs` holds the drawn door
  (`ext_module_door`); a war factory draws its `DeployingAnim=` door open at its
  last opening frame or not at all (`instances::shp::factory_unload_art`), a gate
  its open stage (`building_art::gate_held_body_frame`). The door's own state,
  read and run, is untouched. Test `a_war_factory_reads_its_door`.
- `ImageOverride` (230, preview): `ext_modules/held.rs` keeps the drawn type by
  object (saved, hashed with the held state); `write_image_override` admits a
  TechnoType of the object's category that is not a building. The frame record
  carries it (`module_image`); `instances::units::drawn_model_id` and the
  infantry SHP choice in `instances::shp` draw it; the SHP atlas coverage loads it
  in every colour (`EntityAtlasCoverage`), the unit atlas admits a voxel one
  (`admit_world_and_image_models`), and a changed object with one asks the full
  atlas check. An infantry override draws with the object's own `Sequence=`.
  Test `image_override_takes_a_type_of_the_objects_category`.
- Status templates (`client_status`, client import 21, and `[VeraStatus]` in
  `veraui.ini` and the map; docs/wasm-modules/status-templates.md):
  `app/ui_modules/status.rs` parses them into `vera20k-gui` status templates
  (`Origin::External`), `app/ui_modules/field.rs` paints them at the objects
  `ui_overlays::module_status_anchor` admits and draws the battlefield layer
  after the objects; `HideNative=` clears the original's families from each
  `TechnoExtras` (`hide_families`). Amounts come from the frame record
  (`StatusFacts`), variables under their RFC 6e visibility. Tests in
  `status.rs`.
- `Decoration` (8) and `MapMarker` (9) nodes (preview): `app/ui_modules/tree.rs`
  keeps them as hidden GUI nodes with an `Attached` display (handles, owners and
  removal as every module node); `app/ui_modules/field.rs` draws them in the
  battlefield layer (pictures, numbers, labels, dots, lines and ground circles)
  and the markers as radar dots (`MinimapRenderer::build_marker_dots_in_rect`).
  A decoration's `Tint` and `HideAnchor` reach the frame records as local looks
  (`SimHost::set_local_looks`, `frame::LocalLook`): `lighting::custom_tint` folds
  the tint after the Phobos tints, and the body admission skips a hidden body
  (it stays selectable). `Palette` and `Anim` are not drawn yet. Test
  `decorations_and_markers_hold_their_display`.
- Found in the in-game check (BayOPigs, example `kill-stars`):
  a world copy kept no module state at all, so the battlefield layer drawn from
  the presentation copy found no variable declarations; `ExtModules`' clone now
  keeps them (values stay the simulation's). Templates offer no `Pips` (its
  length-of-amount row takes a whole share, which `vera20k-gui` gives only the
  host); `Segments` with a count does it. `AtMost` joins the conditions.

## Residuals

- `Deployed` and `DeployAllowed` on an MCV and `Birth` for its yard and for building
  survivors have no engine test (the duel scene's MCV does not deploy; survivor rolls
  are random draws); the Control Bench covers them in a Battle (`movement.DeployMcv`,
  `movement.BirthYard`, `movement.BirthSurvivor`). A vehicle crew's `Birth` is tested
  by neither.

- The stopped-module notice is translated (`VERA:ModuleStopped`), but the fault
  reason inside it is English only.
- Events not raised: stock 0, 24, 31, 33, 51, 53, 54, 59, Ares 87, 88; 4.3
  `HitByWarhead`, `SpawnFire`, `HitTarget`, `ProjectileIncoming`, `HouseTransfer`,
  `ProductionCancelled`, `MissionChanged` (reasons in `events.rs` and `control.rs`).
  `Birth` misses paradrops, spawned aircraft, crate units and starting objects;
  non-departure cargo removal and Ares' trench traverse raise no passenger events;
  `TargetChanged` misses a new object's initial target; `DeployAllowed`/`Deployed`
  miss Ares' convert-on-deploy arms beyond the simple deployer; `CrushAllowed` is not
  asked by Can_Enter_Cell's crush codes (the crusher still drives in);
  `DamageIncoming` does not pre-check Ares rank immunities, CanTargetHouse,
  TypeImmune, bunker or warp gates; type-bound timers scan every object when due, and type-bound
  polls walk the IDs, reading only the due phase's types; `Timer` with only
  weapon/projectile bindings never fires.
- Decisions: decision triggers cannot have immediate actions; `ProductionAllowed`
  cannot lift CanBuild's own refusal and also asks Neutral/Special; `CommandAllowed`
  asks nothing for commands without an 8.3 number or an actor, nor for the AI's own
  missions; nested decisions get no merged rules text.
- Movement: `MoveFrame` and `Push` cover Drive, Ship, Walk, Mech and Hover only
  (not Fly, Jumpjet, Rocket, Tunnel, Teleport, transported, warping or train
  objects) and wait for a native step in flight to end; a module step checks the
  end cell only, treats Can_Enter's crush and traffic codes as blocked, never
  leaves a bridge deck and keeps an infantryman's sub-cell spot; a taken frame
  skips the native Process's own draws (Walk's sub-cell choice); a standing
  infantryman asleep when the first `MoveFrame` decider appears is asked from its
  next full turn; Scatter's immediate Walk Process may ask twice in a frame.
  `ROTMultiplier` scales the body (PrimaryFacing) only: not the turret, whose
  rate `Facing_Update` rewrites from `ROT=` on each aim, nor Fly's turns
  (SecondaryFacing), Hover's steering facing, Jumpjet's flight facing or
  Walk's snapped body; a Carryall's copied rate on its cargo is overwritten by
  a modifier change while carried. `Push` does not launch into the air. `PathFound` answers the
  route's first window only, is not asked for a train's queued extension or a
  route with a tube step, and is asked while the Foot is lifted (Mark0).
- Actions unsupported: `RemovePassenger`, team and projectile actions;
  house modifiers (`PowerOutage`); stock actions that
  need a live trigger (3, 6, 9, 13, 36..38, 52, 74, 75, 81, 91, 111, 113, 119..124,
  126, 130, Ares 146, 147), sound IDs (19, 98, 99), 40, 15, 39, 49, 50.
  Crit and `Unkillable` modifiers exist only in Phobos builds, as their owners do.
  `ROFMultiplier.ApplyOnCurrentTimer` is not applied; `Cloakable` Override 0 does
  not remove a type's own `Cloakable=`. Timed conversions are in the state hash, not
  the MP checksum.
- Properties not read: `Team`/`Tag` values (teams and tags have no kind 0
  handles: teams number from their own counter, tags are tag-storage indices,
  and a handle would change the save layout), a house's `Name` (player names are
  not simulation state), `ControlGroup` (client only: the local player's groups
  would desync the simulation), `DisguiseType` of a Mirage disguised as terrain
  (no name kind for terrain types), the house's `IsCivilian` (answers `Unsupported`,
  so a clause on it matches nothing; found by `sdk/examples/veteran-fury`);
  `Animating` is not writable and reads 1 only for the missile silo's launch (art
  `AnimActive=` is not bound); a `DoorOpen` write holds only the drawn door; `lookup(Armor)`,
  `lookup(Sound)`; `Passable` ignores objects and overlays; `PropertyPoll` reads
  without a viewpoint.
- `Health` writes of 0 or less answer `Refused`; an `Ammo` write restarts a running
  reload clock; `AddExperience` uses the base Cost.
- Modifiers are clamped to `[0, 32768]` (armor at least 2^-16); `Override` replaces
  the Phobos product too.
- Rules stage: `random` writes 0 with `Ok`; `set` on a type reference in `on_rules`
  answers `Unsupported`; the merged rules text is saved with every module save;
  `MapUI` is not saved; the lobby compares neither `on_rules` results nor `Api=`.
- Orders: `IgnoreOrders` is not honoured (no `IgnoresOrders` refusal) and no receiver
  refuses for credits (no `CannotAfford`); `EjectBunker` answers `Unsupported`;
  `Enter` skips the entry click's What_Action rows (they read the local player's
  control view); native AI gives missions without commands, so its units raise no
  `Commanded`; a rally point `CommandAllowed` narrowed still raises `Commanded` on
  every producer named; a player's SUSPEND, mobile PLACE and wall sale refusals carry
  `id` -1; a MEGAMISSION record carrying NavComAux, Planning Mode records and Ares'
  paid two-stage superweapon commands raise neither result event.
- INI: sounds take numbers only; the map `Wasm.Tags` warning is a match notice, not
  a lobby warning; one module per trigger's `Wasm:` actions.
- 6i reads: `Engage` uses the target type's own armor (no `TargetArmor`), and a
  transient GetFireError answer (Rearm, Facing, Moving, Busy, ...) reads as `Ok` in
  range and can hide a later Illegal check; `Visible` uses current sight under
  FogOfWar for every client and counts a tunnel unit's ripple as visible;
  `Zone.<MovementZone>` reads the ground zone under a bridge deck; `Obstructed`
  ignores trees and units; a computer house with two factories of a kind reports
  the first built; `ChargePercent` 50 reads 49 for most recharge times
  (SetPercentage's f32 0.01). The Rust SDK has no `Find` helpers yet for scope 8
  or an object viewpoint (set `Query` fields directly).
- A type name shared by two categories resolves to the first. Houses queries with
  area, type or owner answer `Unsupported`.
- The Rust SDK's delivery area is fixed at address 8 (9 records); one
  `named_actions!` and one `vars!` per module.
- Call depth weighs parameters and declared locals, not operand stack height: a
  function with a very deep expression stack and deep recursion could reach an
  engine's own stack (Wasmtime's 512 KiB, a browser worker's) before the 8,192-slot
  limit; the module then stops with "call stack exhausted" on that machine only.
- Browser: speed measured in Chrome only (not Firefox, Safari or a slow device);
  trap texts differ by browser (only "stopped" enters the state hash); Chrome
  refuses synchronous compilation of a module over 8 MB on the main thread, so a
  rewritten module near the 8 MB input limit fails to load there.

## Validation commands

```
export CARGO_TARGET_DIR=<your build directory>
python3 -m tools.cargo_run -- test -p vera20k-wasm-ext --lib
# Build each sdk/examples/* with `cargo build --release`, then point the
# VERA20K_TEST_*_WASM variables (RULES, MODIFIER, TRIGGERS, ACTIONS, ORDERS, DECIDE)
# at the .wasm files. VERA20K_TEST_PLAYTEST_WASM takes the four playtest mods
# (veteran-fury, siege-doctrine, meteor-storm, trigger-lab), `:`-separated, and
# plays 2000 frames with each (playtest_modules_keep_running, --nocapture prints
# their logs). VERA20K_TEST_MUSTER_WASM takes muster_field.wasm and plays its first
# wave from player command records (muster_field_plays_a_round_from_player_orders)
# and a campaign lost with an empty company (muster_field_ends_when_the_last_banner_falls);
# VERA20K_TEST_MUSTER_UI_WASM adds its muster_field_ui.wasm and plays a muster from
# the interface's messages alone (muster_field_plays_from_its_interface).
# VERA20K_TEST_UI_REINFORCEMENTS takes `rules.wasm|ui.wasm` of
# sdk/examples/ui-reinforcements (ui_reinforcements_round_trip).
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
