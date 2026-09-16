# GATE #4 — Find_Nearby_Passable_Cell Ring Shape, Ordering & Selection — RESOLUTION

**Verdict:** CLOSED.
**Function:** `FootClass::Find_Nearby_Passable_Cell` @ `0x0056DC20`.
**Date:** 2026-06-04. **Source:** gamemd.exe (YR 1.001), live Ghidra MCP, read-only.

This gate resolves the four open questions about the candidate scan SHAPE, per-ring
VISITATION ORDER, SELECTION rule, and the FRAME-COUNTER source/arithmetic. Every fact
below is taken from the function body (decompile + disassembly), not labels.

---

## 0. Identity confirmation

`get_function_by_address 0x0056DC20` → `FootClass__Find_Nearby_Passable_Cell`,
body `0x0056dc20–0x0056e7b5`. `decompile_function 0x0056DC20` and
`disassemble_function 0x0056DC20` agree: 16-param `__thiscall`, `this`=FootClass* in
ECX/EBX, out-cell `param_2`, origin-cell `param_3`. Identity is unambiguous — this is the
same function the chrono-return and MCV-deploy callers invoke (cross-checked against
[miner/PATHFINDING_VALIDATE_ALTERNATE_CHRONO_RETURN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/PATHFINDING_VALIDATE_ALTERNATE_CHRONO_RETURN_GHIDRA_REPORT.md) §1 and
`pathfinding/FIND_NEARBY_PASSABLE_CELL_GHIDRA_REPORT.md`). Full per-candidate validation
pipeline (IsOnScreen, CheckPassability, ±2 height, occupant, bridge, CheckOccupancy) is
documented in that latter report and is NOT re-derived here; this gate covers only ring
shape/order/selection.

---

## (a) Candidate scan SHAPE — concentric diamond rings, radius 0..N-1, cap 24 candidates

**Shape: concentric DIAMOND rings** (Chebyshev-style perimeter walk, not a square, not a
spiral). The outer loop variable is `local_1d0` (= ring radius `r`), counting up from 0.

Radius growth and max ring (disasm `0x0056dce1–0x0056dcfb`, `0x0056e59e–0x0056e5ad`):
```
search_radius = this->[+0xF4] + this->[+0xF8]      ; cached Speed + Sight (cells)
if (search_radius > 0x20) search_radius = 0x20     ; hard cap 32
for (r = 0; r < search_radius; ++r) { ... }         ; rings 0 .. search_radius-1
```
- `0x0056dce7 ADD EAX,EDX` then `0x0056dcef CMP EAX,0x20 / 0x0056dcf4 MOV EAX,0x20` → cap = **32**.
- `0x0056dcf9 CMP EAX,ESI(=0) / JLE 0x0056e79a` → if cap ≤ 0, zero rings, return null cell.
- `0x0056e59e INC EAX / CMP EAX,[ESP+0x2c](=cap) / JL 0x0056dd09` → loop while `r < cap`,
  i.e. the **largest ring index actually scanned is `cap-1`**.

**Candidate cap = 24 (`0x18`).** The collected-count `local_1d4` is compared to `0x18` at
every accept point (`0x0056df2a`, `0x0056e141`, `0x0056e383`, `0x0056e578`, `0x0056e155`,
`0x0056e58f`); on equality it jumps straight to the selection block `LAB_0056e5b3`. The
candidate array `local_120[48 shorts]` holds exactly 24 CellStructs.

**Per-ring early-out (the diamond bias).** After both inner loops of a ring complete, at
`0x0056e596`: `MOV AL,[ESP+0x17] / TEST AL,AL / JNZ 0x0056e5b3`. `[ESP+0x17]` is
`local_1d5`, the "a direct candidate was accepted" flag. So once ANY direct candidate is
accepted, the function finishes the current ring then STOPS (does not scan further rings).
This biases the result toward the nearest ring that yields a direct hit.

---

## (b) Per-ring VISITATION ORDER — two sub-loops, N/S rows then W/E columns

Within ring `r` the perimeter is walked by TWO inner loops. Origin = `(ox, oy)` where
`ox = local_1b4 = origin.X` (`[ESP+0x38]`), `oy = local_1c4 = origin.Y` (`[ESP+0x28]`).

### Inner loop 1 — the two horizontal apex rows (top row `oy-r`, bottom row `oy+r`)
Index `iVar14` (= `d`) runs from `-r` to `+r` (`0x0056dd0d NEG EDI; CMP EDI,EBP; JG…`
entry guard `d ≤ r`; `0x0056e14c INC EDI; CMP EDI,EBP; JLE 0x0056dd28` continue while `d ≤ r`).
For each `d`, in this exact order:
1. **North cell** `(ox + d, oy - r)` — disasm `0x0056dd3b LEA EDX,[EAX + EDI*1]`
   (X=ox+d), `0x0056dd42 SUB EAX,EBP` (Y=oy-r). (`param_15` skip-gate at `0x0056dd2f`.)
2. **South cell** `(ox + d, oy + r)` — disasm `0x0056df54 LEA EAX,[ECX+EDI*1]` (X=ox+d),
   `0x0056df57 LEA ECX,[EDX+EBP*1]` (Y=oy+r). When `param_15`≠0 the south cell is
   skipped for `d == -r` only (gate `0x0056df40 NEG EAX(= -r); CMP EDI,EAX; JLE` skips
   the first south cell so the corner isn't double-counted in skip mode).

### Inner loop 2 — the two vertical columns (west col `ox-r`, east col `ox+r`), interior rows only
Index `iVar14` (= `e`) runs from `1-r` to `r-1` (`0x0056e160 MOV EDI,1; 0x0056e168 SUB
EDI,EBP` → `e = 1-r`; guard `0x0056e16a CMP EDI,EAX(=r-1); JG`; continue `0x0056e583 INC
EDI; LEA EAX,[r-1]; CMP EDI,EAX; JLE 0x0056e17d`). For each `e`, in this exact order:
1. **West cell** `(ox - r, oy + e)` — disasm `0x0056e18c MOV EAX,[ox]; SUB EAX,[r]`
   (X=ox-r), `0x0056e196 MOV ECX,[oy]; ADD ECX,EDI` (Y=oy+e). (`param_15` skip at `0x0056e17d`.)
2. **East cell** `(ox + r, oy + e)` — disasm `0x0056e38e MOV ECX,[oy]; ADD ECX,EDI`
   (Y=oy+e), `0x0056e396 MOV EAX,[ox]; ADD EAX,[r]` (X=ox+r). The east cell is NOT
   gated by `param_15` (always visited in loop 2).

So the per-ring sequence is row-major on the two apex rows (each scanned W→E by `d`),
then the two side columns (each scanned N→S by `e`), interleaving N-then-S inside loop 1
and W-then-E inside loop 2. It is NOT a continuous clockwise/counter-clockwise walk — it
is this fixed 4-segment order: `{(d:N),(d:S)} for d=-r..r`, then `{(e:W),(e:E)} for e=1-r..r-1`.
Ring 0 degenerates to a single cell (loop 1 runs once with `d=0`, N and S coincide;
loop 2 range `1-0..0-1` is empty).

`param_15` ("skip first quadrant", `[ESP+0x224]`) when nonzero suppresses the N cell in
loop 1 and the W cell in loop 2 (and the `d==-r` S corner), yielding a half-diamond scan.
For the chrono-return / MCV / harvester callers `param_15 = 0`, so the full diamond is walked.

---

## (c) SELECTION rule — split into direct/indirect, prefer direct, then frame-modulo OR nearest-distance

After the scan, `LAB_0056e5b3` (`0x0056e5b3`+) splits the up-to-24 collected cells into two
buckets using `FUN_006d6410` (height-corrected cell lookup, `0x006d6410`):
- A cell whose lepton-center `(cx*256+128, cy*256+128, 0)` resolves back to itself →
  **direct**, appended to `local_c0[]`, count `local_1c8` (this report calls it `direct_count`).
- Otherwise → **indirect**, appended to `local_60[]`, count `local_1c4` (`indirect_count`).
(Loop `0x0056e5f0–0x0056e67b`; the resolved cell is compared to the source at
`0x0056e635 CMP AX,DI` / `0x0056e63e CMP [ESP+0x36],BP`.)

Then the choice depends on whether a target cell `param_14` (`[ESP+0x220]`) was supplied:

**Target == null-cell `{0,0}` (no target → frame-counter pick)** — branch taken at
`0x0056e695 CMP word[EBP],AX` / `0x0056e69f CMP DX,word[0x00abd482]` (compare against
`DAT_00abd480` = `{0,0}`):
```
if (direct_count != 0)  result = local_c0[ frame_counter % direct_count ];   ; 0x0056e6d1 IDIV ESI(=direct_count); 0x0056e6de MOV …[+0x17c]
else                    result = local_60[ frame_counter % indirect_count ]; ; 0x0056e6b2 IDIV ECX(=indirect_count); 0x0056e6bf MOV …[+0x11c]
```
Disasm `0x0056e6a8 MOV EAX,[0x00a8ed84]` loads the frame counter; `0x0056e6af CDQ`;
`IDIV` by direct_count (if nonzero, `0x0056e6d1`) else by indirect_count (`0x0056e6b2`);
the EDX remainder indexes the bucket array. Note the decompiler wrote the direct-bucket
access as `local_60[... + -0x18]`; that is just `local_c0[]` (the two arrays are adjacent
on the stack), confirmed by the assembly offsets `+0x17c` (direct/`local_c0`) vs `+0x11c`
(indirect/`local_60`). **Direct bucket is preferred whenever it is non-empty.**

**Target != null (nearest by distance)** — branch `0x0056e6f0` onward:
```
pool = (direct_count != 0) ? local_c0 : local_60;     ; 0x0056e6f2 MOV EBX,direct_count; if 0 -> indirect
best = {0,0}; best_dist = 100000.0;                   ; 0x0056e704 stores 0x40f86a00 = 100000.0f-as-double-hi
for each cell in pool:
    dx = cell.X - target.X; dy = cell.Y - target.Y;   ; 0x0056e73c, 0x0056e743
    d  = sqrt((double)(dx*dx + dy*dy));               ; 0x0056e755 FILD; 0x0056e75c CALL 0x004cac40 (sqrt)
    if (d < best_dist) { best_dist = d; best = cell; } ; 0x0056e761 FCOM; strict-less keeps FIRST on ties
result = best;
```
First-found wins ties (the compare is strict `<`), and because the pool is filled in scan
order, ties resolve to the earlier ring/segment candidate. Direct pool is again preferred.

**Fallback:** zero candidates → `LAB_0056e79a` writes `DAT_00abd480` = null cell `{0,0}`
to `*param_2` (`0x0056e7a1`), caller treats as "no cell".

---

## (d) Ordering/index source is a deterministic FRAME COUNTER — `g_FrameCounter` @ `0x00a8ed84` (NOT RNG)

**Confirmed: the no-target pick uses the global tick/frame counter, not the RNG.**

- The modulo dividend at the selection site is `[0x00a8ed84]` (`0x0056e6a8 MOV
  EAX,[0x00a8ed84]`), divided by the bucket count via `IDIV`, remainder = index. No
  `Random`/`RandomRanged`/`Scen->Random` call appears anywhere in `0x0056dc20–0x0056e7b5`
  (verified by reading the full disassembly — only `IsOnScreen`, `CheckPassability`,
  `Is_Current_Cell_Obstacle_Free`, `CheckOccupancy`, `FUN_006d6410`, `sqrt` are called).
  This corroborates the pass-2 finding that selection is NOT RNG.

- **Which counter:** the global at `0x00a8ed84`. `get_xrefs_to 0x00a8ed84` shows it is
  WRITTEN once per tick in `Main_Tick` and READ across the per-tick logic
  (`LogicClassPerTickUpdateLiveVector`, `LightningStorm__Process`, `AnimClass__Constructor`,
  `MapClass__UpdateCrateRegenTimers`, crate-slot timers, `RenderDebugStatsOverlay`). The
  write is the increment at `0x0055de73`: `MOV EDX,[0x00a8ed84] / INC EDX / MOV
  [0x00a8ed84],EDX` — a plain `+1` per Main_Tick (gated by the pause/replay flags checked
  at `0x0055de4f–0x0055de71`). So it is a **global per-tick frame counter** (read
  `0x00a8ed84` = `0x00000000` at rest), NOT a per-object field.

- **Index arithmetic:** `index = g_FrameCounter % pool_count` (signed `IDIV`,
  EDX remainder). `pool_count` = `direct_count` if directs exist, else `indirect_count`.

- **Label-drift note:** the decompiler named a DIFFERENT global, `0x00887324`, as
  "g_CurrentFrameCounter". That is wrong: `0x00887324` is loaded into ECX as the `this`
  pointer for the `FUN_006d6410` call (`0x0056defb MOV ECX,[0x00887324]`), and
  `get_xrefs_to 0x00887324` shows it written in `CCFileClass__Constructor` / `FUN_00534450`
  — it is an object pointer, not a counter. The real frame counter consumed by selection is
  `0x00a8ed84`, verified from the IDIV dividend and the Main_Tick increment. Do not build
  on the decompiler's `g_CurrentFrameCounter` name.

---

## YR-active vs TS-legacy

**Active in YR.** No SpecialFlags / FogOfWar / subterranean gating anywhere in the body.
Live callers in a normal YR skirmish include MCV deploy-spot search, unit/infantry scatter,
harvester & chrono-miner refinery repositioning, Set_Destination correction, rally-point
placement, and crate spawn — all fire in stock skirmish. The ±2 height check and bridge
handling are live YR logic (bridges exist in YR). Nothing here is dead TS code.

---

## Rust handoff

For cell-validation slice 2 (T7/T8), the shadowed `src/sim/find_nearby_cell.rs` is correct
on the load-bearing semantics and is safe to flip to authority; reconcile these specifics
before the call-site swap + SNAPSHOT_VERSION bump:

1. **Ring order:** the engine's per-ring order is `{N(ox+d,oy-r), S(ox+d,oy+r)} for
   d=-r..+r`, then `{W(ox-r,oy+e), E(ox+r,oy+e)} for e=1-r..+r-1`. The current Rust
   `diamond_ring` emits cells **row-major top→bottom, left-then-right** — a DIFFERENT visit
   order than gamemd's segment order. This only affects (i) which 24 cells survive when the
   cap is hit on a partially-scanned ring, and (ii) nearest-distance TIE order. To be
   bit-identical on the no-target frame-modulo pick and on ties, the Rust collection order
   must match the engine's 4-segment sequence above (N/S apex rows by `d`, then W/E columns
   by `e`), not row-major. **Align `collect_candidates`/`diamond_ring` to that order.**

2. **Direct vs indirect:** the engine's "direct" classification is the
   height-corrected-lookup identity test (`FUN_006d6410`), NOT "on a cardinal axis from the
   seed" as the current Rust `Candidate.direct` computes (`cx==seed.0 || cy==seed.1`). On
   flat terrain every accepted cell is direct, so the two agree; they diverge on sloped /
   bridge terrain. For full parity, classify by the height-projection identity, not the axis
   test. (Acceptable to defer if slice 2 scope is flat-terrain only — but record it.)

3. **Per-ring early-out:** stop scanning further rings once a direct candidate has been
   accepted (finish the current ring first). The current Rust collects until the 24-cap or
   `radius_cap` with no direct-found early-out — add the "direct found → finish ring → stop"
   termination to match candidate-set composition.

4. **Selection index:** `frame_counter % pool.len()` over the direct-preferred pool is
   correct; `frame_counter` MUST be the global per-tick counter (engine `0x00a8ed84`,
   i.e. `Simulation::binary_frame`), never an RNG draw — the current code's contract is right.

5. **Nearest-distance:** engine compares `sqrt(dx*dx+dy*dy)` with strict `<` (first-found
   wins ties). Rust uses integer `dx*dx+dy*dy` with `min_by_key` — monotonic with sqrt so
   the winner matches, AND `min_by_key` keeps the first minimum, matching the engine's
   tie rule. This is parity-safe as written.

---

## Key addresses

| Item | Value | Evidence |
|------|-------|----------|
| `FootClass::Find_Nearby_Passable_Cell` | `0x0056DC20` | get_function_by_address |
| Radius cap (cells) | 32 (`0x20`) | disasm `0x0056dcef` |
| Candidate cap | 24 (`0x18`) | disasm `0x0056df2a` etc. |
| Ring outer-loop bound | `r < Speed+Sight (cap 32)` | disasm `0x0056e59e–0x0056e5ad` |
| Direct/indirect classifier | `FUN_006d6410` @ `0x006d6410` | disasm `0x0056e62e` |
| Global frame counter (selection modulo) | `g_FrameCounter` @ `0x00a8ed84` | disasm `0x0056e6a8`; incr `0x0055de73`; xrefs in Main_Tick |
| Null/sentinel cell `{0,0}` | `DAT_00abd480` | disasm `0x0056e7a1` |
| sqrt helper | `0x004cac40` | disasm `0x0056e75c` |
| Misattributed "g_CurrentFrameCounter" (NOT a counter) | `0x00887324` (object ptr) | disasm `0x0056defb`; xrefs in CCFileClass__Constructor |
