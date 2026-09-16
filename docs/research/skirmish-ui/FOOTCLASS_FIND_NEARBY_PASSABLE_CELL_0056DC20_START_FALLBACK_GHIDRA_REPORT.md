# FootClass Find Nearby Passable Cell 0x0056DC20 - Skirmish Start Fallback Ghidra Research Report

**Address(es):** `0x0056DC20` primary, caller `0x00688380`, validator `0x0056E7C0`, selected-mode liveness through `0x00686B20`, `0x005D6BE0`, `0x005EE9D0`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** `FootClass__Find_Nearby_Passable_Cell @ 0x0056DC20` only as called by `ScenarioClass__Gather_Start_Positions @ 0x00688380` for deficient selected Skirmish start fallback.  
**Non-Scope:** full pathfinding, all other `0x0056DC20` callers, complete `CellClass__CheckCellPassability`, MCV placement fallback `0x00688ED0`, and random-map start generation.  
**Confidence:** High for caller arguments, search bounds/order, no-candidate return, active-path liveness, and Rust-facing delta; Medium for semantic names of some legacy cell fields imported from sibling validator reports.  
**Active in YR:** Yes, conditional on standard selected multiplayer/skirmish maps whose gathered authored start vector is smaller than the required active participant count.

## 0. Working Notes Required By Swarm Slot

- Target question: For deficient selected Skirmish starts, what exact `0x0056DC20` argument contract, search order, map-bound behavior, 8x8 rectangle meaning, no-result behavior, and YR liveness should Rust reproduce?
- Non-goals: Do not investigate full A*, all `Find_Nearby_Passable_Cell` callers, full `CellClass__CheckCellPassability`, MCV nearby placement `0x00688ED0`, or random-map generated starts.
- Evidence needed to mark COMPLETE: caller assembly for the argument pushes, primary decompile and assembly for radius/order/return, validator decompile for the `8,8` rectangle loop, and selected Skirmish call-chain evidence.
- Stop conditions: stop once every material branch used by the `0x00688380` caller is resolved or explicitly deferred, and no Rust/document handoff depends on an unverified lower-level passability claim.

## 1. Overview

When `ScenarioClass__Gather_Start_Positions @ 0x00688380` has too few authored multiplayer starts, it chooses a random seed cell inside map bounds and calls `0x0056DC20` to turn that seed into a passable fallback start. The call requests an `8x8` top-left rectangle passability check, skips zone matching, skips final occupancy checking, allows structural bridge cells at the `0x0056DC20` bridge-filter layer, and retries in gather if the helper returns the invalid-cell sentinel.

Active in YR: Yes. `ScenarioClass__Full_Init @ 0x00686B20` runs this path in selected multiplayer mode after `ScenarioClass__Create_Houses`; Battle-style selected vtable `+0x80` target `0x005D6BE0` calls `0x00688380`, and `ScenarioClass__AssignStartingPoints @ 0x005EE9D0` calls `0x00688380` before assigning starts.

## 2. Start-Fallback Call Contract

Caller assembly at `0x00688572..0x006885B5` pushes the exact `0x0056DC20` arguments:

| Argument | Start-fallback value | Meaning in this slice | Active in YR |
|---|---:|---|---|
| `this` / `ECX` | `0x0087F7E8` | global map/search context; search cap reads `this+0xF4` plus `this+0xF8`, capped to `32` | Yes |
| output cell | local stack pointer | receives chosen packed cell | Yes |
| origin / seed cell | random cell from `0x00688380` | ring-search origin | Yes |
| `param_4` | `1` | SpeedType-style input to `CheckPassability` | Yes |
| `param_5` | `-1` | required zone id disabled; `0xFFFF` is normalized to `-1` in callee | Yes |
| `param_6` | `0` | MovementZone / zone-row family input to `CheckPassability` | Yes |
| `param_7` | `0` | bridge-aware height input false; also enables lepton-center round-trip classification | Yes |
| `param_8` | `8` | rectangle width passed to `CellRect__CheckPassability` | Yes |
| `param_9` | `8` | rectangle height passed to `CellRect__CheckPassability` | Yes |
| `param_10` | `0` | reject-any-overlay flag passed false | Yes |
| `param_11` | `0` | origin/candidate height-difference gate disabled | Yes |
| `param_12` | `0` | `TechnoClass__Is_Current_Cell_Obstacle_Free` gate disabled | Yes |
| `param_13` | `1` | structural bridge cells are allowed by the separate `0x0056DC20` bridge filter | Yes |
| `param_14` | local cell initialized to `0,0` | reference cell; static invalid sentinels are also zero in the program image | Yes |
| `param_15` | `0` | no quadrant/edge skip | Yes |
| `param_16` | `0` | final `CellRect__CheckOccupancy` disabled | Yes |

Evidence: decompile `0x00688380`; assembly `0x00688572..0x006885B5`; decompile `0x0056DC20`; static reads of `DAT_00ABD480` and `DAT_00B05458` show zero in the program image.

## 3. Core Logic Used By This Caller

### 3.1 Search Radius

`0x0056DC20` derives the ring cap from `*(this+0xF4) + *(this+0xF8)`, then clamps it to `32`. For the deficient-start caller, `this` is the global at `0x0087F7E8`, not a start-specific object and not the `8,8` rectangle dimensions. If the cap is `<= 0`, the helper writes `DAT_00ABD480` to the output and returns.

Active in YR: Yes on the deficient-start path. Evidence: decompile `0x0056DC20`; assembly `0x0056DCE1..0x0056DD03`; caller `0x006885A6..0x006885B5`.

### 3.2 Ring / Candidate Order

The helper scans rings starting at radius `0`. For each radius `r`, it collects candidates in this order, stopping at `24` stored candidates:

1. Top and bottom rows: for `delta = -r..=r`, test `(origin.x + delta, origin.y - r)` and then `(origin.x + delta, origin.y + r)`.
2. Left and right columns, excluding corners: for `delta = 1-r..=r-1`, test `(origin.x - r, origin.y + delta)` and then `(origin.x + r, origin.y + delta)`.

With `param_15 == 0`, the start-fallback caller does not skip either side. Radius `0` can therefore test the origin twice through the top/bottom row logic. This is not the same ordering as MCV placement fallback `0x00688ED0`.

Active in YR: Yes when deficient starts call the helper. Evidence: decompile `0x0056DC20`; assembly ranges `0x0056DD09..0x0056E14D` and `0x0056E160..0x0056E589`.

### 3.3 Candidate Acceptance Gates For This Caller

For each candidate top-left cell, the start-fallback call uses only these active gates:

- Cell lookup by `y * 0x200 + x`, valid index `[0, 0x3FFFF]`; out-of-range or null cells substitute dummy cell `DAT_00ABDC50`.
- `TechnoClass__IsOnScreen(candidate_cell, 1)` must pass before passability is checked.
- `CellRect__CheckPassability(candidate, 8, 8, 1, -1, 0, -1, 0, 0)` must pass.
- Structural bridge rejection is disabled because `param_13 == 1`.

The start-fallback call does not enable the height-difference gate, current-cell obstacle-free gate, or final rectangle occupancy gate. `CellRect__CheckOccupancy` is never called for this caller because `param_16 == 0`.

Active in YR: Yes for deficient selected Skirmish fallback. Evidence: decompile `0x0056DC20`; passability calls at `0x0056DE0E`, `0x0056E024`, `0x0056E265`, `0x0056E467`; disabled occupancy branches at `0x0056DE86..0x0056DE9D`, `0x0056E09C..0x0056E0B3`, `0x0056E2DF..0x0056E2F6`, `0x0056E4E1..0x0056E4F8`.

### 3.4 What `8x8` Means Here

`8,8` are rectangle dimensions passed to `CellRect__CheckPassability`. The validator loops `x_offset = 0..7` outside and `y_offset = 0..7` inside; every sub-cell from `(candidate.x, candidate.y)` through `(candidate.x+7, candidate.y+7)` must pass `CellClass__CheckCellPassability` for the supplied arguments. This is a top-left rectangle check, not a radius, not an 8-cell ring, and not a single-cell test.

For this caller, required zone id is `-1`, so zone equality is skipped. Required height is also `-1` at the validator layer. The separate `CheckOccupancy` rectangle, including its final `MapClass__IsRectInPlayfield` corner check, is disabled by `param_16 == 0`; map-bound behavior comes from candidate-cell `IsOnScreen`, per-sub-cell lookup/dummy-cell passability, and whatever `CheckCellPassability` rejects.

Active in YR: Yes for deficient selected Skirmish fallback. Evidence: caller pushes `8` at `0x00688587` and `0x00688591`; `CellRect__CheckPassability @ 0x0056E7C0` loops over `param_2`/`param_3`; call sites from `0x0056DC20` pass `param_8`/`param_9`.

### 3.5 Stop And Selection Behavior

The helper keeps scanning outward until one of these happens:

- `24` candidates are stored.
- At least one direct candidate is found and the current ring finishes.
- Radius reaches the computed cap.

For `param_7 == 0`, a candidate is classified as direct only if the lepton center `(x*256+128, y*256+128)` round-trips through `FUN_006D6410` back to the same cell. Candidates that pass the gates but fail that round-trip are still stored, but they do not set the early-stop flag.

After scanning, candidates are partitioned into direct and indirect lists by the same round-trip. If the reference cell equals `DAT_00ABD480`, the helper chooses by `g_CurrentFrameCounter % direct_count` when direct candidates exist, otherwise by `g_CurrentFrameCounter % indirect_count`. The start-fallback caller passes a zero reference cell, and both static invalid-cell globals read as zero in this program image. If a future runtime changes that sentinel, the alternate path chooses the candidate nearest to the reference cell by Euclidean distance.

Active in YR: Yes for this helper path. Evidence: decompile `0x0056DC20`; assembly `0x0056E596..0x0056E5B3`, partition and modulo selection `0x0056E5BF..0x0056E6ED`, nearest-reference selection `0x0056E6F0..0x0056E790`; static reads of `0x00ABD480` and `0x00B05458`.

### 3.6 No-Candidate Return

If no candidate is stored, `0x0056DC20` writes `DAT_00ABD480` to the output and returns the output pointer. `0x00688380` immediately compares the returned packed cell against `DAT_00B05458/00B0545A`; if it is invalid, it appends nothing and repeats the fallback loop with a new random seed. There is no retry cap in `0x00688380`.

Active in YR: Conditional on deficient starts and failed nearby search. Evidence: helper no-candidate return `0x0056E79A..0x0056E7B3`; caller invalid check `0x006885BA..0x006885D5`; gather loop `0x00688639..0x00688643`.

## 4. INI Keys

No INI key is read directly by `0x0056DC20` or `0x00688380` in this slice. The caller-supplied `SpeedType`/`MovementZone`-style values are constants for this start fallback, and the map cells/waypoints were already loaded before gather runs.

| Data source | Used here? | Active in YR | Evidence |
|---|---|---|---|
| `[Waypoints]` `0..7` | Indirect input to `0x00688380`; deficiency triggers this helper | Yes | `0x006883B7..0x00688643` |
| Rules/art `SpeedType=` / `MovementZone=` | Not read here; constants `1` and `0` are supplied by this caller | Not in this slice | caller assembly `0x00688580..0x006885B5` |

## 5. Integration Points

| Integration | Status | Evidence | Active in YR |
|---|---|---|---|
| Selected Skirmish `Full_Init` path | verified | `0x00686B20` calls selected mode `+0x80`, then `AssignStartingPoints` when `DAT_00A8B244 == 2` | Yes |
| Battle-style selected `+0x80` | verified | disassembly `0x005D6BE0..0x005D6C63`; call to `0x00688380` at `0x005D6BEC` | Yes for Battle-style selected modes |
| `AssignStartingPoints` consumer | verified | decompile `0x005EE9D0`; first call is `ScenarioClass__Gather_Start_Positions` | Yes |
| Deficient gather caller | verified | decompile `0x00688380`; call at `0x006885B5` | Conditional on deficient authored starts |
| Other `0x0056DC20` callers | touched only | xrefs list includes many movement/production/superweapon contexts | out-of-scope for this report |

## 6. Current Rust Implementation Status

- `src/app_skirmish.rs:187..188` reads `waypoints::multiplayer_start_waypoints` and passes the authored list directly to `assign_launch_starts`.
- `src/app_skirmish.rs:375..418` marks `unsupported_deficient_starts` when authored starts are fewer than launch slots and never calls a fallback generator.
- `src/app_skirmish.rs:192..235` spawns only assigned starts, so a deficient map can produce missing MCVs rather than native fallback starts.
- `src/sim/miner/miner_dock_sequence.rs:268..285` has a partial `Find_Nearby`-style ring helper for refinery exit tests, but it returns from a `PathGrid`/`OccupancyGrid` abstraction and does not model the start-fallback caller's `8x8` `CheckPassability`, direct/indirect partition, or exact argument flags.
- `src/sim/pathfinding/core.rs` and `src/map/resolved_terrain.rs` have `PathGrid`, zone, and reduced terrain data, but there is no exact start-fallback rectangle validator surface.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Required working notes | verified | section 0 | none |
| `0x00688380 -> 0x0056DC20` argument order | verified | decompile `0x00688380`; assembly `0x00688572..0x006885B5` | none |
| `this`/radius cap | verified | decompile/disassembly `0x0056DCE1..0x0056DD03` | semantic names of `0x0087F7E8+F4/F8` deferred |
| Ring order and `24` candidate cap | verified | decompile/disassembly `0x0056DD09..0x0056E5B3` | none |
| Start-fallback active/inactive candidate gates | verified | decompile `0x0056DC20`; caller arguments | none |
| `8x8` rectangle meaning | verified | `0x00688587/0x00688591`; `0x0056E7C0` | full `CheckCellPassability` internals are sibling-slot scope |
| No-candidate sentinel return | verified | `0x0056E79A..0x0056E7B3`; caller `0x006885BA..0x006885D5` | runtime sentinel mutation not observed, static image is zero |
| Selected YR liveness | verified | `0x00686B20`, `0x005D6BE0`, `0x005EE9D0` | non-Battle selected modes not claimed here |
| Rust scan | verified | `src/app_skirmish.rs`, `src/sim/miner/miner_dock_sequence.rs`, `src/sim/pathfinding/core.rs` | no implementation performed |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-01 - Is `0x0056DC20` active in standard selected YR Skirmish start fallback? -> Yes, conditionally through deficient `0x00688380` starts on the selected Full_Init path.` (evidence: `0x00686B20`, `0x005D6BE0`, `0x005EE9D0`, `0x00688380`)
- `[RESOLVED] OQ-02 - What arguments does `0x00688380` pass? -> `this=0x0087F7E8`, output, seed, `1,-1,0,0,8,8,0,0,0,1,ref(0,0),0,0`.` (evidence: `0x00688572..0x006885B5`)
- `[RESOLVED] OQ-03 - Are `8,8` dimensions or radius? -> Rectangle width/height for `CellRect__CheckPassability`, not radius.` (evidence: `0x00688587`, `0x00688591`, `0x0056E7C0`)
- `[RESOLVED] OQ-04 - Is zone checking active? -> No, required zone id is `-1`.` (evidence: caller push `-1` at `0x00688598`; helper normalizes `0xFFFF` to `-1` at `0x0056DC43..0x0056DC60`)
- `[RESOLVED] OQ-05 - Is final occupancy active? -> No, `param_16 == 0`, so `CellRect__CheckOccupancy` branches are skipped.` (evidence: caller final push `0`; helper branch sites `0x0056DE86`, `0x0056E09C`, `0x0056E2DF`, `0x0056E4E1`)
- `[RESOLVED] OQ-06 - Are structural bridge cells rejected by this caller? -> No at the separate FNPC filter; `param_13 == 1` bypasses the reject-bridge branch.` (evidence: caller push `1` at `0x00688582`; helper branch `0x0056DE6C..0x0056DE80`)
- `[RESOLVED] OQ-07 - What is the search radius? -> `min(*(this+0xF4)+*(this+0xF8), 32)` using `this=0x0087F7E8`.` (evidence: `0x0056DCE1..0x0056DCF9`; caller `0x006885A6`)
- `[RESOLVED] OQ-08 - What happens when no candidate is found? -> helper writes invalid sentinel; gather skips append and retries with no gather-level cap.` (evidence: `0x0056E79A..0x0056E7B3`, `0x006885BA..0x00688643`)
- `[RESOLVED] OQ-09 - Does this match the MCV `0x00688ED0` fallback? -> No, this is a separate ring collector and validator path with different order and flags.` (evidence: `0x0056DC20` vs prior `SKIRMISH_MCV_NEARBY_PLACEMENT_FALLBACK_00688ED0_GHIDRA_REPORT.md`)
- `[DEFERRED] OQ-10 - Exact semantic names and initialization for `0x0087F7E8+0xF4/+0xF8`.` (category: requires-different-system-context; reason: radius formula and cap are verified, but map/search-context field naming belongs to map bounds/tactical setup; next-step-if-pursued: map bounds global initialization slice)
- `[DEFERRED] OQ-11 - Full `CellClass__CheckCellPassability` branch taxonomy for all terrain/overlay combinations.` (category: out-of-scope; reason: this slot only claims the caller flags and `8x8` top-left rectangle contract; next-step-if-pursued: use the sibling CellRect start rectangle report)

Adversarial corner cases answered:

- If the random seed itself passes the `8x8` rectangle and direct round-trip checks, radius `0` can satisfy the helper; the origin can be collected twice by top/bottom logic.
- If only indirect candidates are found on early rings, scanning can continue until a direct candidate, the `24` cap, or radius cap.
- If no candidate exists, native gather can keep retrying forever because invalid helper results do not increment the vector count.
- The `8x8` rectangle is not a center footprint around the seed; it starts at the candidate top-left.
- A Rust helper that returns first passable cell is not equivalent when multiple direct candidates exist; native chooses by frame-counter modulo when the reference equals the invalid sentinel.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Deficient-start fallback calls `0x0056DC20` with an `8x8` top-left `CheckPassability` rectangle, zone disabled, bridge structural filter allowed, and final occupancy disabled. | `0x00688572..0x006885B5`; `0x0056DE0E/0x0056E024/0x0056E265/0x0056E467`; `0x0056E7C0` | missing; Rust sets `unsupported_deficient_starts` and has no fallback validator | `src/app_skirmish.rs::assign_launch_starts`; future map/passability helper | Generate missing start cells only from helper results that satisfy the native `8x8` passability contract and caller flags | Two-player map with one authored start and a blocked single-tile seed does not accept that seed unless the full `8x8` top-left rectangle passes | `skirmish_deficient_start_uses_8x8_find_nearby_passability_rect` | Do not accept a single passable tile as sufficient |
| The helper scans candidate rings from radius `0`, collects up to `24`, stops after the first direct-candidate ring, and chooses by frame-counter modulo when the reference is the invalid sentinel. | `0x0056DD09..0x0056E5B3`; selection `0x0056E5BF..0x0056E6ED`; static invalid sentinel reads zero | no equivalent in Skirmish; miner test helper is partial and `PathGrid`-based | future shared nearby-passable helper; `src/sim/miner/miner_dock_sequence.rs` is a cautionary partial precedent | With several valid ring-1 starts, the selected fallback changes according to `g_CurrentFrameCounter % count`, not first-passable or sorted distance | `skirmish_deficient_start_find_nearby_uses_first_direct_ring_modulo_selection` | Do not reuse a first-passable spiral for Skirmish start fallback |
| No-candidate return is an invalid sentinel; gather skips append and retries without a gather-level attempt cap. | helper `0x0056E79A..0x0056E7B3`; caller `0x006885BA..0x00688643` | missing; Rust currently avoids generation entirely | `src/app_skirmish.rs`, engine safety policy around startup generation | First invalid helper result does not fail launch if a later random seed succeeds | `skirmish_deficient_start_retries_after_invalid_find_nearby_result` | Do not add a small silent retry cap that leaves active slots without starts |

### Negative Facts / Do Not Do

- Do not reuse MCV fallback `0x00688ED0` ordering for deficient start generation; this caller uses `0x0056DC20` ring collection. Active in YR: Yes/conditional. Evidence: caller `0x006885B5`.
- Do not treat the `8,8` arguments as search radius or center clearance; they are top-left rectangle dimensions for `CheckPassability`. Active in YR: Yes/conditional. Evidence: `0x00688587`, `0x00688591`, `0x0056E7C0`.
- Do not enable final `CheckOccupancy` for this start-fallback call. Active in YR: Yes/conditional. Evidence: final argument `0` and skipped branches in `0x0056DC20`.
- Do not zone-lock the generated start to the random seed's zone; required zone id is `-1`. Active in YR: Yes/conditional. Evidence: caller push `-1` at `0x00688598`.
- Do not use a deterministic first-passable ring result when multiple direct candidates exist; native uses frame-counter modulo when reference equals invalid. Active in YR: Yes/conditional. Evidence: `0x0056E6A8..0x0056E6ED`.

### Stale Docs / Follow-up Docs

- [docs/research/skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md): replace vague "8x8 clearance" with "`Gather_Start_Positions` deficient fallback calls `FootClass__Find_Nearby_Passable_Cell` with an `8x8` top-left `CellRect__CheckPassability` rectangle, required zone `-1`, bridge structural filter allowed, and final `CheckOccupancy` disabled; the helper scans rings from radius `0`, collects the first direct-candidate ring up to 24 candidates, and returns the invalid sentinel when none are found."
- `docs/research/skirmish-ui/SKIRMISH_GATHER_START_POSITIONS_DEFICIENT_WAYPOINT_FALLBACK_00688380_GHIDRA_REPORT.md`: keep the fallback conclusion, but replace "8x8 passability rectangle" with "`8,8` are `CheckPassability` width/height for a top-left candidate rectangle; `CheckOccupancy` is not enabled by this caller."
- `docs/research/FIND_NEARBY_PASSABLE_CELL_GHIDRA_REPORT.md`: replace the misleading `param_13` label "`Reject bridge cells`" with "`allow structural bridge cells`: zero rejects candidates with `CellClass+0x140 & 0x100`; nonzero allows them past this separate `0x0056DC20` filter."

## 10. Remaining Uncertainty

- Exact names/initialization sources for `0x0087F7E8+0xF4/+0xF8` remain out of scope; their use and `32` cap are verified.
- Exact terrain/overlay/occupation semantics inside `CellClass__CheckCellPassability` are delegated to the sibling CellRect/validator reports; this report only claims the start-fallback caller flags and `8x8` rectangle contract.
- Runtime confirmation of pathological no-passable maps spinning forever would need a fixture/debugger observation; static gather evidence shows no local attempt cap.

## Sources

- Ghidra read-only decompile: `FootClass__Find_Nearby_Passable_Cell @ 0x0056DC20`
- Ghidra read-only disassembly: `0x0056DC20..0x0056E7B3`
- Ghidra read-only decompile: `ScenarioClass__Gather_Start_Positions @ 0x00688380`
- Ghidra assembly context: `0x00688572..0x006885B5`
- Ghidra read-only decompile: `CellRect__CheckPassability @ 0x0056E7C0`
- Ghidra read-only decompile: `CellRect__CheckOccupancy @ 0x00586780`
- Ghidra read-only decompile: `ScenarioClass__Full_Init @ 0x00686B20`, `ScenarioClass__AssignStartingPoints @ 0x005EE9D0`
- Ghidra assembly context: Battle-style selected `+0x80` target `0x005D6BE0..0x005D6C63`
- Ghidra xrefs to `0x0056DC20`
- Prior docs referenced: `SKIRMISH_GATHER_START_POSITIONS_DEFICIENT_WAYPOINT_FALLBACK_00688380_GHIDRA_REPORT.md`, [CELLRECT_CHECKPASSABILITY_0056E7C0_FULL_ARG_DECODE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_CHECKPASSABILITY_0056E7C0_FULL_ARG_DECODE_GHIDRA_REPORT.md), [CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLRECT_PASSABILITY_OCCUPANCY_VALIDATORS_GHIDRA_REPORT.md), `FIND_NEARBY_PASSABLE_CELL_GHIDRA_REPORT.md`
- Rust scan: `src/app_skirmish.rs`, `src/sim/miner/miner_dock_sequence.rs`, `src/sim/pathfinding/core.rs`, `src/map/resolved_terrain.rs`
