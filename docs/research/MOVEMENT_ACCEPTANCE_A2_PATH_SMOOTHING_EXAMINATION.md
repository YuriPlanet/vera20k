# A2 "Route choice" — path smoothing examination (read-only)

Binary: `gamemd.exe`, Ghidra project `testProsjekt`, image base `0x00400000`,
10073 functions. Worktree `…/.claude/worktrees/vera20k-yuris-revenge-movement-bc5040`,
branch `feature/vera20k-yuris-revenge-movement-bc5040` @ `eed60afa`.
No files edited, no cargo run.

**Path correction first:** the ledger row and the task both spell the Rust file
`src/sim/movement/path_smooth.rs`. It is at
`src/sim/pathfinding/path_smooth.rs` (tests in `path_smooth_tests.rs` beside it).
Line numbers below refer to the real file.

---

## 0. What the native post-processing chain actually is

`AStar_main_loop @ 0x00429A90`, after `AStar_reconstruct_path @ 0x0042AA90`
(`CALL` at `0x0042A406`), runs **two** unconditional passes:

| | Address | Name (Ghidra) | Call site |
|---|---|---|---|
| Pass 1 outer | `0x0042B210` | `Path_smooth_corners` | `0x0042A415` |
| Pass 1 inner | `0x0042B420` | `Path_smooth_single_segment` | `0x0042B382`, `0x0042B406` |
| Pass 2 outer | `0x0042B7F0` | `Path_optimize_straight_segments` | `0x0042A41E` |
| Pass 2 anchor | `0x0042BCB0` | `Path_Find_Split_Anchor` | from `0x0042B7F0` |
| Pass 2 rewrite | `0x0042BE20` | `Path_Reroute_Straight_Line` | from `0x0042B7F0` |

Both `CALL`s at `0x0042A415` / `0x0042A41E` are `UNCONDITIONAL_CALL` xrefs
(`get_xrefs_to 0x0042B210`, `0x0042B7F0` each return exactly one caller).
Neither pass is gated on TS/fog/SpecialFlags anywhere in the verified chain.

Path-struct shape read by `Path_smooth_corners` (`0x0042B21D..0x0042B228`):
`+0x00` start coord (packed x,y i16), `+0x08` step count, `+0x0C` direction
array (`u32` per step; `8` = tube hop, `0xFFFFFFFE` = delete marker,
`0xFFFFFFFF` = terminator), `+0x14` **parallel per-step z array**, `+0x18`
(pass 2 `param_2[5]`) a second parallel array indexed by split index.

---

## 1. `0x0042B420` — what it does

Signature (from the call site at `0x0042B382` and `RET 0x18`):

```
Path_smooth_single_segment(
    foot,          // param_1 = the mover; arg2 of Path_smooth_corners
    dirs,          // param_2 = &dir_array[seg]
    zs,            // param_3 = &z_array[seg]
    run_len,       // param_4 = length of the anchor-direction run
    zig_len,       // param_5 = length of the following 90-degree run
    &coord)        // param_6 = in/out: walking coordinate
   -> int          // directions consumed; caller advances by it
```

Mechanism, exactly as VERA's `smooth_single_segment` (`path_smooth.rs:162`)
already models it structurally:

* `mid = (dirs[0] + dirs[run_len]) >> 1`, sanity-checked against both endpoints,
  falling back to `0` on the `{1,7}` wrap (`0x0042B43A..0x0042B456`).
* Tube hop (`8`) on either side → walk `run_len + zig_len` and consume
  (`0x0042B45A..0x0042B466` → `0x0042B7A4`).
* `pairs` starts at `min(run_len, zig_len)`; each attempt validates `2*pairs`
  replacement cells stepping in `mid` from a replacement origin
  `local_34 = base + prefix steps of dirs[0]`, `prefix = run_len - pairs`.
* On any rejection: `pairs -= 1`, `local_34 += dir(dirs[0])`, retry
  (`0x0042B665..0x0042B6B9`). On success: `2*pairs` slots overwritten with
  `mid`, return `prefix` (`0x0042B703..0x0042B721`, `STOSD.REP`).
* Step count and endpoint are preserved; only interior cells move.

### The three rejection terms (all at `0x0042B5A5..0x0042B5F5`)

```
0042b5a5  PUSH 0x1 / PUSH 0x0 / PUSH ESI / PUSH EDX / PUSH EDI
0042b5ae  CALL dword ptr [EAX + 0x1ac]     ; T1
0042b5b4  TEST EAX,EAX / JNZ reject
0042b5b8  TEST dword ptr [EDI+0x140],0x40000 ; T2
0042b5c2  JNZ reject
0042b5c4  MOV EAX,[ESP+0x30]               ; foot->+0x21C
0042b5c8  LEA ECX,[ESP+0x10]               ; &local_34  (NOT the candidate cell)
0042b5d3  CALL 0x0056bcd0                  ; T3
0042b5dc  FILD / FMUL [ESP+0x3c] / FCOMP [0x007E1718]
0042b5ef  JZ reject                        ; reject when product >= 1.0
```

**T1 — `Can_Enter_Cell`, vtable slot `+0x1AC`.**
Slot identity proved, not assumed: `UnitClass::Can_Enter_Cell @ 0x0073F0A0` has
exactly one data xref, `0x007F5E1C`; `0x007F5E1C - 0x1AC = 0x007F5C70`, and
`0x007F5C70` is the vtable pointer written by `UnitClass__Constructor`
(`0x0073543A`), `UnitClass__Destructor` (`0x00735794`) and `UnitClass__Load`
(`0x00744521`). So slot `+0x1AC` **is** `Can_Enter_Cell`. Confirmed live from
the same predicate the search itself uses: `AStar_main_loop` calls
`[EDX+0x1AC]` at `0x00429F54`. Arguments here are
`(CellClass* cell, mid_dir, z, 0, 1)` — arg1 is a `CellClass*`, confirmed by
`UnitClass::Can_Enter_Cell`'s prologue reading `[ECX+0x140]` and `[ECX+0x11B]`
off it (`0x0073F0B7`, `0x0073F0CE`).
**The native invariant is that smoothing re-validates with the identical
predicate the A* expansion used.**

**T2 — cell flag `+0x140 & 0x40000`, unconditional.**
Same bit is only *priced* in the search (`AStar_compute_edge_cost 0x004299B0`,
`TEST EDX,0x40000` then `FMUL [0x007E37BC]` = 4.0f) but **hard-rejected** here
and twice in pass 2 (`0x0042BFC0`, `0x0042C0C4`). Program-wide, cell bit
`0x40000` is touched at only seven sites: those four tests plus three
`AND …,0x40000` in `PathfinderClass__UpdateBridgePassability`
(`0x0042AFA5`, `0x0042B035`, `0x0042B05B`).

**T3 — threat, not slope.** `CALL 0x0056BCD0` (Ghidra name
`MapClass__Get_Slope_Cost_At_Cell`), `FMUL` by
`FootClass__Get_Slope_Speed_Factor @ 0x004DC760`, `FCOMP [0x007E1718]`
(= `0x3FF0000000000000` = `1.0`, read verified), reject when `>= 1.0`.

> **Both Ghidra names are mislabels.** Evidence chain:
> * `0x004DC760` returns `*(double*)(foot+0x530)`, forced to `1.0` when
>   `foot+0x5D4` (Team) is non-null and `Team->Type->+0xF2` is set.
> * `foot+0x530` is written only by `FootClass__Unlimbo` from
>   `TechnoTypeClass+0x2F0` (`0x004D72EA FLD [EAX+0x2F0]`,
>   `0x004D72F4 FSTP [ESI+0x530]`).
> * `TechnoTypeClass::ReadINI` fills `+0x2F0` from the key at `0x00844420` =
>   **`ThreatAvoidanceCoefficient`** (`0x00712460..0x0071246D`), in the echo
>   form `ReadDouble(section, key, current)` so an absent key keeps the ctor
>   default.
> * `0x0056BCD0` returns `*(i32*)(foot->+0x21C + 0x59F0 + (x4 + y4*0x82)*4)`
>   where `x4/y4` come from the cell's own stored coord `CellClass+0x24`,
>   divided by 4 with **truncation toward zero** (`(v + ((v>>31)&3)) >> 2`).
> * `foot+0x21C` is `Owner (HouseClass*)` (`docs/research/FOOTCLASS_STRUCT_LAYOUT.md:272`).
> * `HouseClass+0x57E4` is a **130x130 i32 threat grid**:
>   `HouseClass__AI_BuildThreatMap @ 0x00509400` zeroes exactly `0x4204`
>   (= 16900 = 130*130) dwords from `+0x57E4` and then accumulates each hostile
>   Techno's threat value (vtable `+0x2C0`) into a 9-entry 3x3 kernel,
>   clamped at 0. `HouseClass__Adjust_Threat @ 0x004FA2E0` does the same 3x3
>   add/subtract incrementally.
> * `0x59F0 - 0x57E4 = 0x20C = 131*4`, i.e. the `+0x59F0` base is the same grid
>   offset by one border row plus one column — a 1-block margin. So
>   `0x0056BCD0` == `house->ThreatMap[cellY/4 + 1][cellX/4 + 1]`.
>
> **T3 is therefore: "refuse this smoothing when the mover's owner's threat map
> for the containing 4x4 block, scaled by the mover's
> `ThreatAvoidanceCoefficient`, reaches 1.0."** Nothing to do with terrain
> slope. `docs/research/…/MAPCLASS_GET_SLOPE_COST_AT_CELL_PATH_SMOOTHING_GHIDRA_REPORT.md`
> already rated the table semantics "Medium confidence" and deferred it as
> OQ-14; this closes OQ-14. The in-tree comment at `path_smooth.rs:14-16,22-23`
> ("the slope term", "slope terrain is on most retail maps", "needs two native
> helpers") is wrong on identity and therefore on frequency.

### New finding the prior research did not record: T3 samples the wrong cell

T1 and T2 read `EDI`, the `CellClass` of the **moving candidate cursor**
(`local_30` = `[ESP+0x14]`, advanced every iteration at `0x0042B60D..0x0042B633`).
T3 is handed `LEA ECX,[ESP+0x10]` = `local_34`, the **replacement-run origin**
— the last *un*-replaced cell. `[ESP+0x10]` is never written inside the inner
loop (`0x0042B591..0x0042B655` writes only `[ESP+0x1c]`, `[ESP+0x14]`,
`[ESP+0x28]`, `[ESP+0x2a]`); it changes only on the outer retry at
`0x0042B6AF`. So T3 is loop-invariant per attempt: it tests the junction cell,
once per `pairs` value, not each of the `2*pairs` replacement cells. This looks
like a native bug, but it is the behavior and a port must reproduce it.
**Established by disassembly** (instruction stream, not annotation).

### The per-step z and the bridge lift

Initial z for an attempt: `ESI = zs[(pairs - 2*pairs) + run_len] = zs[prefix]`
(`0x0042B569..0x0042B581`). After every candidate
(`0x0042B635..0x0042B651`):

```
z_new = (i8)cell->+0x11B
if (z_prev - z_new == 4 && (cell->+0x140 & 0x100)) z = z_new + 4 else z = z_new
```

`+0x100` is the structural bridge bit and `+0x11B` the cell height
(`docs/research/core-services-map/cell-map.md:169`). This z is arg3 of
`Can_Enter_Cell`, and `UnitClass::Can_Enter_Cell` branches on it immediately:
`0x0073F0B1 MOV EBP,0x100` / `0x0073F0BE TEST EBP,EAX` / then
`|arg3 - cell->+0x11B| <= 1` decides deck-vs-under-bridge
(`0x0073F0C2..0x0073F0DF`). **Without the z, T1 cannot even be asked
correctly on or near a bridge.**

---

## 2. Divergence table — pass 1

| # | Native address | Divergence | Trigger | Frequency in ordinary skirmish | Player-visible effect |
|---|---|---|---|---|---|
| P1-a | `0x0042B5AE` (`[vtbl+0x1AC]`), slot proof `0x007F5C70`/`0x007F5E1C` | VERA validates with the `walkable` closure from `movement_path.rs:539` / `:610`, not `Can_Enter_Cell`. Closure = terrain + `entity_blocks` + `entity_block_map` + marker; native = the full occupancy/ally/wall/facing/level predicate, called with `(cell, mid_dir, z, 0, 1)`. Residual after the closure: the **facing argument** (`mid_dir`) and the **z argument** are both absent, plus the wall codes 4/5 arm and the head-on/in-transit arms (I3/I9b). | Any smoothing candidate whose refusal depends on facing or level rather than occupancy. | Pass 1 runs on **every** successful A*; the facing/level-sensitive subset is narrower — bridge approaches, ramps, and cells with a moving ally. | VERA collapses a zigzag onto a cell retail refuses; on a bridge approach the mover can be routed through a cell at the wrong level. |
| P1-b | `0x0042B5B8` | **Not a VERA omission.** `movement_path.rs:563` and `:633` already reject `marker_overlay` cells in the smoothing closure, which is the same hard rejection native applies. Two real deltas remain: the `&& (x, y) != goal` carve-out has no native counterpart, and VERA's rejection is conditional on a marker overlay being supplied (`marker_search` non-empty, `movement_path.rs:743`) whereas native's flag test is unconditional. | Blocked repath with a non-empty overlay. | Chokepoints / factory exits, several times a match. | VERA may smooth *through* a marked cell when it is the goal; retail never does. |
| P1-c | `0x0042B5D3` + `0x0042B5E4` (`>= 1.0`) | T3 (threat) absent entirely. Needs `HouseClass` threat grid + `ThreatAvoidanceCoefficient`. | Mover with a non-zero `ThreatAvoidanceCoefficient` whose *junction* cell's 4x4 block carries threat. | **Stock-narrow.** `TechnoTypeClass+0x2F0` defaults to `0.0` (`TechnoTypeClass__Constructor 0x00710BA6 MOV [ESI+0x2F0],EBX` with `XOR EBX,EBX` at `0x00710B00`; `+0x2F4` likewise), and stock `RULESMD.INI` sets the key on exactly six sections: `[CMON]=1`, `[CMIN]=.65`, `[HORV]=1`, `[HARV]=.65`, `[SMON]=1`, `[SMIN]=.65` — the harvester/miner family. Everything else has product `0 * grid = 0`, never `>= 1.0`. The exception is the Team override (`0x004DC760`: `Team->Type->+0xF2` forces the scalar to `1.0`), which is AI-team-only. So: **every harvester route near enemy units**, dozens of times a match; **never** for an ordinary player-controlled tank. | A retail harvester keeps the outer L of a zigzag when the junction cell sits in a threatened block; VERA's harvester cuts the corner through it and eats more fire. Path length and arrival tick are unchanged (pass 1 preserves step count and endpoint), so this is a route-shape and damage-taken difference, not a timing one. |
| P1-d | `0x0042B569`, `0x0042B635..0x0042B651` | Per-step z array and the `+4` bridge lift absent. VERA carries no z; `smooth_layered_path` instead splits at `MovementLayer` transitions so no replacement crosses a bridge boundary (`path_smooth.rs:321`). Different mechanism, not a port. | Any smoothing candidate on or beside a bridge deck. | Bridges exist on most retail multiplayer maps; every crossing route. | Native can smooth *across* a deck-to-deck step (the lift keeps `Can_Enter_Cell` on the deck); VERA refuses by construction. Opposite sign from P1-a: here VERA is the more conservative one. |
| P1-e | `0x0042B5C8` | T3's coordinate is the run origin, not the candidate. Any future VERA port that evaluates the threat term per candidate cell would be **stricter than native**. | — | — | Recorded so the port does not "fix" it, in the same spirit as A5's ten-entry cap. |

**Correction to the ledger's count:** of the "three rejection terms", one (T2)
has a working VERA analogue in the closure. The accurate statement is
*"ports `0x0042B420` with T2 only; T1 is replaced by a weaker terrain/occupancy
closure that drops the facing and z arguments, and T3 is absent."*

---

## 3. Pass 2 — verifying the in-tree claim at `path_smooth.rs:368`

### 3a. Is VERA's `find_drift_segment` inert? **Yes — confirmed.**

`find_drift_segment` (`path_smooth.rs:486`) accumulates

```rust
cum_dx += path[i+1].0 as i32 - path[i].0 as i32;   // telescopes over start..=i
```

so after the iteration for `i`, `cum_dx == path[i+1].x - path[start].x`, which
is literally `ideal_dx` computed three lines later; likewise `cum_dy`. The
cross product `cum_dx*ideal_dy - cum_dy*ideal_dx` is therefore
`ideal_dx*ideal_dy - ideal_dy*ideal_dx == 0` for every input, `drift_sq == 0`,
and `0 > dist_sq * 1` is never true. `find_drift_segment` returns `None`
always; `optimize_path` takes the `else { break }` branch on its first
iteration and returns its input unchanged; `optimize_layered_path` does the same
per segment. `reroute_segment` (`path_smooth.rs:536`) is consequently
unreachable from production.

Nuance on the wording: the docstring says "dead". It is **inert, not
unwired** — `optimize_path` is called at `movement_path.rs:636` and
`optimize_layered_path` at `:575` on every produced path. The existing tests
(`path_smooth_tests.rs:234-296`) already say so in prose and assert only
`<=`/endpoint, so nothing pins the inertness; a future edit to
`find_drift_segment` would silently activate `reroute_segment` in production
with the suite still green.

### 3b. Is `0x0042B7F0` a straight-segment optimiser / loop detector? **Confirmed.**

`Path_optimize_straight_segments @ 0x0042B7F0`:

* Window: `if (0x13 < iVar13) break` — at most the first **20** direction-array
  entries are scanned. (The final compaction still walks the whole array.)
* Skips `0xFFFFFFFE` entries; a tube hop (`8`) resets every accumulator and
  re-seeds the segment origin.
* Maintains a running signed accumulator from the segment origin and two
  per-axis high-water marks (`local_70` = max `|dx|`, `local_6c` = max `|dy|`).
  **Segment close condition is an axis reversal**: `|new dx| < local_70 ||
  |new dy| < local_6c`. On close it stores `local_68 = max(|Δx|,|Δy|)` — the
  Chebyshev span of the closed segment — as the bar for the next one, resets
  the accumulators, and **does not advance the index**, so the same step is
  re-read against the fresh origin.
* Otherwise, when Chebyshev displacement `max(|dx|,|dy|)` **fails to exceed**
  `local_68`, it calls `Path_Find_Split_Anchor @ 0x0042BCB0` (which walks
  *backwards* using reversed directions `(d - 4) & 7` to find where the
  wandering began) and then `Path_Reroute_Straight_Line @ 0x0042BE20` with
  strict flag `0`.
* Tail flush: if a segment is still open and
  `max(|Δx|,|Δy|) < (steps_in_segment - 1)`, it reroutes it with strict flag
  `1`.
* `Path_Reroute_Straight_Line` writes `diag_count` diagonal directions, then
  `cardinal_count` cardinals, then fills the remaining slots of the old span
  with **`0xFFFFFFFE` delete markers**.
* `0x0042B7F0` ends with a compaction loop that drops every `0xFFFFFFFE`,
  pads with `0xFFFFFFFF`, and writes `param_2[2] = kept + 1`. **The native
  second pass shortens the path.**

So: "scans a 20-step window, closes a segment when the running `|dx|` or `|dy|`
*decreases* — an axis reversal — and otherwise reroutes when Chebyshev
displacement from the segment origin fails to exceed a stored high-water mark…
its reroute writes `0xFFFFFFFE` delete markers with a final compaction, so the
native pass **shortens** the path" — **every clause of the in-tree claim at
`path_smooth.rs:377-384` is confirmed against the binary.** `0x0042B7F0` is
confirmed, not refuted.

One thing the in-tree note omits: `Path_Reroute_Straight_Line` re-applies all
three pass-1 terms per candidate cell, with **different thresholds**:
`Can_Enter_Cell` (`0x0042BFB2`, `0x0042C0B6`) and the `0x40000` flag
(`0x0042BFC0`, `0x0042C0C4`) reject outright, while the threat term is a
*counter* — `product >= 0.01` (`[0x007E3808]`) increments `steep_count`, and
the reroute fails on `steep_count >= 4`, or on `steep_count >= 1` in strict
mode. It is also gated on `scalar > 1e-5` (`[0x007E3810]`), a gate pass 1 does
**not** have. It carries the same per-step z with the same `+4` bridge lift.

### 3c. Divergence table — pass 2

| # | Native address | Divergence | Trigger | Frequency | Player-visible effect |
|---|---|---|---|---|---|
| P2-a | `0x0042B7F0` + `0x0042BCB0` + `0x0042BE20` | The whole mechanism is absent. VERA calls an inert perpendicular-drift function that models a different thing and never fires. | Every successful A* (`0x0042A41E` is unconditional). | Every path, every match — but the *reroute* only fires when a segment's Chebyshev displacement stalls inside the first 20 steps, i.e. where the path doubles back. **Rate per path UNCHECKED.** | VERA keeps steps retail deletes: units walk the long way around obstacles where retail straightens and shortens, and arrive later. Step count is the one thing pass 1 cannot change, so this is the only pass that changes arrival timing. |
| P2-b | `0x0042BE20` | If `find_drift_segment` is ever "fixed" in place, `reroute_segment` (`path_smooth.rs:536`) activates: it splices a replacement, never deletes, has no `0xFFFFFFFE`/compaction step, no steep counter, no strict/lenient flag, no z. | — | — | Repairing the tautology alone would ship a *wrong* active mechanism. The two must be replaced together, as the docstring already warns. |

---

## 4. Does the ledger row cover pass 1, pass 2, or conflate them?

**Pass 1 only, and it undercounts A2 by one whole mechanism.**

* Row I5 (`docs/plans/2026-09-15-movement-retail-acceptance.md:65`) names
  `0x0042B420` — `Path_smooth_single_segment`, the pass-1 inner helper — and
  anchors at `path_smooth.rs:18`, which is inside the module doc's **Pass 1**
  paragraph (`:7-23`). No conflation: nothing in the row's 121 characters
  refers to `0x0042B7F0`, `0x0042BE20`, deletion, or shortening.
* The A2 criterion row itself (`:22`) also lists only "smoothing `0x0042B420`"
  in its evidence column, so the gap is in the criterion as well as in the
  ledger line.
* Pass 2 is recorded **only** in the source file's own docstring
  (`path_smooth.rs:368-391`) and nowhere in the acceptance ledger. Since pass 2
  is the pass that changes path *length* — and therefore arrival timing and
  "unit took the long way round" — A2 needs a second OPEN row for it before
  the criterion can close.
* Within pass 1, "without three rejection terms" is one too many: T2 has a
  working VERA analogue (§2, P1-b). The row also mis-spells the file's
  directory.

---

## 5. UNCHECKED items and the instrument that would settle each

| Claim | Status | Instrument |
|---|---|---|
| The exact accept/reject outcome of `0x0042B420` for a given fixture | UNCHECKED | `emulate_function 0x0042B420` with a synthetic dir/z array, a hand-built `CellClass` table and a fake vtable whose `+0x1AC` returns a scripted code. The return value (directions consumed) is in `EAX`, so the accept-vs-reject decision **is** observable through the registers-only limit noted in `project_parity_is_instrument_limited`; the mutated `dirs` array is not. |
| Whether VERA's `SearchMarkerOverlay` is the right analogue of cell bit `0x40000` | **UNCHECKED, and it looks suspect** | The bit's only non-test sites in the search path are three `AND …,0x40000` inside `PathfinderClass__UpdateBridgePassability @ 0x0042ACF0` (`0x0042AFA5`, `0x0042B035`, `0x0042B05B`), which `AStar_main_loop` calls at `0x00429C1A`, `0x0042A42D`, `0x0042A44C`. That is a transient **bridge-passability** marker, whereas VERA's overlay is a blocked-repath avoid-set. Settle by reading `0x0042ACF0` end to end and byte-pattern-searching for the `OR` writer (the operand search only finds `TEST`/`AND` forms; an `OR byte ptr [reg+0x143],0x04` encoding would not match). **Adjacent finding, recorded not absorbed.** |
| How often pass 2's reroute actually fires per path in a real match | UNCHECKED | `debugger_set_breakpoint 0x0042BE20` on a live `gamemd` skirmish and count hits per minute; or the oracle rig's capture-compare (`reference_vera20k_oracle_repo`). Static analysis cannot give a rate. |
| Which stock gameplay cases set `TeamTypeClass+0xF2` (forcing the threat scalar to 1.0) | UNCHECKED | Trace writers/readers of `+0xF2` on `TeamTypeClass`. Low priority: AI teams only, and AI is deferred per `feedback_no_ai_yet`. |
| Whether the six harvester sections are the *only* stock `ThreatAvoidanceCoefficient` setters after map/mode INI overrides | UNCHECKED | Six hits in `ini/RULESMD.INI` (7344, 7402, 8210, 8262, 9034, 9086) — map and mode overrides not swept. `asset`/`asset-browser` over the mode INIs would settle it. |
| Retail-vs-VERA route difference on a named map | UNCHECKED | Retail `-win` capture vs VERA on the same seed/map, per `feedback_capture_compare_beats_derivation`. No hand-derived route claim here is a substitute. |

Settled by this pass (previously open): the `+0x1AC` slot identity (vtable
arithmetic, §1 T1); the `+0x59F0` table semantics (OQ-14 of the prior report,
§1 T3); the coordinate T3 samples (§1); every clause of the `0x0042B7F0`
description in `path_smooth.rs:377-384` (§3b); VERA's pass-2 tautology (§3a).

## 6. Sources

Ghidra decompiles: `0x0042B420`, `0x0042B210`, `0x0042B7F0`, `0x0042BE20`,
`0x0042BCB0`, `0x0042C290`, `0x0056BCD0`, `0x004DC760`, `0x00585F40`,
`0x00509400`, `0x004FA2E0`, `0x00481870`.
Disassembly: `0x0042B420..0x0042B7E8` (full), `0x0042B210..0x0042B412` (full),
`0x0073F0A0..0x0073F0EF`, `0x00710AF0..0x00710BB7`, `0x00712440..0x0071247F`.
Xrefs: `0x0073F0A0`, `0x007F5C70`, `0x0042B210`, `0x0042B7F0`, `0x0056BCD0`,
`0x004DC760`, `0x00509400`, `0x004FA2E0`.
Instruction searches: operand `0x59f0` (7 hits), `0x57e4` (9 hits),
`0x40000` (7 relevant), `0x2f0`; `CALL`s in `AStar_main_loop` (18).
Memory reads: `0x007E1718` = `000000000000f03f` = `1.0`.
Repo: `src/sim/pathfinding/path_smooth.rs`, `path_smooth_tests.rs`,
`src/sim/movement/movement_path.rs:429-770`,
`docs/plans/2026-09-15-movement-retail-acceptance.md:22,65`,
`docs/research/FOOTCLASS_STRUCT_LAYOUT.md:51,272`,
`docs/research/core-services-map/cell-map.md:169`,
[docs/research/pathfinding/MAPCLASS_GET_SLOPE_COST_AT_CELL_PATH_SMOOTHING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/MAPCLASS_GET_SLOPE_COST_AT_CELL_PATH_SMOOTHING_GHIDRA_REPORT.md),
`ini/RULESMD.INI` (main checkout).
