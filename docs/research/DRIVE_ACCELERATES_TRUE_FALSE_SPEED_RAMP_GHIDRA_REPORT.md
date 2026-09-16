# Drive Accelerates True/False Speed Ramp — Ghidra Research Report

**Address(es):** `0x004B0F20` (`DriveLocomotionClass::Process_Drive_Track`), `0x004D3710` (`TechnoClass::SetSpeedFraction`), `0x004DB1A0` (`FootClass::GetCurrentSpeed`), `0x00710AF0` (`TechnoTypeClass::Constructor`), `0x00712170` (`TechnoTypeClass::ReadINI`)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** DriveLocomotion speed ramp behavior for `Accelerates=true/false`: target/current speed fractions, ramp/brake formulas, clamps/floors, stop/crush braking clamp, and INI/TechnoType fields read by the ramp.
**Non-Scope:** full `Process_Drive_Track` point stepping, full `Process_Movement` terrain target fraction computation, ship/walk/fly locomotor ramps, runtime live trace timing.
**Confidence:** High for formulas and fields verified in decompile plus assembly; Medium for naming of owner `+0x580`, which remains outside this slice.
**Active in YR:** Yes. Standard YR ground vehicles use DriveLocomotion; the `Accelerates` key is parsed at `0x00715402..0x00715416` and stock YR sets it both true by default and false on many vehicles.

## 0. Working Notes

- **Target question:** How does active YR `DriveLocomotionClass::Process_Drive_Track` ramp or snap speed for `Accelerates=true/false`, including current/target fraction ownership, floors, clamps, and read fields?
- **Non-goals:** Do not re-decode full DriveTrack stepping, pathfinding, queue arrival, or terrain target fraction selection outside direct speed-ramp implications.
- **Evidence needed to mark COMPLETE:** Decompile plus assembly for `0x004B0F20`; decompile plus assembly for `SetSpeedFraction`/`GetCurrentSpeed`; parser/default evidence for `Accelerates`, `SlowdownDistance`, `AccelerationFactor`, `DeaccelerationFactor`; current Rust touchpoint scan.
- **Stop conditions:** Stop once every branch in the `0x004B0F20` ramp block is resolved or explicitly deferred, and one zero-add pass over the ramp block adds no new open question.

## 1. Overview

Drive speed in gamemd is not owned by generic vector stepping. `DriveLocomotionClass::Process_Drive_Track` owns a Drive-local target speed fraction at `DriveLocomotion+0x50` and writes the applied/current speed fraction through `TechnoClass::SetSpeedFraction`, which clamps and stores `Techno/Foot+0x578`.

`FootClass::GetCurrentSpeed` then multiplies raw per-tick `Speed=` budget by house/veterancy/runtime factors and by `+0x578`. Therefore the ramp changes movement budget by changing the current speed fraction before budget accumulation in the same `Process_Drive_Track` call.

## 2. Class Layout / Key Offsets

| Field | Offset | Type | Meaning | Active in YR / evidence |
|---|---:|---|---|---|
| `Drive.target_speed_fraction` | `DriveLocomotion+0x50` | double | Target terrain/track speed fraction consumed by Drive ramp. | Yes; read at `0x004B1150`, `0x004B1193`, `0x004B11AF`, `0x004B11D7`, `0x004B11F1`. |
| `Drive.residual_budget` | `DriveLocomotion+0x4C` | int | Remaining movement budget carried to next tick. | Yes; added after `GetCurrentSpeed` at `0x004B1284..0x004B1295`. |
| `Drive.track_index` | `DriveLocomotion+0x58` | int | Speed ramp block only runs for `< 0x40`. | Yes; gate at `0x004B0FA8..0x004B0FB4`. |
| `Drive.point_index` | `DriveLocomotion+0x5C` | int | Track point index, not used by ramp formulas. | Yes; used later in same function, out of ramp scope. |
| current speed fraction | `Foot/Techno+0x578` | double | Applied speed fraction used by `GetCurrentSpeed`. | Yes; `SetSpeedFraction` writes it at `0x004D3710`; `GetCurrentSpeed` multiplies by it at `0x004DB20D`. |
| runtime multiplier | `Foot/Techno+0x580` | double | Separate multiplier applied before veterancy. | Yes; `GetCurrentSpeed` multiplies by it at `0x004DB1D5`; writer identity deferred. |
| braking/crush clamp flag | `Techno+0x6B5` | byte | Forces speed fraction to min(current, 0.2). | Conditional; live in Drive crush/building interaction. Read at `0x004B1143..0x004B1173`; writes seen elsewhere in `Process_Drive_Track`. |
| alternate decel flag | `Techno+0x3CD` | byte | If set and not within slowdown distance, decelerates with hardcoded `0.0015` and floor `0.1`. | Conditional; read at `0x004B10FC..0x004B1141`; producer not in this slice. |
| `SlowdownDistance` | `TechnoType+0x2F8` | int | Distance threshold in leptons for destination braking. | Yes; default 500 at ctor `0x00710AF0`; parsed at `0x0071247C..0x00712487`; read at `0x004B10B6..0x004B10BE`. |
| `DeaccelerationFactor` | `TechnoType+0x300` | double | Multiplied by raw type speed to compute decel delta. | Yes; default `0.002`; parsed at `0x0071249B..0x007124A8`; read at `0x004B10C0..0x004B10CC` and `0x004B11E1..0x004B11EB`. |
| `AccelerationFactor` | `TechnoType+0x308` | double | Added directly to current fraction when ramping upward. | Yes; default `0.03`; parsed at `0x007124BC..0x007124C9`; read at `0x004B11A3..0x004B11C0`. |
| `Accelerates` | `TechnoType+0xDBD` | byte bool | If false, skips ramp and snaps current fraction to Drive target. | Yes; default true at ctor `0x00710AF0`; parsed at `0x00715402..0x00715416`; read at `0x004B0F69..0x004B0F81`. |

## 3. Core Logic

### 3.1 Ramp Entry Gates

Active in YR: Yes. Evidence: `Process_Drive_Track` is standard Drive locomotor code, and the branch is reached after the initial moving/deploy guards.

The ramp block starts after the high-level movement guards:

1. Read type via owner vtable `+0x84`.
2. If `TechnoType+0xDBD == 0` (`Accelerates=false`), call owner vtable `+0x544` with `Drive+0x50` and skip all ramp/brake math.
3. If `Accelerates=true`, run ramp only when `Drive.track_index < 0x40` and a formation convoy guard permits it.
4. After ramp/snap handling, always call owner vtable `+0x538` (`FootClass::GetCurrentSpeed`) and add to residual budget unless this is a retry call.

Assembly anchors:

- `0x004B0F74..0x004B0F81`: read `TechnoType+0xDBD`, branch false to `0x004B1261`.
- `0x004B1261..0x004B1269`: `Accelerates=false` pushes the double at `Drive+0x50` into vtable `+0x544`.
- `0x004B0FA8..0x004B0FB4`: true-ramp block requires `Drive.track_index < 0x40` and convoy guard byte true.
- `0x004B1274..0x004B1295`: `GetCurrentSpeed`, retry-mask, add `Drive+0x4C`.

### 3.2 Current vs Target Fraction Ownership

Active in YR: Yes.

`DriveLocomotion+0x50` is the target speed fraction. `Techno/Foot+0x578` is the current applied fraction.

Evidence:

- `Accelerates=false` copies `Drive+0x50` to `SetSpeedFraction` at `0x004B1261..0x004B1269`, which is exactly a target-to-current snap.
- Up-ramp compares `Techno+0x578` against `Drive+0x50`, adds `TechnoType+0x308`, and caps at `Drive+0x50` (`0x004B1193..0x004B11C0`).
- Down-ramp compares `Drive+0x50` against `Techno+0x578`, subtracts `raw_speed * TechnoType+0x300`, and floors at `Drive+0x50` (`0x004B11D1..0x004B1202`).
- `TechnoClass::SetSpeedFraction` at `0x004D3710` clamps input to `[0.0, 1.0]` and writes `+0x578`.
- `FootClass::GetCurrentSpeed` at `0x004DB20D` multiplies by `+0x578`.

### 3.3 Formula

Active in YR: Yes.

Pseudocode, using verified roles:

```text
target = Drive+0x50
current = Techno+0x578
raw_speed = owner.vtable[0x38C]()    // same raw speed input used by GetCurrentSpeed
type = owner.GetTechnoType()

if !type.Accelerates:
    SetSpeedFraction(target)          // SetSpeedFraction clamps to [0, 1]
else if Drive.track_index < 64 and convoy guard permits:
    distance = ftol(sqrt((owner.pos - Drive.destination_adjusted_for_bridge)^2))
    decel = false

    if distance < type.SlowdownDistance:
        current = max(current - raw_speed * type.DeaccelerationFactor, 0.3)
        decel = true
    else if owner.byte_0x3CD != 0:
        current = max(current - raw_speed * 0.0015, 0.1)
        decel = true

    if owner.byte_0x6B5 != 0:
        current = min(current, 0.2)
        Drive+0x50 = current
        SetSpeedFraction(current)
    else if decel:
        SetSpeedFraction(current)
    else if current < target:
        current = min(current + type.AccelerationFactor, target)
        SetSpeedFraction(current)
    else if target < current:
        current = max(current - raw_speed * type.DeaccelerationFactor, target)
        SetSpeedFraction(current)

movement_budget = (retry ? 0 : owner.GetCurrentSpeed()) + Drive.residual_budget
```

Tiny details:

- The near-destination check is strict `< SlowdownDistance`, not `<=`. Evidence: `CMP EDI,EAX; JGE 0x004B10FC` after loading `TechnoType+0x2F8`.
- Normal destination decel subtracts `raw_speed * DeaccelerationFactor`, not just `DeaccelerationFactor`. Evidence: `FILD [ESP+0x30]; FMUL [ESI+0x300]; FSUBR [ESP+0x24]` at `0x004B10C0..0x004B10CC`.
- Upward acceleration adds `AccelerationFactor` directly, not multiplied by `raw_speed`. Evidence: `FLD [ESI+0x308]; FADD [ECX+0x578]` at `0x004B11A3..0x004B11A9`.
- Downward correction toward a lower target uses the same `raw_speed * DeaccelerationFactor` as destination braking, but floors at target, not at 0.3. Evidence: `0x004B11E1..0x004B1202`.
- Braking/crush clamp writes `Drive+0x50 = min(current, 0.2)` before calling `SetSpeedFraction`. Evidence: `0x004B1150..0x004B1173`.
- `SetSpeedFraction` clamps only to `0.0` and `1.0`; it does not apply the 0.3/0.2/0.1 floors. Evidence: `0x004D3710` decompile.
- After speed update, `GetCurrentSpeed` is called in the same `Process_Drive_Track` invocation, so the changed `+0x578` affects that tick's movement budget. Evidence: `SetSpeedFraction` sites end at `0x004B1212`, then `GetCurrentSpeed` at `0x004B1274`.

### 3.4 `FootClass::GetCurrentSpeed` Multiplier Chain

Active in YR: Yes.

Verified assembly:

- `0x004DB1B6`: `HouseClass::GetSpeedBonus`.
- `0x004DB1C3`: owner vtable `+0x38C` raw speed.
- `0x004DB1D1`: multiply by house speed bonus.
- `0x004DB1D5`: multiply by `Foot/Techno+0x580`.
- `0x004DB1E8`: `HasWeaponAbility(0)` (`FASTER`).
- `0x004DB1FA`: multiply by `Rules+0x678` (`VeteranSpeed`) if faster.
- `0x004DB20D`: multiply by `Foot/Techno+0x578` current speed fraction.
- `0x004DB226..0x004DB237`: if `WhatAmI()==1` and `+0x6CC != -1`, signed divide by 2.

The ramp's `SetSpeedFraction` result is therefore a multiplier on integer movement budget, after house speed bonus, `+0x580`, and veterancy.

## 4. INI Keys

| Key | Field | Default | Parser evidence | Ramp effect | Active in YR |
|---|---:|---:|---|---|---|
| `Accelerates=` | `TechnoType+0xDBD` byte | true | ctor `0x00710AF0` writes `1`; `0x00715402..0x00715416` reads bool and writes `+0xDBD` | false snaps current fraction to target each tick | Yes; many stock YR vehicles set false. |
| `SlowdownDistance=` | `TechnoType+0x2F8` int | 500 | ctor `param_1[0xBE]=500`; parser `0x0071247C..0x00712487` | strict distance threshold for normal destination braking | Yes. |
| `DeaccelerationFactor=` | `TechnoType+0x300` double | 0.002 | ctor writes double bits `0x3F60624D_D2F1A9FC`; parser `0x0071249B..0x007124A8` | multiplied by raw speed for destination braking and target-down correction | Yes. |
| `AccelerationFactor=` | `TechnoType+0x308` double | 0.03 | ctor writes double bits `0x3F9EB851_EB851EB8`; parser `0x007124BC..0x007124C9` | added directly to current fraction when below target | Yes. |

Stock content examples:

- `[AMCV]` has no `Accelerates=false`, so it uses default true. Active in YR: Yes.
- `[MTNK]`, `[HTNK]`, `[LTNK]`, `[YTNK]`, `[SREF]`, and many other stock vehicles set `Accelerates=false` in `rulesmd.ini`. Active in YR: Yes.
- `[DRON]` and `[CAOS]` set large acceleration/deacceleration factors but also `Accelerates=false`; the ramp block is skipped for them, so those factors do not drive normal Drive speed ramp while false. Active in YR: Conditional; parser stores the values, but Drive ramp bypasses them under `Accelerates=false`.

## 5. Integration Points

| Integration | Evidence | Active in YR |
|---|---|---|
| `Process_Drive_Track` reads `Accelerates` and updates speed before movement budget | `0x004B0F69..0x004B1295` | Yes. |
| `SetSpeedFraction` clamps and writes current fraction | `0x004D3710` | Yes; vtable `+0x544` calls from Drive. |
| `GetCurrentSpeed` consumes current fraction in the same tick | `0x004B1274`; `0x004DB20D` | Yes. |
| Convoy propagation copies current fraction to following units | `0x004B1225..0x004B125D` pushes owner `+0x578/+0x57C` to each `+0x6C8` follower's vtable `+0x544` | Conditional; UnitClass convoy chain only. |
| `Accelerates=false` snap path | `0x004B1261..0x004B1269` | Yes; stock YR vehicles use it. |

## 6. Current Rust Implementation Status

Current Rust is structurally close but not behavior-complete for this slice.

| Surface | Current status | Evidence |
|---|---|---|
| `src/rules/object_type.rs` | Parses `AccelerationFactor`, `DeaccelerationFactor`, `Accelerates`, `SlowdownDistance`; defaults mostly match current binary. Comment still says decel default `0.02`, but code uses `0.002`, which matches gamemd. | `object_type.rs` fields and parser lines around `accel_factor/decel_factor/accelerates/slowdown_distance`. |
| `src/sim/movement/drive_locomotion.rs` | Stores target fraction and snaps current fraction only when `accelerates=false`; true branch preserves current for a future ramp. | `store_drive_speed_fraction`. |
| `src/sim/movement/movement_tick.rs` | Still uses generic `MovementTarget.current_speed`, `accel_factor`, `decel_factor`, and `slowdown_distance`; actual `effective_speed` is `target.current_speed * cell_speed_mod`, not raw speed multiplied by Drive current fraction. | `movement_tick.rs` ramp block and `effective_speed` construction. |
| `src/sim/components.rs` | Has `DriveLocomotionRuntime.target_speed_fraction`, `current_speed_fraction`, `residual_budget`. | Drive runtime struct. |
| `src/sim/movement/navcom.rs` | Stop/crush clamp currently clamps `drive.current_speed_fraction` to 0.2 on stop helper, but gamemd's `+0x6B5` braking clamp is inside the Drive ramp branch and also writes target fraction. | `foot_stop_moving`/Drive stop helper; `0x004B1150..0x004B1173`. |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `DriveLocomotionClass::Process_Drive_Track` ramp block | verified | decompile `0x004B0F20`; assembly `0x004B0F69..0x004B1295` | Full point stepping belongs to other slots. |
| `Accelerates=false` behavior | verified | `0x004B0F74..0x004B0F81`, `0x004B1261..0x004B1269`; parser `0x00715402..0x00715416` | none for normal Drive ramp. |
| `Accelerates=true` up/down ramp | verified | `0x004B1193..0x004B1202` | none for normal Drive ramp. |
| destination braking floor `0.3` | verified | `0x004B10C0..0x004B10F6`; constant `0x007E6240` | none. |
| alternate decel floor `0.1` | verified | `0x004B1109..0x004B1141`; constant `0x007E6248`, rate `0x007E6250` | producer of `Techno+0x3CD` out of scope. |
| braking/crush clamp `0.2` | verified | `0x004B1143..0x004B1173`; constant `0x007E3548` | exact producers of `+0x6B5` beyond visible crush write out of scope. |
| `SetSpeedFraction` clamp | verified | `0x004D3710` | none. |
| `GetCurrentSpeed` current fraction consumption | verified | decompile + assembly `0x004DB1A0..0x004DB245` | `+0x580` semantic deferred. |
| `TechnoType` parser/defaults | verified | ctor `0x00710AF0`; parser assembly `0x0071247C..0x007124C9`, `0x00715402..0x00715416` | none for keys in scope. |
| current Rust comparison | verified | `rg`/file scan of `src/rules/object_type.rs`, `src/sim/movement/*.rs`, `src/sim/components.rs` | exact implementation pending. |

## 8. Open Questions — Final State of the Investigation Log

- `[RESOLVED] OQ-01 — Is `Accelerates=false` active in standard YR? -> Yes; parser writes `TechnoType+0xDBD`, ctor defaults true, stock `rulesmd.ini` sets false on many vehicles.` (evidence: `0x00715402..0x00715416`, `0x00710AF0`, `rulesmd.ini`)
- `[RESOLVED] OQ-02 — Does false still run ramp/brake math? -> No; it jumps to `SetSpeedFraction(Drive+0x50)` and skips the true branch.` (evidence: `0x004B0F74..0x004B0F81`, `0x004B1261..0x004B1269`)
- `[RESOLVED] OQ-03 — Which field is current speed fraction? -> `Techno/Foot+0x578`, written by `SetSpeedFraction` and multiplied in `GetCurrentSpeed`.` (evidence: `0x004D3710`, `0x004DB20D`)
- `[RESOLVED] OQ-04 — Which field is target speed fraction? -> `DriveLocomotion+0x50`, read as cap/target by the ramp and copied directly under `Accelerates=false`.` (evidence: `0x004B1150`, `0x004B1193`, `0x004B1261`)
- `[RESOLVED] OQ-05 — Is `SlowdownDistance` inclusive? -> No, braking branch is strict `<`; equality falls through to alternate/normal ramp.` (evidence: `0x004B10B6..0x004B10BE`)
- `[RESOLVED] OQ-06 — Is acceleration multiplied by raw speed? -> No; upward ramp adds `TechnoType+0x308` directly.` (evidence: `0x004B11A3..0x004B11A9`)
- `[RESOLVED] OQ-07 — Is deceleration multiplied by raw speed? -> Yes; both normal braking and target-down correction do `raw_speed * TechnoType+0x300`.` (evidence: `0x004B10C0..0x004B10CC`, `0x004B11E1..0x004B11EB`)
- `[RESOLVED] OQ-08 — What are normal braking floors? -> destination floor `0.3`, alternate flag floor `0.1`, braking/crush clamp `0.2`.` (evidence: `0x004B10D0`, `0x004B1117`, `0x004B1150`)
- `[RESOLVED] OQ-09 — Does `SetSpeedFraction` apply these floors? -> No; it only clamps to `[0.0, 1.0]`.` (evidence: `0x004D3710`)
- `[RESOLVED] OQ-10 — Is changed speed used in the same tick? -> Yes; ramp calls `SetSpeedFraction` before `GetCurrentSpeed` and budget accumulation.` (evidence: `0x004B1212`, `0x004B1274..0x004B1295`)
- `[RESOLVED] OQ-11 — Does retry call add current speed to budget? -> No; retry masks speed contribution to zero and only uses residual.` (evidence: `0x004B127A..0x004B1295`)
- `[RESOLVED] OQ-12 — Which INI defaults apply? -> `Accelerates=true`, `SlowdownDistance=500`, `DeaccelerationFactor=0.002`, `AccelerationFactor=0.03`.` (evidence: `0x00710AF0`; parser sites in Section 4)
- `[DEFERRED] OQ-13 — What exactly writes `Foot/Techno+0x580`?` (category: out-of-scope; reason: `GetCurrentSpeed` multiplier identity is adjacent but not the Drive ramp branch; next-step-if-pursued: investigate writer at `0x0048306C` and consumers)
- `[DEFERRED] OQ-14 — What are all producers of `Techno+0x3CD` alternate decel flag?` (category: out-of-scope; reason: branch behavior is verified, producer lifecycle is separate state investigation; next-step-if-pursued: xref/write audit for `+0x3CD`)
- `[DEFERRED] OQ-15 — What are all producers of `Techno+0x6B5` braking/crush clamp?` (category: out-of-scope; reason: this slot verified ramp effect, not all crush-rocking writers; next-step-if-pursued: xref/write audit for `+0x6B5`)

Adversarial checks resolved: equality at slowdown threshold, target lower than current, false plus nonzero accel factors, retry call timing, and convoy propagation all have evidence above.

## 9. Negative Facts / Do Not Do

- Do not treat `DriveLocomotionRuntime.current_speed_fraction` as the target fraction. Gamemd's target is `Drive+0x50`; current/applied is owner `+0x578`. Evidence: `0x004B1261`, `0x004D3710`, `0x004DB20D`.
- Do not implement `Accelerates=false` by mutating raw `Speed=` or terrain speed. It only calls `SetSpeedFraction(target)` and leaves raw speed pipeline intact. Evidence: `0x004B1261..0x004B1269`.
- Do not subtract plain `DeaccelerationFactor` during Drive braking. Gamemd subtracts `raw_speed * DeaccelerationFactor`. Evidence: `0x004B10C0..0x004B10CC`, `0x004B11E1..0x004B11EB`.
- Do not multiply `AccelerationFactor` by raw speed in the upward ramp. Gamemd adds it directly to the current fraction. Evidence: `0x004B11A3..0x004B11A9`.
- Do not use `<= SlowdownDistance` for destination braking. Gamemd uses strict `<`. Evidence: `CMP EDI,EAX; JGE` at `0x004B10B6..0x004B10BE`.
- Do not rely on generic `MovementTarget.current_speed` as the Drive movement authority. Gamemd changes owner speed fraction before calling `GetCurrentSpeed`, then accumulates integer residual budget. Evidence: `0x004B1212`, `0x004B1274..0x004B1295`.

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Drive target fraction (`Drive+0x50`) and current/applied fraction (`Foot+0x578`) are separate; `Accelerates=false` snaps current to target. | `0x004B1261..0x004B1269`, `0x004D3710`, `0x004DB20D` | Partially present: Rust has `target_speed_fraction/current_speed_fraction`, false snap exists, but movement still uses generic `MovementTarget.current_speed`. | `src/sim/movement/drive_locomotion.rs`, `src/sim/movement/movement_tick.rs`, `src/sim/components.rs` | Use Drive current fraction as the multiplier for Drive budget/effective speed after target fraction is computed. | MTNK/Grizzly with `Accelerates=false` on flat clear terrain stores target/current `1.0` and moves at full raw speed on first movement tick. | `drive_accelerates_false_snaps_current_fraction_and_moves_full_speed`; risk: mutating raw `Speed=` instead of fraction. |
| For `Accelerates=true`, upward ramp is `min(current + AccelerationFactor, target)`; downward correction is `max(current - raw_speed * DeaccelerationFactor, target)`. | `0x004B1193..0x004B1202`; parser/defaults `0x0071249B..0x007124C9` | Missing: true branch preserves current fraction but does not update it; generic ramp uses `MovementTarget.current_speed`. | `src/sim/movement/drive_locomotion.rs`, `src/sim/movement/movement_tick.rs` | Add a Drive-owned ramp helper with raw per-tick speed input and target/current fractions; run before budget/position advancement. | AMCV from rest with default `0.03` reaches `0.03`, `0.06`, `0.09` fractions on first three normal movement frames until capped by target. | `drive_accelerates_true_ramps_fraction_by_acceleration_factor`; risk: multiplying accel by raw speed. |
| Destination braking starts only when `distance < SlowdownDistance` and applies `max(current - raw_speed * DeaccelerationFactor, 0.3)`. | `0x004B10B6..0x004B10F6`; default threshold/factor from ctor/parser | Mismatch: Rust generic decel subtracts `decel_factor` directly and uses `MovementTarget.current_speed`; code comment default mismatch says `0.02` while code uses `0.002`. | `src/sim/movement/movement_tick.rs`, `src/rules/object_type.rs` comments/tests | Drive braking must be fraction-based and raw-speed-scaled, with strict threshold and floor `0.3`. | AMCV at exactly 500 leptons from destination does not enter braking; at 499 leptons subtracts `raw_speed * 0.002` but not below `0.3`. | `drive_destination_brake_uses_strict_slowdown_distance_and_scaled_decel`; risk: `<=` or unscaled decel. |
| Retry calls do not add new speed budget; only residual carries. | `0x004B127A..0x004B1295` | Unchecked in generic tick; Drive residual exists but is not movement authority. | `src/sim/movement/movement_tick.rs`, `src/sim/movement/drive_track.rs` | Preserve Drive residual integer budget and mask speed contribution on retry/reentrant DriveTrack processing. | A retry/chained call consumes only previously stored residual and does not double-add current speed. | `drive_retry_process_uses_residual_without_new_speed_budget`; risk: adding `dt` movement twice. |

### Stale Docs / Follow-up Docs

- [docs/research/traces/MCV_DRIVE_10_CELLS_STRAIGHT_FLAT_GRASS_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/MCV_DRIVE_10_CELLS_STRAIGHT_FLAT_GRASS_TRACE.md): replace “current < target: `current += AccelerationFactor × tick`” with “current fraction (`Foot/Techno+0x578`) increases by `AccelerationFactor` per normal Drive frame, capped at Drive target fraction (`Drive+0x50`); no raw-speed multiplier on upward acceleration.”
- [docs/research/traces/MCV_DRIVE_10_CELLS_STRAIGHT_FLAT_GRASS_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/MCV_DRIVE_10_CELLS_STRAIGHT_FLAT_GRASS_TRACE.md): replace “within SlowdownDistance=500: `current -= DeaccelerationFactor × speed × tick`” with “within strict `< SlowdownDistance`: current fraction decreases by `raw_speed(vtable+0x38C) * DeaccelerationFactor`, floored at `0.3`.”
- [docs/research/PROCESS_DRIVE_TRACK_DECOMPILATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PROCESS_DRIVE_TRACK_DECOMPILATION.md): rename the field table entry `+0x50 current_speed (double)` to `+0x50 target_speed_fraction (double)` or at least note it is the Drive target fraction, while owner `+0x578` is the applied/current fraction consumed by `GetCurrentSpeed`.
- [docs/research/DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_MOVEMENT_GHIDRA_REPORT.md): in Phase 5, replace “locomotor.current_speed = base_speed” with “locomotor `+0x50` target speed fraction = base terrain/health/slope fraction; `SetSpeedFraction`/owner `+0x578` is the applied current speed fraction.”

## Sources

- Ghidra read-only decompile: `DriveLocomotionClass::Process_Drive_Track @ 0x004B0F20`
- Ghidra read-only assembly context: `0x004B0F69..0x004B1295`
- Ghidra read-only decompile: `TechnoClass::SetSpeedFraction @ 0x004D3710`
- Ghidra read-only decompile and assembly context: `FootClass::GetCurrentSpeed @ 0x004DB1A0`
- Ghidra read-only decompile: `DriveLocomotionClass::Constructor @ 0x004AF540`
- Ghidra read-only decompile: `TechnoTypeClass::Constructor @ 0x00710AF0`
- Ghidra read-only decompile and assembly context: `TechnoTypeClass::ReadINI @ 0x00712170`, especially `0x0071247C..0x007124C9` and `0x00715402..0x00715416`
- `ini/rulesmd.ini`
- `src/rules/object_type.rs`
- `src/sim/movement/drive_locomotion.rs`
- `src/sim/movement/movement_tick.rs`
- `src/sim/components.rs`
