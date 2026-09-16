# A6 "Infantry stepping" — acceptance examination

Read-only. gamemd.exe in Ghidra project `testProsjekt`
(oracle metadata records native SHA256 `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`).
No file edited, no cargo run.

Scope examined: sub-cell slot selection + reservation
(`CellClass::GetSubCell` 0x004810A0, `CellClass::PlaceInfantryInCell` 0x00481180,
`InfantryClass::Mark/UnmarkCellOccupancy` 0x005217C0 / 0x00521850,
`WalkLocomotionClass::FindSubCellDest` 0x0075C240) and the Walk
head/boundary/completion sequence (`WalkLocomotionClass::ProcessMovement`
0x0075AEC0, moving arm from 0x0075BF85, boundary 0x0075C117, completion
0x0075BD70).

VERA side: `src/sim/cell_kernel.rs` (183–241), `src/sim/movement/walk_head.rs`,
`src/sim/movement/walk_host.rs`, `src/sim/movement/movement_step.rs`,
`src/sim/movement/movement_bridge.rs`, `src/sim/occupancy.rs:176`.

---

## 1. Where VERA matches, with evidence

**M1 — Quadrant / preferred slot.** Native inlines GetSubCell into
PlaceInfantryInCell at 0x00481190–0x00481211. Disassembly (not the decompiler,
which renders `uVar11 = *param_3` and misleads): `MOV EAX,[EBP]; MOV ECX,[EBP+4];
AND EAX,0xff; AND ECX,0xff` at 0x00481190–0x0048119B, then the CoordStruct
subtract of {0x80,0x80} at 0x004811C1, `FILD`/square/sum, `Sqrt_Approx`,
`ftol`, `CMP EAX,0x3c; JGE`. The quadrant bits come from the *masked* low bytes
compared strictly-greater against 0x80 (0x004811FB / 0x00481204, both `JLE`),
`INC EDI` only when nonzero. `infantry_preferred_spot` (cell_kernel.rs:183–193)
is instruction-for-instruction equivalent, dead slot 1 included.

**M2 — sqrt/ftol semantics at both decision boundaries.** `Sqrt_Approx`
0x004CAC40 is **not** an IEEE sqrt: it rounds the double to float32, splits
exponent/mantissa, indexes a 14-bit table at 0x008650BC (`SHR ECX,0xA`), and
rebuilds a float32. `Math__ftol` 0x007C5F00 loads the chop control word from
0x00822D80 and `FISTP`s — truncation toward zero. Table reads settle the two
thresholds that matter:

| n | index | table entry @0x008650BC+4·idx | result | VERA `isqrt_i64` |
|---|---|---|---|---|
| 3600 | 14400 | 0x00700000 | exactly 60.0 → ftol 60 → **not** `< 60` | 60, not `< 60` ✓ |
| 3599 | 14396 | 0x006FF777 | < 60 → ftol 59 | 59 ✓ |
| 289 | 1056 | 0x00080000 | exactly 17.0 → ftol 17 → **not** `< 17` | 17, not `< 17` ✓ |

The 60-lepton centre test and the 17-lepton arrival test therefore agree.
(Bounded: three indices, not the whole table — see U6.)

**M3 — Preference rows and scan loop.** Fallback rows at 0x0081CC84 (indexed
`EDI*4`, 0x00481356), rotated rows at 0x0081CC98 (`EAX*4`, 0x0048139F). Loop
0x00481365 skips indices 0 and 1 (`JZ 0x004813BE` twice) and fails on a counter,
`CMP EAX,0x4; JL` at 0x004813C3 — **not** a `(byte & 0x1C) == 0x1C` fullness
test. `INFANTRY_SEQUENCE` / `INFANTRY_ALTERNATE_SEQUENCE` and
`select_infantry_subcell`'s `find(|spot| *spot >= 2 && …)` match exactly.

**M4 — Fast path and RNG accounting.** Quadrant 2/3/4 take the free-slot fast
path at 0x00481316–0x00481350; quadrant 1 skips it (`CMP EDI,1; JZ 0x00481356`);
quadrant 0 draws `Random__RandomRanged(0,3)` at 0x0048139A; the priority/force
argument jumps straight to placement at 0x0048128E, consuming no draw.
`select_slot` (walk_head.rs:31–39) returns before the draw on `priority`, and
draws only when `preferred == 0`, after the blocker gates. Oracle-covered:
`walk_head_occupation.json` `selection`, 176 rows, asserting both RNG indices.

**M5 — Blocker bits use two different bytes.** 0x00481275/0x0048127D select
+0x128 (deck) or +0x124 (ground) for the **0x20** test, but 0x00481298 tests
**0x40 on +0x124 unconditionally**, whatever plane was selected. walk_head.rs:34
(`selected_raw & 0x20 != 0 || (ground_raw & 0x40 != 0 && !ground_gate_open)`)
reproduces the asymmetry.

**M6 — Base and result coordinates.** Base is `in − (in & 0xff)` per axis
(0x00481246–0x0048125E). Result is `base + table[slot]` from 0x0089E9F0
(0x00481402/0x00481443). Z is `GetGroundHeight(**the caller's original input
coord**)` — `PUSH EBP` at 0x00481401/0x00481442, `ECX = 0x87F7E8`,
`CALL 0x00578080` — plus `[0x0089E7B4]` when the deck plane was chosen
(0x0048147B). The `table[slot].z + in.z` sum computed at 0x00481461 is stored to
a dead stack slot and discarded. `selected_head` (walk_head.rs:42–54) and
`prepare_step_head`'s `ground_surface_z_at([input.x, input.y], …)` match,
including sampling ground at the *input* coord rather than the chosen sub-cell.

**M7 — Failure result.** Both failure exits copy DAT_0089E778/77C/780 (0,0,0).
VERA returns `None`; the oracle test maps `(0,0,0)` to `None`.

**M8 — Mark/unmark asymmetry.** Mark 0x005217C0 requires
`groundZ + DAT_00A8F234 <= coord.z` **and** `Cell+0x140 & 0x100`; unmark
0x00521850 requires height alone (no structural flag). Owner slot +0x54/+0x58
resets to −1 only when the remaining byte `& 0x1C == 0`. `raw_at`
(walk_head.rs:83: `deck = coord.z >= ground+416 && (!put || structural)`) and
`RawCellOccupationGrid` reproduce both. Oracle-covered: `walk_head_occupation.json`
`raw`, 160 rows, asserting both planes' bytes and owners.

**M9 — Deck-plane selection in FindSubCellDest.** 0x0075C4xx: deck only when the
**destination** cell has `+0x140 & 0x100` and the infantry's current Z is
strictly greater than `GetGroundHeight(dest) + DAT_00B45C28*3`; equality picks
ground; Object+0x8C is never read. walk_head.rs:750–754 (`current.z > ground+312`)
matches (constant caveat U1).

**M10 — Priority corridor.** Missions {8,9,7,0xB,0x19} funnel to LAB_0075C342;
RTTI 1 and 2 targets compare the target's cell to the request cell, RTTI 6
compares the cell's building; then the slave-manager arm. `prepare_step_head`
lines 683–728 use `[7, 8, 9, 11, 25]`, `Unit | Aircraft` → cell compare,
`Structure` → `first_building_on_layer`, `Infantry` → false, then
`slave_deposit::walk_priority` — with the comment correctly noting that the slave
arm still executes when the ordinary target already granted priority.

**M11 — Arrival test.** 0x0075BD44–0x0075BD73: 2-D only (two `FILD`s), squares
summed, `Sqrt_Approx`, `ftol`, `CMP EAX,0x11; JGE`, evaluated **before** the
step. `completed_walk_head` / `advance_lepton_position` (movement_step.rs:1934,
1975) test `fixed_distance(dx,dy) < 17` in the same position.

**M12 — Completion order.** Native: Mark(REMOVE) 0x0075BD7D → propagate −1 into
+0x5E4 when +0x5E0 == −1 (0x0075BD89) → 23-dword `MOVSD.REP` shift 0x0075BDAC →
+0x63C = −1 → SetCoords(+0x1B4) 0x0075BDC0 → Foot+0x558 = packed cell →
SetHeight(0)(+0x1CC) 0x0075BE01 → FindSubCellDest(NullCoord) 0x0075BE18, which
unmarks the stored head and re-marks the current coord → liveness gates
(+0x90, +0x81) → PerCell(2)(+0x18C) 0x0075BE3C → Mark(PUT) 0x0075C1AE.
`run_completed_walk_step` (walk_host.rs:338–435) walks the same order:
`walk_mark_remove` → `consume_walk_path_replay` → `put_walk_coords` →
`reference_cell` → `commit_ground_height` → `raw_at(old_head,false)` →
`raw_at(current,true)` → per-cell work → `walk_mark_put`, with the
`!survives` early return skipping `walk_mark_put` exactly as the native
0x0075BE4D/0x0075BE5B jumps to the Mark(PUT)-less return at 0x0075C1F1.

**M13 — Boundary order.** Native 0x0075C117: Mark(REMOVE)(+0x124, 0) →
SetCoords(+0x1B4) → old/new CellClass fetches → OnBridge predicate →
SetHeight(0)(+0x1CC) 0x0075C1A1 → Mark(PUT)(+0x124, 1) 0x0075C1AE.
`run_walk_boundary` (walk_host.rs:267–336) is the same sequence.

**M14 — OnBridge predicate.** 0x0075C154–0x0075C193, read from the
disassembly: set 1 only when `(i8)new[+0x11B] == (i8)old[+0x11B] − 4` **and**
`new[+0x140] & 0x100`; clear to 0 only when `!(new & 0x100) && (old & 0x100)`.
`compute_bridge_transition` (movement_bridge.rs:201–220) is identical, with the
already-recorded i8-wrap footnote.

**M15 — Fresh-head tail.** 0x0075BC2A onwards: no raw work, gated on Object+0x90,
sets the atan2 facing through FacingClass+0x4C and `vtable+0x544(1.0)`
(`PUSH 0x3FF00000`). `finish_fresh_head` (walk_head.rs:99–128) sets facing,
clears `facing_target`, sets `applied_fraction = SIM_ONE`, under
`lifecycle.object_alive`. Oracle-covered: `walk_first_step.json` `ready_path`.

**M16 — Gate allowance shape.** 0x00481298–0x004812EA: ground bit 0x40 set →
`FindOccupierByRTTI(6,0)` must return an object whose TypeClass(+0x520)+0x16B7
is set, **and** `BuildingClass__CanGarrison` 0x004525F0 must return nonzero.
0x004525F0 requires `vtable+0x184() == 0x18` (Mission Open) and
`0x004A51B0(this+0x350)` — i.e. the anim state byte +0x18 == 0 and +0x19 == 1.
VERA's `gate_open` (walk_head.rs:761–773) = building type `.gate` **and**
`building_gate.can_garrison_passable()` = `mission_18_active && phase ==
OpenStable` (game_entity.rs:238). Structurally identical; two identity questions
remain (U2, U3).

---

## 2. Established divergences

### D1 — There is no Walk locomotor step at all (the substrate divergence)

* **Native:** `0x0075BFA9`–`0x0075C0CB`. Every moving tick:
  `vtable+0x544(1.0)` → `vtable+0x538` returns an **integer** speed (`FILD dword
  [ESP+0x14]` at 0x0075C082) → `Infantry+0x6B7 = 0` (0x0075BFD1) → desired
  facing = `ftol((atan2(cur.y − head.y, head.x − cur.x) − [0x007E2820]) ×
  [0x007E2818])` (0x0075BFED–0x0075C01C; note the inverted-Y argument order) →
  pushed through `FacingClass` slot +0x4C at 0x0075C035 → polar step with
  `θ = (facing16 − 0x3FFF) × [0x007E2810]`:
  `newX = ftol(cos θ · speed + cur.x)` (0x0075C0B6–0x0075C0C6),
  `newY = ftol(cur.y − sin θ · speed)` (0x0075C09C–0x0075C0A9, note `FSUBR`).
  No clamp to the remaining distance; overshoot is absorbed by next tick's
  `< 17` test.
* **VERA:** no Walk arm. Infantry run through the generic `MovementTarget`
  interpolator; `lepton_step = SimFixed::from_num(frame_budget)` with
  `frame_budget = speed / 15` (movement_step.rs:97–99, 2427, 2232–2236), plus a
  VERA-only overshoot clamp that snaps to `subcell_dest` when
  `frac >= 1` and the path is exhausted (movement_step.rs:2240–2257).
* **Trigger:** every infantry movement tick.
* **Frequency:** continuous, every infantryman, every match.
* **Player-visible effect:** the sub-lepton path shape and the exact tick a step
  completes follow a different rule; a walker does not stay on its facing ray.
* Already recorded in-source at movement_step.rs:2206–2229 (GSI-06.14). This
  examination confirms the native side of that comment against the disassembly
  and adds the per-tick `+0x6B7 = 0` and `vtable+0x544(1.0)` writes it omits.

### D1a — Facing source and granularity

* **Native:** a 16-bit facing recomputed **every tick** from the live position to
  the live head (above), driven through `FacingClass` at the unit's ROT.
* **VERA:** computed once at head acceptance via `facing16_from_delta`
  (direction tables, not atan2) and snapped (walk_head.rs:115–123); then
  `configure_motion_after_transition` overwrites `*facing` with an 8-way
  `facing_from_delta(ndx, ndy)` taken from the **cell** delta
  (movement_step.rs:206, 262–265).
* **Trigger:** every step transition.
* **Frequency:** continuous.
* **Effect:** infantry body sprites select a different facing frame during
  sub-cell approaches, and there is no ROT lag on the walk facing.

### D2 — Crate pickup at head commit is absent

* **Native:** `0x0075C565`–`0x0075C580` inside FindSubCellDest. After the
  selected sub-cell coordinate is stored into `loco+0x28..0x30`, the destination
  CellClass is fetched (`0x00565730`) and `CrateClass::PickupDispatch`
  `0x00481A00` is called with the infantry. 0x00481A00 returns 1 immediately
  unless the cell's overlay index (+0x44) is a crate type
  (`OverlayTypeClass+0x2AA`); otherwise it runs the full crate effect
  (money / veterancy / armour / speed / firepower / unit / explosive / napalm /
  cloak / ICBM / tiberium / shroud). If it returns 0 and `Infantry+0x81` is
  clear, the head is nulled at 0x0075C582 and FindSubCellDest returns false —
  **the step is refused**.
  So retail collects the crate when the infantry *commits* to the step, not on
  arrival.
* **VERA:** no crate dispatch exists anywhere under `src/sim/`; crates appear only
  as startup overlay placement (`app/loading/init.rs:108`,
  `app/frontend/skirmish.rs:2887`).
* **Trigger:** an infantryman selecting a head into a cell that holds a crate
  overlay.
* **Frequency:** every crate an infantryman walks onto, in any crates-enabled
  skirmish.
* **Effect:** infantry never collect crates — no credits, no veterancy, no
  explosion, no spawned unit — and the head-refusal path (unit crate that
  occupies the target cell) never blocks the step.

### D3 — Speed fraction is not zeroed when the path queue empties on arrival

* **Native:** `0x0075BE1D`–`0x0075BE2F`. Immediately after
  FindSubCellDest(NullCoord), `if (Foot+0x5E0 == −1) vtable+0x544(0.0)`.
* **VERA:** `run_completed_walk_step` never writes `foot_speed.applied_fraction`;
  `finish_fresh_head` set it to `SIM_ONE` and only the Drive/Ship stop paths
  (navcom.rs:423, 459) ever clear it.
* **Trigger:** the last step of every infantry move order.
* **Frequency:** every completed infantry move.
* **Effect:** the arrived infantryman keeps `applied_fraction == 1.0`, so
  consumers of the cached speed (walk animation cadence, next-order
  acceleration) start from "moving" rather than "stopped".

### D4 — Blocked-latch clear cadence

* **Native:** `Infantry+0x6B7 = 0` on **every** moving tick (0x0075BFD1) and at
  arrival (0x0075BE11). The latch gates re-arming the Rules+0x1768 hold timer in
  the `Can_Enter_Cell` code-2 arm (`if (+0x6B7 == 0) { +0x6B7 = 1; arm timer }`).
* **VERA:** cleared only at cell transitions — `run_walk_boundary`
  (walk_host.rs:331–334) and `apply_cell_transition_remainder`
  (movement_step.rs:167–169).
* **Trigger:** an infantryman that was blocked and then resumes moving without
  leaving its cell.
* **Frequency:** common in crowded infantry groups and choke points.
* **Effect:** the latch stays armed, so the next block does not re-arm the hold
  timer — different pause/repath timing when infantry jostle.

### D5 — Boundary tail call `0x00486920` missing

* **Native:** `0x0075C1DB`–`0x0075C1E2`, immediately after Mark(PUT) at a walk
  boundary: `Get_CellClass_At_Coord(current)` then `FUN_00486920`. That function
  fires when the cell's overlay index (+0x44) is `0x7E`, density (+0x11E) > 0x2F,
  +0x11C is clear and cell flag `0x20000` is clear: it walks the ground occupier
  list and, for each object with RTTI < 6 whose type lacks +0xC91 and has no
  weapon ability 8, spawns `Rules+0xE8`'s anim and latches cell flag 0x20000.
* **VERA:** `walk_mark_put` calls only `recalculate_track_cell`; no equivalent.
* **Trigger:** infantry crossing into a cell with overlay type 0x7E at density
  > 0x2F.
* **Frequency:** map-dependent; zero on maps without that overlay.
* **Effect:** a missing one-shot per-cell animation/effect on those cells.
* Overlay type 0x7E's identity is **UNCHECKED** — do not assume "veins".

---

## 3. UNCHECKED, with the instrument that would settle each

* **U1 — the three bridge constants are supplied, not read.** `DAT_00B45C28`
  (VERA assumes 104, used as `3 × 104 = 312`), `g_nCellGroundStructuralDeckOffsetLeptons`
  at `0x0089E7B4` (VERA assumes 416) and `DAT_00A8F234`, the InfantryClass
  mark/unmark threshold (VERA assumes 416 — **the same number as 0x0089E7B4**).
  All three are BSS and read as zeros in the static image;
  `tools/spatial_oracle/walk_head_occupation.meta.json` lists
  *"Supplied initialized subcell offsets and height steps 104/416"* as an
  assumption. If 0x0089E7B4 and 0x00A8F234 differ, heads and marks disagree
  about which plane an infantryman is on.
  **Instrument:** `get_xrefs_to` each address to find its writer and read the
  immediate, or a live-process/debugger read of all three after rules load.

* **U2 — `BuildingTypeClass+0x16B7` identity.** VERA maps it to the `Gate=` INI
  flag. Not verified here.
  **Instrument:** the `BuildingTypeClass::Read_INI` field map (xrefs to +0x16B7
  from the INI reader), not the YRpp-imported struct names.

* **U3 — the gate allowance corridor was never natively executed.**
  `walk_head_occupation.meta.json` substitutions: *"47C4D0 ground building lookup
  supplies missing or Gate object; 4525F0 supplies closed/open result"*. So the
  `0x40` allowance is a stub in the oracle, and the failure/allow split is not
  native-evidenced end to end.
  **Instrument:** an oracle row driving 0x0047C4D0 and 0x004525F0 against a real
  BuildingClass with a real +0x350 gate anim state.

* **U4 — the priority and crate arms of 0x0075C240 were never exercised.**
  Same meta: *"75C240 producer uses no-target/no-slave owner, no crate overlays"*.
  The 176 selection rows and 160 raw rows say nothing about D2, M10 or the slave arm.
  **Instrument:** extend `walk_head_occupation.py` with target-installed,
  slave-installed and crate-overlay rows.

* **U5 — the unit/aircraft priority cell comparison.** Native compares the
  target's `vtable+0x1B8` result (a packed CellStruct getter) against the request
  cell at 0x0075C36x; VERA derives the cell from
  `position_world_coord(&target.position) / 256`.
  **Instrument:** decompile the UnitClass/AircraftClass `+0x1B8` slot and check
  whether it is coordinate-derived or occupancy-derived.

* **U6 — `isqrt_i64` ≡ `ftol(Sqrt_Approx())` is spot-checked only.** Three table
  indices verified (n = 289, 3599, 3600).
  **Instrument:** enumerate n ∈ 0..=32768 (quadrant test) and the reachable
  arrival-distance range through the 0x008650BC table with the exact
  exponent/mantissa arithmetic from 0x004CAC40 — a pure table read plus integer
  math, no emulation needed — and diff against `isqrt_i64`.

* **U7 — walk speed units.** Native takes an integer from `vtable+0x538`; VERA
  computes `speed / 15`. Whether those agree for stock infantry Speed values is
  unverified (and moot until D1 is closed).
  **Instrument:** a native trace of `vtable+0x538` for E1/GI at stock Speed.

* **U8 — the 24th path-queue slot.** Native writes `Foot+0x63C = −1` after the
  23-dword shift (0x0075BDB1). The oracle rows compare only
  `.take(23)` (walk_head.rs:585), so the terminator slot is outside coverage.

---

## 4. Documentation correction

`docs/research/INFANTRY_SUBCELL_POSITIONING.md` lines ~475–523
("Current Rust Engine Status", "Dead Code", "Authenticity Bug: Wrong Functional
Sub-Cells", "Missing: Preference Table Logic") are **stale**. They describe
`movement.rs:1862`, `FUNCTIONAL_SUB_CELLS = [0,3,4]`, an instant snap on
completion and an absent preference table. Current code has the correct
`{2,3,4}` set, both preference tables, the quadrant fast path and the RNG row
selection in `src/sim/cell_kernel.rs:183–241`, and walks to the sub-cell head
rather than snapping.

The document's native claims that I re-derived are correct, with three
sharpenings:

1. Its step 3/4 reads as if one occupancy byte serves both pre-checks. The
   binary uses two: `0x20` on the **selected** plane byte, `0x40` on the
   **ground** byte regardless of plane (0x00481275/0x0048127D vs. 0x00481298).
2. Its step 4 stops at the `+0x16B7` flag. The native additionally requires
   `0x004525F0` to return nonzero, i.e. mission `0x18` and the +0x350 anim state
   {+0x18 == 0, +0x19 == 1}. A **closed** gate refuses placement.
3. It labels 0x004CAC40 "sqrt" implicitly. It is a float32 table approximation
   (0x008650BC), not FSQRT — relevant to any future re-derivation of the
   60-lepton and 17-lepton thresholds.

Also: the tier-8 record in the same file ("the cell-full test is not
`== 0x1C`", "only {0,2,3,4} reachable", RNG accounting) is confirmed by this
pass.

---

## 5. Proposed A6 ledger row

> **A6 Infantry stepping** — Selection/reservation kernel is native-evidenced and
> matches: quadrant (0x004810A0 inlined at 0x00481190), fast path + preference
> rows (0x0081CC84 / 0x0081CC98), 4-iteration counter exit, the 0x20-selected /
> 0x40-ground blocker split, input-coordinate ground sampling, and the
> mark/unmark structural-flag asymmetry (0x005217C0 / 0x00521850) — oracle rows
> 176 selection + 160 raw. Head/boundary/completion **ordering** matches
> (0x0075BD70 / 0x0075C117 / 0x0075BE18). **Not accepted:** no Walk locomotor
> polar step (D1/D1a, continuous), no crate pickup at head commit (D2), no
> speed-fraction zero on the last step (D3), blocked-latch clear cadence (D4),
> missing 0x00486920 boundary tail (D5). Constants 104/416/416 are supplied, not
> read (U1); the gate, priority, slave and crate arms of 0x0075C240 are outside
> oracle coverage (U3, U4).
