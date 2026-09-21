# Native comparison workflow

Native comparison guidance; adapt as useful and distinguish findings from hypotheses.

Use original `gamemd.exe` instructions to produce reference outputs, then compare
the Rust function used by the engine against them. The shared Python runner handles
executable identity, bounded execution, fresh function-call state, diagnostics, and
reference-file checks. It does not establish that a chosen function or fixture
represents the game's active path.

### Capstone

Python `capstone` is also installed in this development environment. Use it when
helpful for native binary analysis; consult its documentation as needed.

## Run an existing comparison

From the repository root, with Python 3.10+ and Unicorn **2.1.4** installed:

```powershell
python -m pip install unicorn==2.1.4
$env:VERA20K_GAMEMD_EXE = 'C:/your-retail-install/gamemd.exe'
python -m tools.color_oracle.hsv_to_rgb
cargo test -p vera20k --lib rules::color_scheme::tests::hsv_to_rgb_matches_native_oracle_all_hues_at_sv_boundaries
```

`RA2_DIR` is an alternative; an explicit executable path takes precedence and never
silently falls back. The loader hashes the same immutable bytes it maps. Only the
retail executable with SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`
is accepted, including under optimized Python. No executable is distributed here.

Both halves matter: the Python command checks current native output against the
reference; the Rust test checks production conversion against that reference.
Python success alone does not show Rust parity. Normal Rust tests use committed
outputs and do not require Unicorn or an executable. Follow `AGENTS.md` before Cargo.

These generators now default to **read-only checks**, also spelled `--check`:

| Python module | Sampled native behavior | Rust consumer/test location |
| --- | --- | --- |
| `tools.rmg_oracle.gen_rng_vectors` | Seeded state and 16 draws for five seeds | `src/map/rmg/rng.rs` |
| `tools.rmg_oracle.gen_x87_vectors` | Eight Gaussian draws for two seeds | `src/map/rmg/x87.rs` |
| `tools.storage_oracle.sed_description` | 28 fresh-file Description fixtures and one cached-section diagnostic; original reader with supplied INI indexes | `src/map/rmg/description.rs`; disk-load regression in `saved_seeds.rs` |
| `tools.projectile_oracle.ordinary_motion` | 120 cases, eight gravity/candidate blocks each | `src/sim/projectile.rs`, `src/sim/world/projectile_collision.rs` |
| `tools.projectile_oracle.vertical_motion` | 110 cases, eight velocity/candidate blocks each | Same projectile consumers |
| `tools.color_oracle.hsv_to_rgb` | All 256 hues at nine saturation/value pairs | `src/rules/color_scheme.rs` |
| `tools.spatial_oracle.shroud_current_sight` | 23 ordinary-cell reveal/gap/fire/Psychic/SpySat-bulk/120-frame sequences plus9 signed Foot timer gates; actual MapCell, selected Gap/bulk blocks and complete periodic sweep | `src/sim/vision/vision_tests.rs`; world, restore and tactical CPU regressions |
| `tools.spatial_oracle.map_queries` | 114 lookup cases, 15 LocalSize prefixes, 2,010 playfield queries and five retained-dummy calls | `src/sim/cell_rect_native_tests.rs`, production `cell_rect.rs` / `map/playfield.rs` |
| `tools.spatial_oracle.bridge_records` | 83 ordered high/Tube record cases, CellIterator first-null traversal and shared dummy coordinates | `src/sim/bridge_state/record_native_tests.rs`, production `record_scan.rs` / `map/resolved_terrain.rs` |
| `tools.spatial_oracle.bridge_gap_flags` | 20 ordered fresh-load inactive high-record restamps and48 original setter anchor prefixes | `src/sim/bridge_state/gap_restamp_tests.rs`; real flags/save/hash and production building placement |
| `tools.spatial_oracle.bridge_restamp_retail` | Deadman original producer/restamp/ore-admission composition; pre-fix export regenerated at4f72210f | `src/sim/movement/bridge_restamp_retail_probe.rs`; current loader compares80 native post-state cells |
| `tools.spatial_oracle.bridge_base_edges` | 86 signed/clamped endpoint, canonical pair, reverse bucket order and deduplication cases | `src/sim/pathfinding/bridge_base_native_tests.rs`, production `zone_build.rs`; cache/restore regression in `world/navigation_tests.rs` |
| `tools.spatial_oracle.tube_hierarchy` | 65 high/Tube hierarchy helper cases, five ReadTubes writes, six raw-zone gate cases, five constant-walk endpoints and seventeen DWORD zone queries; raw data domain remains bounded | `src/sim/pathfinding/tube_hierarchy_native_tests.rs`, production `hierarchy_bridge.rs`; route/transaction regressions in `zone_search_tests.rs` and `app/persistence/tube_hierarchy_restore_tests.rs` |
| `tools.spatial_oracle.path_entry` | 19 original endpoint lookup/query/retained projection and conditional playfield cases; final mover predicate supplied | `src/sim/pathfinding/native_path_entry.rs`, production wrappers and tests in `zone_search_tests.rs` |
| `tools.spatial_oracle.bounce_height` | 48 ground/query/surface cases through original Bounce update; flat contacts and airborne slopes, velocity signed zero excluded from Rust comparison | `src/sim/world/bounce_terrain_tests.rs`, live `bounce_terrain.rs` and `bounce.rs`; separate cliff/slope physics remains open |
| `tools.spatial_oracle.bounce_startup_capture` | Original process captures Bounce104/416 scalars and x87 control at four startup boundaries; executable section integrity | Inputs to `bounce_height`; constructor/startup evidence, not complete debris physics |
| `tools.spatial_oracle.locomotor_can_use_track` | ILocomotion `+0xA4` `Can_Use_Track` (Drive `0x004B4B00`, Ship `0x006A4130`): 6,400 Drive and 5,920 Ship cases over every turn selector, cursors on/beside each raw chain index, both short-track states and ten path-queue heads | `src/sim/movement/drive_track_tests.rs` `occupant_can_use_track_matches_native_oracle`; production `drive_track.rs::occupant_can_use_track`, consumed by `cell_entry.rs::classify_blocker` and `bump_crush.rs::build_entity_block_sets` |
| `tools.spatial_oracle.tube_startup_capture` | Unpatched Windows process startup FPCW/adjacent data at four hardware breakpoints; full executable section integrity | Original initializer inputs for `tools.spatial_oracle.tube_hierarchy`; startup-only evidence, not runtime immutability |
| `tools.spatial_oracle.walk_paid_step` | 40 original paid Walk steps through the coordinate branch, including facing equality, zero speed, cell boundaries and five chained Slice6 production steps; movement-speed integer supplied | `src/sim/movement/walk_step_tests.rs`; production `walk_step.rs`, Slice6 frame assertions and move-to-fire/save-restore regression |
| `tools.spatial_oracle.walk_direction_table` | All 65,536 heading words: original sine/cosine indexes and table bits, plus the complete retail table bytes | `src/util/native_trig.rs::tests::every_walk_heading_uses_the_original_trig_entries`; full-heading paid Walk displacement |

Run them as modules (`python -m ...`). Imports do not emulate or write files;
`--help` works without retail configuration. `--output <path>` selects another
reference. A missing reference or mismatch fails; checking never creates or fixes it.
`--write` explicitly replaces the payload and its `.meta.json` sidecar. Review any
changed values against native evidence before accepting them. This is not a way to
make failing Rust tests green.

Sidecars identify the executable, Unicorn Python binding and native core versions,
entries, fixture assumptions, substitutions, coverage, and canonical payload hash.
Metadata changes fail checks even if outputs match, prompting review of the changed
environment. Existing references without sidecars remain checkable, with an explicit
message that their historical environment is unknown. Regeneration can record the
current environment; it cannot recover historical provenance.

## Add a comparison

Start with `tools/color_oracle/hsv_to_rgb.py` for a complete example. Establish the
body, active callers, calling convention, input/state ownership, and observable
outputs before constructing the fixture. Select cases that exercise meaningful
branches and boundaries. Avoid a second Python implementation of the algorithm.

* `call(address, ecx=..., stack_args=[...], writes={...}, dumps={...})` maps verified
  code in a fresh emulator for each invocation. Entries must lie in an original
  executable section; fixture writes cannot replace native instructions. Carry only
  the native state your comparison deliberately preserves between calls.
* For an interior block, use `load_image(uc)` and `run_checked(uc, begin, end)` after
  establishing its live registers, memory and x87 state. Multiple legitimate stop
  boundaries can be a tuple. This low-level API supports custom fixtures; it cannot
  certify their initialization or detect every patch a caller makes.
* Declare necessary intermediate addresses with `required_addresses=[...]` when
  reaching an endpoint alone could conceal a bypass. They record instruction-hook
  visits, not proof of instruction effects or of full path equivalence. Endpoints
  stop **before** executing their instruction and cannot be required addresses.
* Use `finish_vectors(generate, path, provenance=lambda: provenance(...))`. Lazy
  callables keep help and argument errors independent of native work. Record the
  scope, assumptions, substitutions (including none), and native entries explicitly.
* Consume native outputs in a focused Rust test of the actual production function.
  Assert case identity/coverage as well as output equality, and review the connection
  to callers independently. The HSV example checks 2,304 inputs, not all 256³ inputs
  or final rendered colors.

## Execution guarantees and limits

Unicorn can return normally when its instruction or time budget expires. The runner
therefore requires a declared endpoint and rejects timeouts, faults, unexpected
stops, and missing required visits. Failures include recent instruction addresses.
Existing custom exit configuration is disabled so it cannot override those boundaries.
Budgets default to five million instructions and ten seconds; choose a tighter
instruction budget for small leaves. Existing hooks remain the fixture owner's
responsibility. No zero-return or unmapped-memory fallback silently invents an answer.

The loader preserves the legacy broad RWX image mapping and zero-filled BSS. It is
**not a Windows loader**: imports, constructors, TLS, OS services, and runtime global
state are not initialized. A mapped read is not evidence that its value is realistic.
Declare supplied state and hooks; substituted function results validate only the
remaining computation. Whole-game, scheduling, device, and GPU behavior require
additional evidence.

The legacy default x87 control word is `0x0E7F` (53-bit precision, truncation); verify
the appropriate ambient state for each native entry. `capture_st0=True` executes an
external FSTP observation stub and stores **binary64**, not the full 80-bit register.
The stub must finish before a result is returned. Unicorn transcendental instructions
such as FYL2X may differ from physical x87 hardware. Gaussian vectors retain that
caveat; equality is not hardware parity. Floating-point comparisons preserve signed
zero, and native bit dumps remain preferable where precise representation matters.

`tools/rmg_oracle/harness.py` keeps legacy imports working through the verified loader
and checked `call`. Other scripts that directly invoke `emu_start` have **not** all
been migrated. In particular, do not infer completion checking or read-only CLI
behavior for the entire oracle directory from the listed examples.

Run the runner's synthetic failure checks with:

```powershell
python -m unittest tools.test_native_oracle -v
```

These test runner behavior, not retail behavior. The tests cover limits, faults,
premature stops, alternate exits, missing paths, fresh state, FSTP completion,
executable identity, reference preservation, provenance mismatches, and diagnostics.

## Saved-map browser comparisons

The [browser evidence](../docs/research/skirmish-ui/2026-09-12-saved-seed-browser.md)
links three additional bounded comparisons: `tools.storage_oracle.seed_order`
(original qsort and timestamp comparator, 36 cases), `tools.storage_oracle.crt_random`
(supplied TLS seeds, 224 draws), and `tools.storage_oracle.saved_scrollbar`
(original x87 thumb arithmetic, 450 supplied geometries). Each supports `--check`
and explicit `--write` through the common runner and records provenance beside
its JSON payload. None establishes native full-window visual parity or a live
first-save RNG state.

## Upstream references

* [Official Unicorn tutorial](https://www.unicorn-engine.org/docs/tutorial.html):
  explicit memory mapping, CPU state and emulation setup.
* [Unicorn 2.1.4 public API](https://github.com/unicorn-engine/unicorn/blob/2.1.4/include/unicorn/unicorn.h):
  `uc_emu_start` budgets, `UC_QUERY_TIMEOUT`, hooks, and exits overriding `until`.
* [Unicorn 2.1.4 execution implementation](https://github.com/unicorn-engine/unicorn/blob/2.1.4/uc.c):
  the instruction-count hook stops emulation normally; success alone does not
  establish that a native return was reached.
