# Wasm extension modules: development status

The current checkpoint for implementing [RFC 0001](0001-wasm-extension-modules.md)
(API in the [appendix](0001-wasm-extension-modules-api.md)). Replace this file's
content as work lands; do not append a diary.

## Where the work is

- Branch `feature/wasm-extension-modules` on `libi/vera20k`, PR
  [YuriPlanet/vera-mod-engine#5](https://github.com/YuriPlanet/vera-mod-engine/pull/5)
  (the RFC's experiment repository).
- Scope: RFC phase 1 (`rules.wasm`) first, then phase 2 (`ui.wasm`), then phase 3.
  The TypeScript SDK is deferred by the maintainer; validate with the Rust SDK.

## Map of the code

| Piece | Where |
| --- | --- |
| Sandbox: Wasmtime engine, load checks, fuel, imports, saves of memory/globals | `wasm-ext/src/native.rs` (desktop); `browser.rs` is a stub that refuses modules |
| ABI types: import table, `Status`, `SimHost` trait, `PROPERTIES`, `Query`/`Clause` parsing | `wasm-ext/src/lib.rs` |
| Sandbox tests with a fake host | `wasm-ext/src/sandbox_tests.rs` |
| Simulation side: slots, module phase, host (`Host` holds `&mut Simulation` during a call), saves, hash, checksum | `src/sim/ext_modules.rs`, tests in `ext_modules_tests.rs` (XMP03T4, retail) |
| Module phase hook | end of the Triggers arm in `src/sim/world/master_frame.rs` |
| State hash / MP checksum / save restore | `world_hash.rs`, `multiplayer_checksum.rs`, `snapshot/simulation_restore.rs` |
| Player notice when a module stops | `TriggerEffect::ExtModuleStopped`, handled in `src/app/match_runtime/sim_tick.rs` |
| Map embedding (`[VeraWasm]`, `[VeraWasmRulesPack]`) | `src/map/ext_module_pack.rs` |
| Loading | `headless_scenario.rs`, `app/loading/init.rs` (mod folder `rules.wasm`, then map) |
| `asset wasm-check / wasm-embed / wasm-extract` | `src/asset_tools/verb_wasm.rs` |
| Rust SDK | `sdk/rust/vera20k-sdk` (no_std); example `sdk/examples/minimal-rules` |

Build the example: `cd sdk/examples/minimal-rules && cargo build --release`
(its `.cargo/config.toml` targets wasm32 with the allowlisted features). The engine test
`a_rust_built_module_runs` runs it when `VERA20K_TEST_RULES_WASM=<path>` is set.

## Done (phase 1)

- Loading and validation, operation budgets, deterministic NaN, module random stream,
  disable-on-fault with an in-game notice, saves, state hash, MP checksum.
- Map embedding, mod-folder `rules.wasm`, `asset` verbs.
- Imports: `frame`, `random`, `log`, `has_kind`, `lookup`, `name_of`, `get`,
  `get_many`, `set`, `var_get`, `var_set` (Red Alert 2 variables 0..149), `find`,
  `action` (only `AddMoney`).
- Readable properties: objects `Type`, `Owner`, `Category`, `Health`, `Strength`,
  `HealthPercent`, `X`, `Y`, `Z`, `Cell`; houses `Index`, `Money`, `PowerOutput`,
  `PowerDrain`, `LowPower`, `IsDefeated`, `IsHuman`; TechnoType `Strength`.
- `lookup`/`name_of`: TechnoType, Country, Property. `find`: Objects, Houses.
- Rust SDK: raw bindings for all 23 imports, wrappers for the above, export macros.
- Runtime triggers: `trigger_create/remove/bind/unbind` (`TriggerDef` parsing in
  `wasm-ext/src/triggers.rs`); the registry lives in the trigger runtime
  (`src/sim/trigger_runtime/module_triggers.rs`), saved, hashed and checksummed with it;
  raising, clauses, firing and delivery in `src/sim/ext_modules/triggers.rs`. Events
  `TakeDamage` (object_health commit), `Killed` (end of `record_the_kill`), `Fire`
  (end of `emit_admitted_fire`), `Timer` (module phase). Object and TechnoType
  bindings; filter clauses on Subject/Other/House properties and RA2 variables;
  immediate `AddMoney` and `SetVar` (RA2 variables) through the host's own `action` and
  `var_set`; `Module` actions queue `EventRecord`s delivered by `on_events` (sorted by
  subject, budget- and backlog-limited, dropped for disabled/removed triggers or an
  ended game). `vera.actions` and `vera.delivery` are read and checked at load;
  `lookup(ModuleAction)`; trigger `Enabled`/`House`/`Repeat` via `get`/`set`;
  `lookup`/`name_of` for Weapon and Warhead. SDK: `Trigger` builder, `ActionDef`,
  event views, `on_events!`, `named_actions!`; example `sdk/examples/panic-triggers`
  (engine test `a_rust_built_trigger_module_receives_its_records`,
  `VERA20K_TEST_TRIGGERS_WASM=<path>`).

## Provisional numbering (open question 5)

- Property IDs: section 7 names in first-appearance order, duplicates merged
  (`vera20k_wasm_ext::PROPERTIES`).
- Atomic actions from 1000 in appendix 5.3 order (`AddMoney` = 1015, `SetVar` = 1024,
  `Module` = 1036).
- New delivered events from 1000 in appendix 4.3 order (`TakeDamage` = 1000,
  `Killed` = 1004, `Fire` = 1005, `Timer` = 1023).
- Weapon/Warhead IDs: position in the rules' weapon/warhead registries.
- Trigger handles: creation serials from the shared object counter.
- TechnoType ID: position in the rules object registry; Country ID: `CountryIdx`.
- References: object = entity `stable_id`; house = bit 47 over the house index (the RFC
  wants houses on the shared creation counter); type `(1<<48)|(kind<<32)|id`;
  cell `(2<<48)|(x<<16)|y`.

## Remaining in phase 1

1. Triggers, rest: more event kinds (stock 0..61, the other 4.3 events, polling 4.4),
   weapon/warhead/projectile and cell bindings, stock and other atomic actions in
   triggers, `vera.vars` clauses and `SetVar`, deferred chain triggers and their caps
   (open question 17), `Killed` lethal warhead, map `Action=Wasm:name`.
2. INI bindings: `Wasm.Tags` on TechnoType/Weapon/Warhead/Projectile, tag and trigger
   sections (appendix "INI keys").
3. Modifiers: `modify`/`unmodify`, the 7.2 multipliers wired into combat/movement,
   expiry, saves and hash.
4. Variables: `vera.vars` (IDs from 1000, global/house/object scope, visibility).
5. Property writes (`Health`, `Owner`, `Mission`, ...) through their owners; more
   properties (7.1, 7.6 cells, 7.7 types and rules).
6. Actions: atomic actions 5.3 and stock actions 5.2 through the trigger action
   executor.
7. `order` (6g), `rules_get`/`rules_set` and `on_rules` (RFC 8), mod `[VeraWasm]`
   `Budget`/`MapBudget`/`MapWasm`/`MapUI`.
8. Two or three decidable events and `on_decide` (open question 10).
9. SDK: property constants generated from `PROPERTIES`, wrappers for each new import.

## Residuals

- Hash and checksum read all module memory each frame, not the 1/15 page rotation.
- The stopped-module notice is English only.
- Every property is read-only; `Health`/`Owner` writes answer `Unsupported`.
- A type name shared by two categories resolves to the first.
- Houses queries with area, type or owner answer `Unsupported`.
- Event sites check a per-kind flag, then a map lookup by subject, not an entity side
  column (RFC 15.1 item 1).
- A disabled module's triggers are removed with it.
- The Rust SDK's delivery area is fixed at address 8 (9 records): `link_section`
  statics cannot hold an address. A stack within 1 KiB of its 1 MiB limit during
  `on_events` would overwrite unread records. One `named_actions!` per module.

## Validation commands

```
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- test -p vera20k-wasm-ext --lib
VERA20K_MIN_FREE_GIB=4 RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 \
  python3 -m tools.cargo_run -- test -p vera20k-core --lib sim::ext_modules::
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- check -p vera20k --lib --bins
VERA20K_SNAPSHOT_LAYOUT=1 VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- \
  check -p vera20k-core --lib --target wasm32-unknown-unknown --features wasm
```

Last run (triggers branch): wasm-ext 12/12, `sim::ext_modules::` with
`sim::trigger_runtime::` 338 passed (5 ignored), client check and wasm32 check Finished.
