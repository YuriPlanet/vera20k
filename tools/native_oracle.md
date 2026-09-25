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
| `tools.spatial_oracle.track_fresh_response` | 106 Drive/Ship rows (53 each): the real Unit setter `0x741970`, then the outer Process through `Process_Movement(&out, 1, 0)` (`0x4B2630` / `0x6A1C80`) and every recursion it makes, stopping where it returns to the outer Process: the facing gate, code-0 straight/turning/extension/straight-forcing acceptance and finalize, second-candidate codes, the code-2 latch/timer/urgency ladder, the code-3 gate tail, the code-1/4/5/6/7 retries, the code-6 CloseEnough stop (with a Tunnel row) and forced scatter, the wall Override, the accepted target speed (Track/Wheel slopes, the Road row, the zero row, the clamp and ConditionYellow), core Find_Path failures, the early selector write, the ParalysisTimer gate, the Foot+68B latch and the crusher paths. Unit `Can_Enter_Cell` and `Find_Path` answers are supplied (`core_null` runs the original wrapper with only the AStar core 0x4CBBA0 answering NULL); `Scatter_Objects` and `Override_Mission` are recorded | `src/sim/world/track_fresh_response_tests.rs` (104 rows; the two far-zone rows need a zone split); production `sim/movement/track_fresh.rs`, `pathfinding/terrain_speed.rs::fresh_track_speed_fraction` |
| `tools.spatial_oracle.tube_startup_capture` | Unpatched Windows process startup FPCW/adjacent data at four hardware breakpoints; full executable section integrity | Original initializer inputs for `tools.spatial_oracle.tube_hierarchy`; startup-only evidence, not runtime immutability |
| `tools.spatial_oracle.walk_paid_step` | 40 original paid Walk steps through the coordinate branch, including facing equality, zero speed, cell boundaries and five chained Slice6 production steps; movement-speed integer supplied | `src/sim/movement/walk_step_tests.rs`; production `walk_step.rs`, Slice6 frame assertions and move-to-fire/save-restore regression |
| `tools.spatial_oracle.facing_class` | 61 original retained histories through both constructors, signed ROT, Set/Snap/Current/IsRotating, live rate changes and frame boundaries | `movement/facing_class.rs`; production Unit spawn/combat facing, rotation latch and full save/restore continuation in `world/world_spawn/tests.rs` |
| `tools.spatial_oracle.aircraft_approach_range` | 235 original Object Distance_To queries and selected strafe range branches: strict raw-lepton boundary, sqrt ties, elite fallback, cell targets and all 22 foundations with Bib both ways | `combat::object_distance_to`; `aircraft/approach_range_tests.rs` compares every distance/range predicate and production in-range dispatch/save-restore. Native execution stops before the out-of-range setter; it does not prove full approach behavior |
| `tools.spatial_oracle.aircraft_approach` | 49 full Mission_Attack calls: 8 initial transitions and 41 approach branches, actual speed versus request, strafe/Fighter precedence, NavCom thresholds, primary/elite FLH, facings, setters, ammo/timers and RNG | `world/aircraft_approach_tests.rs` compares production dispatch and restored continuation. Three FLH samples retain documented one-lepton f32 differences with identical steering histories. Supplied flat airborne fixtures exclude tilt, full flight and remaining destination lifecycle effects |
| `tools.spatial_oracle.aircraft_states` | The whole original Mission_Attack `0x00417FE0` per visit: states 4..9 over every code, class, Ammo, Target, CurleyShuffle and IsClose; the state-9 delay; state 10 over pending, Ammo, Target, `+3D4`, house control, Airstrike and edge, plus 20 seeded edge scans. 368 stored rows stand for about 9,400 executed visits. SelectWeapon, IsClose, GetFireError, FireAt, Uncloak, Assign_Destination, Queue_Mission and Enter_Idle_Mode stubbed; Scatter entry-hooked | `aircraft/attack_mission_tests.rs` replays every covered combination through the pure states and state 10 through the production edge picker; the stubbed callees keep their own evidence |
| `tools.spatial_oracle.aircraft_attack_release` | 316 original admitted state4 loops/suffixes, 72 mission and 144 AI pending-ammo prefixes, 21 initialization and 7 ammo gates; selection/FireAt supplied and reveal excluded | `combat/aircraft_release_tests.rs`, `aircraft/release_tests.rs`, constructor/fire-gate tests; bounded control/state parity, not complete projectile, damage, navigation or scheduling parity |
| `tools.spatial_oracle.techno_burst_index` | 54 original FireAt signed increment/remainder cases; GetROF callback records the incremented index and returns20 | `combat/burst.rs`; retained index used by shared emission, not complete rearm or FireAt parity |
| `tools.spatial_oracle.techno_target_burst` | 108 original Unit target/burst/passive-state cases through the setter and Event, EnterIdle and Sticky call boundaries; original vtables, no substituted calls | `combat/burst.rs` compares retained state; production command, movement arrival, mission arrival and pursuit regressions. Excludes preceding native admission, Infantry override and full setter effects |
| `tools.spatial_oracle.aircraft_fire_location` | 51 full original FindFireLocation calls: target identity, native rings/ranking, map/visibility, live reservations, Spawned/Carryall/AirportBound admission and RNG continuation; supplied runtime state, no substituted calls | `util/native_trig.rs` compares candidate geometry and ranked distances against existing deterministic math; production search, destination assignment and state1 integration remain required |
| `tools.spatial_oracle.fly_destination` | 26 original non-null Fly MoveTo calls: retained XYZ, signed-cell landing refusal, power gate, signed Ammo/Target height substitution, FlightLevel fallback, mode/readiness suffix; no substituted calls | `world/fly_height_tests.rs` compares retained destination and refusal through the air order boundary, plus production cell orders and save/restore; moving/mode suffix, null/Stop and full Aircraft/Foot navigation remain unported |
| `tools.spatial_oracle.fly_takeoff` | 80 original callbacks with live facing histories, strict height thresholds, Carryall landing base, bridge/slope, signed ROT and both flag clears; no substituted calls | `world/fly_height_tests.rs` compares production callback fields; existing FacingClass owns both turns. Excludes BeginTakeoff, landing and full flight |
| `tools.spatial_oracle.fly_takeoff_phase` | 75 full original phase-dispatch calls with actual Foot/Techno/Object Mark and Display, health/flag gates and same-layer reordering | `world/fly_height_tests.rs` compares the production pure-takeoff transaction and save/restore continuation; excludes preceding Process motion, landing and non-Landable branches |
| `tools.spatial_oracle.fly_landing_phase` | 29 full original accepted ordinary landing phases: retained Top-to-Ground resubmission, threshold/latch, air-tracker removal, neighbor-counter migration, destination clear/path timer and unchanged RNG | Evidence for the pending Rust landing callback; no substituted gameplay calls. Covers already-OnBridge state, not changed-layer phase suffixes, refusal/search/destruction, AirportBound docking, Carryall/type+C95 effects or full flight/rendered parity |
| `tools.spatial_oracle.fly_paid_step` | 199 original paid-motion ranges with real Primary.Current, Fly speed getter, type Speed conversion and trig; signed/Q16 speeds, full headings, active turns and boundaries | `world/fly_height_tests.rs` compares all numeric candidates and196 in-grid production steps, plus save/restore; map-edge correction, IsMoving admission, slowdown/navigation/landing remain required |
| `tools.spatial_oracle.fly_target_speed` | 374 original Process target-speed writes (`4CE145..4CE2E5`) with real Aircraft/Unit, IFlyControl, GetWeapon and GetHeight: the SlowdownDistance ratio and cap (zero and negative distances included), the 0.1 floor and its 85-lepton stop-and-halve, the zero-distance clamp and 0.05 creep; `4D0180` lock/landing/cruise/FlyBy/strafe/fighter/Inviso/unarmed/Ammo precedence; HunterSeeker; the health, landing, half-height takeoff and null-destination gate. Distance and type locals supplied; no substituted calls | `movement/air_movement.rs::tests::native_target_speed_rows` replays every row through the production writer (each speed within one Q16 quantum); `spawn_manager_tests::every_hornet_of_a_carrier_wing_flies_a_whole_strafe_pass` drives it through `advance_tick`. Excludes the ramp, Horizontal_Step's arrival arm and the landing trigger |
| `tools.spatial_oracle.aircraft_mission_only` | 96 original Aircraft Unlimbo flag suffix / empty-selection click-gate pairs; retained history, failure, type gates, base/elite Camera | `world/aircraft_deployment_tests.rs` and `app/input/entity_pick.rs` compare production reveal and click selection; excludes full Unlimbo, reinforcement bodies, nonempty/forced selection and flight navigation |
| `tools.spatial_oracle.fly_map_edge` | 78 original Fly candidate-admission ranges with Aircraft/Team/Script/map/RNG callees; FlyBy, raw/queued missions, cursor/waypoint height, nudge, single scatter and coordinate refusal | Native evidence for the pending production port; supplied Team states exclude creation/activation, and execution stops before SetCoords/Mark/height effects. Pins zero/one RNG draw and continuation; no Rust parity claim |
| `tools.spatial_oracle.team_creation` | 39 original TeamType creation paths, including trigger4 dispatch and complete Team/Script/Tag/Trigger constructors; owner/limit gates, initial state, waypoint, registry publication and timer RNG | Supplies allocation storage only. Native evidence for pending Team/Trigger lifecycle integration; excludes definition loading, recruitment/activation, Spring and deletion. No Rust parity claim |
| `tools.spatial_oracle.trigger_type_flags` | 64 original TriggerType flag-reader cases with ReadString512, strtok and atoi: disabled polarity, missing/empty tokens, nonzero values, overflow and prior-field retention | Rust compares all 29 applied fresh-definition cases for flags9C..9F. Reload retention and fieldA0 remain evidence only. Excludes House/link/name resolution, Events/Actions and live Tag/Trigger lifecycle |
| `tools.spatial_oracle.trigger_action_values` | 61 original TAction constructor/read/global-local dispatch cases: parameter type versus operand, token8 overrides, empty tokens, signed overflow and variable bounds | `sim/trigger_runtime_tests.rs::parsed_variable_actions_match_native_reader_dispatch_and_restore` compares parsed actions through production frames and save/restore. Excludes named sound/theme/speech lookup and live timer-reset fanout |
| `tools.spatial_oracle.trigger_event_records` | 54 original counted reader cases: variable record widths, numeric/type-name materialization, reversed event list, timer/elapsed/global/local predicate samples | `sim/trigger_runtime_tests.rs::parsed_event_records_match_native_list_and_production_predicates` compares parsed records and supported predicates through production frames. Excludes resolved Team references, nonempty TechnoType scans and live Trigger completion/timers |
| `tools.spatial_oracle.tag_lifecycle` | 49 original histories: shared/fresh Tags, reversed linked execution, repeat modes/refcounts, Object/cell attachment and detach, completion bits, timer/RNG/reset fanout, force/enable/disable, deferred destructors and Logic polling/list mutation | Evidence for the pending live-instance migration. Supplies allocation/free storage, OS pointer probing and initialized registries; no gameplay calls replaced. Excludes scenario registration classification/countdown expiry, Team/physical Object destruction and gated House-win/cursor updates; no Rust live-instance parity claim |
| `tools.spatial_oracle.techno_rearm` | 267 original GetROF numeric/RNG cases with explicit virtual query substitutions; original RTTI leaves separately establish Unit1, Aircraft2, Building6, Infantry15 | Evidence for rearm migration; Unit delay and Building one-frame shortcut identities corrected; no complete production rearm parity claim |
| `tools.spatial_oracle.building_fire_turn` | 140 original voxel-building facing retry decisions using raw Type ROT; stops before Snap/GetFireError | `combat/combat_turret_facing_tests.rs` drives the production fire receiver against every decision |
| `tools.spatial_oracle.walk_direction_table` | All 65,536 heading words: original sine/cosine indexes and table bits, plus the complete retail table bytes | `src/util/native_trig.rs::tests::every_walk_heading_uses_the_original_trig_entries`; full-heading paid Walk displacement |
| `tools.spatial_oracle.crate_speed_effect` | 23 original speed-crate recipient loops followed by live Foot speed queries; class/owner/factor gates, native 3-D distance and announcement flag | Native evidence for the open pickup/Foot speed dependency; no Rust production parity claim |
| `tools.spatial_oracle.flight_level` | 30 original type FlightLevel reads and effective-height queries, including the exact -1 fallback | `rules/object_type.rs`; Fly construction, attack recovery and paradrop carrier initialization use the resolved type value |
| `tools.spatial_oracle.fly_height` | 144 bounded original vertical steps with real GetHeight/SetHeight, Aircraft interface and type getter | `movement/fly_height.rs` compares all vectors; `world/fly_height_tests.rs` exercises healthy production height updates. Excludes complete Process admission, crash, descent drift and phase/Display transactions |
| `tools.spatial_oracle.refinery_dock` | 94 original War Miner refinery rows on real Unit/Building vtables: the Radio core HELLO/OVER_OUT and the Techno/Foot/Unit/Building receivers (18), Building DOCKING `0x0E` through NEED_TO_MOVE, MOVE_HERE, TETHER and PREPARE_TO_DOCK (19), `FootClass::Mission_Enter` (9), Mission_Harvest states 2 and 3 (9), the harvester branch of `UnitClass::Mission_Unload` through its dumps, purifier/AI-virtual/IncomeMult payouts, state 4 and contact loss (30), the PerCellProcess DOCK_NOW arm (6) and the StageClass tick (3), with every transmit reply and RNG draw. Enter_Idle_Mode, Scatter and the animation producers are recorded, not run; Find_Docking_Bay, Find_Nearby_Passable_Cell and Ready_To_Commence answers are supplied | `world/refinery_dock_oracle_tests.rs` replays every row through production `radio::receive`, `miner::refinery_dock` and Mission_Harvest states 2/3 (IncomeMult 0.9 pays 899 natively and 900 in VERA's integer economy, the documented residual); `world/refinery_dock_cycle_tests.rs` runs a whole visit through `advance_tick` |

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
