# A9 "Hover motion" — examination of ledger rows I4b and I7

Read-only. Worktree `C:\Users\enok\Documents\ra2-rust-game\.claude\worktrees\vera20k-yuris-revenge-movement-bc5040`,
branch `feature/vera20k-yuris-revenge-movement-bc5040` @ `eed60afa`. Ghidra `gamemd.exe` in
`testProsjekt` (image base 0x00400000, 10073 functions). No file edited, no cargo run.

---

## 0. Native call shape (established, body-read 2026-09-16)

`HoverLocomotionClass__Move 0x00514310` is the per-tick entry. Two object bases are in play and
they differ by 4 — this must be held straight or every offset shifts:

- `0x0051432D  LEA EDI, [ESI - 0x4]` / `0x00514342 MOV ECX,EDI` / `0x00514344 CALL 0x00515ED0`
  — `SpeedUpdate`'s `this` is `Move`'s `this - 4`. Ghidra's decompiler renders `Move` with
  `LinkedTo` at `+0x08` and `SpeedUpdate`/`FUN_00514F70` with `LinkedTo` at `+0x0C`.
  Below, **"loco+N"** always means the `SpeedUpdate`/`FUN_00514F70` base (the one the research
  report uses): `LinkedTo +0x0C`, `Head_To +0x24`, `SpeedRequest +0x48`, `SpeedCurrent +0x50`,
  `SpeedMult +0x58`, own `FacingClass +0x30`, turn-decay flag `+0x68`, pending delta `+0x6C`,
  `IsFacingChangeActive +0x70`.

Per-tick order inside `Move`:

1. `0x00514344` → `SpeedUpdate 0x00515ED0` (facing command + throttle ramp).
2. `0x00514372 CALL [EAX+0x538]` → owner base speed (int);
   `0x0051437C FILD` ; `0x00514380 FMUL double [ESI+0x4c]` (= loco+0x50 `SpeedCurrent`) ;
   `0x00514383 CALL 0x007C5F00` (`Math__ftol`) → **`speed_leptons = ftol(base_Speed × SpeedCurrent)`**,
   stored `[ESP+0x10]`.
3. Arrival arm (`Sqrt_Approx` distance to Head_To ≤ speed) → `FUN_00514F70` at `0x00514499` /
   `0x00514636` (the two `Move` call sites in the xref list; the third, `0x00516309`, is
   `SpeedUpdate`'s halt-and-arrive branch).
4. **Translation gate:** `0x00514746 MOV EAX,[ESP+0x10]` ; `0x0051474A TEST EAX,EAX` ;
   `0x0051474C JLE 0x00514A21`. The *only* condition on translating is `speed_leptons > 0`.
   There is no facing/turn gate anywhere on this path.
5. Occupation release `0x005147CC..0x005147DF` (this is the I3b mechanism, already LANDED).
6. XY integration `0x005147F7 LEA ECX,[ESI+0x2c]` (= **loco+0x30**, the locomotor's own
   `FacingClass`) → `0x004C93D0 Current` → `0x00514806 SUB EDX,0x3FFF` →
   `FMUL double [0x007E2810]` (= −9.587672516830327e-05 = −2π/65536) → `Sin 0x004CACB0` /
   `Cos 0x004CAD00` × speed, added to the owner coord, `Set_Coord` slot `0x1B4`.
7. Wake/water/bridge tail `0x00514A21..0x00514C12`, then `FUN_00513D20` (vertical controller).

---

## 1. Row I4b — "Hover hard-turn translation while braking"

Current ledger text (line 64): `| I4b … | A9 | OPEN | Disclosed approximation at
movement_step.rs:309 (Hover steering, part of I7). |` — 143 chars, no native address.

### 1.1 Native address of the mechanism

- **Turn-stall test:** `SpeedUpdate 0x00515FA5..0x00515FCF`.
  `0x00515FA5 LEA EDX,[ESP+0x1c]` / `0x00515FA9 MOV ECX,EBX` (EBX = `LEA EBX,[ESI+0x30]` at
  `0x00515F70`, the locomotor's own FacingClass) / `0x00515FAC CALL 0x004C93D0` (`Current`) ;
  `0x00515FB4 SUB AX, word [ESP+0x14]` (current − desired) ; abs ;
  `0x00515FBF MOV EAX,0x2000` ; `0x00515FCD CMP EAX,ECX` ; `0x00515FCF JGE 0x00515FDC`.
  → stall iff `|Δfacing16| > 0x2000` (strict), 45° of the 0x10000 circle.
  The `JZ 0x00515FD1` at `0x00515FA3` also routes here when owner-interface slot `0x60`
  returns false.
- **What the stall does:** `0x00515FD1 MOV AL,[ESI+0x70]` ; `0x00515FD6 JZ 0x00516103` →
  `0x00516105 MOV [ESI+0x48],EAX` / `0x00516108 MOV [ESI+0x4c],EAX` with EAX = 0, i.e.
  **`SpeedRequest = 0.0` and nothing else**. It then falls straight into the ramp at
  `0x0051610B FLD [ESI+0x48]`.
- **The ramp:** accel `0x00516191 FADD [ESI+0x50]` / `0x00516196 FST [ESI+0x50]` with step
  `1/(Rules+0x5E0 × 900)`; brake `0x005161A7 FCOMP [ESI+0x50]` → `0x005161C9 FSUBR [ESI+0x50]`
  → `0x005161D2 FST [ESI+0x50]` → clamp at 0 `0x005161E4 FSTP [ESI+0x50]`, step
  `1/(Rules+0x5E8 × 900)`.
  **`SpeedCurrent` (loco+0x50) is never zeroed by the stall — it only decays at `HoverBrake`.**
- **The translation that follows:** `Move 0x00514372..0x00514383` recomputes
  `ftol(base_Speed × SpeedCurrent)` *after* `SpeedUpdate` has run, and
  `0x00514746..0x0051474C` admits the XY step on `> 0` alone.

So natively a hover unit that enters a >45° turn **keeps gliding** along its (rotating) hull
heading while the throttle bleeds off at `HoverBrake`, and only stops translating once
`ftol(base_Speed × SpeedCurrent)` reaches 0.

Stock rates (`ini/rulesmd.ini` L349-354): `HoverAcceleration=.02` → 18 ticks 0→1;
`HoverBrake=.03` → 27 ticks 1→0; `HoverBoost=150%`.

### 1.2 The precise divergence

VERA computes the same stall flag and the same braking ramp, then **additionally forces the
position to hold**:

- `src/sim/movement/movement_step.rs:316` `hover_steer(...) -> bool` returns
  `hover::hover_turning_hard(current16, desired16)` (line 357) — threshold identical, strict
  `> 0x2000`, verified against `0x00515FCF`.
- `src/sim/movement/movement_tick.rs:2437-2440`:
  ```
  if hover_stall {
      effective_speed = SIM_ZERO;
      frame_budget = 0;
  }
  ```
  The throttle ramp above (movement_tick.rs:2359-2372) is correct and matches the native
  brake step, but the lepton advancement is suppressed outright.
- The approximation is disclosed at `movement_step.rs:305-313` and pinned by the test
  `hover_mover_swings_through_corner_braking_not_freezing`
  (`src/sim/movement/movement_tests.rs:5788`), which asserts
  `"no lepton drift while stalled"` and `position held during hard turn`.

**Divergence:** native translates `Σ ftol(base_Speed × SpeedCurrent)` leptons over the stall;
VERA translates 0. Native's stall never suppresses motion on its own — it only removes the
throttle *request*.

Secondary, from the same site: the stall arm is additionally gated on
`IsFacingChangeActive` (loco+0x70) being clear (`0x00515FD1`). When that byte is set, native
does **not** zero the request at all and instead takes the proximity branch, where
`0x005160F7 MOV [ESI+0x50],0` + `0x3ff00000` pegs `SpeedCurrent` to **1.0** mid-turn. VERA has
no `+0x70` equivalent. — **UNCHECKED**: no writer of loco+0x70 appears in `Move`, `SpeedUpdate`
or `FUN_00514F70`, so how often the byte is set in active YR is unproven. Instrument: writer
search over the remaining `HoverLocomotionClass` members, then a live breakpoint at
`0x00515FD1` in a skirmish with Robot Tanks.

### 1.3 Stall duration and glide magnitude

All four stock hover units have `ROT=5`. `FacingClass::Set 0x004C9220` sets
duration = `|Δ16| / rate`; a Robot Tank's 90° corner is `0x4000`, and the stall ends when
`|Δ| ≤ 0x2000`, so the stall lasts `0x2000/(5<<8) = 6.4` → ~6-7 ticks; a 180° reversal
`0x6000/1280 = 19.2` → ~19 ticks.

Over a 6-tick stall the throttle decays 1.00 → 0.78 (mean ≈ 0.89), so native glides roughly
5.3 full-speed ticks' worth of distance through a 90° corner and ~15 through a reversal.
VERA glides zero and also freezes for those 6-19 ticks.
**UNCHECKED**: the absolute lepton figure, because the `[EAX+0x538]` base-speed units for a
hover owner are not traced here. Instrument: `emulate_function` on slot `0x538` for a `ROBO`
owner, or a retail `-win` capture measuring cells-per-second.

### 1.4 Trigger / frequency / player-visible effect

- **Trigger:** any order that makes a hover unit's next waypoint more than 45° off its current
  hull heading *while it already has throttle*. In practice: a 2-octant (90°) or larger corner
  in an A* route, a re-order that reverses direction, attack-move re-aim, scatter, formation
  re-aim. A 45° corner does **not** trigger it (strict `>`), so single-octant zig-zags are fine.
  Starting from rest is *not* a divergence: throttle is already 0 there and native holds still too.
- **Frequency in ordinary skirmish:** every multi-turn hover route hits it several times.
  `[ROBO]` (Robot Tank) is `TechLevel=2`, `Prerequisite=GAWEAP,GAROBO`, Allied/Alliance, a
  mainline massed battle tank — so on any Allied game that builds Robot Tanks this fires
  continuously. The other three users are naval transports (below), so their exposure is
  per-amphibious-assault rather than constant.
- **Player-visible effect:** retail hover craft sweep through a corner in one continuous arc,
  hull yawing while the body keeps sliding. VERA's stop dead at each >45° corner, pivot on the
  spot, then accelerate again — the stop-rotate-go signature of a tracked vehicle, on a craft
  that in retail never stops. Knock-ons: hover route times are 6-19 ticks longer per corner, so
  a hover unit falls behind a Drive escort in a mixed group; and the frozen ticks are the cause
  of the I3b residual "VERA's turn-stall holds the enable set during a >45° turn where native
  has no stall".

### 1.5 Verdict

**Row stays OPEN, but it is a real divergence with body evidence, not a stub.** Replace the
143-char text with the addresses in §1.1-§1.2 above. The fix is not "delete the hold": the
disclosed reason at `movement_step.rs:310-313` ("the path-directed crossing loop cannot absorb
a sideways cell exit") is the actual blocker, and native's model has no path-directed crossing
loop at all — it free-integrates the coordinate and discovers cell changes afterwards
(`0x00514890`-ish compare of `new>>8` against `old>>8`, then the unmark/Set_Coord/remark arm).
Note also that `movement_step.rs:312` cites "the P2b plan doc", which **does not exist**
anywhere under `docs/` (grep for `P2b` hits only that comment). Dangling reference.

---

## 2. Row I7 — "Hover locomotor phases 2/3"

Current ledger text (line 67): `| I7 Hover locomotor phases 2/3 | A9 | OPEN | hover.rs header. |`
— 66 chars.

### 2.1 The row's stated scope is already delivered

`src/sim/movement/hover.rs:14-16` (the "header" the row points at) says:

> Wiring it into a standalone hover movement path — the continuous cos/sin XY integrator that
> replaces the drive-track ride these units borrow today — is phase 2; the vertical bob/float
> controller is phase 3.

Both are wired in production today:

| Phase | Native | VERA production site | Status |
|---|---|---|---|
| 2 — XY integrator on the hull heading | `Move 0x005147F7..0x00514890` | `movement_step.rs:316 hover_steer` (sets `move_dir` = unit vector of the hull facing, `move_dir_len = 1`), called from `movement_tick.rs:2167` | wired |
| 2 — throttle (`SpeedUpdate`) | `0x00515ED0` whole body | `movement_tick.rs:2312-2373`; `target.current_speed = target.speed * new_throttle` | wired |
| 3 — vertical spring + bob | `FUN_00513D20` (called `Move 0x00514A3x`) | `movement_tick.rs:4247-4311`, every hover entity moving **or** parked; `loco.altitude`/`loco.hover_bob_offset` | wired, and rendered (`src/render/locomotor_visual.rs:136`) |

Hover no longer rides the drive track: `uses_drive_locomotor` is `LocomotorKind::Drive` only
(`movement_tick.rs:2228`), the hover arm branches before `handle_vehicle_rotation`
(`movement_tick.rs:2159-2176`), and `compute_cell_speed_modifier` returns `1.0` for Hover
(`src/sim/pathfinding/terrain_speed.rs:99-101`), so the `× cell_speed_mod` at
`movement_tick.rs:2425` is a no-op rather than a divergence.

**So the row as written is stale and should not stay `OPEN — hover.rs header`.** The `hover.rs`
header lines 14-16 are the stale prose.

### 2.2 Where VERA already matches (evidence for landing the row)

Each pair below was re-read from the body today:

- **Turn-stall threshold** — native `0x00515FB4..0x00515FCF` (strict `> 0x2000`) ==
  `hover::hover_turning_hard`. Exact, including the "45° does not stall" boundary.
- **Effective step** — native `0x00514372..0x00514383` `ftol(base_Speed × SpeedCurrent)` ==
  `target.current_speed = target.speed * new_throttle`; native's translate-only-if-`> 0` gate
  (`0x00514746..0x0051474C`) corresponds to VERA's integer `frame_budget`
  (`movement_step.rs:97-99`). (The already-recorded I3b residual — VERA divides by 15 before
  truncating, so it can translate where native's `ftol` holds still — lives here.)
- **Proximity request** — native `Distance3D ≤ 0xFF` → 0.5 (`LAB_005160D6`,
  `0x005160DD MOV [ESI+0x4c],0x3fe00000`), `Sqrt_Approx < 0x100` from `StepStart` → 0.5,
  else 1.0 (`0x005160E9/0x005160F2`) == `hover::hover_speed_request` with
  `HOVER_APPROACH_SLOWDOWN_LEPTONS = 0xFF`.
- **Boost + post-boost clamp** — native `0x00516111..0x00516155`: `SpeedMult = Rules+0x5D8`
  only when `LinkedTo+0x5E0 != -1 && == +0x5E4`, then `min(mult × request, 1.0)` ==
  `hover::hover_speed_target`. VERA derives the straightaway from the facings of the two queued
  path steps (`movement_tick.rs:2340-2354`) rather than comparing the two stored direction
  bytes — equivalent on an 8-direction path, a representation difference worth naming.
- **Ramp rates** — native `1/(Rules+0x5E0 × 900)` up, `1/(Rules+0x5E8 × 900)` down, floor 0,
  no ceiling on the brake side == `hover::hover_ramp_throttle` / `hover_ramp_step`.
  Stock `.02`/`.03` (rulesmd L353-354).
- **Vertical controller** — `FUN_00513D20` == `hover::hover_vertical_tick`, including the
  `Kscale` 1.0/1.1 period, the `2·cos` wobble, the `Gravity/3` integer kick below
  `HoverHeight/4`, the `HoverDampen` multiply and the unpowered-sink branch. Stock keys
  `HoverHeight=120`, `HoverDampen=40%`, `HoverBob=.04` (rulesmd L349-351), `Gravity=6` (L756).
- **Wake over water** — native tail `Move 0x00514A21..0x00514ABA` (`Is_Moving_Now` slot 0x80,
  `g_CurrentFrameCounter % 10 == 0`, not on bridge `+0x8C`, `CellClass+0xEC == 2`,
  `AnimClass(Rules+0x94, coord, 0, 1, 0x600, 0, 0)`) == `Simulation::spawn_wakes_for_frame`
  (`src/sim/world/mod.rs:5946` + `wake_anchor_for` at 6990), which is locomotor-agnostic and
  therefore covers hover. `Wake=WAKE1` (rulesmd L525).
- **Occupation-enable lifecycle** — already LANDED as I3b.

### 2.3 What is genuinely still missing (the residuals the row should carry)

**(a) Look-ahead facing target — ESTABLISHED, unmodelled in VERA.**
`SpeedUpdate 0x005161E7..0x005162B9` is the **last** facing write of every cruising tick:

```
005161E7 FLD  [ESI+0x48]      ; SpeedRequest
005161EA FCOMP [0x007E2800]   ; 0.0
005161F5 JNZ  0x0051630E      ; not > 0 → return
005161FB MOV  AL,[ESI+0x70]
00516200 JNZ  0x0051630E      ; IsFacingChangeActive → return
00516209 MOV  EBX,[EDI] / EBP,[EDI+4] / EDI,[EDI+8]   ; Head_To (loco+0x24)
00516211 MOV  EAX,[ECX+0x5E0]                          ; LinkedTo NextCellPath[0]
0051621B CMP  EAX,-1  → JZ skip ;  00516224 CMP EAX,8 → JZ skip
00516230 LEA  EDX,[EAX*8 + 0x89F6D8] ; 00516237 MOV EAX,[EAX*8+0x89F6D8] ; 0051623E MOV EDX,[EDX+4]
00516241 ADD  EAX,EBX ; 00516243 ADD EDX,EBP           ; target = Head_To + one cell delta
00516257..00516279 atan2(owner.Y - target.Y, target.X - owner.X) ; ftol
005162A3 ADD  ECX,0x388                                ; owner PrimaryFacing
005162AD CALL 0x004C9220                               ; FacingClass::Set (rate-limited)
```

So whenever the hover has throttle, its **body** facing is commanded at the cell *one step
beyond* the current waypoint — it leads the next corner. On the last leg (`+0x5E0` = −1 or 8)
the target collapses back to `Head_To`. VERA's `hover_steer`
(`movement_step.rs:326-346`) always aims at `target.path[target.next_index]`, the current
waypoint, and uses one facing for both render and `move_dir`.
- Trigger: every ordinary move order longer than one cell. Frequency: every cruising tick.
- Visible: retail hover hulls yaw into the next cell while still sliding toward the current
  one — a continuous lead-in arc. VERA's hull tracks the cell chain, so the path reads as
  segmented and the hull swings late.

Doc correction while here: `docs/research/HOVER_LOCOMOTION_CLASS_GHIDRA_REPORT.md` §6 calls
`FacingClass::Set` a "snap". The body at `0x004C9220` is the **rate-limited retarget**
(stores target `+0x00`, reconstructed prior `+0x04`, start frame `+0x08`, duration
`|Δ16|/rate` at `+0x10`); it snaps only when the rate is ≤ 0 or the duration is 0. The report
also does not record the look-ahead tail at all.

**(b) Two-facing split — ESTABLISHED in the body, consequence UNCHECKED.**
The XY step reads the **locomotor's own** `FacingClass` at loco+0x30
(`0x005147F7 LEA ECX,[ESI+0x2c]`, Move base), and `SpeedUpdate`'s Head_To-directed
`Set`/`UpdateFacing` at `0x00515F82`/`0x00515F90` writes that same loco+0x30. The look-ahead
tail (a) and `Move`'s decay tail (c) instead write the **owner's** facing at
`LinkedTo+0x388` (`0x005162A3`, `0x00514AED`, `0x00514B13`). VERA has exactly one facing.
- **UNCHECKED:** which of the two the renderer samples, and therefore whether the visible hull
  leads the movement vector or the two are kept in lockstep by a path I have not traced.
  Instrument: `get_xrefs_to` the unit-draw read of `TechnoClass+0x388` versus any reader of
  loco+0x30, **or** a retail `-win` capture of a Robot Tank taking a 90° corner compared frame
  by frame against VERA (the `capture-compare beats derivation` precedent).

**(c) Turn-decay tail — present in the body, reachability UNCHECKED.**
`Move 0x00514A21..0x00514B60`: while the byte at Move-base `+0x64` (loco+0x68) is set, each
tick advances the owner facing by `(signed pending delta at Move-base +0x68 (loco+0x6C) << 8)
+ Current()` via `FacingClass::UpdateFacing` (`0x00514AED` `Current`, `0x00514B13`
`UpdateFacing`, both on owner+0x388), then steps the pending magnitude one toward 0; at 0 the
flag clears. VERA has nothing.
- **UNCHECKED:** no writer of loco+0x68/+0x6C appears in `Move`, `SpeedUpdate` or
  `FUN_00514F70` (searched: `search_instructions` over each function for those operands, zero
  hits). Reachability in active YR is therefore unproven and must not be assumed.
  Instrument: writer search over the remaining `HoverLocomotionClass` members (the
  `0x00513C20..0x00517400` band), then a live breakpoint at `0x00514A2B` (the flag test) during
  a skirmish with Robot Tanks.

**(d) Deep-water and bridge arms of `Move` — UNCHECKED.**
`Move 0x00514B60..0x00514C12`: on a cell whose `CellClass+0xEC` LandType is 2 (Water) with the
unit's Z below `ground + DAT_00A8F1C0`, `Move` calls owner slot `+0xEC`. VERA's own bridge
evidence (`src/sim/movement/movement_bridge.rs:1045-1046`) identifies slot `+0xEC` as
`ObjectClass::DropIn 0x005F4160` — so the research report's "force-float-up" naming for that
slot is wrong or at least unproven. Separately, on a cell change `Move` sets the owner's
on-bridge byte `+0x8C` from `cell+0x140 & 0x100` plus a height test against `DAT_00A8F1B4`,
and clears it when the flag drops. VERA has no hover-specific arm for either.
- Instrument: retail-map harness run of an `LCRF`/`SAPC` crossing open water and a bridge,
  against a retail capture.

### 2.4 Verdict

**Re-scope the row.** "Phases 2/3" is done — both were delivered and are live in
`movement_tick.rs` and rendered. The honest ledger move is:

- Split the delivered part out as **LANDED**, citing §2.2 (each native address against its
  VERA site, with the boost-representation and sub-lepton residuals named), and
- keep an **OPEN** successor row scoped to §2.3 (a) the look-ahead facing target — which is
  established and unmodelled, so it is the one that earns a row — with (b), (c), (d) carried as
  its UNCHECKED items and their instruments.

Do **not** close the row silently: (a) is a per-tick, every-move divergence that was never
recorded anywhere, and closing "phases 2/3" without it would lose it.

---

## 3. Stock hover users — the [ROBO]/[LCRF]/[SAPC]/[YHVR] claim

**The list of four is CORRECT.** Verified against `ini/rulesmd.ini` in this worktree.
Hover CLSID `{4A582742-9839-11d1-B709-00A024DDAFD1}` occurs on five lines:

| rulesmd line | Section (header line) | Active? |
|---|---|---|
| 5283 | `[YURIPR]` (5246) | **No** — the line is `;Locomotor={4A582742-…}`, commented out. `[YURIPR]`'s live `Locomotor={4A582744-…}`, `SpeedType=Amphibious`. |
| 7056 | `[LCRF]` (7012) | yes |
| 7453 | `[ROBO]` (7417) | yes |
| 7933 | `[SAPC]` (7881) | yes |
| 8918 | `[YHVR]` (8870) | yes |

| ID | Name | SpeedType | MovementZone | ROT | Speed | Tech | Prereq | Owner |
|---|---|---|---|---|---|---|---|---|
| LCRF | Landing Craft | Hover | Amphibious | 5 | 6 | 4 | GAYARD | Allied |
| ROBO | Robot Tank | Hover | AmphibiousDestroyer | 5 | 10 | 2 | GAWEAP,GAROBO | Allied |
| SAPC | Armored Transport | Hover | Amphibious | 5 | 6 | 2 | NAYARD | Soviet |
| YHVR | Hover Transport Yuri | Hover | Amphibious | 5 | 6 | 2 | YAYARD | Yuri |

Relevant for frequency weighting: **only `[ROBO]` is a land battle unit**; the other three are
naval transports gated behind a Naval Yard. So A9's ordinary-skirmish exposure is dominated by
Allied Robot Tanks, then by amphibious assaults on water maps.

Coverage limit: stock `rulesmd.ini` only. Map and mission INI overrides are not scanned, and a
map that adds a hover `Locomotor=` would not appear above.

---

## 4. Summary of what changes in the ledger

- **I4b — keep OPEN**, replace the 143-char stub with: native `SpeedUpdate 0x00515FA5..0x00515FCF`
  (stall test) / `0x00515FD1..0x00516108` (request → 0 only) / `0x005161A7..0x005161E4`
  (brake decay, `SpeedCurrent` never zeroed) and `Move 0x00514372..0x00514383` +
  `0x00514746..0x0051474C` (translate on `ftol(Speed × SpeedCurrent) > 0`, no turn gate);
  VERA `movement_tick.rs:2437-2440` zeroes `effective_speed` and `frame_budget`, pinned by
  `hover_mover_swings_through_corner_braking_not_freezing`. Trigger/frequency/effect per §1.4.
  UNCHECKED: the loco+0x70 gate, and the absolute lepton glide.
- **I7 — split.** LANDED half per §2.2. New OPEN successor scoped to the look-ahead facing
  target (`0x005161E7..0x005162B9`), carrying the two-facing split, the turn-decay tail and the
  water/bridge arms as UNCHECKED with their instruments.
- **Doc corrections** to `docs/research/HOVER_LOCOMOTION_CLASS_GHIDRA_REPORT.md`:
  §6's "`RateTimer.Set()` (snap)" is wrong (`0x004C9220` is the rate-limited retarget);
  §4/§6 omit the look-ahead tail and the loco+0x30 vs owner+0x388 split; §4's
  "slot 0xEC (force-float-up)" conflicts with VERA's own identification of `+0xEC` as
  `ObjectClass::DropIn 0x005F4160`.
- **Code hygiene:** `src/sim/movement/hover.rs:14-16` claims phases 2/3 are future work; both
  ship. `src/sim/movement/movement_step.rs:312` cites a "P2b plan doc" that exists nowhere
  under `docs/`.
