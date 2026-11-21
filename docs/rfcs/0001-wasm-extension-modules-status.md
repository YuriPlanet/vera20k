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

## Provisional numbering (open question 5)

- Property IDs: section 7 names in first-appearance order, duplicates merged
  (`vera20k_wasm_ext::PROPERTIES`).
- Atomic actions from 1000 in appendix 5.3 order (`AddMoney` = 1015).
- TechnoType ID: position in the rules object registry; Country ID: `CountryIdx`.
- References: object = entity `stable_id`; house = bit 47 over the house index (the RFC
  wants houses on the shared creation counter); type `(1<<48)|(kind<<32)|id`;
  cell `(2<<48)|(x<<16)|y`.

## Remaining in phase 1

1. Triggers: `trigger_create/remove/bind/unbind`, `TriggerDef`/`EventDef`/`ActionDef`,
   filter clauses, delivery queue and `on_events` with `EventRecord`, `vera.actions`
   custom section and module-action dispatch, `vera.delivery`, deferred chain triggers.
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

## Validation commands

```
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- test -p vera20k-wasm-ext --lib
VERA20K_MIN_FREE_GIB=4 RA2_DIR=<retail> VERA20K_REQUIRE_RETAIL_ASSETS=1 \
  python3 -m tools.cargo_run -- test -p vera20k-core --lib sim::ext_modules::
VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- check -p vera20k --lib --bins
VERA20K_SNAPSHOT_LAYOUT=1 VERA20K_MIN_FREE_GIB=4 python3 -m tools.cargo_run -- \
  check -p vera20k-core --lib --target wasm32-unknown-unknown --features wasm
```

Last run (HEAD `15d5881e2`): wasm-ext 9/9, `sim::ext_modules` 7/7, client check and
wasm32 check Finished.
