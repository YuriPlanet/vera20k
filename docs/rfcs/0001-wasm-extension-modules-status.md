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
| Modifier store and the per-object cache the combat and movement owners read | `src/sim/ext_modules/modifiers.rs` |
| Declared variables (`vera.vars` values, IDs from 1000) | `src/sim/ext_modules/vars.rs`; section parser `wasm-ext/src/sections.rs` |
| Module phase hook | end of the Triggers arm in `src/sim/world/master_frame.rs` |
| State hash / MP checksum / save restore | `world_hash.rs`, `multiplayer_checksum.rs`, `snapshot/simulation_restore.rs` |
| Player notice when a module stops | `TriggerEffect::ExtModuleStopped`, handled in `src/app/match_runtime/sim_tick.rs` |
| Map embedding (`[VeraWasm]`, `[VeraWasmRulesPack]`) | `src/map/ext_module_pack.rs` |
| Loading | `headless_scenario.rs`, `app/loading/init.rs` (mod folder `rules.wasm`, then map) |
| `asset wasm-check / wasm-embed / wasm-extract` | `src/asset_tools/verb_wasm.rs` |
| Rust SDK | `sdk/rust/vera20k-sdk` (no_std; `props.rs` property constants, `vars.rs` and `vars!`); examples `sdk/examples/minimal-rules`, `sdk/examples/modifier-vars` |

Build an example: `cd sdk/examples/minimal-rules && cargo build --release`
(its `.cargo/config.toml` targets wasm32 with the allowlisted features). The engine tests
`a_rust_built_module_runs` and `a_rust_built_modifier_module_runs` run them when
`VERA20K_TEST_RULES_WASM=<path>` and `VERA20K_TEST_MODIFIER_WASM=<path>` are set.

## Done (phase 1)

- Loading and validation, operation budgets, deterministic NaN, module random stream,
  disable-on-fault with an in-game notice, saves, state hash, MP checksum.
- Map embedding, mod-folder `rules.wasm`, `asset` verbs.
- Imports: `frame`, `random`, `log`, `has_kind`, `lookup`, `name_of`, `get`,
  `get_many`, `set`, `modify`, `unmodify`, `var_get`, `var_set`, `find`, `action` (only
  `AddMoney`).
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
  candidate.
- Writes through owners: `Health` (no kill credit; 0 or less refused), `Owner`
  (ChangeOwner, as the stock change-house action), `Rank` (SetRookie/Veteran/Elite).
- Readable properties: objects `Type`, `Owner`, `Category`, `Health`, `Strength`,
  `HealthPercent`, `X`, `Y`, `Z`, `Cell`, `Rank`, the three wired multipliers; houses `Index`, `Money`, `PowerOutput`,
  `PowerDrain`, `LowPower`, `IsDefeated`, `IsHuman`; TechnoType `Strength`.
- `lookup`/`name_of`: TechnoType, Country, Property, Variable. `find`: Objects,
  Houses, Modifiers.
- Rust SDK: raw bindings for all 23 imports, wrappers for the above, export macros,
  `Prop::<Name>` constants for every property (a wasm-ext test checks them against
  `PROPERTIES`), `Modifier` (`multiply`, `add`, `override_`, `remove`), `vars!`.

## Provisional numbering (open question 5)

- Property IDs: section 7 names in first-appearance order, duplicates merged
  (`vera20k_wasm_ext::PROPERTIES`).
- Atomic actions from 1000 in appendix 5.3 order (`AddMoney` = 1015).
- TechnoType ID: position in the rules object registry; Country ID: `CountryIdx`.
- References: object = entity `stable_id`; house = bit 47 over the house index (the RFC
  wants houses on the shared creation counter); type `(1<<48)|(kind<<32)|id`;
  cell `(2<<48)|(x<<16)|y`; modifier = a serial from the objects' shared creation
  counter (`allocate_stable_id`), as the RFC asks.
- Variable IDs: 1000 on, mod module's declarations then the map's, in declaration order.

## Remaining in phase 1

1. Triggers: `trigger_create/remove/bind/unbind`, `TriggerDef`/`EventDef`/`ActionDef`,
   filter clauses, delivery queue and `on_events` with `EventRecord`, `vera.actions`
   custom section and module-action dispatch, `vera.delivery`, deferred chain triggers.
2. INI bindings: `Wasm.Tags` on TechnoType/Weapon/Warhead/Projectile, tag and trigger
   sections (appendix "INI keys").
3. Modifiers: the other 7.2 properties (ROF, range, crit, `Unkillable`, cloak, ...),
   house modifiers (`PowerOutage`), `ModifierStart`/`Stacks`/`Expire` events, Phobos
   AttachEffect through the same owner, stack limits (open question 13).
4. Variables: client visibility (phase 2); a write resetting `VariablePoll` timers.
5. Property writes `Mission`, `Ammo`, `Tag`, `Online` and a credit-free kill by
   `Health`; more properties (7.1, 7.6 cells, 7.7 types and rules).
6. Actions: atomic actions 5.3 and stock actions 5.2 through the trigger action
   executor.
7. `order` (6g), `rules_get`/`rules_set` and `on_rules` (RFC 8), mod `[VeraWasm]`
   `Budget`/`MapBudget`/`MapWasm`/`MapUI`.
8. Two or three decidable events and `on_decide` (open question 10).
9. SDK: property constants generated from `PROPERTIES`, wrappers for each new import.

## Residuals

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
- A type name shared by two categories resolves to the first.
- Houses queries with area, type or owner answer `Unsupported`.

## Validation commands

```
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- test -p vera20k-wasm-ext --lib
VERA20K_MIN_FREE_GIB=4 RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 \
  python3 -m tools.cargo_run -- test -p vera20k-core --lib sim::ext_modules::
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- check -p vera20k --lib --bins
VERA20K_SNAPSHOT_LAYOUT=1 VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- \
  check -p vera20k-core --lib --target wasm32-unknown-unknown --features wasm
```

Last run (modifiers/variables commit): wasm-ext 12/12, `sim::ext_modules` 13/13 with both
Rust examples built, `sim::movement` 551 ok, `sim::world::object_turn` 39 ok, client check,
wasm32 check and `--features phobos` core check Finished.
