# A3 "Drive turning and tracks" — acceptance examination

Read-only. Worktree `…/.claude/worktrees/vera20k-yuris-revenge-movement-bc5040`,
branch `feature/vera20k-yuris-revenge-movement-bc5040` @ `eed60afa`.
Binary: `gamemd.exe` in Ghidra project `testProsjekt`, image base `0x00400000`,
10073 functions. No file edited, no cargo run.

## What was actually read

Native, in full: `DriveLocomotionClass::Process_Drive_Track @ 0x004B0F20`
(decompiled; disassembled — 1656 instructions, read the ramp block
`0x004B0F20..0x004B1297` line by line, plus the loop head/tail, the facing
update, the terminal credit and the post-loop interpolation).
`DriveLocomotionClass::Process_Movement @ 0x004B2630` (decompiled in full; of
its 2600-instruction disassembly I read the facing gate, the selector finalize
and the cursor init windows, not the whole listing).
`FacingClass::UpdateFacing @ 0x004C9300`, `FootClass::GetCurrentSpeed @ 0x004DB1E8`.

Native data read byte-for-byte: TurnTrack table `0x007E7B28` (12-byte stride,
entries 0..7), RawTrack table `0x007E7A28` (16-byte stride, entries 0..5),
raw-track-1 point tail `0x007E6348..0x007E6377`, constants `0x007E6240`,
`0x007E6248`, `0x007E6250`, `0x007E3548`, `0x007E7FB0`, `0x007E7FB8`,
`0x007E1718`. Instruction searches for writers of `+0x3CD` and `+0x6C8`.

Rust: `src/sim/movement/drive_track.rs` (header, `RawTrack`/`TurnTrack` decls,
`RAW_TRACKS`, `plan_drive_track_from_path`, `select_drive_track`,
`begin_*_drive_track`, `advance_drive_track_with_budget_mode`,
`interp_sub_step` region), `src/sim/movement/drive_locomotion.rs` (all of it),
`src/sim/movement/movement_step.rs` (`advance_shared_track`,
`apply_track_residual`, `finish_shared_track`, `configure_motion_after_transition`,
`handle_vehicle_rotation`, budget helpers), `src/sim/movement/movement_tick.rs`
(ramp host `2195..2440`, chain adoption `1400..1475`),
`src/sim/movement/track_head.rs` (`accept_fresh_progress`),
`src/rules/object_type.rs` (ramp key parsing).
No claim below rests on `docs/research/traces/`.

Evidence classes used: **native established** = body/disassembly/data read here.
**Rust code-read** = current source read here. No runtime capture, no
`emulate_function` run, no cargo — so nothing here is *parity demonstrated*.

---

## Verdict

A3 is **not clean**. Turn-table curve selection and the ROT/facing contract
match gamemd. The **residual budget does not**: VERA's fresh-curve cursor is
off by one against the native read-before-increment loop, and the native
end-of-curve budget credit is absent — together roughly a 6% per-cell speed
excess for every ground vehicle. The `Accelerates` ramp reproduces the three
main arms but omits four native arms, two of which (the sinking brake band and
the crush clamp) are player-visible.

| Area | Verdict |
|---|---|
| Turn-table curve selection | MATCHES (D5/D8 aside) |
| ROT / rotate-in-place gate | MATCHES |
| Residual budget | DIVERGES — D1, D2, D10 |
| `Accelerates` ramp | PARTIAL — D3, D4, D5, D6, D7, D8 |

---

## Where VERA matches

**M1 Selector formula `from_octant*8 + to_octant`.**
Native `0x004B4016 LEA EAX,[ESI + EBX*0x8]` (ESI = next path direction,
EBX = current facing octant) → `0x004B401D MOV [EBP+0x58],EAX`.
VERA `drive_track.rs:3834` `turn_index = from_dir * 8 + to_dir`. The 72-entry
table is indexed by the octant pair, not by turn angle — the header comment in
`drive_track.rs` is correct.

**M2 Null-track fallback `from_octant*9`.**
Native `0x004B4023 MOV CL,byte [EAX*4 + 0x7E7B28]` (entry byte +0, the normal
raw-track index); `0x004B402C JNZ` past `0x004B402E LEA ECX,[EBX + EBX*0x8]`,
`0x004B4031 MOV [EBP+0x58],ECX`. VERA `drive_track.rs:3833-3837` — identical,
including the comment that `from*9` is never itself null.

**M3 Normal/short variant byte.**
Native `0x004B4019` clears `drive+0x60` on every fresh selection; the loop head
`0x004B1526..0x004B153A` reads entry byte +1 when `drive+0x60` is set and byte
+0 otherwise. VERA carries the same two fields (`normal_track`, `short_track`)
and `plan_drive_track_from_path` / the chain path both pass `use_short = false`.

**M4 Two-node path consumption flag.**
Native `0x004B403A TEST byte [EDX*0x4 + 0x7E7B30],0x8` — bit 3 of the entry's
`+0x08` flags dword decides whether the selection consumes one or two path
nodes. VERA `TURN_TRACK_TURNS_FLAG = 0x08` (`drive_track.rs:3731`), used at
`:3855` for the head delta and `:3877` for `nodes`. Bit 3 is correctly excluded
from the mirror/flip transform (`transform_flags = flags & 0x07`).

**M5 TurnTrack payload.** Entries 0..7 at `0x007E7B28` decode to
`{1,0,0x00,0}`, `{3,7,0x20,8}`, `{4,9,0x40,8}`, `{0,0,0x60,0}`,
`{0,0,0x80,0}`, `{0,0,0xA0,0}`, `{4,9,0xC0,0x0A}`, `{3,7,0xE0,0x0A}` — byte for
byte equal to `TURN_TRACKS[0..8]` in `drive_track.rs:733..`.
*Coverage limit: 8 of 72 entries sampled.*

**M6 RawTrack payload.** Entries 0..5 at `0x007E7A28` give
(chain `+0x04`, entry `+0x08`, handoff `+0x0C`) =
`(0,0xC0,0)`, `(-1,0,-1)`, `(-1,0,-1)`, `(37,12,22)`, `(26,11,19)`, `(45,15,31)`
— equal to `RAW_TRACKS[0..6]`. *Coverage limit: 6 of 16 entries sampled.*

**M7 Facing→octant quantizer.** Native `((f >> 4) + 1) >> 1 & 7`
(`0x004B22DC` block and the chain block). VERA
`util::direction::direction_from_facing` = `(f.wrapping_add(16) / 32) & 7`.
Algebraically identical across 0..255 (checked at every 16-lepton boundary,
including the 0xF0 wrap).

**M8 The ROT gate is an exact-facing precondition, and VERA has it.**
Native `0x004B3408..0x004B344C`: `FacingClass::Current(owner+0x388)` →
`AX`; `CX = path_head_dir << 13`; `EAX = abs((short)(AX - CX))`;
`0x004B3439 JGE` — any **non-zero** difference takes
`0x004B344C CALL [ECX+0x4C]` with `CX` as the desired facing and
`0x004B3452 MOV AL,1 / RET` — i.e. the mover sets desired facing, takes **no**
step, and no track is selected. There is no ROT tolerance band.
VERA `drive_track.rs:3819-3825` `DriveTrackDecision::TurnFirst { desired_facing }`
with `desired_facing = from_dir * 0x20` (== `dir << 13` in the 16-bit frame),
consumed at `movement_step.rs:219` and `:259-266`. The in-place turn itself runs
through `handle_vehicle_rotation` on a `FacingClass` seeded at the rules ROT.

**M9 No ROT limiting *inside* a curve — the hull snaps to the track point.**
Native `0x004B1AAB MOV CH,byte [ESP+0x30]` (the transformed track-point facing),
`CL = 0`, `0x004B1ABB ADD ECX,0x388`, `0x004B1AC1 CALL 0x004C9300`.
`FacingClass::UpdateFacing @ 0x004C9300` writes both the current and the desired
facing and zeroes the rotation duration — an instant snap.
VERA `movement_step.rs:1864` `*facing = advance.facing; *facing_target = None`.

**M10 Per-tick budget arithmetic and carried residual.**
Native: `0x004B1274 CALL [EDX+0x538]` (GetCurrentSpeed),
`0x004B1287..0x004B1295` `budget = (param2 ? 0 : speed) + drive+0x4C`;
`0x004B1553` / `0x004B1F50` loop condition `budget > 7`;
`0x004B159D SUB EDI,0x7` per step; `0x004B1F64 MOV [EBP+0x4C],EAX` saves the
leftover. VERA `drive_track.rs:4140-4178` is the same arithmetic
(`fresh_budget + residual`, `while budget > 7 { budget -= 7 }`, save back).
Step **count** per tick and the carried residual agree exactly — the VERA test
`drive_track_first_native_frame_uses_native_frame_budget` pins budget 11 → one
step, residual 4, which is what native does too. (What diverges is *which*
point that step renders — see D1.)

**M11 Sub-step interpolation shape.**
Native post-loop: `0x004B1F67` return when `budget <= 0`; `0x004B1F73` return
when selector `< 0`; `0x004B23C6 FMUL 0x007E7FA8` → `ScaleByFactor(delta,
residual * 0.14285715)`; trust gate
`(cell(interp) == cell(before)) || (cell(interp) == cell(full)) || (3 < residual)`.
VERA `interp_sub_step` + `INTERP_TRUST_BUDGET = 3` reproduces all four terms.
Native scales in `float`, VERA in integer truncation — sub-lepton, not listed
as a divergence.

**M12/M13/M14 The three modelled ramp arms.**
- Arrival brake `0x004B10BE..0x004B10FA`: `frac -= rawSpeed × TechnoType+0x300`,
  floor `0x007E6240` = `0.30000001192092896` (a widened `0.3f`).
  VERA `DRIVE_DESTINATION_BRAKE_FLOOR = 0.3`, same two terms.
- Accelerate `0x004B11A3..0x004B11C0`: `frac = TechnoType+0x308 + owner+0x578`,
  clamped down to `drive+0x50`. VERA identical.
- Decelerate-to-target `0x004B11E1..0x004B1202`:
  `frac = owner+0x578 − rawSpeed × TechnoType+0x300`, clamped up to `drive+0x50`.
  VERA identical.
VERA's note that the locomotor-owned target is *not* clamped natively (the clamp
lives in `TechnoClass::SetSpeedFraction @ 0x004D3710`) is confirmed: every arm
here reaches vtable `+0x544`.

**M15 `Accelerates=false`.** Native `0x004B0F74 MOV CL,byte [EAX+0xDBD]`,
`0x004B0F81 JZ 0x004B1261` → `0x004B1269 CALL [EDX+0x544]` with `drive+0x50`
straight through. VERA `!accelerates → current = target.clamp(0,1)`.

**M16 Chain adoption cursor.** Native `0x004B1CA4 MOV [EBP+0x5C],ECX` with
`ECX = RawTrack+0x08 − 1`, then the loop tail's `INC`. VERA
`movement_tick.rs:1457` `new_track.point_index = sel.entry_index - 1`, with a
comment that says exactly why. The chain "from" direction is the **completed
curve's `target_facing`** (`movement_tick.rs:2857`), matching native's read of
entry byte `+0x04` rather than the live interpolated hull facing.

---

## Divergences

### D1 — Fresh-curve cursor off by one: every curve runs one point short and one point ahead

- **Native address.** Cursor init `0x004B4659 MOV dword [EBP+0x5C],0x0`
  (the common selection finalize in `Process_Movement`). Loop head
  `0x004B1596 MOV EAX,[EBP+0x5C]` → `0x004B15A7 MOV EDX,[EBX + ECX*0x4]`
  reads `points[cursor]` and the body places the object there; the increment is
  at the loop **tail**, `0x004B1F4F INC ECX` / `0x004B1F53 MOV [EBP+0x5C],ECX`.
  So on a fresh curve the first paid point renders `points[0]`, and after N
  paid points the object stands on `points[N-1]` with cursor = N.
- **VERA.** `begin_selected_drive_track` (`drive_track.rs:3888-3898`) sets
  `point_index = 0`; `advance_drive_track_with_budget_mode`
  (`drive_track.rs:4143-4146`) increments **before** reading, so the first paid
  point renders `points[1]` and `points[0]` is never occupied. After N paid
  points the object stands on `points[N]`.
- **Corollary at the far end.** Native terminates by reading the `(0,0)`
  sentinel one slot past VERA's array — verified for raw track 1 at
  `0x007E636C` (`x=0,y=0,f=0`), whose 23 stored points run `y=245 … y=3`
  (`0x007E6360` = `(0,3,0)`). VERA's loop guard `point_index < last_index` stops
  at index 22 and `finish_shared_track` snaps the remaining 3 leptons to the
  stored head for free.
- **Arithmetic for raw track 1** (the straight curve, the commonest one):
  native pays 23×7 = 161 to reach `points[22]`, then a 24th 7 to read the
  sentinel, then credits back `ftol((1.0 − manhattan/11.0) × 7.0)` = 5 → **163
  budget for the 256-lepton cell**. VERA pays 22×7 = **154**. ≈5.8% cheaper.
- **Trigger.** Any ground-vehicle move order. Every freshly selected curve: the
  first cell of every order, every cell where the exact-facing gate forced a
  turn first, and every cell where chaining was declined.
- **Frequency.** Continuous — every Drive and Ship mover, every cell, every match.
- **Player-visible effect.** Ground vehicles cross ground about 5–6% faster
  than gamemd, and at every intermediate tick they sit one track point
  (≈11 leptons ≈ 4% of a cell, plus the matching facing sample on a turning
  curve) further along the arc than gamemd. Two identical tanks racing in VERA
  and in retail separate by roughly a cell per ~18 cells travelled.
- **Downstream risk.** `point_index` is `u16`, so the chained path already
  works around this by storing `entry_index - 1` — a fresh curve cannot use the
  same trick at index 0. Fixing it means either changing the cursor to a signed
  type or moving the read to before the increment, and every test that pins
  `point_index` or `drive.track.cursor` moves with it.
- **Evidence class.** Native established (disassembly + point data). Rust
  code-read. **Not** runtime-measured — the 5.8% is arithmetic over the read
  data, not a capture.

### D2 — End-of-curve budget credit is not modelled

- **Native address.** `0x004B1FB3..0x004B1FF9`:
  `EBX = |head.x − obj.x| + |head.y − obj.y|`; `FILD`; `FMUL [0x007E7FB8]`
  (= 1/11 = 0.0909090909…); `FSUBR [0x007E1718]` (= 1.0);
  `FMUL [0x007E7FB0]` (= 7.0); `ftol`; `0x004B1FF9 ADD EBX,EAX` — the credit is
  **added** to the running budget at the terminal point.
- **VERA.** `finish_shared_track` (`movement_step.rs:1732-1760`) copies the
  stored head and returns; the residual is untouched.
- **Trigger / frequency.** Every completed curve, i.e. every cell a Drive mover
  finishes. **Downstream risk:** correcting D1 alone still leaves this term, so
  the per-cell budget would then be short by up to 7.
- **Effect.** Part of the same per-cell speed error as D1; on its own it is
  worth up to 7 budget units (≈11 leptons) per cell.

### D3 — `Accelerates` second brake band (the sinking latch) missing

- **Native address.** `0x004B10FC..0x004B1141`: when `dist >= SlowdownDistance`
  **and** `owner+0x3CD != 0`, `frac -= rawSpeed × 0.001500000013038516`
  (`0x007E6250`) with floor `0.10000000149011612` (`0x007E6248`), and the brake
  flag is set so neither the accel nor the decel arm runs.
- **`+0x3CD` identity — now settled.** Exhaustive instruction search for
  `MOV byte [reg+0x3cd], 0x1` returns exactly five writers:
  `JumpjetLocomotionClass::State5_Touchdown 0x0054CEB7`,
  `TemporalClass::AI 0x00629C69`,
  `TeleportLocomotionClass::PostWarpValidation 0x0071896B` and `0x00718AC2`,
  `UnitClass::ReceiveDamage 0x00737E51`.
  Ghidra's own field label at the read site `0x0070B5AE` is `IsSinking`
  (`docs/research/BODY_ROCKING_GHIDRA_REPORT.md`). It is the sinking/falling
  latch, not a generic damage flag. This upgrades the "identity is UNCHECKED"
  note in `drive_locomotion.rs:186-200`.
- **VERA.** `update_vehicle_speed_fraction` (`drive_locomotion.rs:290-312`) has
  no arm for it.
- **Trigger.** A vehicle destroyed into water; a chrono-teleport landing
  validated onto invalid ground; a Yuri Temporal warp-out; a jumpjet touchdown.
- **Frequency.** Every sinking wreck, plus every Chrono Legionnaire /
  Chronosphere / jumpjet landing. Common in naval and Yuri matchups; a handful
  of visible moments per ordinary match.
- **Effect.** gamemd lets the doomed/warping vehicle coast down to 10% of its
  speed while it sinks or lands; VERA keeps it on the ordinary ramp, so the
  wreck drifts at near-full speed until the sink animation takes it.
- **Downstream risk.** VERA has no equivalent of `+0x3CD` at all on the drive
  path — landing this means wiring a sinking/falling latch first.

### D4 — Crush speed clamp missing

- **Native address.** `0x004B1143..0x004B117D`: while `owner+0x6B5 != 0` the
  whole ramp is replaced by `frac = min(drive+0x50, 0.2)` (the compare constant
  is the exact double `0.2` at `0x007E3548`), and `0x004B1173 FSTP [EBP+0x50]`
  **writes it back to the locomotor-owned target**, not just the owner slot.
  `+0x6B5` is raised at `0x004B1A2F` when the mover steps onto a crushable.
- **VERA.** Not modelled. Already recorded at `drive_locomotion.rs:236-248`;
  confirmed here against the disassembly, including the write-back.
- **Trigger.** A crusher mid-crush.
- **Frequency.** Common. The block lives inside the `Accelerates` branch, and
  of the 29 `Crusher=yes` types in stock `rulesmd.ini` 15 accelerate — including
  `[APOC]`, both MCVs, `[V3]`, every ore miner and the amphibious transports.
  (That count is carried from the existing header note; not re-derived here.)
- **Effect.** Retail slows to a fifth speed while driving over infantry; VERA
  does not slow at all.

### D5 — `drive+0x58 >= 0x40` / `Passive=` gate: native skips the call entirely, VERA ramps

- **Native address.** `0x004B0FA8 CMP dword [EBP+0x58],0x40; 0x004B0FAC JGE
  0x004B126F` and the `Passive` arm `0x004B0F8F..0x004B0FA4`
  (`UnitTypeClass+0xE0C`), `0x004B0FB4 JZ 0x004B126F`. `0x004B126F` is **past**
  the `SetSpeedFraction` at `0x004B1269`, so on either gate
  `Process_Drive_Track` makes **no** speed-fraction call at all — the owner's
  `+0x578` keeps last frame's value.
- **Doc correction.** The comment at `drive_locomotion.rs:216-219` says native
  "skips the `drive+0x50` write *and* the whole ramp, **setting the owner
  fraction directly**". The direct-set behaviour belongs to
  `Process_Movement` (`0x004B3DFA CMP [EBP+0x58],0x40`, whose else-arm calls
  `SetSpeedFraction` only when the value differs from `owner+0x578`), not to
  this function. In `Process_Drive_Track` the gate means *no write*.
- **Trigger / frequency / effect.** As already recorded: only a live
  `Force_Track` selector ≥ 0x40, i.e. Yuri Tank Bunker install/eject curves
  (`0x00458E50`, `0x004593A0`, `0x004595C0`). Zero in any match without a
  garrisoned `[NATBNK]`.

### D6 — Convoy speed-fraction propagation missing

- **Native address.** `0x004B1218..0x004B125F`: after the ramp, if
  `What_Am_I() == 1` (UnitClass) and `owner+0x6C8 != 0`, walk the `+0x6C8`
  chain and call each link's `SetSpeedFraction(owner+0x578, owner+0x57C)`; the
  walk stops on null or on a self-referencing link.
- **`+0x6C8` identity.** Instruction search for writers gives, on the UnitClass
  side: `UnitClass::Constructor 0x007353EC`,
  `ScenarioClass::Read_Units_Section 0x0074368C` and `0x0074369B`,
  `UnitClass::PointerExpired 0x007446FF`,
  `UnitClass::Transfer_Convoy_On_Owner_Change 0x007463CF`. Only the scenario
  reader creates a non-null link. (The same reading is in
  `docs/research/CONVOY_FORMATION_SYSTEM_GHIDRA_REPORT.md` and
  `CHANGEOWNER_SUBCLASS_WRAPPERS_RESWARM_20260528.md`; I confirmed the writer
  set from the binary rather than taking the docs.)
- **VERA.** No convoy chain, no propagation.
- **Trigger.** A map whose `[Units]` section declares a follower chain.
- **Frequency.** **Zero in ordinary skirmish** — nothing but the scenario
  reader populates the field.
- **Effect.** On campaign/scripted maps, convoy followers would not inherit the
  leader's throttle. Not a skirmish parity blocker.

### D7 — Brake-distance metric

- **Native address.** `0x004B0FBA..0x004B1082`: the distance fed to the brake
  gate is the **3-D** length from the owner's exact coordinate to `drive+0x34`
  with the destination Z **replaced** by
  `CellClass::GetGroundHeight(dest) + (dest bridge ? g_BridgeZOffset_Drive : 0)`
  (`0x004B0FE1..0x004B1001`), through `Sqrt_Approx` then `Math__ftol`, compared
  as an integer against `TechnoType+0x2F8` at `0x004B10BC`.
- **VERA.** `movement_tick.rs:2244` uses `distance_to_goal_leptons` — a 2-D
  lepton distance to the final-goal **cell** — and adds `BRIDGE_Z_OFFSET` only
  for water movers.
- **Trigger.** Braking into a destination at a different height: a ramp, a
  cliff top, a bridge deck approached by a ground mover.
- **Frequency.** Every arrival on sloped or bridged terrain.
- **Effect.** gamemd's distance is larger, so it enters the brake band a few
  frames earlier than VERA. Sub-cell; visible only as arrival-timing drift.
- **Same block, smaller term:** VERA passes `raw_speed_per_frame =
  target.speed / 15` as a fractional fixed-point value, where native `FILD`s the
  **integer** in `[ESP+0x30]` from vtable `+0x38C` (`0x004B10B2`, used at
  `0x004B10C0` and `0x004B11E1`). Sub-lepton per frame; compounding only inside
  a long brake.

### D8 — `drive+0x50` written unconditionally

- **Native.** `Process_Movement` writes `drive+0x50` only under
  `0x004B3DFA CMP dword [EBP+0x58],0x40` (`< 0x40`); `Process_Drive_Track`
  writes it only on the crush clamp (`0x004B1173`).
- **VERA.** `update_vehicle_speed_fraction` (`drive_locomotion.rs:287`) does
  `*target_slot = target_fraction` on every call, unconditionally.
- **Trigger / frequency.** Only when a forced (≥ 0x40) curve is live — the
  Tank Bunker family again, so effectively zero in non-Yuri skirmish.
- **Effect.** The retained locomotor target is clobbered during a forced curve.

### D9 — "already at target" arm (recorded, not a defect)

Native `0x004B11D1..0x004B11DF`: when `owner+0x578 == drive+0x50` and neither
brake band fired, `TEST AH,0x41; JNZ 0x004B1218` skips the `SetSpeedFraction`
call entirely. VERA falls through and writes `current.clamp(0,1)`. Because the
owner value is already clamped by `SetSpeedFraction`, the written value equals
the existing one. No behavioural difference; recorded so a future reader does
not "fix" it into one.

### D10 — Residual reset condition is narrower in VERA

- **Native address.** The entry gate `0x004B0F2C..0x004B0F63` jumps to
  `0x004B25F2 MOV dword [EBP+0x4C],0x0` — the residual is zeroed on **every**
  frame where the locomotor has no committed head (`drive+0x63 == 0`) **or**
  selector `== -1`, provided the path head is not tube sentinel 8; and also on
  the `drive+0x62` / `TechnoType+0xCA1` gate.
- **VERA.** The only production site that zeroes the owner residual is
  `movement_step.rs:2134-2140`, reached when `target.next_index >=
  target.path.len()` (path exhausted). `track_head::accept_fresh_progress`
  deliberately **preserves** the residual across a fresh selection — which is
  correct, since native's `Process_Movement` does not clear it either.
- **Trigger.** Any frame where a Drive mover has a movement target but no live
  curve: most importantly the frames spent rotating in place under the
  exact-facing gate (M8), and the frames between a declined chain and the next
  selection.
- **Frequency.** Every in-place turn (i.e. every direction change sharper than
  the curve set can absorb) and every declined chain. Common.
- **Effect.** VERA carries up to 7 stale budget units into the next curve — a
  free part-step of up to ~11 leptons — where gamemd starts each curve from
  zero. Small individually; it stacks with D1/D2 in the same direction.
- **Confidence caveat.** Native side established from the disassembly. The VERA
  side is a grep-complete search for `residual = 0` / `residual: 0` /
  `track.residual` under `src/sim/movement/` plus `src/sim/components.rs`; I did
  not trace every path that could construct a fresh `DriveLocomotionRuntime`.

---

## UNCHECKED — what would settle each

1. **The 5.8% per-cell figure in D1.** Arithmetic over raw track 1 only. To
   settle: a retail `-win` motion-over-time capture (a single Grizzly crossing a
   measured straight run) against the same run in `--release`, or a Unicorn
   harness driving `Process_Drive_Track` over a synthetic track and counting
   budget to the sentinel. No such capture exists today — the A3 row's
   "no retail motion-over-time capture exists" note still holds.
2. **TurnTrack entries 8..71 and RawTrack entries 6..15.** I read 8 and 6
   respectively. To settle: read `0x007E7B28 + 12*i` for i = 8..71 and
   `0x007E7A28 + 16*i` for i = 6..15 and diff against `TURN_TRACKS` /
   `RAW_TRACKS`. Cheap; worth doing before A3 is signed off.
3. **Native defaults for `AccelerationFactor` / `DeaccelerationFactor` /
   `SlowdownDistance`.** The reads are `TechnoTypeClass::ReadINI @ 0x007124A8`
   (`+0x300`, deacceleration) and `0x007124C9` (`+0x308`, acceleration); I did
   not read the default operand or the `TechnoTypeClass` constructor
   initialisers. VERA uses 0.03 / 0.002 / 500. To settle: disassemble
   `0x00712480..0x007124D0` for the pushed defaults, or read the ctor stores.
   Note in passing: `src/rules/object_type.rs:286` says "Default 0.02" while
   `:1656` parses `0.002` — an internal doc/code mismatch regardless of which
   is native.
4. **Exact gameplay condition inside `UnitClass::ReceiveDamage` that raises
   `+0x3CD`.** I have the write address `0x00737E51` and the field identity, not
   the surrounding predicate. To settle: decompile
   `UnitClass::ReceiveDamage` and read the branch guarding `0x00737E51`.
5. **Ship divergences.** VERA adds `SHIP_MAX_SHARED_RAW_TRACK = 13`
   (`drive_track.rs:3737`), forcing Ship selections above raw track 13 back to
   `from*9`. Ship runs its own `Process_Movement` (the header cites
   `0x006A12A3`), which I did not open — this is out of A3's Drive scope but the
   constant is VERA-internal and unproven. To settle: read the Ship
   `Process_Movement` selector finalize.
6. **Whether the native chain "needs chaining" flag being computed once per
   frame (`0x004B1553` block, from `owner+0x5E0` read at `0x004B128F`) can
   diverge from VERA's per-chain-point evaluation (`movement_tick.rs:2859`)
   when two chains land in one frame.** I could not construct a case; to settle
   would need a stepped trace with a high-speed mover on a two-chain curve.

## Note on prior-work quality

Everything in `drive_track.rs` / `drive_locomotion.rs` that I could check
against the binary was accurate, including the several "this is not modelled"
confessions. The two corrections this pass produces are D5's description of
what the `< 0x40` gate does in *this* function (no call, not a direct set) and
D3's identity of `+0x3CD` (the sinking latch — no longer UNCHECKED). The real
gap the A3 row was hiding is D1/D2/D10: the residual budget, not the ramp.
