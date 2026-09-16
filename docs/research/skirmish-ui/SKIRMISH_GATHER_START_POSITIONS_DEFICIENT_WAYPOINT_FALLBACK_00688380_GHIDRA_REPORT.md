# Skirmish Gather Start Positions Deficient Waypoint Fallback - Ghidra Research Report

**Address(es):** `0x00688380` primary, `0x005EE9D0` consumer, `0x005D6BE0` selected-mode explicit-start prepass, `0x0056DC20` nearby passable-cell helper, `0x0065C7E0` random ranged helper  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** `ScenarioClass__Gather_Start_Positions @ 0x00688380` behavior when selected Skirmish has fewer usable authored multiplayer start waypoints than active launch participants.  
**Non-Scope:** MCV exact/nearby placement after a house start is assigned, Choose Map UI population, random-map generator internals, and selected-mode `MCVDeploy` handling.  
**Confidence:** High for collection/fallback/control-flow; Medium for the internal meaning of every `FootClass__Find_Nearby_Passable_Cell` option flag beyond the observed `8,8` rectangle passability call.  
**Active in YR:** Yes for standard selected multiplayer/skirmish initialization; conditional on `g_GameMode != 0` and the normal selected-mode start-assignment path.

## 0. Working Notes Required By Swarm Slot

- Target question: Does standard selected YR fail deficient waypoint pools, or does `Gather_Start_Positions @ 0x00688380` fill missing starts, and with what bounds/validation?
- Non-goals: Do not re-investigate MCV placement fallback, random-map generation, Choose Map UI, combo/listbox behavior, or trackbars.
- Evidence needed to mark COMPLETE: primary decompile plus assembly ranges for authored waypoint scan, required-start count, fallback random seed bounds, `8,8` passability helper call, no attempt cap, and live selected-mode callers/consumers.
- Stop conditions: report is complete once all material branches in `0x00688380` and the immediate selected-mode consumer path are resolved or explicitly deferred.

## 1. Overview

`ScenarioClass__Gather_Start_Positions` returns a vector-like list of packed cell coordinates used by selected multiplayer start assignment. Authored waypoints are accepted only from the multiplayer start prefix and only rejected if they equal the invalid-cell sentinel. If there are fewer collected starts than the number of required non-observer human plus AI players, the function appends random nearby-passable fallback cells until the vector reaches the required count.

Active in YR: Yes. `ScenarioClass__Full_Init @ 0x00686B20` calls selected-mode vtable `+0x80` and then `ScenarioClass__AssignStartingPoints @ 0x005EE9D0` on the standard selected path when `DAT_00A8B244 == 2`; the Battle-style vtable at `0x007EE184 + 0x80` points to `0x005D6BE0`, which calls `0x00688380`.

## 2. Class Layout / Key Offsets

| Field / global | Offset / address | Type / shape | Purpose | Active in YR |
|---|---:|---|---|---|
| Scenario start waypoint array | `ScenarioClass+0x632` | 8 packed cells, 4 bytes each | Source cells for authored multiplayer starts `0..7` | Yes |
| Scenario explicit-start owner table | `ScenarioClass+0x1180` | 16 dwords | Start-index ownership table; `-1` means unoccupied | Yes |
| Invalid cell sentinel | `DAT_00B05458/0x00B0545A` | two shorts | Rejects empty waypoint/fallback result | Yes |
| Human node array | `DAT_00A8DA78` | pointer array | Human/session nodes | Yes |
| Human node count | `DAT_00A8DA84` | int | Count used in required-start formula | Yes |
| Node observer marker | node `+0x6B == -1` | int | Humans with this marker are not counted as required starts | Yes |
| AI count | `DAT_00A8B274` | int | Added to non-observer human count | Yes |
| Map min Y | `DAT_0087F90C` | int | Added to fallback random Y | Yes |
| Map min X | `DAT_0087F910` | int | Added to fallback random X | Yes |
| Map height-like bound | `DAT_0087F914` | int | Upper operand for fallback random Y minus 10 | Yes |
| Map width-like bound | `DAT_0087F918` | int | Upper operand for fallback random X minus 10 | Yes |

## 3. Core Logic

1. The function initializes a dynamic vector with growth quantum `10` and scans `ScenarioClass+0x632` from index `0` upward.
   - It stops the first count scan at the first invalid sentinel or after eight entries.
   - It also guards against impossible index bounds `i < 0 || i >= 0x2BE`, but the loop starts at zero and has an `i < 8` loop condition, so the practical authored start range is `0..7`.
   - Active in YR: Yes. Evidence: decompile `0x00688380`; assembly `0x006883B7..0x006883E9`.

2. Required start count is computed as:
   - `non_observer_humans = DAT_00A8DA84 - count(nodes where node+0x6B == -1)`
   - `required = non_observer_humans + DAT_00A8B274`
   - `target_count = max(required, authored_prefix_count)`
   - Active in YR: Yes. Evidence: decompile `0x00688380`; assembly `0x006883EB..0x0068842F`.

3. The authored waypoint append loop iterates `i = 0..target_count-1`, not strictly `0..7`.
   - A valid packed cell at `ScenarioClass+0x632+i*4` is appended and logged.
   - An invalid packed cell is skipped.
   - No passability, occupancy, water, or clearance check is applied to authored waypoint cells in this function.
   - Because `target_count` is at least the contiguous authored-prefix count but can be the required player count, non-contiguous authored starts after an early invalid slot can be ignored on deficient maps. Example: with two required starts and only waypoint `7` authored, this function checks indices `0` and `1`, appends neither, then generates two random fallback starts.
   - Active in YR: Yes. Evidence: decompile `0x00688380`; assembly `0x00688431..0x006884F2`.

4. If `target_count > vector_count`, the function logs the deficiency and enters a fallback loop.
   - The condition is count-based: repeat while `vector_count < target_count`.
   - There is no separate maximum retry counter in `0x00688380`.
   - If the helper returns the invalid-cell sentinel, the function does not append and immediately tries another random seed.
   - Active in YR: Conditional. It is active when authored starts are deficient. Evidence: decompile `0x00688380`; assembly `0x006884F8..0x00688643`.

5. Fallback random seed formula, in packed cell terms:
   - `seed_x = RandomRanged(0, DAT_0087F918 - 10) + DAT_0087F910 + 10`
   - `seed_y = RandomRanged(10, DAT_0087F914 - 10) + DAT_0087F90C`
   - The random helper is inclusive and swaps endpoints if `max < min`.
   - Active in YR: Conditional on deficient starts. Evidence: decompile `0x00688380`; assembly `0x00688528..0x0068857C`; `Random__RandomRanged @ 0x0065C7E0`.

6. Fallback passable-cell query uses `FootClass__Find_Nearby_Passable_Cell` with this observed argument shape:
   - this/object context: `0x0087F7E8`
   - output cell pointer and seed cell pointer
   - `param_4 = 1`
   - `param_5 = -1`
   - three zero flags before dimensions
   - rectangle dimensions `8, 8`
   - three more zero flags
   - one flag byte set to `1`
   - reference cell pointer initialized to `(0,0)`
   - final two flag bytes `0,0`
   - Active in YR: Conditional on deficient starts. Evidence: decompile `0x00688380`; assembly `0x00688572..0x006885B5`; helper decompile `0x0056DC20`.

7. The `8,8` claim is real for passability rectangle dimensions.
   - Inside `FootClass__Find_Nearby_Passable_Cell`, the corresponding arguments are passed through to `CellRect__CheckPassability(candidate, param_8, param_9, ...)`.
   - For this caller, `param_8 == 8` and `param_9 == 8`.
   - Active in YR: Conditional on deficient starts. Evidence: caller assembly `0x00688587..0x00688591`; helper decompile `0x0056DC20`.

8. `FootClass__Find_Nearby_Passable_Cell` can return an invalid fallback result.
   - If it finds no candidates, it writes `DAT_00ABD480` to the caller output. In this program, the caller compares the returned packed cell against the invalid-cell sentinel before append.
   - Active in YR: Conditional on deficient starts and failed nearby search. Evidence: helper decompile `0x0056DC20`; caller assembly `0x006885BA..0x006885D5`.

9. Helper search behavior relevant to this caller:
   - It scans outward from the seed cell by rings up to a computed radius `(this+0xF4)+(this+0xF8)`, capped at `0x20`.
   - It collects up to `0x18` candidate cells.
   - It applies screen/cell existence checks plus `CellRect__CheckPassability` before a candidate can be used.
   - It partitions candidates by whether a center/lepton round-trip maps back to the same cell, then chooses from one partition by frame-counter modulo or by nearest to the reference cell.
   - Active in YR: Conditional through this fallback call. Evidence: helper decompile `0x0056DC20`.

10. The fallback cells are appended to the same vector as authored cells.
    - Later start assignment sees generated fallback entries by index; there is no "deficient" status propagated by native code.
    - Active in YR: Yes. Evidence: decompile `0x005EE9D0` consumes `local_18/local_14` vector returned by `0x00688380`; `FUN_005EE6F0 @ 0x005EE6F0` writes chosen vector index into `ScenarioClass+0x1180` and returns the packed cell.

## 4. INI Keys

This slice does not read INI keys directly. The source data for authored starts is the already-loaded scenario waypoint array, which originates from map `[Waypoints]` parsing elsewhere.

| Key / section | Used here? | Default / effect | Active in YR |
|---|---|---|---|
| `[Waypoints]` `0..7` | Indirect only | Populates `ScenarioClass+0x632` before this function | Yes |
| Rules/art keys | No | None in this slice | Not applicable |

## 5. Integration Points

| Integration | Evidence | Behavior | Active in YR |
|---|---|---|---|
| `ScenarioClass__Full_Init @ 0x00686B20` | decompile `0x00686B20`; vtable read `0x007EE184` | selected multiplayer path calls mode `+0x80`, then `ScenarioClass__AssignStartingPoints` when `DAT_00A8B244 == 2` | Yes for standard selected Skirmish |
| Battle-style mode `+0x80` | vtable bytes `0x007EE184+0x80 -> 0x005D6BE0`; assembly `0x005D6BE0..0x005D6C63` | calls `Gather_Start_Positions`, applies explicit `House+0x16058` starts into `ScenarioClass+0x1180` | Yes for Battle-style selected mode |
| `ScenarioClass__AssignStartingPoints @ 0x005EE9D0` | decompile `0x005EE9D0` | calls `Gather_Start_Positions`, builds 16-byte occupied table, assigns human houses first and AI houses second | Yes when `DAT_00A8B244 == 2` |
| `FUN_005EE6F0` | decompile `0x005EE6F0` | chooses free vector entries and writes `ScenarioClass+0x1180 + index*4` | Yes |
| `FootClass__Find_Nearby_Passable_Cell @ 0x0056DC20` | decompile `0x0056DC20`; caller assembly `0x00688572..0x006885B5` | validates fallback candidate cells using an `8x8` rectangle passability query | Conditional |

## 6. Current Rust Implementation Status

Current Rust mismatch is concentrated in `src/app_skirmish.rs` and `src/map/waypoints.rs`.

- `src/map/waypoints.rs:18..61` parses and returns all start waypoints `0..=7` sorted by waypoint index. It does not model native contiguous-prefix behavior after an invalid/missing lower start slot.
- `src/app_skirmish.rs:187..188` calls `waypoints::multiplayer_start_waypoints` and immediately passes the sorted list to `assign_launch_starts`.
- `src/app_skirmish.rs:375..418` marks `unsupported = starts.len() < slots.len()` and never generates fallback passable starts.
- `src/app_skirmish.rs:398..407` assigns auto slots to the first unused authored waypoint only; if none remains, it leaves the slot unassigned and flags unsupported.
- `src/app_skirmish.rs:192..235` spawns only for assignments returned by `assign_launch_starts`, so deficient waypoint pools can produce missing MCVs in Rust where native attempts to fill the missing start cells.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Working-notes target/non-goals/evidence/stop conditions | verified | section 0 | none |
| `0x00688380` authored start count scan | verified | decompile `0x00688380`; assembly `0x006883B7..0x006883E9` | none |
| Required-start formula | verified | decompile `0x00688380`; assembly `0x006883EB..0x0068842F` | none |
| Existing waypoint validation | verified | decompile `0x00688380`; assembly `0x00688431..0x006884F2` | none |
| Deficient fallback loop | verified | decompile `0x00688380`; assembly `0x006884F8..0x00688643` | none |
| Fallback seed bounds | verified | decompile `0x00688380`; assembly `0x00688528..0x0068857C`; `0x0065C7E0` | exact semantic names of the four map-bound globals are inherited from surrounding map code, not re-derived here |
| `Random__RandomRanged` inclusive/swap behavior | verified | decompile `0x0065C7E0` | none |
| `FootClass__Find_Nearby_Passable_Cell` caller arguments | verified | decompile `0x00688380`; assembly `0x00688572..0x006885B5` | none |
| `8,8` rectangle passability | verified | helper decompile `0x0056DC20`; caller assembly `0x00688587..0x00688591` | lower-level `CellRect__CheckPassability` internals out of scope |
| No attempt cap in gather loop | verified | decompile `0x00688380`; assembly `0x00688639..0x00688643` | none |
| Helper no-candidate return | verified | decompile `0x0056DC20`; caller sentinel check `0x006885BA..0x006885D5` | exact runtime probability of pathological hang requires map/runtime fixture |
| Selected-mode liveness | verified | decompile `0x00686B20`, vtable bytes `0x007EE184`, assembly `0x005D6BE0..0x005D6C63`, decompile `0x005EE9D0` | non-Battle selected modes not claimed beyond standard path |
| Rust delta scan | verified | `src/app_skirmish.rs:187..418`, `src/map/waypoints.rs:18..61` | implementation not performed |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-01 - Is `0x00688380` on a live selected YR Skirmish path? -> Yes, via selected-mode `+0x80` and `ScenarioClass__AssignStartingPoints`.` (evidence: `0x00686B20`; vtable `0x007EE184+0x80`; `0x005D6BE0`; `0x005EE9D0`)
- `[RESOLVED] OQ-02 - Which authored waypoints are considered? -> Multiplayer start slots beginning at `ScenarioClass+0x632`, practically indices `0..7`, with a first count pass that stops at the first invalid sentinel or eight entries.` (evidence: `0x006883B7..0x006883E9`)
- `[RESOLVED] OQ-03 - Are existing authored starts passability-validated here? -> No; non-invalid packed cells are appended without passability/occupancy checks in this function.` (evidence: `0x00688431..0x006884F2`)
- `[RESOLVED] OQ-04 - How many starts are required? -> `max(authored_prefix_count, non_observer_humans + ai_count)`.` (evidence: `0x006883EB..0x0068842F`)
- `[RESOLVED] OQ-05 - Are observer humans counted? -> No; human nodes with `node+0x6B == -1` are subtracted from `DAT_00A8DA84`.` (evidence: `0x006883F6..0x00688415`)
- `[RESOLVED] OQ-06 - Does deficient waypoint count fail launch? -> No; it enters a fallback append loop until vector count reaches target count.` (evidence: `0x006884F8..0x00688643`)
- `[RESOLVED] OQ-07 - What are fallback seed bounds? -> `x = RandomRanged(0, width_bound-10) + min_x + 10`; `y = RandomRanged(10, height_bound-10) + min_y`.` (evidence: `0x00688528..0x0068857C`)
- `[RESOLVED] OQ-08 - Is `RandomRanged` inclusive? -> Yes; it returns within inclusive endpoints and swaps if arguments are reversed.` (evidence: `0x0065C7E0`)
- `[RESOLVED] OQ-09 - Is the 8x8 clearance claim real? -> Yes for passability rectangle dimensions passed through to `CellRect__CheckPassability`.` (evidence: `0x00688587..0x00688591`; `0x0056DC20`)
- `[RESOLVED] OQ-10 - What happens when the nearby helper returns invalid? -> The fallback cell is skipped and the loop tries again; no native error is returned by gather.` (evidence: `0x006885BA..0x006885D5`; `0x00688639..0x00688643`)
- `[RESOLVED] OQ-11 - Is there a maximum fallback attempt count in gather? -> No separate cap was found; the loop condition is only `vector_count < target_count`.` (evidence: `0x00688639..0x00688643`)
- `[RESOLVED] OQ-12 - Do generated fallback starts become normal assignment candidates? -> Yes; they are appended to the same vector consumed by `FUN_005EE6F0`.` (evidence: `0x00688380`; `0x005EE6F0`; `0x005EE9D0`)
- `[RESOLVED] OQ-13 - Does native preserve sparse authored waypoint indices? -> Not as external IDs in this slice; it assigns by vector index after gather. Early invalid gaps can cause later authored slots to be ignored in deficient cases.` (evidence: `0x00688431..0x006884F2`; `0x005EE6F0`)
- `[RESOLVED] OQ-14 - Does current Rust fill deficient starts? -> No; it sets `unsupported_deficient_starts` and drops slots without assignments.` (evidence: `src/app_skirmish.rs:187..188`; `src/app_skirmish.rs:375..418`)
- `[RESOLVED] OQ-15 - Does current Rust model contiguous-prefix/gap behavior? -> No; it collects all `0..=7` starts sorted by key.` (evidence: `src/map/waypoints.rs:18..61`)
- `[RESOLVED] OQ-16 - Null/empty waypoint pool edge case? -> Empty authored starts still enters fallback if required starts are positive.` (evidence: `0x006883EB..0x00688643`)
- `[RESOLVED] OQ-17 - Zero active participants edge case? -> If target count is `0`, the fallback branch is skipped and an empty vector is returned.` (evidence: `0x00688431..0x00688502`)
- `[RESOLVED] OQ-18 - Pathological no-passable fallback edge case? -> Gather can loop indefinitely because invalid helper results do not increment count and no attempt cap was found.` (evidence: `0x006885BA..0x00688643`)
- `[DEFERRED] OQ-19 - Exact lower-level `CellRect__CheckPassability` tile/overlay/object rules for an 8x8 start rectangle.` (category: out-of-scope; reason: this slot only verifies the deficient-start call contract and not the complete passability engine; next-step-if-pursued: targeted passability rectangle investigation)
- `[DEFERRED] OQ-20 - Runtime value/source names for the four map-bound globals in all theaters and generated maps.` (category: requires-different-system-context; reason: the caller formula is verified, but global initialization belongs to map bounds/radar/local-size setup; next-step-if-pursued: investigate `LocalSize`/map-bound global initialization)

Adversarial corner-case answers:

- Missing waypoint `0`, present waypoint `7`, two players: native ignores `7` in this function and generates two fallback starts because `target_count` is two and the append loop checks indices `0` and `1`.
- One valid authored start and two players: native keeps the authored start and generates one fallback start.
- Authored start on blocked terrain: this function still appends it; later MCV placement handles placement failure/fallback.
- Tiny or malformed map bounds: `RandomRanged` swaps reversed endpoints, but the fallback loop can still spin if no passable 8x8 cell is found.
- No active required starts: no fallback is generated.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Deficient authored start pools are filled with generated fallback cells until the start vector reaches required active participant count. | `0x006884F8..0x00688643`; decompile `0x00688380` | Missing; Rust flags `unsupported_deficient_starts` and leaves slots unassigned | `src/app_skirmish.rs::assign_launch_starts`, `apply_skirmish_launch_session` | Generate fallback start cells before assignment instead of treating deficiency as unsupported/no-spawn | `skirmish_deficient_waypoints_generate_passable_fallback_start`: two active slots on a map with one valid start still receive two assigned base cells | Do not convert deficient waypoint pools into launch failure or missing MCVs |
| Fallback seed cells use native asymmetric bounds: X from `0..width_bound-10` plus `min_x+10`; Y from `10..height_bound-10` plus `min_y`. | `0x00688528..0x0068857C`; `0x0065C7E0` | Missing | Future map/skirmish start fallback helper | Use deterministic native RNG and bounds when producing fallback seed cells | `skirmish_deficient_start_seed_bounds_match_native_edges`: captured RNG sequence produces seed coordinates with the same min/max inclusivity | Do not use a uniform "10 cells inset on all four sides" shortcut; X and Y formulas differ in the binary |
| Fallback nearby search requests an `8x8` passability rectangle and appends only non-invalid helper results. | caller `0x00688572..0x006885B5`; helper `0x0056DC20`; sentinel check `0x006885BA..0x006885D5` | Missing; no fallback passability query exists | `src/app_skirmish.rs`, future map passability/placement query | Search for a passable cell that satisfies the native `8x8` rectangle contract before appending | `skirmish_deficient_start_requires_eight_by_eight_passable_area`: a seed near obstacles chooses a fallback cell whose 8x8 area is passable | Do not accept any single passable tile as a start fallback |
| Authored waypoint starts are sentinel-validated only in gather; blocked authored starts are still appended. | `0x00688431..0x006884F2` | Rust currently accepts authored starts, but later direct spawn may fail and mark human owner lost | `src/app_skirmish.rs`, MCV placement surface from slot 1 | Keep authored starts as candidates even if placement later needs MCV nearby fallback | `skirmish_blocked_authored_start_still_assigned_before_mcv_fallback`: blocked start cell remains the house base cell before placement fallback | Do not pre-filter authored starts by passability in the gather/assignment layer |
| Native uses a contiguous-prefix style authored count and vector index assignment; sparse later waypoints may be ignored when an earlier start slot is invalid and the map is deficient. | count scan `0x006883B7..0x006883E9`; append loop `0x00688431..0x006884F2`; `FUN_005EE6F0` | Mismatch; Rust collects all `0..=7` waypoints sorted, even sparse ones | `src/map/waypoints.rs::multiplayer_start_waypoints`, `src/app_skirmish.rs::assign_launch_starts` | Model native start-vector construction separately from generic waypoint parsing | `skirmish_gather_start_positions_ignores_sparse_late_start_after_gap_when_deficient`: waypoints `{7}` with two active slots produce two generated starts, not authored slot 7 | Do not reuse the generic sorted `0..=7` waypoint list as the native gather output for deficient maps |
| There is no native fallback attempt cap in `0x00688380`; invalid helper results cause another random try. | `0x006885BA..0x00688643` | Missing, and Rust currently avoids the loop by not generating fallback | fallback helper design | For parity-sensitive mode, the success condition is count reached, not attempts exhausted; if Rust adds a safety guard, it should be explicit and test-covered as an engine safety deviation | `skirmish_deficient_fallback_retries_after_invalid_helper_result`: first invalid helper result is skipped and a later valid helper result is appended | Do not add a small retry cap that silently leaves players without starts |

### Negative Facts / Do Not Do

- Do not treat deficient waypoint pools as a Start-button validation failure or launch failure. Active in YR: Yes on standard selected Skirmish. Evidence: `0x006884F8..0x00688643`.
- Do not pre-filter authored waypoints for passability inside gather. Active in YR: Yes. Evidence: `0x00688431..0x006884F2`.
- Do not implement fallback as "random single passable cell"; the caller requests an `8x8` rectangle passability check. Active in YR: Conditional on deficiency. Evidence: `0x00688587..0x00688591`; `0x0056DC20`.
- Do not assume fallback seed bounds are symmetric. The binary's X and Y formulas differ. Active in YR: Conditional on deficiency. Evidence: `0x00688528..0x0068857C`.
- Do not preserve sparse authored waypoint IDs as assignment IDs in this layer. Native assigns vector indices after gather. Active in YR: Yes. Evidence: `0x005EE6F0`.

### Stale Docs / Follow-up Docs

- `skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md`: keep the conclusion that deficient starts are generated, but replace any vague "8x8 clearance" sentence with "fallback calls `FootClass__Find_Nearby_Passable_Cell` with `8,8` rectangle dimensions that flow into `CellRect__CheckPassability`; lower-level passability semantics remain a separate slice."
- [skirmish-ui/SKIRMISH_SPAWN_PLACEMENT_AFTER_ASSIGNED_START_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SPAWN_PLACEMENT_AFTER_ASSIGNED_START_GHIDRA_REPORT.md): correct the fallback seed formula if reused. The verified formula is `x = RandomRanged(0, DAT_0087F918 - 10) + DAT_0087F910 + 10`, `y = RandomRanged(10, DAT_0087F914 - 10) + DAT_0087F90C`.
- Any implementation note saying deficient starts are unsupported/no-spawn should be replaced with "standard selected YR fills missing start-vector entries with random nearby-passable fallback cells before assignment."

## 10. Remaining Uncertainty

- The exact semantics of every lower-level `CellRect__CheckPassability` rule for the `8x8` rectangle were not drained in this slot. The handoff only claims the call contract and dimensions.
- The semantic names and initialization sources for `DAT_0087F90C/F910/F914/F918` were not re-derived here; the caller formula and use are verified.
- A pathological map with no possible `8x8` passable fallback appears capable of spinning in native gather, but reproducing the visible runtime load behavior would require a fixture/runtime observation.

## Sources

- Ghidra read-only decompile: `ScenarioClass__Gather_Start_Positions @ 0x00688380`
- Ghidra assembly context: `0x006883B7..0x00688643`
- Ghidra read-only decompile: `ScenarioClass__AssignStartingPoints @ 0x005EE9D0`
- Ghidra read-only decompile: `FUN_005EE6F0 @ 0x005EE6F0`
- Ghidra assembly context: Battle-style selected `+0x80` target `0x005D6BE0..0x005D6C63`
- Ghidra read-only decompile: `ScenarioClass__Full_Init @ 0x00686B20`
- Ghidra memory read: vtable `0x007EE184`, with `+0x80 -> 0x005D6BE0`
- Ghidra read-only decompile: `FootClass__Find_Nearby_Passable_Cell @ 0x0056DC20`
- Ghidra read-only decompile: `Random__RandomRanged @ 0x0065C7E0`
- Prior reports referenced: [skirmish-ui/SKIRMISH_SPAWN_PLACEMENT_AFTER_ASSIGNED_START_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SPAWN_PLACEMENT_AFTER_ASSIGNED_START_GHIDRA_REPORT.md), `skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md`, [skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md)
- Rust scan: `src/app_skirmish.rs`, `src/map/waypoints.rs`
