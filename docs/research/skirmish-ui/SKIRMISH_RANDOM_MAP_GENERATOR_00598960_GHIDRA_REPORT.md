# Skirmish Random Map Generator 0x00598960 - Ghidra Research Report

**Address(es):** `0x00598960`, seed-loader vtable slot `0x00597A30`, init helper `0x00599650`  
**Investigation Mode:** coverage-map  
**Claimed Scope:** Rust-facing launch contract for `FUN_00598960` after the verified `.SED` branch: seed/options fields, first-pass generation stages, scenario/map outputs needed before first playable frame, and launch-vs-preview separation.  
**Non-Scope:** full terrain/noise formulas, exact water/region/tiberium/hill/LAT tile placement algorithms, random-map dialog layout, online-only map-generation exchange, and exact runtime error UX for malformed custom `.SED` files.  
**Confidence:** High for entry/field/stage/output contract; Medium for formula branch details intentionally deferred.  
**Active in YR:** Conditional. Active in standard YR when Skirmish launch selects a `.SED` random-map seed such as `RandMap.Sed`; inactive for ordinary map filenames.

## Working Notes

- Target question: what does `FUN_00598960` consume and produce after `ScenarioClass__Read_Scenario` calls it for `RandMap.Sed`?
- Non-goals: do not reopen Choose Map preview/listbox/combo/trackbar/start-marker findings; do not fully reimplement every random terrain formula in this slot.
- Evidence needed to mark COMPLETE: seed-loader field map, generator stage order, launch/full-init branch, output scenario/map state before first playable frame, Rust handoff.
- Stop conditions: stop once Rust can distinguish `.SED` load from normal map load and can see the minimum generated-map state required; defer formula internals with exact follow-up targets.

## 1. Overview

`FUN_00598960` is not a lightweight preview helper. On the Skirmish launch path it receives the global `MapSeedClass` object at `0x00ABDFD8`, uses `MapSeed+0x74` to initialize the random stream, constructs a generated scenario/map in memory, creates start waypoints, recalculates cell attributes multiple times, initializes tiberium queues, computes radar bounds/surfaces, and leaves the session filename as the original `.SED` seed filename through the caller.

The first stack argument controls preview repaint, not whether the map is generated. Launch calls `0x00598960(0,0)`, so every guarded `GenerateTerrainPreview(); SendMessage(hwnd, WM_PAINT)` block is skipped, while the full scenario/map initialization branch in `0x00599650` runs because the HWND argument passed there is zero.

## 2. Key Offsets / Inputs

| Field / global | Meaning in this slice | Evidence | Active in YR |
|---|---|---|---|
| `MapSeed+0x38` | theater index `0..4`, mapped to `TEMPERATE`, `SNOW`, `URBAN`, `DESERT`, `NEWURBAN`; NOT clamped by the normalizer `0x005975E0` (verified via decompile_function 0x005975E0, 2026-07-20) | `0x00597B90..0x00597BAA`; `0x005997AB..0x005997D5`; PE read at `0x007E1B78` | Conditional |
| `MapSeed+0x3C` | map type bucket; values `3`/`4` select water/island-style branches | `0x00598AED..0x00598B14`; `0x00598D55..0x00598D82` | Conditional |
| `MapSeed+0x40` | `Resources` option, clamped `0..3` by `0x005975E0` (verified via decompile_function 0x005975E0, 2026-07-20) | seed-loader assembly `0x00597C97..0x00597CB2`; key string `0x0082BB5C` | Conditional |
| `MapSeed+0x44` | `Ruggedness` percent `0..100` | `0x00597BDE..0x00597BF8`; clamp `0x005975E0` | Conditional |
| `MapSeed+0x48` | `Time`/lighting bucket `0..3`; indexes ambient/tint tables | `0x00597BAD..0x00597BC7`; `0x00599863..0x005998A8`; `0x00599650` tail writes scenario lighting | Conditional |
| `MapSeed+0x4C` | `WaterAmount` percent; for map types `3/4`, zero skips `0x0059C580` | `0x00598AF3..0x00598B0D` | Conditional |
| `MapSeed+0x50` | `NumPlayers`, clamped to `2..8`; drives start creation loops | key load `0x00597B42..0x00597B5C`; clamp `0x005975E0`; start loop `0x005A1FB0` | Conditional |
| `MapSeed+0x54` | `Tiberium` percent, clamped to `1..100` | key load `0x00597C2C..0x00597C46`; clamp `0x005975E0` | Conditional |
| `MapSeed+0x58` | `TiberiumLayout` percent/bucket | key load `0x00597C49..0x00597C63` | Conditional |
| `MapSeed+0x5C` | `Vegetation` percent | key load `0x00597C60..0x00597C8C` | Conditional |
| `MapSeed+0x60` | `UrbanPresence` percent | key load `0x00597C7A..0x00597C94` | Conditional |
| `MapSeed+0x64` | `Width` option, clamped `0..3` by `0x005975E0` (verified via decompile_function 0x005975E0, 2026-07-20); the `1/3` scaling and non-`3/4` map-type `1.2` cap happen later in the map-dimension computation inside `0x00599650`, not in the normalizer | key load `0x00597B11..0x00597B2B`; map init `0x00599665..0x005996D7` | Conditional |
| `MapSeed+0x68` | `Height` option, clamped `0..3` by `0x005975E0` (verified via decompile_function 0x005975E0, 2026-07-20); same map-init scaling/cap path as width | key load `0x00597B28..0x00597B54`; map init `0x00599678..0x005996D7` | Conditional |
| `MapSeed+0x6C` | `Accessibility` percent | key load `0x00597BFB..0x00597C15` | Conditional |
| `MapSeed+0x70` | `RegionSize` percent | key load `0x00597BC4..0x00597BF0` | Conditional |
| `MapSeed+0x74` | seed, clamped to `0..0xFFFF`; copied into RNG init before generation | `0x0059897B..0x0059899B`; `0x0065C6D0`; clamp `0x005975E0` | Conditional |
| `MapSeed+0x178` | cached previous generated seed/object state used by preview/full reinit decision | `0x00599650` preview branch compares `+0x38/+0x50/+0x64/+0x68` | Conditional |
| `MapSeed+0x180/+0x184` | generated map width/height in internal scenario dimensions | writes `0x00599700`, `0x00599748`; computed from player count and scaled width/height | Conditional |
| `MapSeed+0x304/+0x308` | scratch pointers/state cleared during cleanup | `0x005993AF..0x005993B5`; `0x00598960` cleanup | Conditional |

## 3. Seed / Options Load

The previously unresolved vtable call behind `0x00597A10` is concrete enough for this slice. `MapSeedClass__Constructor @ 0x00595680` writes vtable pointer `0x007ED8E4`; PE bytes at `0x007ED8E8` contain `0x00597A30`, so wrapper `0x00597A10` dispatches vtable slot `+0x4` to `0x00597A30` for the non-null filename passed by the `.SED` launch branch. Active in YR: Conditional. Evidence: constructor assembly `0x005956CB..0x005956D1`; wrapper assembly `0x00597A10..0x00597A1E`; PE read of vtable bytes.

`0x00597A30` opens/loads the seed file and reads `[RandomMap]` keys, keeping the existing field value as default for every integer read. It reads `Description`, `Width`, `Height`, `NumPlayers`, `Seed`, `MapType`, `Theater`, `Time`, `RegionSize`, `Ruggedness`, `Accessibility`, `WaterAmount`, `Tiberium`, `TiberiumLayout`, `Vegetation`, `UrbanPresence`, and `Resources`. Active in YR: Conditional on seed file load success. Evidence: assembly `0x00597ADE..0x00597CB2`; key strings at `0x0082BB24`, `0x0081B1A4`, `0x0082BBE4`, `0x0081A7A8`, `0x0082BBD8`, `0x0082BBD0`, `0x0082BBC8`, `0x00818658`, `0x0081F11C`, `0x0082BBBC`, `0x0082BBB0`, `0x0082BBA0`, `0x0082BB94`, `0x00817278`, `0x0082BB84`, `0x0082BB78`, `0x0082BB68`, `0x0082BB5C`.

The normalizer `0x005975E0` clamps option fields before generation: Resources (`+0x40`) `0..3`, MapType (`+0x3C`) `0..4`, Time (`+0x48`) `0..3`, Ruggedness (`+0x44`) / WaterAmount (`+0x4C`) / TiberiumLayout (`+0x58`) / Vegetation (`+0x5C`) / UrbanPresence (`+0x60`) / Accessibility (`+0x6C`) / RegionSize (`+0x70`) percentages `0..100`, NumPlayers (`+0x50`) `2..8`, Tiberium (`+0x54`) `1..100`, Width (`+0x64`) `0..3`, Height (`+0x68`) `0..3`, Seed (`+0x74`) `0..0xFFFF`. **Theater (`+0x38`) is NOT clamped — the normalizer never touches it.** (Re-verified via decompile_function 0x005975E0, 2026-07-20; the earlier "theater 0..4" clamp claim was wrong, and Resources is a `0..3` bucket, not a percent.) Active in YR: Conditional whenever a seed object is randomized/loaded before generation. Evidence: decompile `0x005975E0`; call sites after randomization at `0x00597380` and dialog paths.

The generator does not rely on the process global RNG for the generated terrain stream after the seed is known. At function entry it calls `FUN_0065C6D0(seed)` and copies `0xFD` dwords into global `DAT_00ABE890`. Active in YR: Conditional. Evidence: `0x0059897B..0x0059899B`; RNG helper `0x0065C6D0` writes table state from the 16-bit seed.

## 4. Core Generation Order

Launch path order inside `0x00598960`:

1. Initialize seeded RNG from `MapSeed+0x74`, install short global callback/scratch block at `DAT_00ABDFB8`, and optionally show shell progress only when `ScenarioClass+0x3598 == 0`. Active in YR: Conditional. Evidence: `0x0059897B..0x00598A52`.
2. Call `0x00599650` before terrain stages. On launch, the HWND argument is zero, so it runs the full scenario init branch: frees tiberium queues, calls scenario full init, initializes map/tactical/theater structures, computes internal map dimensions, writes `[Basic]`, `[Map]`/`LocalSize`, `[Lighting]`, and allocates the RMG cell scratch array `DAT_00ABED10`. Active in YR: Conditional. Evidence: call at `0x00598A67..0x00598A74`; full-init branch in `0x00599650`; stack arg zero from caller `0x00684980..0x00684989`.
3. Read generator rules/defaults via `0x005981F0`, including `[General]` RMG and lighting vectors. Local repo `rules.ini/rulesmd.ini` has no matching `RMG*` keys, so standard local defaults come from binary/default vectors unless MIX-provided INI data adds them. Active in YR: Yes/Conditional. Evidence: call `0x00598ADC..0x00598AE3`; decompile `0x005981F0`; local `rg` found no `RMG*` keys.
4. Seed water/terrain base: map types `3/4` call `0x0059C580` only when `WaterAmount != 0`; all other map types call `0x0059A6C0`; all paths then call `0x0059C630`. Active in YR: Conditional by `MapType`/`WaterAmount`. Evidence: `0x00598AED..0x00598B14`; decompile `0x0059C630`.
5. Initialize regions: clear RMG cell region fields `+0x38/+0x3C` to `-1`, process region objects, and run `0x0058CF90`, `0x0058E740`, `0x0058E9B0`, `0x0058D010`. Active in YR: Conditional. Evidence: decompile `0x00598960`; assembly `0x00598B12..0x00598D01`.
6. For map types `3/4`, run extra region/bridge passes `0x0058EBC0`, `0x0058EF10`, `0x005A19E0`, `MapClass__MarkBridgesForRepair_Low(0,-1)`, and `0x005A17F0`. Active in YR: Conditional. Evidence: `0x00598D55..0x00598D82`.
7. Run `FUN_0059B740` (green-tile spread): collects clear cells cardinal-adjacent to existing green tiles, then writes `g_GreenTile` into up to `min(candidate_count/3, 1000)` randomly drawn candidates (draws from `g_MapGenRng`), re-adding new cardinal neighbors as it goes. Runs after the region passes and the map-type-`3/4` block, BEFORE the first full cell-attribute recalc. Active in YR: Conditional. Evidence: decompile_function 0x00598960 + decompile_function 0x0059B740, 2026-07-20 (call sits immediately before the first `RMG: Recalculating cell attributes` block).
8. Recalculate cell attributes, create starting points, add tech buildings when `MapType != 0`, create tiberium, free scratch region objects, recalc cells again, create hills, then create LATs/rocks: when Theater (`+0x38`) `== 0` run `FUN_005A38C0` + `FUN_005A3AE0`; for any other theater, fill the scratch cell array's per-cell probability fields with `0.005` (`0x3F747AE147AE147B` into `+0x28`) and `0.001` (`0x3F50624DD2F1A9FC` into `+0x20`) for every cell, then run `FUN_005A4280` (verified via decompile_function 0x00598960, 2026-07-20). Then decrement `g_MapEditorMode`, final cell recalc, initialize all tiberium growth/spread queues, free scratch `DAT_00ABED10`, initialize cell attributes, compute radar bounds, and rebuild radar surfaces. Active in YR: Conditional. Evidence: `0x00598E18..0x0059951F`.

Detailed per-stage water/terrain/region formulas now live in `RMG_TERRAIN_SHAPING_CORE_GHIDRA_REPORT.md` (2026-07-19).

## 5. Outputs Required Before First Playable Frame

The minimum generated-map state is not a filename; it is in-memory scenario/map state. Before the caller proceeds to `ScenarioClass__Post_Map_Init(1)`, native has:

- Scenario/full map structures initialized, including generated theater and internal map dimensions. Active in YR: Conditional. Evidence: `0x00599650`; caller `0x0068498E..0x00684990`.
- Generated terrain/cell data and recalculated `CellClass` attributes. Active in YR: Conditional. Evidence: repeated `MapClass__CellIterator_Init/Next -> CellClass__RecalcAttributes` blocks in `0x00598960`.
- Generated start positions for `NumPlayers`, with `0x00598960` looping until `0x00594B50()` and `0x005A1FB0()` both return nonzero. `0x005A1FB0` loops `i < MapSeed+0x50`, reads/writes scenario waypoint slots through `FUN_0068BCC0/FUN_0068BF50`, and flood-fills clear tiles around each start. Active in YR: Conditional. Evidence: `0x00598E9E..0x00598EB6`; decompile `0x005A1FB0`.
- Optional generated tech buildings, controlled by `MapType != 0`. Active in YR: Conditional. Evidence: `0x00598EB8..0x00598ED?` in decompile, `FUN_005A95B0` guarded by `MapSeed+0x3C != 0`.
- Generated tiberium plus initialized growth/spread queues. Active in YR: Conditional. Evidence: `FUN_005A23A0` call in `0x00598EF2..0x00598EF4`; `TiberiumClass__InitGrowthQueues_All` and `TiberiumClass__InitSpreadQueues_All` near function tail.
- Radar map bounds and surfaces rebuilt. Active in YR: Conditional. Evidence: tail calls `RadarClass__ComputeRadarMapBounds(&DAT_0087F8E4)` and `RadarClass__RebuildRadarSurfaces` at `0x0059945E..0x00599472`.
- Scenario lighting fields updated from generated theater/time tables: `ScenarioClass+0x3528`, `+0x3534`, `+0x3538`, `+0x353C`, `+0x3544`. Active in YR: Conditional. Evidence: tail of `0x00599650`.

## 6. Current Rust Implementation Status

Rust currently has a shell-level sentinel but no generated-map loader. `SkirmishScenarioRecord::random_map_sentinel` stores `file_name = "RandMap.Sed"` but leaves min/max players unset. `SkirmishLaunchSession.selected_map_file` is a plain string, and `load_map` routes requested names through `load_map_by_name_or_path`, which would treat `RandMap.Sed` as a concrete map file. No Rust surface models `[RandomMap]` seed/options, generated terrain output, generated start waypoints, or the launch-only `0x00598960(0,0)` branch.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `0x00598960` entry and launch/preview split | verified | decompile; assembly `0x0059897B..0x00599527`; caller `0x00684980..0x00684990` | none for handoff |
| vtable seed loader `0x00597A30` | verified | constructor `0x005956CB..0x005956D1`; wrapper `0x00597A10..0x00597A1E`; vtable bytes; assembly `0x00597A30..0x00597CB2` | no Ghidra function boundary; formulas not affected |
| option field mapping/clamps | verified | `0x00597A30` assembly; `0x005975E0` decompile | none for handoff |
| map init `0x00599650` | verified | decompile and assembly `0x00599650..0x005998A8`; tail writes scenario fields | exact dimension interpolation tables can be formula-follow-up |
| RMG settings reader `0x005981F0` | verified | decompile; local INI scan | exact binary default vectors not exhausted |
| water/terrain/region formulas | touched-not-exhausted | `0x00598AED..0x00598D82`; decompiled `0x0059C630` and start helpers | dedicated formula report |
| start creation minimum contract | verified for handoff | loop `0x00598E9E..0x00598EB6`; decompile `0x005A1FB0` | exact placement scoring in `0x00594B50` deferred |
| tiberium/hills/LAT formulas | touched-not-exhausted | stage calls in `0x00598960` | dedicated formula reports if implementing full RMG parity |
| current Rust delta | verified | `src/skirmish_scenarios.rs`, `src/skirmish_launch.rs`, `src/app_init.rs`, `src/app_list_maps.rs` | implementation not performed |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-RMG-01 - Is `0x00598960` active in standard YR Skirmish? -> Yes, conditionally after `.SED` seed load succeeds.` (evidence: prior branch report; caller `0x00684980..0x00684990`)
- `[RESOLVED] OQ-RMG-02 - Which function loads the `.SED` fields? -> `0x00597A10` dispatches vtable `+0x4` to `0x00597A30`.` (evidence: `0x005956CB..0x005956D1`; vtable PE bytes; `0x00597A10..0x00597A1E`)
- `[RESOLVED] OQ-RMG-03 - What section/keys are consumed? -> `[RandomMap]` keys listed in section 3, including dimensions, players, seed, map type, theater, and terrain density fields.` (evidence: `0x00597A30..0x00597CB2`; key strings)
- `[RESOLVED] OQ-RMG-04 - What field seeds the terrain RNG? -> `MapSeed+0x74`, clamped to `0..0xFFFF`, feeds `0x0065C6D0` before generation.` (evidence: `0x0059897B..0x0059899B`; `0x0065C6D0`; `0x005975E0`)
- `[RESOLVED] OQ-RMG-05 - Does launch use preview repaint? -> No; launch passes first arg `0`, so preview blocks are skipped.` (evidence: `0x00684980..0x00684989`; `0x00598A8A..0x00598AD1` pattern repeated)
- `[RESOLVED] OQ-RMG-06 - Does launch run full scenario/map init? -> Yes; `0x00598960` passes the zero HWND arg into `0x00599650`, which takes its full-init branch.` (evidence: `0x00598A67..0x00598A74`; `0x00599650`)
- `[RESOLVED] OQ-RMG-07 - Does the generator produce start positions before post-init? -> Yes; it loops until `0x00594B50` and `0x005A1FB0` both succeed, and `0x005A1FB0` loops over `NumPlayers`.` (evidence: `0x00598E9E..0x00598EB6`; `0x005A1FB0`)
- `[RESOLVED] OQ-RMG-08 - Are tiberium queues ready before return? -> Yes; growth and spread queues are initialized near the tail.` (evidence: decompile `0x00598960`)
- `[RESOLVED] OQ-RMG-09 - Are radar bounds/surfaces ready before return? -> Yes; compute/rebuild calls occur after final cell attribute init.` (evidence: `0x0059945E..0x00599472`)
- `[RESOLVED] OQ-RMG-10 - Does Rust already have the load path? -> No; it has the sentinel record but still normal-loads requested map strings.` (evidence: Rust scan)
- `[DEFERRED] OQ-RMG-11 - Exact water/terrain/region/tiberium/hill/LAT formulas.` (category: bounded-cost-too-high; reason: immediate callees are large enough for separate slots; next-step-if-pursued: split by `0x0059A6C0/0x0059C580`, `0x0058*` regions, `0x005A23A0`, `0x005A35F0`, `0x005A38C0/0x005A3AE0/0x005A4280`)
- `[DEFERRED] OQ-RMG-12 - Exact start-placement scoring/fallback inside `0x00594B50`.` (category: bounded-cost-too-high; reason: handoff only needs the generated-waypoint contract; next-step-if-pursued: dedicated start-point generator formula report)
- `[DEFERRED] OQ-RMG-13 - Exact binary default RMG vector values when INI keys are absent.` (category: bounded-cost-too-high; reason: local INI absence and reader addresses are verified; next-step-if-pursued: dump constructor/default vectors for `MapSeedClass`)
- `[DEFERRED] OQ-RMG-14 - Malformed `.SED` user-facing error UX.` (category: needs-runtime-debugger; reason: static load-failure gate is known, exact modal/log behavior needs runtime experiment)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `.SED` launch must load `[RandomMap]` seed/options, not normal map INI; fields include theater, map type, resources, dimensions, players, seed, and terrain density controls. | `0x00597A10`; vtable `0x007ED8E4+4 -> 0x00597A30`; key reads `0x00597ADE..0x00597CB2` | missing | `src/app_init.rs`, `src/app_list_maps.rs`, new app/map-layer random-map seed model | Add a `.sed` random-map load branch that parses `[RandomMap]` and validates/clamps native option ranges before generation. | Starting `RandMap.Sed` reaches a random-map path and reports parsed seed/options instead of "map not found". Proposed test: `skirmish_randmap_sed_parses_randommap_seed_options_before_generation` | Do not feed `RandMap.Sed` to the normal concrete-map loader or treat missing seed data as an ordinary map. |
| `MapSeed+0x74` is the deterministic terrain seed; `0x00598960` initializes its own generated-map random table before all terrain stages. | `0x0059897B..0x0059899B`; `0x0065C6D0`; clamp `0x005975E0` | missing | future random map generator module under map/app layer, not `sim/` | Seeded generation must be deterministic for the same `.SED` options and independent of UI preview repaint state. | Same `.SED` seed/options generate identical terrain/start metadata across two launches. Proposed test: `skirmish_randmap_generation_is_deterministic_from_seed_and_options` | Do not use wall-clock/process RNG for launch terrain once a seed is present. |
| Launch generation is full map construction with no Choose Map preview repaint; full init, cell attributes, starts, tiberium queues, radar bounds, and scenario lighting are ready before post-init. | caller `0x00684980..0x00684990`; `0x00598A67..0x0059951F`; `0x00599650` | missing | `src/app_init.rs`, `src/map/*`, `src/app_skirmish.rs`, render atlas build pipeline | Random-map launch should return a real in-memory `MapFile`-equivalent with terrain, waypoints/start positions, lighting/theater, overlays/tiberium, and radar/render data ready for normal skirmish spawn setup. | Launching random map with local+AI players creates a playable generated map with at least requested start waypoints before MCV seeding. Proposed test: `skirmish_randmap_launch_generates_map_state_before_spawn_setup` | Do not reuse the modal preview texture or `RandMap.img` as game terrain; preview is only UI. |
| Generated start positions are required for `NumPlayers`; native loops until start generation succeeds and then `ScenarioClass__Post_Map_Init(1)` runs in the caller. | `0x00598E9E..0x00598EB6`; `0x005A1FB0`; caller `0x0068498E..0x00684990` | missing | random map generation plus `apply_skirmish_launch_session` start selection | Random maps must provide multiplayer start waypoints/slots matching active launch participants before applying Skirmish house/spawn setup. | `NumPlayers=4` seed yields at least four usable generated starts and Start with local+3 AIs does not hit deficient-start fallback. Proposed test: `skirmish_randmap_numplayers_controls_generated_start_count` | Do not leave random maps with empty `multiplayer_start_waypoints` like the current sentinel record. |

### Negative Facts / Do Not Do

- Do not parse `RandMap.Sed` as a concrete map file. Native loads `[RandomMap]` seed/options through `0x00597A30` and calls generator stages instead of `ScenarioClass__Read_Scenario_INI`. Evidence: `0x00597A10`, `0x00597A30`, prior branch `0x00684961..0x006849C9`.
- Do not use `RandMap.img` or Choose Map preview output as game terrain. Launch passes preview flag `0`, while preview repaint blocks require nonzero first argument. Evidence: `0x00684980..0x00684989`; repeated `CMP byte ptr [ESP+0x42C], 0` guards in `0x00598960`.
- Do not leave the random sentinel with no player capacity/start model. Native seed/options include `NumPlayers` clamped `2..8`, and generation creates start positions before return. Evidence: `0x005975E0`; `0x005A1FB0`.
- Do not make random generation part of deterministic `sim/` state. Native generation is pre-play map/scenario construction and shell/app loading work; the playable sim consumes the generated map after post-init. Evidence: `0x00598960` runs during scenario load before `ScenarioClass__Post_Map_Init(1)`.
- Do not implement only terrain tiles and skip cell attributes/tiberium queues/radar/lighting. Native recalculates cells repeatedly, initializes tiberium queues, computes radar bounds/surfaces, and writes scenario lighting before return. Evidence: `0x00598E18..0x0059951F`; `0x00599650` tail.

### Remaining Uncertainty

- Exact water/terrain/region/tiberium/hill/LAT formulas are deferred into narrower formula slots.
- Exact start-placement scoring/fallback inside `0x00594B50` is deferred; this report verifies that generated starts are mandatory and `NumPlayers`-driven.
- Exact binary default values for RMG vector settings when absent from local `rulesmd.ini` are not fully dumped.
- Malformed/custom `.SED` runtime error UX needs a native runtime experiment.

### Stale Docs / Follow-up Docs

Path: [docs/research/skirmish-ui/SKIRMISH_RANDOM_MAP_BRANCH_AFTER_SELECTED_MAP_LOAD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDOM_MAP_BRANCH_AFTER_SELECTED_MAP_LOAD_GHIDRA_REPORT.md)

Add after the prior OQ-9 replacement:

> `0x00598960(0,0)` is full launch-time generated-map construction, not preview reuse. The `.SED` seed loader vtable slot resolves to `0x00597A30`, which reads `[RandomMap]` keys including `Width`, `Height`, `NumPlayers`, `Seed`, `MapType`, `Theater`, `Time`, `RegionSize`, `Ruggedness`, `Accessibility`, `WaterAmount`, `Tiberium`, `TiberiumLayout`, `Vegetation`, `UrbanPresence`, and `Resources`. Launch initializes the seeded RMG random table from `Seed`, runs full scenario/map initialization, generates terrain/regions/starts/tiberium/hills/LATs, recalculates cell attributes, initializes tiberium queues, computes radar bounds/surfaces, and returns with generated map state in memory while the scenario filename remains `RandMap.Sed`.

## Sources

- Ghidra read-only decompile / assembly: `0x00598960`, `0x00595680`, `0x00597A10`, `0x00597A30` assembly context, `0x005975E0`, `0x005981F0`, `0x00599650`, `0x0065C6D0`, `0x00594B50`, `0x005A1FB0`, `0x0059C630`.
- PE byte reads from local `gamemd.exe`: vtable `0x007ED8E4`, string/key table addresses, theater string table `0x007E1B78`.
- Local INI scan: `ini/rules.ini`, `ini/rulesmd.ini`.
- Rust scan: `src/skirmish_scenarios.rs`, `src/skirmish_launch.rs`, `src/app_init.rs`, `src/app_list_maps.rs`.

## 10. Map-Prep 0x00599650 Internals (decoded 2026-07-20)

Scope: the RMG map-prep function `0x00599650` (thiscall, `this` = MapSeed object at `0x00ABDFD8`, `param_2` = preview flag). Ghidra's label for it ("CCINIClass__Constructor") is pollution — the body writes MapSeed geometry, rebuilds the map, and sets scenario lighting (verified via decompile_function 0x00599650, 2026-07-20). All FP below runs under control word `0x0E7F` = 53-bit precision, round-toward-zero (read_memory 0x00822D80 = `7f 0e 00 00`, 2026-07-20).

### 10.1 genW/genH formula (VERIFIED, disassembly-level)

From `disassemble_bytes 0x00599650..0x00599780` (2026-07-20):

```
n  = NumPlayers(MapSeed+0x50) - 2                       ; 0..6 for 2..8 players
sW = (float32)( WidthOption(MapSeed+0x64)  * 0.33333334f )   ; FILD; FMUL dword[0x007ED968]; FSTP dword
sH = (float32)( HeightOption(MapSeed+0x68) * 0.33333334f )
if MapType(MapSeed+0x3C) != 3 and != 4:
    sW = (sW < 1.2) ? sW : 1.2       ; FCOMP qword[0x007E5190]; C0 test; capped value re-stored to memory as float32
    sH = (sH < 1.2) ? sH : 1.2       ; capped sH stays on the FPU stack (never re-stored)
genW = ftol( MinW[n]*(1.0f - sW) + MaxW[n]*sW )   -> MapSeed+0x180 (= global 0x00ABE158)
genH = ftol( MinH[n]*(1.0f - sH) + MaxH[n]*sH )   -> MapSeed+0x184 (= global 0x00ABE15C)
```

Exact FPU op order (width): `FILD MinW[n]` ; `FLD 1.0f` ; `FSUB sW(mem)` ; `FMULP` ; `FILD MaxW[n]` ; `FMUL sW(mem)` ; `FADDP` ; `CALL ftol`. Height is identical except `1.0f - sH` uses `FSUB ST0,ST2` and `MaxH[n]*sH` uses `FMUL ST0,ST2` (sH taken from the FPU stack, so for MapType 3/4 the *uncapped* sH is used, and the uncapped sW is re-read from memory).

Constants (read_memory, 2026-07-20):
- `0x007ED968` = `AB AA AA 3E` = float32 `0.33333334`
- `0x007E5190` = `33 33 33 33 33 33 F3 3F` = double `1.2`
- `0x007E2AC8` = `00 00 80 3F` = float32 `1.0` (so the blend is a plain lerp `Min + (Max-Min)*s`, with s up to 1.2 = extrapolation)

Tables (read_memory 0x007ED634 x112 bytes, 2026-07-20), indexed by `n = NumPlayers-2`:

| n (players) | 0 (2) | 1 (3) | 2 (4) | 3 (5) | 4 (6) | 5 (7) | 6 (8) |
|---|---|---|---|---|---|---|---|
| MinW `0x007ED634` | 70 | 70 | 70 | 80 | 90 | 100 | 100 |
| MaxW `0x007ED650` | 80 | 80 | 80 | 90 | 100 | 110 | 120 |
| MinH `0x007ED66C` | 70 | 70 | 70 | 80 | 90 | 100 | 100 |
| MaxH `0x007ED688` | 80 | 80 | 80 | 90 | 100 | 110 | 120 |

(MinH/MaxH are separate tables that happen to contain the same values as MinW/MaxW.)

`ftol` = `0x007C5F00`: compares the live CW against `dword[0x00822D80]` (= `0x0E7F`, RC=11 truncate) and FISTPs under it — result is truncation toward zero (disassemble_bytes 0x007C5F00, read_memory 0x00822D80, 2026-07-20). Between computing sW/sH and using them, map-prep calls `0x005981F0` — the RMGMD.INI options loader (see 10.4); it does not touch sW/sH.

### 10.2 Derived geometry (all formulas in terms of mapW = genW+4, mapH = genH+12)

Map-prep writes `Size=(0,0,genW+4,genH+12)` and `LocalSize=(2,5,genW,genH)` into the global map INI object at `0x0081FFF0` (disassemble_bytes 0x00599780..0x005997F8: "Size" str 0x00820178, "LocalSize" str 0x00820164, 2026-07-20). Geometry lands in the g_Map object at `0x0087F7E8` via `MapClass__Resize 0x00565C10` (map-prep calls the thunk `0x00653F50(&Size,1,0,1)`; `ScenarioClass__Full_Init` re-reads Size/LocalSize from the INI at `0x0068746F`/`0x00687502` and calls the same thunk / `RadarClass__ComputeRadarMapBounds 0x00654490`). All verified via decompile_function 0x00565C10 / 0x00654490 / 0x00567230, disassemble_bytes 0x00687440..0x0068755E, 2026-07-20.

| Global | Identity | Formula | Writer |
|---|---|---|---|
| `0x0087F8DC` | g_Map+0xF4 (map width)  | `mapW = genW+4` | `MapClass__Resize` (`this+0xF4 = rect.w`; `+0xEC/+0xF0` forced 0) |
| `0x0087F8E0` | g_Map+0xF8 (map height) | `mapH = genH+12` | same |
| `0x00ABED04` | diamond bound min | `mapW` | `MapClass__Resize` @0x00566306 |
| `0x00ABED08` | diamond bound max | `mapW + 2*mapH` | `MapClass__Resize` @0x00566317 |
| `0x0087F90C/0x0087F910` | g_Map+0x124/+0x128 | `1, 1` | `MapClass__Resize` |
| `0x0087F914/0x0087F918` | g_Map+0x12C/+0x130 | `mapW + mapH - 1` (both) | `MapClass__Resize` (`rect.w + rect.h - 1`) |
| `0x0087F8E4..F0` | g_Map+0xFC/+0x100/+0x104/+0x108 (local view rect x,y,w,h) | LocalSize clipped: `x<3 -> 2`, `y<3 -> 2`, `w = min(w, mapW-x-2)`, `h = min(h, mapH-y-6)`; with LocalSize=(2,5,genW,genH) this is exactly `(2,5,genW,genH)` | `0x00567230` (called by `RadarClass__ComputeRadarMapBounds 0x00654490`); a second writer `0x006E21E0` copies from another object's +0x34..+0x40 (map-editor path) |
| `0x0089C2DC` g_PathfinderLinearMapWidth | — | `mapW + mapH + 1 = genW + genH + 17` | `PathfinderClass__ResizeMapArrays 0x0042AC00` @0x0042AC60, arg = `&(g_Map+0xEC)`, reads `+0xF4/+0xF8`; called from `MapClass__InitZoneMap 0x00567110` |

Cell-iterator start state (drives iteration and the "start row"): `this+0x10C=1`, `+0x110=mapW`, `+0x114=mapW-1`, `+0x118 = cellPtrArray(+0x13C) + mapW*0x800 + 4` (decompile_function 0x00565C10; the same 4-store sequence is inlined in map-prep itself). Cell validity predicate in Resize: cell (x,y) exists iff `x+y > mapW && x-y < mapW && y-x < mapW && x+y <= mapW + 2*mapH`. Map-prep also reallocates the pathfinding scratch `DAT_00ABED10 = new[(g_PathfinderLinearMapWidth^2) * 0x50]` and sets each cell's level byte (`cell+0x11B`) from `MapSeed+0x30C` (decompile_function 0x00599650).

### 10.3 Lighting tail (VERIFIED, loader-confirmed identities)

Vector layout on MapSeed: DynamicVector base at X, data pointer at X+4. The options loader `0x005981F0` binds INI keys to bases: RMGLevelLightSettings +0x188, TemperateAmbientLight +0x1A4, SnowAmbientLight +0x1C0, TemperateAmbientRed +0x1DC, TemperateAmbientGreen +0x1F8, TemperateAmbientBlue +0x214, SnowAmbientRed +0x230, SnowAmbientGreen +0x24C, SnowAmbientBlue +0x268 (decompile_function 0x005981F0, key strings 0x0082BCB8..0x0082BD8C, 2026-07-20). Note: the earlier handoff offsets (+0x198/+0x1B4/...) are these bases +0x10, not the bases.

Tail of `0x00599650`, with `t = Time(MapSeed+0x48)` and `theater = MapSeed+0x38` (`0` = TEMPERATE; any nonzero theater takes the Snow vectors):

| ScenarioClass field | Source (theater==0 / else) | Map-INI meaning |
|---|---|---|
| `+0x3528` | TemperateAmbientLight[t] / SnowAmbientLight[t] | `[Lighting] Ambient` x100 (e.g. 75 -> 0.75) |
| `+0x3534` | TemperateAmbientRed[t] / SnowAmbientRed[t] | `[Lighting] Red` x100 |
| `+0x3538` | TemperateAmbientGreen[t] / SnowAmbientGreen[t] | `[Lighting] Green` x100 |
| `+0x353C` | TemperateAmbientBlue[t] / SnowAmbientBlue[t] | `[Lighting] Blue` x100 |
| `+0x3544` | RMGLevelLightSettings[t] (no theater branch) | Level field, raw element (rmgmd.ini value 3) |

No scaling in the tail — raw ints are stored (decompile_function 0x00599650). The x100 semantics is proven in-binary: the same function's INI-write block emits `[Lighting] Ambient = AmbientLight[t] * 0.01` (double) and `Level = RMGLevelLightSettings[t] / 100` (C integer division — rmgmd value 3 emits `Level=0`), plus `RedTint/GreenTint/BlueTint = 1.0`, `Ground = 0.0`, `IonAmbient = Ambient`, fixed Ion RGB `(0.3, 0.4, 0.75)`, `IonGround/IonLevel = 0`. rmgmd.ini stock vectors (4 elements, indexed by Time 0..3): TemperateAmbientLight=75,100,75,35; SnowAmbientLight=75,100,75,55; RMGLevelLightSettings=3,3,3,3; RGB rows per theater as in `ini/rmgmd.ini` lines 24-33.

### 10.4 OrePatchLamps (parsed but no consumer found)

`TemperateOrePatchLamps` / `SnowOrePatchLamps` (rmgmd.ini: TEMMORLAMP,TEMDAYLAMP,TEMDUSLAMP,TEMNITLAMP / SNO*) are parsed by the options loader `0x005981F0` into BuildingTypeClass* vectors at MapSeed+0x2C4 (Temperate) and +0x2E0 (Snow) via strtok + `BuildingTypeClass__FindOrAllocate` (decompile_function 0x005981F0, strings 0x0082BC94/0x0082BC80, 2026-07-20). Consumer search came up empty on two independent axes: (a) absolute-address xrefs to every dword of both vector objects (`get_bulk_xrefs 0x00ABE29C..0x00ABE2D0`, 2026-07-20) show only the destructor's vtable writes at `0x005959A4/0x005959B5`; (b) program-wide instruction scans for member-relative access (`search_instructions` operands `+0x2C4]`, `+0x2C8]`, `+0x2D4]`, `+0x2E0]`, `+0x2E4]`, 2026-07-20) find only the MapSeed constructor `0x00595740`, destructor, and loader — nothing in the RMG generation range or anywhere else. Conclusion: in retail gamemd the lamp lists are loaded but never consumed — the generator never places ore-patch lamp buildings (one list entry per Time index suggests a cut time-of-day lamp feature). Residual risk: an indexed/indirect access pattern not covered by these two scans; none was observed.

### 10.5 DAT_00ABE2E4 identity

`DAT_00ABE2E4` = MapSeed base `0x00ABDFD8` + `0x30C` — the MapSeed "default ground level" field. Written to 4 in the MapSeed constructor `0x00595740` (`+0x30C = 4`) and twice in map-prep `0x00599650` (`MOV [EDI+0x30C], ESI(=4)` at 0x0059976E and again at 0x005997AE after the INI object setup); map-prep also emits it as `[Map] Level=4` into the generated INI and stamps every cell's level byte (`cell+0x11B`) from it (decompile_function 0x00599650, disassemble_bytes 0x00599650..0x00599780, 2026-07-20). The region flood-fill `0x0058C800` reads it as the default level byte (get_xrefs_to 0x00ABE2E4: reads at 0x0058CC4A/0x0058CDD0).

### Unverified (10.x)

- Which rect map-prep's own later `RadarClass__ComputeRadarMapBounds` calls pass (decompiler dropped the arg; net formulas verified through `0x00567230` regardless).
- `AlphaShapeClass__ClipRect` internals (label is a hint; treated as clip-to-map-rect ahead of the x/y/w/h clamps in `0x00567230`).
- Exact set of theater indices the RMG UI can produce (the tail's rule `theater != 0 -> Snow vectors` is verified; which nonzero values occur is UI-side).
- The engine's normal map-load scale for ScenarioClass `+0x3544` (Level) — here it receives the raw vector element; the INI round-trip (`/100` write) is verified but the standard `[Lighting] Level` parse scale was not re-checked in this pass.

### 10.6 Scratch initial state, coord stamping, and the first draw (parent session, 2026-07-20)

Verified via decompile_function 0x0059A2E8-containing function (= map-prep
0x00599650; label "CCINIClass__Constructor" is drift), 2026-07-20:

- **Scratch records initialise zeroed** — every field 0 (coords (0,0), +0x38
  region = 0, +0x3C stamp = 0, +0x45/+0x4B flags 0) EXCEPT `+0x40 = -1` (shore
  mask cache) and `+0x4A = 1` (shore enable). The water stage's "0 = free"
  convention comes from this; the region stage later explicitly resets
  +0x38/+0x3C to -1 (0x00598C52 loop).
- **Coords ARE stamped**: immediately after allocation, a cell-iterator loop
  writes each valid cell's packed coord into its scratch record `+0x00` and
  stamps `cell+0x11B = MapSeed+0x30C` (=4). Invalid records keep (0,0) — the
  "unused slot" convention.
- **First draw of every generation**: map-prep rolls the river-bridge enable
  `MapSeed+0x310 = (draw * K < 0.25)` (K = the perturbed 2^-32) — consumed for
  ALL map types, immediately before the region-object cleanup and radar
  rebuild, i.e. BEFORE the water stage. Any draw-stream reproduction must
  consume this draw first.
- The full-init branch also writes waypoint 700 (and 699 on the preview
  branch) at the map centre `(w/2 + h/2 + 1, w/2 + h/2)` and ORs the centre
  cell's +0x140 bit 4 — pre-terrain home-cell setup.
