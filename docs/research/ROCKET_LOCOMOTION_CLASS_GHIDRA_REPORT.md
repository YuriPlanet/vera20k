# RocketLocomotionClass — Ghidra Research Report

**Primary Address:** `0x006622C0` (Process), `0x00661EC0` (Constructor)
**IUnknown VTable:** `0x007F0BE8` (separate from ILocomotion)
**ILocomotion VTable:** `0x007F0B1C`
**CLSID:** `{B7B49766-E576-11d3-9BD9-00104B972FE8}`
**Confidence:** HIGH for struct layout, RocketStruct field offsets, phase machine, and INI key bindings.
MEDIUM for the Phase 0 → 1 entry path (likely `Move_To_Coord` at `0x006632E0` but the function is unregistered in Ghidra and only its prologue was decoded).
**Active in YR:** Yes — used by exactly 3 sub-units (`V3ROCKET`, `DMISL`, `CMISL`) launched by V3 Launcher / Dreadnought / Boomer Sub respectively.

## 1. Overview

RocketLocomotionClass is the COM locomotion controller for ballistic missile sub-units
that run a fixed multi-phase trajectory (pause → tilt/raise → boost → cruise → descent →
detonate). Unlike Fly/Hover/Drive which are continuous "go to cell" controllers, the
rocket locomotor is a **state machine that runs once from launch to impact**. The owner
unit (the missile body itself, e.g. `V3ROCKET`) is destroyed at impact via the locomotor
calling its detonation helper.

**Active YR units bound to this CLSID** (verified via `rulesmd.ini` grep at lines
11389, 11429, 11472):

| ID | Display Name | Launched By | Notes |
|----|--------------|-------------|-------|
| V3ROCKET | V3 Rocket | V3 Rocket Launcher (V3) | Soviet, lazy-curve trajectory |
| DMISL | Dreadnought Missile | Dreadnought (DRED) | Soviet naval, vertical-raise launch |
| CMISL | Cruise Missile | Boomer Sub (BSUB) | Soviet naval, used by Yuri faction in YR |

**Critical architectural note vs. our implementation:** The original rocket does NOT
home toward the target — once Phase 4 (cruise) sets the facing via `atan2`, the unit
flies along that facing only. Phase 5 then descends with a clamped per-tick turn rate
(from `*RocketTurnRate`), but lateral correction is minimal. The trajectory is
effectively deterministic given launch conditions; "lazy curve" missiles (V3, LazyCurve=yes)
get the broader S-shape, while DMisl (LazyCurve=no) gets a tighter arc.

**Facing storage:** Verified at assembly `0x00662112` — the rocket reads its facing
from **Owner+0x388** (the FootClass body FacingClass), NOT from a field inside the
locomotor itself. The locomotor only stores PitchAngle locally.

## 2. RocketLocomotionClass Struct Layout

**Total size:** at least 0x60 bytes (96 bytes). Constructor at `0x00661EC0`.

**param_1 type discipline:**
- **Constructor** uses `undefined4 *param_1` (this=base+0, indexed in 4-byte strides):
  `param_1[N]` = byte offset `4*N`. Direct byte writes use raw offsets (e.g. byte at +0x51).
- **Process** (`0x006622C0`) is invoked through the ILocomotion vtable thunk and
  receives `int *param_1` where `param_1 = base + 4` (ILocomotion subobject pointer):
  `param_1[N]` = byte offset `4 + 4*N`.
- **Helpers** `FUN_006620F0`, `FUN_00661FE0`, `FUN_00663030` use `int param_1` (raw byte
  offsets, this=base+0): `*(float *)(param_1 + 0x54)` is byte 0x54.

This three-way calling convention is the source of every Ghidra-decompile pitfall in
this file — always cross-check the param type before mapping an offset.

### Base Fields (from LocomotionClass)

| Offset | Size | Type | Field | Init | Evidence |
|--------|------|------|-------|------|----------|
| +0x00 | 4 | ptr | IUnknown_vtable | `&vtable@0x7F0BE8` | Constructor: `*param_1 = ...` |
| +0x04 | 4 | ptr | ILocomotion_vtable | `&vtable@0x7F0B1C` | Constructor: `param_1[1] = ...` |
| +0x08 | 4 | FootClass* | OwnerAlt (LocomotionClass base) | 0 | `LocomotionClass::Link_To_Object` writes both +0x4 (rel) and +0x8 (rel), → byte +0x8 and +0xC of base |
| +0x0C | 4 | FootClass* | Owner | 0 | Process reads `param_1[2]` as Owner; FUN_006620F0 reads `*(this+0xC)` |
| +0x10 | 1 | bool | IsPowered | 1 | LocomotionClass base ctor |
| +0x11 | 1 | bool | IsLockedDown | 1 | LocomotionClass base ctor |
| +0x14 | 4 | int | (base zero) | 0 | LocomotionClass base ctor |

### RocketLocomotionClass-Specific Fields

| Offset | Size | Type | Field | Init | Evidence |
|--------|------|------|-------|------|----------|
| +0x18 | 4 | int (lepton X) | DestX | `g_NullCoord_Rocket_X` (sentinel) | Constructor `param_1[6]`; Process `param_1[5]`; `Is_Moving` `[this+0x14]`; `Destination` copies it |
| +0x1C | 4 | int (lepton Y) | DestY | `g_NullCoord_Rocket_Y` | Constructor `param_1[7]`; Process `param_1[6]` |
| +0x20 | 4 | int (lepton Z) | DestZ (target ground Z, used as impact threshold) | `g_NullCoord_Rocket_Z` | Constructor `param_1[8]`; FUN_006620F0 compares predicted Z vs `*(this+0x20)` |
| +0x24 | 4 | int | StartFrame (current phase timer base) | `g_CurrentFrameCounter` | Constructor `param_1[9]`; Process `param_1[8]` updates on every phase change |
| +0x28 | 4 | int | (free / scratch) | — | Process writes `param_1[9] = local_50` (loop-local int) |
| +0x2C | 4 | int | TimerDuration (frames remaining-style; combined with StartFrame) | 0 | Constructor `param_1[0xb]=0`; Process `param_1[0xa]` |
| +0x30 | 4 | int | TotalDuration (current-phase duration in frames) | 0 | Constructor `param_1[0xc]=0`; Process `param_1[0xb]`; checked against TimerDuration to detect end-of-phase |
| +0x34 | 4 | int | SmokeStartFrame (smoke-puff timer base) | `g_CurrentFrameCounter` | Constructor `param_1[0xd]`; Process `param_1[0xc]` |
| +0x38 | 4 | int | (free / scratch) | — | Process `param_1[0xd]` |
| +0x3C | 4 | int | SmokeInterval (frames between smoke puffs) | — | Process `param_1[0xe]`; set to 0x18 (24 frames) at every smoke emission |
| +0x40 | 4 | int | **Phase** (state machine cursor 0..6) | 0 | Constructor `param_1[0xf]=0`; Process `switch(param_1[0xf])` |
| +0x44 | 4 | int | (Reserved / always 0) | 0 | Constructor `param_1[0x10]=0`; not seen written elsewhere |
| +0x48 | 8 | double | Altitude (continuous Z accumulator above launch ground) | 0.0 | Constructor `param_1[0x12]=0`; Process `*(double *)(param_1+0x11)` r/w during phases 3/4 |
| +0x50 | 1 | bool | IsRenderVisible (render-list visibility flag) | 1 | Constructor byte at +0x50 = 1; Process toggles via `param_1[0x13]` byte |
| +0x51 | 1 | bool | IsElite (cached `Owner.Veterancy.IsElite()` result) | 0 | Constructor byte at +0x51 = 0; Process re-evaluates each tick from VeterancyClass and caches here |
| +0x52 | 2 | — | (padding) | — | Inferred from alignment |
| +0x54 | 4 | float (int-stored) | PitchAngle (radians; FPU-loaded as `*(float *)(this+0x54)`) | 0.0 | Constructor `param_1[0x15]=0`; Process `param_1[0x14]` r/w; FUN_006620F0/00663030/00661FE0 read as float |
| +0x58 | 4 | int | TotalDescentDistance (3D distance at the moment phase 3 → 4 transition) | 0 | Constructor `param_1[0x16]=0`; Process case 3 sets via `Math__ftol(Sqrt_Approx(dx*dx+dy*dy))`; phase 4 uses to compute interpolation factor |
| +0x5C | 4 | int | (free / unused observed) | — | — |

## 3. Rules-Class RocketStruct Lookup

`Process` selects one of three `RocketStruct` blocks based on `Owner.TypeClass`:

| Owner.TypeClass == Rules.X | RocketStruct base | Type-pointer field |
|----------------------------|-------------------|--------------------|
| `Rules+0x4E0` (V3Rocket)   | `Rules+0x4B0`     | `Rules+0x4E0` (`V3RocketType=V3ROCKET`) |
| `Rules+0x548` (DMisl)      | `Rules+0x518`     | `Rules+0x548` |
| else (default → CMisl)     | `Rules+0x4E4`     | `Rules+0x514` |

Each `RocketStruct` is 0x30 bytes; the type-pointer field at `+0x30` is the
`AircraftTypeClass*` resolved from `[General] V3RocketType=` etc. at INI parse time.

### RocketStruct Field Layout (12 fields × 4 bytes + type ptr)

| Offset | Type | Field | INI key (V3 example) |
|--------|------|-------|----------------------|
| +0x00 | int | PauseFrames | `V3RocketPauseFrames=0` |
| +0x04 | int | TiltFrames | `V3RocketTiltFrames=60` |
| +0x08 | float | PitchInitial | `V3RocketPitchInitial=0.21` |
| +0x0C | float | PitchFinal | `V3RocketPitchFinal=0.5` |
| +0x10 | float | TurnRate | `V3RocketTurnRate=0.05` |
| +0x14 | int (or fixed) | RaiseRate | `V3RocketRaiseRate=1` |
| +0x18 | float | Acceleration | `V3RocketAcceleration=0.4` |
| +0x1C | int | Altitude (cruise altitude in leptons) | `V3RocketAltitude=768` |
| +0x20 | int | Damage | `V3RocketDamage=200` |
| +0x24 | int | EliteDamage | `V3RocketEliteDamage=400` |
| +0x28 | int | BodyLength (leptons) | `V3RocketBodyLength=256` |
| +0x2C | bool (1 byte read) | LazyCurve | `V3RocketLazyCurve=yes` |
| +0x30 | AircraftTypeClass* | (associated rocket type) | `V3RocketType=V3ROCKET` |

**Verified field offsets** by cross-checking Process accesses against the existing
[LOCOMOTION_MATH_AND_CONSTANTS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOCOMOTION_MATH_AND_CONSTANTS.md) table:
- Case 1 reads `*(iVar9 + 4)` → TiltFrames after PauseFrames ✓
- Case 2 reads `*(local_a0 + 0x8)` (PitchInitial) and `*(local_a0 + 0xc)` (PitchFinal) ✓
- Case 3 adds `*(float *)(iVar9 + 0x18)` (Acceleration) to Altitude; tests against `*(int *)(iVar9 + 0x1c)` (Altitude) ✓
- Case 5 uses `*(float *)(local_a0 + 0x10)` (TurnRate) as per-tick clamp ✓
- Case 6 adds `*(iVar9 + 0x14)` (RaiseRate) to Owner.Z; sets pitch from `*(iVar9 + 0xc)` (PitchFinal) ✓
- Case 4 reads `*(char *)(iVar9 + 0x2c)` (LazyCurve) as 1-byte boolean ✓

## 4. Damage Routing (Detonation)

`FUN_00663030` (Detonate) selects the warhead from `RulesClass` based on missile type
**and** elite status:

| Missile | Normal warhead | Elite warhead |
|---------|---------------|---------------|
| V3 (Owner.TypeClass == Rules+0x4E0) | `Rules+0xFB0` | `Rules+0xFB8` |
| DMisl (Owner.TypeClass == Rules+0x548) | `Rules+0xFC0` | `Rules+0xFC4` |
| else (CMisl / generic)               | `Rules+0xFB4` | `Rules+0xFBC` |

After warhead lookup, Detonate:
1. Calls `FUN_004135D0` (probably FootClass cleanup pre-explode)
2. Computes the exact impact 3D coord from `(PitchAngle, OwnerFacing)` using sin/cos lookups
3. Picks an explosion anim via `Warhead__SelectExplosionAnim`
4. Constructs the AnimClass at the impact coord with flags `(0, 1, 0x2600, owner→self_link)`
5. Calls `FUN_0048A620` (probably radar event / area effect spawn)
6. Calls `Apply_area_damage(Owner, warhead, 1, 0)` — the damage call uses `Apply_area_damage` not the per-target `Take_Damage` path
7. Calls `vtable+0xF8` on Owner — likely `Limbo` / `Mark_For_Removal`

## 5. Phase State Machine

Phase is at byte +0x40, stored as int. Constructor initializes to 0. Phase 0 has no
case in the `Process` switch — it is a **resting/uninitialized** state. Transition to
Phase 1 must happen via the missile being given a destination (likely
`Move_To_Coord` at `0x006632E0`, but that function is unregistered in Ghidra and only
its prologue was decoded — see Section 6).

```
                                         [V3 / CMisl]
                                              |
                                              v
   [0]    --(Move_To_Coord)-->   [1]   ---->  [2]   ----------------> [3]
   start                       Pause/        Tilt              Boost ascend
                               smoke         (PitchInitial     (Altitude
                                              → PitchFinal,     accumulates
                                              TiltFrames)       += Accel/tick)
                                              \                       |
                                               \                      v
                                                \                  [4]
                                              [DMisl]            Cruise
                                                 \             (3D travel,
                                                  v             distance check)
                                                 [6]                  |
                                              Vertical-raise          v
                                              (Owner.Z += RaiseRate, [5]
                                               smoke loop, then     Descent
                                               PitchFinal, → 3)    (TurnRate-clamped
                                                                   pitch turn,
                                                                   FUN_006620F0
                                                                   ground check
                                                                   → detonate)
```

**Branch decision at end of Phase 1** (at `0x00662420` region):

```c
param_1[0xf] = (-(uint)(*(int *)(iVar9 + 0x30) != *(int *)(g_RulesClass_Instance + 0x548))
                & 0xfffffffc) + 6;
```

- If `RocketStruct.TypePtr != Rules.DMisl` (i.e. V3 or CMisl) → result `2` (tilt)
- If equal (is DMisl) → result `6` (vertical raise)

The "raise" semantic for DMisl matches the dreadnought silo gameplay (vertical launch
out of the deck), and the V3 "tilt" matches its launcher animation (lying flat → tilt up).

### Phase Notes

- **Phase 1 (Pause + smoke):** plays smoke anim every 24 frames (`SmokeInterval=0x18`)
  at Owner.position. Hides the unit's render visibility (byte +0x50 cleared) until
  the smoke phase completes. **Exception:** if the missile type IS DMisl, Phase 1 keeps
  visibility on (the silo doors logic). On timer end → Phase 2 or Phase 6.
- **Phase 2 (Tilt):** linearly interpolates PitchAngle from `PitchInitial * 2π/65536`
  to `PitchFinal * 2π/65536` over `TiltFrames`. On end, plays a launch sound at the
  Owner cell position (verified via `VocClass__PlayAt` call) and emits a single
  ignition anim. Transitions to Phase 3.
- **Phase 3 (Boost ascend):** every tick adds `Acceleration` to Altitude (the +0x48
  double). When Altitude reaches `RocketStruct.Altitude` (cruise altitude in leptons),
  caches the current 2D distance to target into `+0x58 TotalDescentDistance` and
  transitions to Phase 4.
- **Phase 4 (Cruise):** continues to add Acceleration to Altitude (capped at the
  Owner's max-Z `[Owner+0x678]`). Two sub-modes by LazyCurve:
  - LazyCurve=true AND `+0x58 != 0`: lerp the pitch toward `atan2(dz, dx_2d)` weighted
    by `(currentDist / TotalDescentDistance)` — produces the wide V3 arc.
  - LazyCurve=false: just decay PitchAngle toward 0 by `LAB_007E1748` (small constant)
    each tick — straighter trajectory.
  Sets Owner.facing each tick via `RateTimer__Set` based on `atan2(dy, -dx)` to the
  target. When the 3D distance falls below the Z-component-derived threshold,
  transitions to Phase 5.
- **Phase 5 (Descent):** calls `FUN_006620F0` (impact predictor) every tick. If
  predicted next-Z would land below DestZ (or hit a building), calls Detonate. Otherwise
  clamps PitchAngle's per-tick change to `±TurnRate * 2π/65536`.
- **Phase 6 (Raise):** for DMisl. Adds `RaiseRate` (typically 1 lepton) to Owner.Z
  each tick, plays smoke, until total raise duration elapses. Then sets PitchAngle to
  `PitchFinal`, plays launch voice if at non-shroud cell, and transitions to Phase 3.

### Per-Tick Position Update (post-switch tail of Process)

After the switch, when `Altitude > 0`:
1. Compute `dz = ftol(Altitude)`
2. Compute facing yaw via `RateTimer__Current` on Owner+0x388 (BodyFacing); convert
   16-bit facing to radians: `(facing - 0x3FFF) * (-2π/65536)`
3. Compute new position: `(X + cos(yaw)*sin(pitch)*?, Y + sin(yaw)*sin(pitch)*?, Z = floor + cos(pitch)*?)`
   — exact formula per FUN_00661FE0 which writes the velocity vector to a CoordStruct.
4. Cell-bounds check; if in bounds call Owner.vtable+0x1B4 (probably `Set_Position`).
5. Call Owner.vtable+0x1B8 (probably `Get_Position`); if cell changed, call FUN_004138C0
   (likely `Mark_Cell_Layer_Move` or `Update_Occupation_Bits`).
6. If `Owner.Health < 1` (shot down mid-flight), force Detonate.

### Termination

When Detonate runs (from Phase 5 ground intercept, building hit, or owner death),
`vtable+0xF8` on Owner is called — this is the limbo / despawn that removes the
missile sub-unit from the world. The Locomotor itself is then deallocated when its
COM refcount drops to zero.

## 6. Helper Functions

| Address | Suggested name | Purpose |
|---------|----------------|---------|
| `0x00661EC0` | `RocketLocomotionClass::Constructor` | Init all fields, install vtables |
| `0x006635C0` | `RocketLocomotionClass::Constructor(byte)` | Re-init with optional `FUN_007C8B3D` cleanup; second-form constructor used for object recycling |
| `0x00661F50` | `ILoco::Is_Moving` (unregistered, 41 bytes) | Returns 1 iff DestX/Y/Z differ from `g_NullCoord_Rocket_*` |
| `0x00661FB0` | `ILoco::Destination` (unregistered, ~30 bytes) | Copies DestX/Y/Z into output CoordStruct |
| `0x00661FE0` | `RocketLocomotion::Compute_Velocity_Vector` | Computes 3D unit-velocity from `(pitch, ownerFacing)` and writes (dx, dy, dz) to param2 |
| `0x006620F0` | `RocketLocomotion::Predict_And_Check_Impact` | Predicts next-tick Z via sin/cos; returns 1 (and detonates) if Z would hit ground or building, 0 otherwise |
| `0x006622C0` | `ILoco::Process` (slot 16 in vtable @ 0x7F0B1C) | Main per-tick driver; phase state machine |
| `0x00663030` | `RocketLocomotion::Detonate` | Selects warhead by missile type + elite, spawns explosion anim, applies area damage, limbos owner |
| `0x006632E0` | (likely) `ILoco::Move_To_Coord` (unregistered) | Sets DestX/Y/Z from caller and initializes Phase 1 / PauseFrames timer. Prologue confirms type lookup mirrors Process; full body not decoded. |
| `0x006633C0` | (likely) `ILoco::Mark_All_Occupation_Bits` (unregistered) | Companion to Move_To_Coord — not decoded |
| `0x00663460` | (likely) `ILoco::Force_Track` or other utility | Not decoded |

## 7. INI Bindings (Active Keys)

All 12 keys per missile family parse into the `RocketStruct` blocks above. Total
**36 keys** across V3/DMisl/CMisl prefixes. Parsing happens in `RulesClass::Read_Ini`
under `[General]`.

**Active YR rules entries** (from `rulesmd.ini` lines 157-180):
```
V3RocketPauseFrames=0       DMislPauseFrames=20    CMislPauseFrames=18
V3RocketTiltFrames=60       DMislTiltFrames=60     CMislTiltFrames=30
V3RocketPitchInitial=0.21   DMislPitchInitial=0    CMislPitchInitial=0.21
V3RocketPitchFinal=0.5      DMislPitchFinal=0.5    CMislPitchFinal=0.6
V3RocketTurnRate=0.05       DMislTurnRate=0.08     CMislTurnRate=0.04
V3RocketRaiseRate=1         DMislRaiseRate=1       CMislRaiseRate=1
V3RocketAcceleration=0.4    DMislAcceleration=0.8  CMislAcceleration=0.7
V3RocketAltitude=768        DMislAltitude=...      CMislAltitude=...
V3RocketDamage=200          DMislDamage=...        CMislDamage=...
V3RocketEliteDamage=400     DMislEliteDamage=...   CMislEliteDamage=...
V3RocketBodyLength=256      DMislBodyLength=...    CMislBodyLength=...
V3RocketLazyCurve=yes       DMislLazyCurve=no      CMislLazyCurve=...
V3RocketType=V3ROCKET       DMislType=DMISL        CMislType=CMISL
```

## 8. Tiberian Sun Legacy Notes

- The "raise" branch (Phase 6) is structurally a TS Cruise Missile launch path — the
  `V3RocketRaiseRate=1 ;GEF (for Cruise Missile only)` comment in stock rulesmd.ini
  confirms the field was originally CMisl-specific. In the YR binary it is **active**
  and used by DMisl (the dreadnought missile inherits the TS-style vertical-raise),
  so **do implement** it. Confirmed live in the binary — not dormant.
- All three missile types (V3, DMisl, CMisl) and their CLSID bindings appear in the
  stock YR rules and are in active gameplay use. No part of this locomotor is dead
  in YR.
- The duplicate constructor at `0x006635C0` (which calls `LocomotionClass::Destructor`
  then optionally `FUN_007C8B3D`) is a **placement-recycling** form, not a TS leftover.

## 9. Open Questions / Unverified

- **Phase 0 → 1 entry**: assumed to be `Move_To_Coord` at `0x006632E0`. The function's
  prologue confirms type lookup pattern but the body was not decoded — needs a follow-up
  to confirm Phase 1 timer setup and PauseFrames source.
- **`+0x44` field**: constructor zeroes it; no read seen in any decoded function. Could
  be a 4-byte alignment pad before the 8-byte `Altitude` double, or a reserved field.
- **`+0x5C` field**: never read or written outside the constructor — likely tail padding
  to round the struct to 0x60 / 16-byte multiple. If a real field, it would be visible
  in `Save_Data`/`Load_Data` (not yet decoded).
- **Sound triggers**: case 2 and case 6 both call `VocClass__PlayAt` with arg 0 (no
  voice index). The voice index must come from VeterancyClass / Owner.TypeClass —
  this side-effect chain is not fully traced.
- **Vtable slot 16 = Process** (this report's mapping). The standard ILocomotion
  layout puts Process around slot 7-9 in YRpp/Ares headers. The `0x80` virtual call at
  the end of Process (`(**(code **)(*param_1 + 0x80))(param_1)`) confirms slot 32 is
  also called — likely an "extension" interface. The full vtable mapping for
  `0x7F0B1C` is left as a follow-up; only Process at +0x40 was confirmed by direct
  decompilation here.

---

**Verified 2026-04-19** from gamemd.exe (image base 0x00400000) via Ghidra MCP:
constructor decompile, Process decompile, FUN_006620F0 decompile, FUN_00663030
decompile, FUN_00661FE0 decompile, vtable bytes at 0x7F0B1C, Is_Moving raw
disassembly, Destination raw disassembly, FUN_006620F0 raw disassembly (to confirm
Owner+0x388 facing access), and rulesmd.ini cross-check at lines 157-180 and
11364-11472.
