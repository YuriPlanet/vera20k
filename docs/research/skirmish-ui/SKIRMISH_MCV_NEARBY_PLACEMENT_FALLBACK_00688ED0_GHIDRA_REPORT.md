# Skirmish MCV Nearby Placement Fallback 00688ED0 - Ghidra Research Report

**Address(es):** `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0` (formerly `FUN_00688ED0`), caller `MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030` (formerly `FUN_005D7030`)
**Investigation Mode:** exhaustive-slice plus live audit
**Last audited:** 2026-08-18 against active `gamemd.exe` SHA-256 `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`
**Audit verdict:** **CORRECTED**. The search-order reconstruction remains valid. The previous report did not establish where its clamp globals came from, and its Rust status was stale. Live Ghidra proves that native uses a `Size`-derived full cell-array rectangle for clamping and a separate `LocalSize`-derived diamond for acceptance. Current Rust incorrectly uses raw `LocalSize` as both an axis-aligned containment box and the clamp rectangle.
**Claimed Scope:** Exact startup MCV nearby placement fallback used by the standard selected Skirmish MCV callback after exact base-cell `Place` fails.
**Non-Scope:** Shell UI, start assignment policy, deficient waypoint generation, non-MCV extra unit placement, custom Siege/Unholy callbacks, broader pathfinding, and full `Place`/`Unlimbo` internals beyond the acceptance boundary needed here.
**Confidence:** High for standard selected-mode Battle-style MCV fallback search order, both bounds systems, authored-start reachability, delete boundary, and YR liveness; Medium for complete internal object `Unlimbo` passability semantics because those are delegated outside this slice.
**Active in YR:** Yes, conditional on standard selected MPModes with `Bases=yes` and an MCV exact placement failure.

## 0. Working Notes

Target question: What exact nearby-cell search does `FUN_00688ED0` perform for the standard selected Skirmish MCV after direct house base-cell placement fails?

Non-goals: Do not rediscover Choose Map/UI, start assignment UX, random-map sentinel setup, deficient waypoint generation internals, generic pathfinding, or non-startup object placement except callee contracts needed for this fallback.

Evidence needed to mark COMPLETE: `FUN_005D7030` caller evidence proving standard selected-mode liveness and argument order; `FUN_00688ED0` decompile evidence for radius bounds, direction order, jitter order, playfield/object gates, and return value; vtable/function evidence for MCV object type and `Place` boundary; Rust scan for affected surfaces and test handoff.

Stop conditions: Stop once standard selected Battle-style MCV fallback semantics are reconstructed. Defer full `ObjectClass__Unlimbo`/`TechnoClass__Unlimbo` passability details and non-standard callbacks.

## 1. Overview

The standard selected Skirmish MCV callback first tries to place the new MCV exactly at the house base cell. If that fails, it calls `FUN_00688ED0(mcv, base_cell, 1)`. The `1` is only the starting radius: the helper searches outward from radius `1` through `31` inclusive and returns success on the first candidate whose generic object gate and object `Place` both accept.

If every candidate fails, `FUN_00688ED0` returns `0`; `FUN_005D7030` then deletes the newly constructed MCV and returns failure. This path is live in normal YR selected Battle-style Skirmish because `ScenarioClass__Post_Map_Init` calls selected MPModes setup and then `FUN_005D6D80`, whose standard callback reaches `0x005D7030`.

The assigned waypoint is not converted into a different coordinate frame or collapsed before this point. Retail carries the authored cell through start gathering, stores it at `HouseClass+0x5490`, and tries that same cell. The important correction is what happens at placement: native does **not** compare the cell to an axis-aligned `[Map] LocalSize` box. It first applies the elevation-aware isometric diamond predicate. Only outward-search probes are clamped, and that clamp is the separate full canonical cell-array rectangle derived from `[Map] Size`. (Active `gamemd.exe`; `decompile_function(address="0x0068BDC0", program="gamemd.exe")`, `decompile_function(address="0x0050E000", program="gamemd.exe")`, `decompile_function(address="0x00688ED0", program="gamemd.exe", timeout=60)`.)

## 2. Key Offsets And Functions

| Item | Meaning in this slice | Evidence | Active in YR |
|---|---|---|---|
| `MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030` | standard selected-mode MCV/base-unit callback | `decompile_function(address="0x005D7030", program="gamemd.exe")`; RTTI/vtable reads including Battle `0x007EE184 + 0xC8` | Yes |
| `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0` | exact retry plus nearby object-placement helper | `decompile_function(address="0x00688ED0", program="gamemd.exe", timeout=60)`; caller at `0x005D70BF` | Yes |
| `House+0x5490` / `House+0x5494` | primary/override base cells passed to fallback accessor | `FUN_0050DEF0`, `FUN_0050DF30` | Yes |
| `UnitClass` vtable `+0xD8` | concrete startup MCV `Place` target, `UnitClass__Unlimbo @ 0x00737BA0` | `read_memory 0x007F5D48 -> 0x00737BA0`; decompile `0x00737BA0` | Yes |
| vtable `+0x2C` | abstract type / `What_Am_I` used by helper object prefilter | `UnitClass__What_Am_I @ 0x00746E20` returns `1`; `InfantryClass__What_Am_I @ 0x00523340` returns `0xF` | Yes |
| `g_nMapLocalSizeLeft/Top/Width/Height @ 0x0087F8E4..0x0087F8F0` | clipped/normalized `LocalSize` parameters for the playable diamond | `MapClass__Set_Clipped_LocalSize @ 0x00567230`; `MapClass__Is_Cell_In_Playfield @ 0x00578460` | Yes |
| `g_nMapCellArrayBoundsLeft/Top/Width/Height @ 0x0087F90C..0x0087F918` | separate full canonical cell-array clamp rectangle | `MapClass__Resize @ 0x00565C10`, assembly `0x0056631C..0x00566338`; helper clamp block | Yes |

## 3. Core Logic

### 3.1 Standard selected-mode caller and argument order

`FUN_005D7030` is live for standard selected Skirmish MPModes through `ScenarioClass__Post_Map_Init @ 0x00686890`: when `DAT_00A8B23C != null`, the engine calls selected mode vtable `+0x84`, then `FUN_005D6D80`; `FUN_005D6D80` calls the selected mode `+0xC8` callback for each non-special house. `FUN_005D7030` has data xrefs from standard MPModes vtables and is the callback for this branch. Active in YR: Yes. Evidence: decompile `0x00686890`, `0x005D6D80`, `0x005D7030`; xrefs to `0x005D7030` from `0x007EE24C`, `0x007EE344`, `0x007EE4EC`, `0x007EE5D4`, `0x007EE6BC`, `0x007EEE28`.

The fallback call's concrete calling sequence is:

1. `LEA EDX,[ESP+0x18]` prepares a local cell buffer.
2. `PUSH 0x1` leaves radius `1` on the stack for `FUN_00688ED0`.
3. `PUSH EDX; MOV ECX,EDI; CALL 0x0050DEF0` writes/returns the house base cell pointer.
4. `MOV EDX,EAX; MOV ECX,ESI; CALL 0x00688ED0`.

So the effective call is `FUN_00688ED0(candidate_object = MCV in ECX, base_cell = EDX, start_radius = 1 on stack)`. Active in YR: Yes. Evidence: assembly context `0x005D70AD..0x005D70BF`; decompile signature `undefined4 __fastcall FUN_00688ED0(int *param_1, short *param_2, int param_3)`.

If the fallback returns nonzero, `FUN_005D7030` returns success. If it returns zero, the caller deletes the MCV through vtable `+0x20` with argument `1` and returns failure. Active in YR: Yes. Evidence: assembly context `0x005D70C4..0x005D70D8`.

### 3.2 Initial exact-cell retry inside the helper

Before doing the outward search, `FUN_00688ED0` retries the passed cell if `MapClass__Is_Cell_In_Playfield(cell, 1)` accepts it. It then gets the cell class and calls `CellClass__Find_Nearest_Object` on the cell object lists. The candidate proceeds only if there is no nearest object, or if the nearest object reports `What_Am_I == 0xF` and the candidate object also reports `What_Am_I == 0xF`. Active in YR: Yes. Evidence: decompile `0x00688ED0`; assembly context `0x00688EDD..0x00688F39`.

For a startup MCV, the candidate object is `UnitClass`: `UnitClass__What_Am_I @ 0x00746E20` returns `1`, not `0xF`. Therefore the infantry-compatible exception does not make other units or infantry acceptable for MCV prefiltering; an MCV candidate only passes this prefilter when `Find_Nearest_Object` returns null. Active in YR: Yes. Evidence: `UnitClass` vtable `+0x2C` points to `0x00746E20`, decompile returns `1`; `InfantryClass__What_Am_I @ 0x00523340` returns `0xF`; `FUN_00688ED0` compares both calls to `0xF`.

When the exact cell is attempted, the coordinate is cell-centered: `x = cell_x * 0x100 + 0x80`, `y = cell_y * 0x100 + 0x80`, `z = CellClass__GetGroundHeight(coord)`. The object vtable `+0xD8` is then called with second argument `0`. For `UnitClass`, vtable `+0xD8` resolves to `UnitClass__Unlimbo @ 0x00737BA0`, which delegates through `FootClass__Unlimbo`, `TechnoClass__Unlimbo`, and `ObjectClass__Unlimbo @ 0x005F4EC0`. The prior report's `ObjectClass__Reveal` name for `0x005F4EC0` was wrong/stale. Active in YR: Yes. Evidence: `decompile_function(address="0x00688ED0", program="gamemd.exe", timeout=60)`; `read_memory(address="0x007F5D48", length=4, program="gamemd.exe")`; `decompile_function` at `0x00737BA0`, `0x004D7170`, `0x006F6CA0`, and `0x005F4EC0`.

### 3.3 Radius bounds and direction order

If exact placement fails or is skipped, the helper loops while `param_3 <= 0x1F`. Because the standard caller passes `1`, the startup MCV fallback tries radii `1..31` inclusive. After radius `31` fails, `param_3` increments to `32`; the next loop header returns `0`. Active in YR: Yes. Evidence: decompile `0x00688ED0` has `if (0x1f < param_3) return 0;` before candidate generation, and `param_3 = param_3 + 1` after two passes.

Each radius starts at `Random__RandomRanged(0,7)` and then advances one direction at a time modulo 8 until eight directions have been tested. Direction mapping:

| Direction | Cell before clamp |
|---:|---|
| 0 | `(x, y-r)` |
| 1 | `(x+r, y-r)` |
| 2 | `(x+r, y)` |
| 3 | `(x+r, y+r)` |
| 4 | `(x, y+r)` |
| 5 | `(x-r, y+r)` |
| 6 | `(x-r, y)` |
| 7 | `(x-r, y-r)` |

The order is randomized only by the starting direction; after that it is clockwise by incrementing direction number and wrapping from `7` to `0`. Active in YR: Yes. Evidence: `Random__RandomRanged(0,7)` at `0x00688FDD`; switch in decompile `0x00688ED0`; direction increment/wrap after each candidate.

Every compass candidate is clamped before testing to `x in [g_nMapCellArrayBoundsLeft, left + width - 1]` and `y in [g_nMapCellArrayBoundsTop, top + height - 1]`. These fields are **not** `LocalSize`; their initializer is proven below. Active in YR: Yes. Evidence: `decompile_function(address="0x00688ED0", program="gamemd.exe", timeout=60)` and `disassemble_function(address="0x00688ED0", program="gamemd.exe")`, especially `0x00688FFA..0x006890A1`.

### 3.4 The two native bounds systems

Native uses two deliberately different boundaries in this helper.

#### 3.4.1 `LocalSize` defines an isometric diamond

`MapClass+0xFC..+0x108` holds `LocalSize.left`, `top`, `width`, and `height`. Normal scenario loading reads `[Map] LocalSize`, then `MapClass__Set_Clipped_LocalSize @ 0x00567230` clips it against `[Map] Size`, forces `left/top >= 2`, caps `width <= Size.width-left-2`, and caps `height <= Size.height-top-6`. A trigger-action path can replace those four values later without changing the full cell-array clamp. (Active `gamemd.exe`; `decompile_function(address="0x00686B20", program="gamemd.exe")`, `decompile_function(address="0x00654490", program="gamemd.exe")`, `decompile_function(address="0x00567230", program="gamemd.exe")`, `disassemble_function(address="0x00567230", program="gamemd.exe")`, `decompile_function(address="0x006E21E0", program="gamemd.exe")`.)

For cell `(x,y)`, let `W = Size.width`, `L/T/LW/LH` be those normalized `LocalSize` fields, `s=x+y`, `d=x-y`, and `h` the signed cell-level/slope adjustment used when the caller passes flag `1`. `MapClass__Is_Cell_In_Playfield @ 0x00578460` accepts exactly when:

```text
W + 2*T + h             <  s
s                       <= W + 2 + 2*(T + LH) + h
d                       <  2*(L + LW) - W
-d                      <  W - 2*L
```

Both the helper's exact retry and every outward candidate call this predicate with flag `1`. Only the two sum limits move with elevation; the difference limits remain unchanged. (Active `gamemd.exe`; `decompile_function(address="0x00578460", program="gamemd.exe")`, `disassemble_function(address="0x00578460", program="gamemd.exe")`, exact call at `0x00688EDB`, expansion call at `0x006891BA`.)

#### 3.4.2 The outward-search clamp is the full canonical cell array

`MapClass__Resize @ 0x00565C10` writes the separate rectangle at `MapClass+0x124..+0x130`:

```text
left   = 1
top    = 1
width  = Size.width + Size.height - 1
height = Size.width + Size.height - 1
```

The helper therefore clamps both axes to `[1, Size.width + Size.height - 1]`, then applies the diamond predicate above. It does not clamp to `LocalSize.left..left+width-1` / `top..top+height-1`. (Active `gamemd.exe`; `decompile_function(address="0x00565C10", program="gamemd.exe")`, `disassemble_function(address="0x00565C10", program="gamemd.exe")`, writes at `0x0056631C..0x00566338`; helper reads at `0x00688FFA..0x00689021`.)

### 3.5 Two passes per radius and jitter footprint

For each radius there are two complete eight-direction passes:

- pass `0`: test the clamped compass candidate directly.
- pass `1`: apply randomized jitter to the compass candidate, then test.

The direction index is not re-randomized between passes. After eight increments it wraps back to the original random start direction, so the jitter pass repeats the same direction order. Active in YR: Yes. Evidence: decompile loop variables `iStack_54` and `iStack_38`; `iStack_54` increments after eight directions and breaks only when it becomes greater than `1`.

Jitter is independent per axis:

- draw offset magnitude with `Random(0,1)`;
- draw sign selector with `Random(0,99)`;
- if selector `< 0x32` (`50` decimal), add the offset and clamp high;
- otherwise subtract the offset and clamp low.

This is done once for X and once for Y, so a jitter candidate can equal the original compass candidate if either offset is `0`, can move inward, can move outward, or can move diagonally. Active in YR: Yes. Evidence: random calls at `0x006890E9`, `0x00689102`, `0x00689138`, `0x0068914E`; compare to `0x32` at `0x00689107` and `0x00689153`.

The expansion phase marks a clamped/jittered candidate equal to the original base cell and skips it after the diamond call, before the nearest-object/Unlimbo gates. This matters near map edges because clamping can collapse a spoke back to the original cell. It does not remove the helper's initial exact-cell retry, and there is no general duplicate suppression: zero jitter or edge clamping can retry other already-tested cells. Active in YR: Yes. Evidence: equality/flag at `0x00689196..0x006891A6`, diamond call at `0x006891BA`, flag test at `0x006891C7..0x006891CF` in `disassemble_function(address="0x00688ED0", program="gamemd.exe")`.

Because radius increments beyond the caller's starting radius, `FUN_00688ED0(mcv, cell, 1)` can place far beyond one cell. The non-jitter compass footprint reaches Chebyshev radius `31`; jitter can attempt cells one step inward/outward from the clamped compass candidate, subject to map clamps and original-cell skip. Active in YR: Yes. Evidence: `param_3` loop through `0x1F`; jitter offset `Random(0,1)`.

### 3.6 Candidate acceptance and return boundary

Every expansion candidate is processed in this order:

1. `MapClass__Is_Cell_In_Playfield(candidate, 1)`.
2. reject if it was marked equal to the original cell.
3. `CellClass__Find_Nearest_Object` gate: null nearest object, or both nearest and candidate are `What_Am_I == 0xF`.
4. centered lepton coordinate plus ground height.
5. object vtable `+0xD8` `Place`/`Unlimbo` with second argument `0`.

The helper returns `1` immediately on the first successful `Place`, returns `0` only after exhausting all radii/passes/directions, and does not delete the object itself. Deletion belongs to the caller `FUN_005D7030`. Active in YR: Yes. Evidence: decompile `0x00688ED0`; assembly contexts `0x006891F5..0x00689218` for object gate, `0x0068921A..0x00689283` for centered coordinate and `Place`, `0x005D70C4..0x005D70D8` for caller delete.

The native standard MCV path can therefore issue two attempts at the same effective start cell: the caller's direct virtual `Unlimbo`, then the helper's own exact-cell retry before radius expansion. Static evidence proves both calls; whether any mutable placement side effect makes the second attempt observably different after the first returns false remains UNCHECKED.

### 3.7 NearOreF reproduction

The inspected `nearoref.map` fixture (SHA-256 `E976C159DABAA9C4D95D6EE59863C01D8440F94F42941D008DB6E121DF5F4622`) has `Size=0,0,80,58`, `LocalSize=2,4,76,48`, and starts:

```text
(38,63) (53,48) (71,32) (99,32)
(39,106) (68,107) (85,90) (100,75)
```

All eight satisfy the native diamond. Current Rust instead constructs the axis-aligned box `x=2..77, y=4..51`, so only `(53,48)` and `(71,32)` pass unchanged. Directly clamping the six rejected starts to that wrong box produces `(38,51)`, `(77,32)`, `(39,51)`, `(68,51)`, `(77,51)`, `(77,51)`: the exact edge/corner collapse pattern behind the reported pile-up. The actual Rust fallback probes around each origin and occupancy nudges collisions, so final cells depend on RNG and prior spawns, but the false rejection and edge concentration do not. Fixture evidence: `<local>/Documents/cncnet-yr-client-package/package/Maps/Yuri's Revenge/nearoref.map`, `[Map]` and `[Waypoints]`; Rust bounds and fallback cited in section 5.

## 4. INI Keys

| INI key | Stock YR value | Role in this slice | Evidence | Active in YR |
|---|---|---|---|---|
| `[MultiplayerDialogSettings] Bases` | `yes` in `rulesmd.ini` | if false, `FUN_005D7030` returns success without creating an MCV, so fallback is not entered | `0x005D7030` entry branch; `ini/rulesmd.ini:3032` | Yes |
| `[General] BaseUnit` | `AMCV,SMCV,PCV` in `rulesmd.ini` | source vector for constructed startup MCV before exact/fallback placement | `0x005D7030` calls `FUN_00505310(Rules+0xB20)`; `ini/rulesmd.ini:390` | Yes |
| `[Map] Size` | map-specific | `width` is the diamond base; `width+height-1` initializes each full-array clamp span | `ScenarioClass__Full_Init`, `MapClass__Resize @ 0x00565C10` | Yes |
| `[Map] LocalSize` | map-specific | clipped/normalized parameters for `MapClass__Is_Cell_In_Playfield`; not a Cartesian clamp rectangle | `0x006874FD..0x00687546`, `MapClass__Set_Clipped_LocalSize @ 0x00567230`, predicate `0x00578460` | Yes |

No INI key controls the `1..31` fallback radius limit, direction order, two-pass count, or jitter constants in this helper; those are binary constants.

## 5. Current Rust Implementation Status

The previous report's conclusion that Rust had no nearby fallback is obsolete. The current production path is [`src/app/loading/init.rs`](../../../src/app/loading/init.rs) into [`src/sim/scenario_bootstrap.rs`](../../../src/sim/scenario_bootstrap.rs): `apply_resolved_skirmish_launch_session` passes each distinct assigned waypoint to `place_starting_mcv`, which calls `place_starting_object_near_base`.

The authored-start and assignment stages do not create this pile-up. Complete authored waypoints are copied unchanged, and the Battle assignment marks selected indices occupied before assigning the next house (`scenario_bootstrap.rs:90..100`, `276..304`, `344..401`, `781..839`). The divergence begins at the physical placement gate.

Current parity status:

| Mechanism | Rust status | Evidence / consequence |
|---|---|---|
| radii `1..31`, one random start direction per radius, clockwise wrap | implemented | `scenario_bootstrap.rs:1147..1214` |
| direct pass, then X magnitude/sign and Y magnitude/sign jitter draws | implemented | `scenario_bootstrap.rs:1216..1227` |
| expansion candidate equal to original is skipped | implemented | `scenario_bootstrap.rs:1229..1239` |
| exact start-cell acceptance | **wrong** | `NativeStartBounds::contains` applies raw `LocalSize` as an axis-aligned rectangle (`scenario_bootstrap.rs:38..73`, `1491..1501`) instead of calling the already-implemented diamond in [`src/sim/cell_rect.rs`](../../../src/sim/cell_rect.rs) `cell_is_in_playfield` (`881..948`) |
| outward-search clamp | **wrong** | the same `LocalSize` rectangle clamps every probe (`scenario_bootstrap.rs:1211..1227`); native uses `[1, Size.width+Size.height-1]` on both axes, then the diamond |
| `LocalSize` initialization | partial | [`src/sim/runtime.rs`](../../../src/sim/runtime.rs) `365..373` and session fields keep the header values verbatim; native first clips/normalizes them via `0x00567230`. NearOreF already satisfies the native margins, so this secondary gap is not needed to trigger its pile-up |
| caller exact attempt plus helper exact retry | partial | Rust performs one exact precheck/spawn at `scenario_bootstrap.rs:1200..1203`, then enters rings |
| nearest-object compatibility gate | approximate | Rust occupancy permits compatible infantry stacks, but equivalence to `CellClass__Find_Nearest_Object` is not established (`scenario_bootstrap.rs:1502..1518`) |
| final fallible virtual `Unlimbo` boundary | partial | `spawn_object` reaches Rust `unlimbo`, but startup prechecks and unconditional placement evidence are not proven equivalent to native `ObjectClass__Unlimbo` rejection semantics |

The existing test `skirmish_mcv_start_uses_radius_fallback_when_start_cell_blocked` in [`src/app/frontend/skirmish.rs`](../../../src/app/frontend/skirmish.rs) covers only an occupied exact cell followed by immediate success at the first radius-1 direction. It does not exercise a real isometric map, the diamond/clamp split, direction wrap, jitter, radius expansion, total failure, or the native double exact attempt.

## 6. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| standard selected-mode liveness to `0x005D7030` | verified | `0x00686890`, `0x005D6D80`, xrefs to `0x005D7030` | none for Battle-style selected modes |
| `0x005D7030` fallback call argument order | verified | assembly context `0x005D70AD..0x005D70BF` | none |
| caller delete boundary | verified | assembly context `0x005D70C4..0x005D70D8` | none |
| `FUN_00688ED0` exact retry | verified | decompile `0x00688ED0`; assembly context `0x00688EDD..0x00688F9A` | none |
| radius bound and expansion | verified | decompile `0x00688ED0` loop/header | none |
| direction order | verified | decompile switch and `Random(0,7)` at `0x00688FDD` | none |
| jitter order and constants | verified | random calls at `0x006890E9`, `0x00689102`, `0x00689138`, `0x0068914E` | none |
| `LocalSize` load, clipping, normalization | verified | `0x00686B20`, `0x00654490`, `0x00567230` | savegame-specific writers not exhaustively inventoried; ordinary new skirmish is closed |
| elevation-aware diamond acceptance | verified | decompile/disassembly `0x00578460`; both calls in `0x00688ED0` pass `1` | none for this helper |
| full cell-array clamp initializer | verified | `MapClass__Resize @ 0x00565C10`, writes `0x0056631C..0x00566338` | none for ordinary new skirmish |
| authored waypoint reaches house start and placement helper unchanged | verified | `0x0068BDC0`, `0x00688380`, `0x0050E000`, `0x0050DEF0/30`, `0x005D7030` | none for complete stock Battle starts |
| prefilter `0xF` meaning for MCV | verified | `UnitClass__What_Am_I @ 0x00746E20`; `InfantryClass__What_Am_I @ 0x00523340` | none for MCV |
| full `Place`/`Unlimbo` passability semantics | deferred | `0x00737BA0`, `0x004D7170`, `0x006F6CA0`, `0x005F4EC0` touched | separate object-placement investigation |
| custom mode callbacks | deferred | xrefs show other callers | out of scope |
| current Rust delta | verified | `src/sim/scenario_bootstrap.rs`, `src/sim/cell_rect.rs`, `src/sim/runtime.rs`, `src/sim/world/world_spawn.rs` | fix the bounds split; preserve implemented search ordering |

## 7. Open Questions - Final State

- `[RESOLVED] OQ-01 - Is `0x00688ED0` active in standard YR selected Skirmish? -> Yes, through `Post_Map_Init -> FUN_005D6D80 -> standard mode +0xC8 -> 0x005D7030` when direct MCV placement fails.` (evidence: `0x00686890`, `0x005D6D80`, `0x005D7030`)
- `[RESOLVED] OQ-02 - What arguments does standard MCV caller pass? -> ECX=MCV object, EDX=house base-cell pointer returned by `0x0050DEF0`, stack radius=1.` (evidence: `0x005D70AD..0x005D70BF`)
- `[RESOLVED] OQ-03 - Is radius 1 exclusive to one-cell fallback? -> No; it is the starting radius and the helper expands through radius 31 inclusive.` (evidence: `0x00688ED0` loop header and increment)
- `[RESOLVED] OQ-04 - What direction order is used? -> random start in 0..7, then increment/wrap through all eight compass directions.` (evidence: `0x00688FDD`, switch in `0x00688ED0`)
- `[RESOLVED] OQ-05 - Does the helper do more than one pass per radius? -> Yes, direct compass pass plus jitter pass; both cover eight directions.` (evidence: `iStack_54`/`iStack_38` loop in `0x00688ED0`)
- `[RESOLVED] OQ-06 - What are jitter constants? -> X and Y each use offset `Random(0,1)` and sign selector `Random(0,99) < 50` for plus, else minus.` (evidence: random calls `0x006890E9`, `0x00689102`, `0x00689138`, `0x0068914E`)
- `[RESOLVED] OQ-07 - Can the helper retry the original base cell? -> Yes once at helper entry. Only the outward expansion phase skips candidates that collapse back to the original; the diamond call still occurs before that skip.` (evidence: exact path `0x00688EDB..0x00688FB4`; expansion equality/diamond/skip `0x00689196..0x006891CF`)
- `[RESOLVED] OQ-08 - Does nearest-object gate allow another MCV/unit in the candidate cell? -> No for MCV prefilter; the same-type exception is specifically both objects `What_Am_I == 0xF`, while UnitClass returns `1`.` (evidence: `0x00688ED0`, `0x00746E20`, `0x00523340`)
- `[RESOLVED] OQ-09 - Who deletes the MCV after total failure? -> Caller `0x005D7030`, not `0x00688ED0`.` (evidence: `0x005D70C4..0x005D70D8`)
- `[RESOLVED] OQ-10 - Does current Rust have this nearby fallback? -> Yes, including the verified radius/direction/jitter skeleton, but it applies the wrong axis-aligned `LocalSize` bounds for both containment and clamping.` (evidence: `src/sim/scenario_bootstrap.rs:1147..1250`, `1491..1529`)
- `[DEFERRED] OQ-11 - Exact internal passability checks inside `ObjectClass__Unlimbo`/`TechnoClass__Unlimbo`.` (category: out-of-scope; reason: the fallback helper delegates final acceptance to virtual `Unlimbo`; next-step-if-pursued: object placement/Unlimbo parity slice)
- `[DEFERRED] OQ-12 - Non-standard Siege/Unholy callback differences.` (category: out-of-scope; reason: this slot is standard selected Battle-style MCV fallback only; next-step-if-pursued: decode xref callers `0x005CAB9A`, `0x005CB506`, `0x005D73C4`)
- `[DEFERRED] OQ-13 - Runtime RNG seed transcript for a named blocked-start scenario.` (category: needs-runtime-debugger; reason: static call order/ranges are verified but an exact seeded transcript needs runtime logging; next-step-if-pursued: breakpoint `0x00688ED0` and log `Random__RandomRanged`)

## 8. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Test | Risk / do-not-do |
|---|---|---|---|---|---|---|---|
| Exact and expansion candidates use the `LocalSize`-derived elevation-aware diamond | `MapClass__Is_Cell_In_Playfield @ 0x00578460`; calls `0x00688EDB`, `0x006891BA` | **wrong:** raw `LocalSize` axis-aligned `contains` | `NativeStartBounds`, `starting_object_cell_placeable`, existing `cell_rect::cell_is_in_playfield` | route exact and expansion acceptance through the existing verified diamond with terrain height/slope data | NearOreF's eight authored starts all pass unchanged; none enters fallback merely because it lies outside the raw header box | `nearoref_all_eight_mcv_starts_pass_native_diamond` | Do not rotate/convert waypoint cells or use `LocalSize` as Cartesian min/max |
| Expansion probes clamp to the full canonical cell-array rectangle derived from `Size`, independently of the diamond | `MapClass__Resize @ 0x00565C10`, `0x0056631C..0x00566338`; helper `0x00688FFA..0x006890A1` | **wrong:** `NativeStartBounds::clamp` uses `LocalSize` | `NativeStartBounds` / replacement clamp type; map-header `Size` and canonical-grid initialization | maintain a separate clamp with left/top `1`, both spans `Size.width+Size.height-1`, then diamond-test | an off-diamond spoke clamps only to full-array storage limits and is subsequently rejected by the diamond; it never collapses to a `LocalSize` edge | `mcv_fallback_clamp_uses_size_derived_cell_array_not_localsize` | Do not share one rectangle between storage safety and playability |
| Standard selected MCV caller performs direct `Unlimbo`, then helper exact retry, then rings `1..31`, deleting only after total failure | `0x005D7090..0x005D70D8`; helper exact path `0x00688EDB..0x00688FB4` | partial: one exact spawn attempt, then rings | `place_starting_mcv`, `place_starting_object_near_base`, `spawn_object`/Unlimbo boundary | preserve the two native calls if the first failed attempt can mutate relevant state; otherwise document/prove equivalence before intentionally collapsing them | instrumented/fake Unlimbo rejects once and accepts the second exact call, or a proof-backed test establishes collapse equivalence | `mcv_start_retries_exact_inside_helper_before_rings` | Do not state that native never retries the original cell |
| Radius/direction/direct+jitter order is already modeled | `0x00688FCE..0x006892D3` | substantially implemented | `place_starting_object_near_base`, deterministic Scenario RNG | retain current ordering while correcting bounds; add coverage for wrap, jitter, radius expansion, and total failure | seeded candidate mask produces the same first success and RNG cursor as the verified native order | `mcv_fallback_direction_jitter_and_radius_order_match_gamemd` | Do not rewrite the search as a perimeter scan or sorted distance search |
| MCV prefilter requires no nearest object; `0xF/0xF` compatibility is infantry-only | `0x00688ED0`, `0x00746E20`, `0x00523340` | approximate occupancy gate | occupancy/place probe surface | prove or close equivalence to `CellClass__Find_Nearest_Object`, then delegate final legality to normal fallible Unlimbo | occupied MCV candidate is skipped; first empty and otherwise legal candidate is used | `mcv_fallback_skips_nearest_object_before_unlimbo` | Do not allow an MCV merely because the occupant is another unit |

### Related documents requiring a later independent audit

- [`SKIRMISH_SPAWN_PLACEMENT_AFTER_ASSIGNED_START_GHIDRA_REPORT.md`](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_SPAWN_PLACEMENT_AFTER_ASSIGNED_START_GHIDRA_REPORT.md) still describes the clamp globals without their initializer and has stale Rust status. This audit does not mutate it because the `audit` workflow owns one report at a time.
- [`SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md`](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md) should consume this corrected bounds split when it is next audited.

## 9. Negative Facts / Do Not Do

- Do not use raw `[Map] LocalSize` as an axis-aligned cell-coordinate rectangle. Native uses it as parameters to an isometric diamond after clipping/normalization. Active in YR: Yes. Evidence: `MapClass__Set_Clipped_LocalSize @ 0x00567230`, `MapClass__Is_Cell_In_Playfield @ 0x00578460`.
- Do not clamp nearby probes to `LocalSize`. Native clamps to the separate full canonical cell-array rectangle initialized by `MapClass__Resize`, then diamond-tests. Active in YR: Yes. Evidence: `0x0056631C..0x00566338`, `0x00688FFA..0x006891C1`.
- Do not change start assignment to fix this symptom. Complete authored starts reach `HouseClass+0x5490` distinctly; collapse occurs in Rust's later placement bounds. Active in YR: Yes for the stock Battle path. Evidence: `0x0068BDC0`, `0x00688380`, `0x0050E000`, `0x005D7030`.
- Do not implement the standard MCV fallback as only the eight adjacent cells. Active in YR: Yes. Evidence: `FUN_00688ED0` increments radius until `param_3 > 0x1F`.
- Do not scan from a fixed north or row-major order. Active in YR: Yes. Evidence: `Random__RandomRanged(0,7)` chooses the first direction at `0x00688FDD`, then direction increments/wraps.
- Do not skip the jitter pass. Active in YR: Yes. Evidence: second pass guarded by `iStack_54 > 0`; four random calls decide X/Y offset and sign.
- Do not say the original cell is never retried. The helper retries it before radius expansion; only expansion candidates equal to it are skipped after the diamond call. Active in YR: Yes. Evidence: `0x00688EDB..0x00688FB4`, `0x00689196..0x006891CF`.
- Do not treat `0xF` same-type gate as "other units allowed." Active in YR: Yes. Evidence: `UnitClass__What_Am_I` returns `1`, `InfantryClass__What_Am_I` returns `0xF`, and `FUN_00688ED0` compares both nearest and candidate against `0xF`.
- Do not delete the MCV inside the fallback helper in Rust if modeling the helper separately. Active in YR: Yes. Evidence: `FUN_00688ED0` only returns `0/1`; caller `0x005D7030` invokes vtable `+0x20` delete on false.

## 10. Remaining Uncertainty

- Full `ObjectClass__Unlimbo` / `TechnoClass__Unlimbo` placement legality is not exhausted here; this report treats vtable `+0xD8` as the final `Place` acceptance boundary.
- Whether a failed caller-level exact `Unlimbo` can mutate state that makes the helper's second exact attempt observably different remains UNCHECKED.
- Exact runtime RNG transcript for a named map/seed remains unrecorded; static call order and ranges are verified.
- Non-standard MPModes callbacks that also xref `FUN_00688ED0` are outside this slot.
- Exact original Westwood source names are unknown. Ghidra labels added by this audit are descriptive evidence labels, not claims about shipped symbol names.

## Sources and reproducibility

- Active binary: `<ra2-install>/gamemd.exe`, x86 PE image base `0x00400000`, size `5,286,504`, SHA-256 `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`; Ghidra project `testProsjekt`, program `/gamemd.exe`, analysis complete.
- Helper evidence: `get_function_by_address(address="0x00688ED0", program="gamemd.exe")`; `decompile_function(address="0x00688ED0", program="gamemd.exe", timeout=60)`; `disassemble_function(address="0x00688ED0", program="gamemd.exe")`; `get_function_callers(address="0x00688ED0", program="gamemd.exe")`; `get_xrefs_to(address="0x00688ED0", program="gamemd.exe")`.
- Bounds evidence: `decompile_function` and `disassemble_function` with `program="gamemd.exe"` at `0x00578460`, `0x00565C10`, and `0x00567230`; `decompile_function` at `0x00686B20`, `0x00654490`, and `0x006E21E0`; `audit_global` for `0x0087F8E4`, `0x0087F8E8`, `0x0087F8EC`, `0x0087F8F0`, `0x0087F90C`, `0x0087F910`, `0x0087F914`, and `0x0087F918`.
- Startup/caller evidence: `decompile_function` at `0x0068BDC0`, `0x00688380`, `0x0050E000`, `0x0050DEF0`, `0x0050DF30`, `0x00686890`, `0x005D6D80`, and `0x005D7030`; `read_memory(address="0x007EE184", length=208, program="gamemd.exe")` for `MultiplayerBattle` vtable ownership.
- Unit RTTI/vtable evidence: `read_memory(address="0x007F5C6C", length=224, program="gamemd.exe")`, `read_memory(address="0x0080CC68", length=20, program="gamemd.exe")`, `read_memory(address="0x00842D80", length=64, program="gamemd.exe")`; `decompile_function` at `0x00746E20`, `0x00523340`, `0x00737BA0`, `0x004D7170`, `0x006F6CA0`, and `0x005F4EC0`.
- INI: [`ini/rulesmd.ini`](../../../ini/rulesmd.ini), `[MultiplayerDialogSettings] Bases` and `[General] BaseUnit`.
- Rust: [`src/sim/scenario_bootstrap.rs`](../../../src/sim/scenario_bootstrap.rs), [`src/sim/cell_rect.rs`](../../../src/sim/cell_rect.rs), [`src/sim/runtime.rs`](../../../src/sim/runtime.rs), [`src/app/loading/init.rs`](../../../src/app/loading/init.rs), [`src/app/frontend/skirmish.rs`](../../../src/app/frontend/skirmish.rs), and [`src/sim/world/world_spawn.rs`](../../../src/sim/world/world_spawn.rs).

## Ghidra metadata synchronized by this audit

After all read-only workers stopped, the audit dry-ran, applied, and saved these certainty-gated annotations:

- Functions: `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0`, `MapClass__Set_Clipped_LocalSize @ 0x00567230`, `MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030`, `MultiplayerGameMode__Generate_Starting_Units @ 0x005D6D80`, `HouseClass__Set_Starting_Cell @ 0x0050E000`, `HouseClass__Get_Base_Or_Starting_Cell @ 0x0050DEF0`, `HouseClass__Get_Base_Or_Starting_Coord @ 0x0050DF30`, each with an evidence plate comment.
- Globals: the four `g_nMapLocalSize*` fields at `0x0087F8E4..0x0087F8F0` and four `g_nMapCellArrayBounds*` fields at `0x0087F90C..0x0087F918`, typed `int` and plate-commented.
- The shared `0x0087F7E8` primary singleton label was deliberately not changed: this audit proved MapClass field use but did not re-audit the full fused inheritance/object-layout naming question.

## Status

**CORRECTED / COMPLETE** for the ordinary new-game standard selected Skirmish MCV path, its authored-cell handoff, both boundary systems, and nearby-search mechanics. Partial only for generic object `Unlimbo` internals, possible observable effects of the double exact attempt, savegame-only writers, runtime RNG transcripts, and non-standard callbacks, all explicitly outside this slot.
