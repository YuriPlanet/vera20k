# Voxel Slope Tilt System — Ghidra Analysis of gamemd.exe

## Overview

When a vehicle sits on a sloped cell, the engine tilts its VXL body (and turret) to
match the terrain. This is purely visual — the tilt does not affect simulation. The
system reads the **slope type** from `cell+0x11C`, looks up a **pre-computed 3x4
rotation matrix** from a global table, and multiplies it with the vehicle's facing
rotation to produce the final tilted voxel transform.

## Slope Type Values (cell+0x11C)

From TS++ `TIBSUN_DEFINES.H`, confirmed via WAE source and Ghidra tilt matrix angles:

| Slope Type | Direction | Compass Angle | Tilt Constant | Description |
|---|---|---|---|---|
| 0 | None | — | — | Flat ground, no tilt |
| 1 | **West** | 270° (4.712 rad) | `_DAT_00b44310` | Two west-side corners raised |
| 2 | **North** | 180° (π rad) | `_DAT_00b44310` | Two north-side corners raised |
| 3 | **East** | 90° (π/2 rad) | `_DAT_00b44310` | Two east-side corners raised |
| 4 | **South** | 0° | `_DAT_00b44310` | Two south-side corners raised |
| 5 | **CornerNW** | 225° (5π/4 rad) | `_DAT_00b43f08` | NW corner raised half cell |
| 6 | **CornerNE** | 135° (3π/4 rad) | `_DAT_00b43f08` | NE corner raised half cell |
| 7 | **CornerSE** | 45° (π/4 rad) | `_DAT_00b43f08` | SE corner raised half cell |
| 8 | **CornerSW** | 315° (7π/4 rad) | `_DAT_00b43f08` | SW corner raised half cell |
| 9-12 | MidNW/NE/SE/SW | — | — | Three corners raised half cell |
| 13-16 | SteepSE/SW/NW/NE | — | — | Mid + far corner full cell |
| 17-20 | Double ramps | — | — | Alternating corners |

The slope type byte is read from the isometric tile data at byte offset `+0x2a`
(decimal 42) by `FUN_005471b0`, then stored at `cell+0x11C` during cell
recalculation (`FUN_0047d2b0`).

## Tilt Path: Vehicle Body Only

The engine computes vehicle body tilt via `ILocomotion::Draw_Matrix` (vtable
slot 9) using the slope-matrix table lookup described below. **There is no
separate turret-tilt path at `FUN_00729B40` — that earlier identification was
wrong.** See "FUN_00729B40 — Misidentification, RESOLVED 2026-05-19" below for
the correction. Where the actual turret-on-slope tilt is computed inside
`TechnoClass::GetFLH` / `GetRenderFLH` is an open question (see end of doc).

### Vehicle Body Tilt — `ILocomotion::Draw_Matrix`

| Locomotor | Function Address | Size |
|---|---|---|
| DriveLocomotionClass | `0x004AFF60` | ~1189 bytes |
| ShipLocomotionClass | `0x0069F670` | ~1189 bytes |

Called every frame via the ILocomotion vtable (slot 9) during voxel body rendering.

**Logic:**
1. If vehicle has no dynamic roll/pitch:
   - Build facing rotation matrix (`FUN_0055a730`)
   - If `locomotor+0x18` (cached slope type) != 0:
     - Look up slope matrix from `DAT_00b45188[slope_type]` via `FUN_007559b0`
     - Or interpolate via `FUN_00755a40` if transitioning between slopes
   - Multiply (row-major operand order): `result = slope × facing`
     — i.e., applied to a row vector `v`, equivalent to `(v · slope) · facing`,
     so slope is applied first and facing second. Verified at `0x004B03DA` —
     `Locomotion_Matrix(out, slope, facing)` with `param_2=slope` (EDX) and
     `param_3=facing` (stack arg).
2. If vehicle has active roll/pitch (from acceleration/braking):
   - Read body tilt floats from `entity+0x328` (roll) and `entity+0x32C` (pitch)
   - Compute dynamic tilt via `FUN_005aef60` (X-axis) + `FUN_005af080` (Y-axis)
   - Build facing rotation
   - Look up slope matrix (same as above)
   - Multiply all three (row-major): `result = dynamic_tilt × slope × facing`
     — order: dynamic_tilt first, then slope, then facing.

### `FUN_00729B40` — Misidentification, RESOLVED 2026-05-19

The prior version of this doc described `FUN_00729B40` as the **turret/barrel
tilt** computation called from `TechnoClass::GetFLH` (`FUN_006F3AD0`) and
`GetRenderFLH` (`FUN_006F3D60`). That identification was wrong:

- `get_xrefs_to 0x00729B40` returns **exactly one DATA xref** at `0x007F5A48`
  and **zero CODE xrefs**. Neither `GetFLH` nor `GetRenderFLH` calls it.
- `0x007F5A48` = `TunnelLocomotionClass` ILocomotion vtable slot 9, i.e. the
  `Draw_Matrix` slot for that locomotor.
- `TunnelLocomotionClass` is the Tiberian Sun subterranean-unit locomotor.
  Per `TS_DORMANT_LOCOMOTORS_GHIDRA_REPORT.md §4`, its CLSID has zero `rules.ini`
  / `rulesmd.ini` references; no YR unit instantiates it.

So `FUN_00729B40` is `TunnelLocomotionClass::Draw_Matrix` (dormant TS code),
not a turret-tilt routine. The state machine described in the old version
(`state byte at param_1+0x14`, states 0/2/3/5/6/7) is real but operates on
TunnelLocomotion's body, not on aircraft turrets. **Do not implement states
2–7 in the Rust port** — they are unreachable in standard YR.

For full state-machine details (state-field offset, timer fields, π/2 constants
verified), see [TURRET_TILT_STATE_MACHINE_FUN_00729B40_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TURRET_TILT_STATE_MACHINE_FUN_00729B40_GHIDRA_REPORT.md).
The filename retains the historical "turret tilt" wording for traceability;
the report itself corrects the identity.

**Open question — where DOES turret-on-slope tilt actually live?** It must be
computed somewhere on the `TechnoClass::GetFLH` / `GetRenderFLH` path, since
YR clearly tilts turrets along with the body on slopes. A follow-up
investigation is needed; the doc-section-formerly-known-as "Turret/Barrel
Tilt" has been retired pending that work.

## Slope Matrix Table

**Address:** `DAT_00b45188` (BSS, populated at init by `FUN_00754CB0`)

Each entry is **48 bytes** (12 floats = 3×4 row-major matrix), indexed by slope type.
Additional entries after index 8 repeat the pattern for different zoom levels.

### Matrix Construction (`FUN_005ae6f0`)

Each slope matrix is built using the rotate-tilt-unrotate pattern:

```
matrix = Identity
Rz(compass_direction)      // rotate to align slope direction with X axis
Rx(tilt_amount)             // pitch by the slope tilt angle
Rz(-compass_direction)      // rotate back to world coordinates
```

Where:
- `Rz` = Z-axis rotation (`FUN_005af1a0`, modifies columns 0,1)
- `Rx` = X-axis rotation (`FUN_005aef60`, modifies columns 1,2)
- `compass_direction` = the angle from the table above (0°, 45°, 90°, etc.)
- `tilt_amount` = `_DAT_00b44310` for types 1-4 (full-edge ramps),
  `_DAT_00b43f08` for types 5-8 (corner ramps)

### Rotation Function Reference

| Function | Axis | Columns Modified | Purpose |
|---|---|---|---|
| `FUN_005af1a0` | Z (yaw) | [0],[1] per row | Compass facing / slope direction |
| `FUN_005aef60` | X (pitch) | [1],[2] per row | Slope tilt angle |
| `FUN_005af080` | Y (roll) | [0],[2] per row | Aircraft states / body roll |
| `FUN_005ae860` | — | all | Set 3×4 identity matrix |
| `FUN_005ae610` | — | all | Copy 3×4 matrix (48 bytes) |
| `FUN_005af980` | — | all | Multiply two 3×4 matrices |
| `FUN_005ae6f0` | composite | all | Build slope matrix: Rz × Rx × Rz⁻¹ |

Trig identification: `FUN_004cad00` = cos, `FUN_004cacb0` = sin (verified: cos(0)=1,
sin(0)=0 produces identity).

## Tilt Angle Constants

| Address | Value | Used For | Initialized By |
|---|---|---|---|
| `_DAT_00b44310` | `0.5214767 rad` (≈29.88°) | Full-edge ramp tilt (types 1-4) | `VXL_Init_EdgeTiltAngle` (0x00754A50) |
| `_DAT_00b43f08` | `0.3858827 rad` (≈22.10°) | Corner ramp tilt (types 5-8) | `VXL_Init_CornerTiltAngle` (0x00754A20) |

Both reduce to clean closed forms around `LevelHeight = 104 leptons`:

- **Edge:** `atan(2 × LevelHeight / cellDiagonal) = atan(2 × 104 / 256√2) = atan(13√2/32)`
- **Corner:** `atan(LevelHeight / cellSide) = atan(104 / 256) = atan(13/32)`

### Init Chain (verified 2026-05-10)

```
DAT_00B43F00 (CameraPitch)    = (π/180) × 60 = π/3              [VXL_Init_CameraPitch    @ 0x007549A0]  # corrected 2026-05-28: was 0x007549AC; binary entry=0x007549A0 via get_function_by_address — GHIDRA_ADDRESS_SHIFT (+0xC into body)
DAT_00B43ED8 (CellHalfHeight) = (π/180) × 90 = π/2              [VXL_Init_CellHalfHeight @ 0x007549C0]  # corrected 2026-05-28: was 0x007549CC; binary entry=0x007549C0 via get_function_by_address — GHIDRA_ADDRESS_SHIFT (+0xC into body)
DAT_00B43EF8 (CellDiagonal)   = sqrt_approx(2 × pow(256, 2))    [VXL_Init_CellDiagonal   @ 0x00754910]
                              = 256√2 ≈ 362.04 leptons

# VXL_Init_CellHeightRatio @ 0x007549E0:
#   The decompile drops a hidden `× 0.5` (FMUL [0x007E1738]) that the asm shows.
#   The "Sin_Lookup_Table4096" (0x004CAD50) is actually a tan LUT and has its
#   own hidden `× 4096/(2π)` scaler at 0x007E8970 to convert radians→BAM index.
DAT_00B45578 (LevelHeight) = ftol(tan(π/2 − π/3) × 256√2 × 0.5)
                           = ftol(tan(π/6) × 128√2)
                           = ftol(128√(2/3))
                           = ftol(104.532...) = 104

DAT_00B44310 = atan(2 × 104 / 256√2) ≈ 0.5214767 rad
DAT_00B43F08 = atan(104 × 1/256)     ≈ 0.3858827 rad
```

The `0x007F6948` constant used by `Init_CameraPitch` and `Init_CellHalfHeight`
is `π/180` (degrees-to-radians); the literals at `0x007E1708/10/28/30` are
`2.0`, `256.0`, `60.0`, `90.0`; `0x007E1740` is `1/256`. **Confidence:** verified
from binary in the 2026-05-10 session.

The labels `CameraPitch` and `CellHalfHeight` are Ghidra-applied guesses for
the 60° and 90° literals — they don't correspond to RA2's actual iso camera
angle (~26.57°). The difference (90° − 60° = 30°) is what the tan LUT
samples to drive the LevelHeight calculation.

## Direction Encoding in param_3

`DriveLocomotionClass::Draw_Matrix` packs direction info into a cache value
for change detection:

```c
*param_3 = direction * 64 + slope_type;     // body cache key
// Final:
*param_3 = atan_result & 0x3F | facing << 8; // compressed form
```

This lets the renderer skip matrix recomputation when facing and slope haven't changed.

(The prior version of this section also showed a `FUN_00729B40` "turret"
variant of the same packing; that function is TunnelLocomotion::Draw_Matrix,
not turret tilt — see the misidentification note above.)

## Key Addresses Summary

| Address | What |
|---|---|
| `0x004AFF60` | DriveLocomotionClass::Draw_Matrix (body tilt) |
| `0x0069F670` | ShipLocomotionClass::Draw_Matrix (body tilt) |
| `0x00729B40` | TunnelLocomotionClass::Draw_Matrix (TS-dormant; previously misidentified as turret tilt) |
| `0x00754CB0` | Master VXL init — populates slope matrix table (3290 bytes) |
| `0x007559B0` | Direct matrix lookup from `DAT_00b45188[index * 0x30]` |
| `0x00755A40` | Interpolated matrix lookup (quaternion slerp between two facing indices) |
| `0x0055A730` | Build facing rotation matrix from entity direction |
| `0x005AE6F0` | Build slope matrix: Rz(dir) × Rx(tilt) × Rz(-dir) |
| `DAT_00b45188` | Slope/facing matrix table (48 bytes per entry) |
| `DAT_00b43188` | Quaternion table for interpolated lookups (16 bytes per entry) |
| `cell+0x11C` | Slope type byte (SlopeIndex from TECHNO_CLASS_FIELD_MAP) |
| `entity+0x328` | Dynamic body roll (float, from driving physics) |
| `entity+0x32C` | Dynamic body pitch (float, from driving physics) |

## Rust Engine Status

**Fixed:** `RampDirection` enum and `canonical_ramp_from_slope_type()` mapping
corrected to match TS++ canonical values (1=West, 2=North, 3=East, 4=South).

**Not yet implemented:**
- Slope tilt matrix computation and application during VXL rendering
- Dynamic body roll/pitch from driving acceleration
- Turret tilt for aircraft takeoff/landing states
- Slope matrix table initialization (pre-compute at startup)
- Corner ramp tilt (types 5-8)
- Diagonal-corner CORNER tilt (types 9-12, alias of 5-8)
- Diagonal-corner EDGE tilt (types 13-16) — see "Slope Matrix Table — Full Entry List" below
- Slope types 17-20: gamemd populates NO matrix; reads degenerate (zero) matrix from BSS

---

# Slope Matrix Table — Full Entry List

Verified 2026-05-10 from `VXL_MasterLighting_Init` (`0x00754CB0`) and
`VXL_GetFacingMatrix` (`0x007559B0`) in gamemd.exe. Resolves the open
question this doc previously listed for slope types 9-20.

**Method:** the table at `DAT_00b45188` is BSS — uninitialized in the binary
image. Static `read_memory` returns all zeros. The table is populated at
runtime by `VXL_MasterLighting_Init` (single caller: `CCFileClass__Constructor`
@ `0x0052BA60`). The 16 explicit `Matrix3x4_BuildFromRotateXAndFacing(angle,
tilt)` calls inside that init function are therefore the ground truth for what
the table holds at runtime. Each call's first arg is the compass angle (IEEE
754 single literal) and second arg is the tilt magnitude (`fVar1` =
`_DAT_00b44310` = EDGE tilt, or `fVar2` = `_DAT_00b43f08` = CORNER tilt).

## Compass angle constants (re-verified)

All angle constants used in `VXL_MasterLighting_Init` for slope-table entries.
Compass convention: `0° = south, increases clockwise (90°=E, 180°=N, 270°=W)`.

| IEEE 754 hex | Value | Compass | Used by slopes |
|---|---|---|---|
| `0x00000000` | `0.0` | South (0°) | 4 |
| `0x3F490E56` | `π/4 ≈ 0.78540` | SE (45°) | 7, 11, 15 |
| `0x3FC90E56` | `π/2 ≈ 1.57080` | East (90°) | 3 |
| `0x4016CAC1` | `3π/4 ≈ 2.35619` | NE (135°) | 6, 10, 14 |
| `0x40490E56` | `π ≈ 3.14159` | North (180°) | 2 |
| `0x407B51EC` | `5π/4 ≈ 3.92699` | NW (225°) | 5, 9, 13 |
| `0x4096CAC1` | `3π/2 ≈ 4.71239` | West (270°) | 1 |
| `0x40AFEC8B` | `7π/4 ≈ 5.49779` | SW (315°) | 8, 12, 16 |

No surprise constants. No half-angle or negated variants. The same 8 compass
literals serve all 16 populated slopes.

## Per-entry breakdown

`DAT_00b45188 + slope_type * 0x30` holds a 3×4 row-major matrix produced by
`Rz(compass) · Rx(tilt) · Rz(-compass)`. Entry 0 (slope_type=0) is BSS-zero
and is never read in practice (both `Draw_Matrix` and `Turret_barrel_tilt`
early-out on `slope_type == 0` and use identity directly).

| Slope | Entry addr | Compass arg (hex / rad) | Tilt arg | Tilt magnitude | Source line in `VXL_MasterLighting_Init` |
|---|---|---|---|---|---|
| 0 | `0xB45188` | — | — | (BSS zero, never read; early-out at caller) | n/a |
| 1 | `0xB451B8` | `0x4096CAC1` / 3π/2 (W) | `fVar1` | EDGE = 0.5214767 (atan(13√2/32)) | "DAT_00b451b8" block |
| 2 | `0xB451E8` | `0x40490E56` / π (N) | `fVar1` | EDGE | "DAT_00b451e8" block |
| 3 | `0xB45218` | `0x3FC90E56` / π/2 (E) | `fVar1` | EDGE | "DAT_00b45218" block |
| 4 | `0xB45248` | `0x00000000` / 0 (S) | `fVar1` | EDGE | "DAT_00b45248" block |
| 5 | `0xB45278` | `0x407B51EC` / 5π/4 (NW) | `fVar2` | CORNER = 0.3858827 (atan(13/32)) | "DAT_00b45278" block |
| 6 | `0xB452A8` | `0x4016CAC1` / 3π/4 (NE) | `fVar2` | CORNER | "DAT_00b452a8" block |
| 7 | `0xB452D8` | `0x3F490E56` / π/4 (SE) | `fVar2` | CORNER | "DAT_00b452d8" block |
| 8 | `0xB45308` | `0x40AFEC8B` / 7π/4 (SW) | `fVar2` | CORNER | "DAT_00b45308" block |
| 9 | `0xB45338` | `0x407B51EC` / 5π/4 (NW) | `fVar2` | CORNER (byte-identical to entry 5) | "DAT_00b45338" block |
| 10 | `0xB45368` | `0x4016CAC1` / 3π/4 (NE) | `fVar2` | CORNER (byte-identical to entry 6) | "DAT_00b45368" block |
| 11 | `0xB45398` | `0x3F490E56` / π/4 (SE) | `fVar2` | CORNER (byte-identical to entry 7) | "DAT_00b45398" block |
| 12 | `0xB453C8` | `0x40AFEC8B` / 7π/4 (SW) | `fVar2` | CORNER (byte-identical to entry 8) | "DAT_00b453c8" block |
| 13 | `0xB453F8` | `0x407B51EC` / 5π/4 (NW) | `fVar1` | EDGE (corner direction with EDGE tilt) | "DAT_00b453f8" block |
| 14 | `0xB45428` | `0x4016CAC1` / 3π/4 (NE) | `fVar1` | EDGE | "DAT_00b45428" block |
| 15 | `0xB45458` | `0x3F490E56` / π/4 (SE) | `fVar1` | EDGE | "DAT_00b45458" block |
| 16 | `0xB45488` | `0x40AFEC8B` / 7π/4 (SW) | `fVar1` | EDGE | "DAT_00b45488" block |
| 17-20 | `0xB454B8` – `0xB45577` | — | — | **NOT POPULATED.** BSS-zero at runtime. | (no call writes here) |

The 16 builder calls cover indices 1-16 strictly. There is no tail loop, no
default-fill, and no second population path (only caller of
`VXL_MasterLighting_Init` is `CCFileClass__Constructor`). The 4 slope-table
slots for indices 17-20 stay at process-startup BSS (all zeros).

### Confirmation reads

`read_memory @ 0x00B45188 length 1024` — all zeros (binary image, before
process init runs). This is consistent with the population happening at
runtime, not at link-time data initialization.

`inspect_memory_content @ 0x00B454B8 length 256` — all zeros. The region
starting at slope_type=17's slot through 0xB455B7 is contiguous zero-BSS in
the binary image. Since no code in `VXL_MasterLighting_Init` writes there,
this region remains zero at runtime as well.

## Indexing function: `VXL_GetFacingMatrix` @ `0x007559B0`

Disassembly (8 instructions, no clamp):

```
007559b0  PUSH ESI
007559b1  LEA  ESI, [EDX + EDX*0x2]  ; ESI = slope_type * 3
007559b4  MOV  EAX, ECX               ; EAX = output buffer (param_1)
007559b6  PUSH EDI
007559b7  SHL  ESI, 0x4               ; ESI = slope_type * 0x30
007559ba  ADD  ESI, 0xb45188          ; ESI = table base + slope_type * 0x30
007559c0  MOV  ECX, 0xc               ; copy 12 dwords (48 bytes)
007559c5  MOV  EDI, EAX
007559c7  REP  MOVSD ES:EDI, ESI
007559c9  POP  EDI
007559ca  POP  ESI
007559cb  RET
```

**Findings:**
- No bounds check, no mask, no signed/unsigned distinction at this site —
  EDX is used directly as the multiplier.
- Caller passes `slope_type` in EDX via fastcall convention.
- For slope_type ∈ [17, 20] the read pulls 48 zero bytes from BSS — produces
  an all-zero 3×4 matrix at the output. Applying that matrix to any vertex
  collapses the vertex to (0, 0, 0). Visually the unit's voxels render to a
  single point in voxel-local space (degenerate / invisible body).
- For slope_type = 21 the read starts AT `0xB45578` which is `LevelHeight`
  (the 4-byte int = `0x00000068` = 104). The next 44 bytes are adjacent
  globals — would produce a non-zero garbage matrix using real engine state
  as float bits. Standard YR map TMPs do not appear to use slope_type ≥ 21
  (the TS++ enum tops out at 20), so this is theoretical.
- Identical lookup formula (same `+ param_3 * 0x30`, same 12-dword copy, no
  clamp) appears in `VXL_InterpolatedFacing` @ `0x00755A40`. The interpolated
  variant has its own quaternion-table lookup at `DAT_00b43188 + param_3 *
  0x10` — also unclamped — used when `param_2 != param_3` (i.e., during a
  slope transition, `from_index ≠ to_index`).

## Matrix builder: `Matrix3x4_BuildFromRotateXAndFacing` @ `0x005AE6F0`

```c
param_1[1] = 0;    param_1[2] = 0;    param_1[3] = 0;
param_1[4] = 0;                        param_1[6] = 0;    param_1[7] = 0;
param_1[8] = 0;    param_1[9] = 0;                        param_1[0xb] = 0;
*param_1   = 0x3f800000;  // m[0][0] = 1.0
param_1[5] = 0x3f800000;  // m[1][1] = 1.0
param_1[10]= 0x3f800000;  // m[2][2] = 1.0
Matrix3x4_RotateZ(param_2);     // composes Rz(facing)  on identity
Matrix_rotate_x_axis(param_3);  // composes Rx(tilt)    on the result
Matrix3x4_RotateZ(-param_2);    // composes Rz(-facing) on the result
return param_1;
```

Confirms the row-major 3×4 layout (12 floats; columns 0-2 hold the rotation,
column 3 reserved for translation = always zero in slope entries) and the
`Rz · Rx · Rz⁻¹` composition order. The translation column is **never written**
by the slope-table builder, so all slope entries have zero translation —
they are pure rotations stored in 3×4 form.

`Matrix3x4_RotateZ` (`0x005AF1A0`) modifies columns [0],[1] of each row in
place; `Matrix_rotate_x_axis` (`0x005AEF60`) modifies columns [1],[2]. Both
use the convention:

```
m[r][a]_new = m[r][a]*sin(θ) + m[r][b]*cos(θ)
m[r][b]_new = m[r][b]*sin(θ) − m[r][a]*cos(θ)
```

— a rotation by `(π/2 − θ)` in standard math convention (because RA2's
compass angle measures CW from south, while standard math rotation measures
CCW from +X). This sign convention applies uniformly to all 16 populated
slope entries — slopes 9-16 obey the same convention as 1-8.

## TS++ enum semantics vs. binary behavior — RESOLVED

This doc previously cited `TIBSUN_DEFINES.H` for the meaning of slope types
9-20:

| TS++ name | Binary actually populates |
|---|---|
| 9 = MidNW   | NW direction, **CORNER tilt** (alias of slope 5) |
| 10 = MidNE  | NE direction, CORNER tilt (alias of slope 6) |
| 11 = MidSE  | SE direction, CORNER tilt (alias of slope 7) |
| 12 = MidSW  | SW direction, CORNER tilt (alias of slope 8) |
| 13 = SteepSE | **NW direction**, EDGE tilt |
| 14 = SteepSW | **NE direction**, EDGE tilt |
| 15 = SteepNW | **SE direction**, EDGE tilt |
| 16 = SteepNE | **SW direction**, EDGE tilt |
| 17 = DoubleRamp NE | (no matrix populated — degenerate) |
| 18 = DoubleRamp NW | (no matrix populated) |
| 19 = DoubleRamp SE | (no matrix populated) |
| 20 = DoubleRamp SW | (no matrix populated) |

**Key disagreements:**
1. TS++ "Mid*" implies a third tilt magnitude between EDGE and CORNER. gamemd
   has only two tilt magnitudes (EDGE, CORNER) and slopes 9-12 reuse CORNER.
   They are byte-identical aliases of slopes 5-8.
2. TS++ "Steep*" name ordering (SE/SW/NW/NE) does NOT match the binary's
   actual ordering for 13-16 (NW/NE/SE/SW). The TS++ direction labels for
   13-16 are wrong.
3. TS++ "Double ramp" matrices simply do not exist in gamemd — those slope
   types render with a degenerate (all-zero) matrix.

The TS++ ENUM ORDINAL (the integer value 9-20 used in TMP `+0x2A` and
`cell+0x11C`) survives as the table index; the human-readable name does
not describe what the binary actually does. **Do not trust the TS++ name
when describing gamemd behavior — use the per-entry table above.**

Active in YR: **Yes** for slopes 1-16 (the table is unconditionally
populated at engine init by `CCFileClass__Constructor`, no `SpecialFlags`
gate, no `Rules` flag check). Slopes 17-20 are inactive in the sense that
no matrix is populated — but the lookup still runs without bounds-checking,
producing degenerate output. There is no TS-only or YR-only branch inside
`VXL_MasterLighting_Init` itself.

## Cell → locomotor flow (Q5 from the plan)

The cell stores a 1-byte slope index; the locomotor stores it as a 4-byte
int. Width conversion happens during caching, with no transformation.

```
TMP entry +0x2A  (signed char read via *(char *)(ptr + 0x2A) in
                 TMP_ReadSlopeType @ 0x005471B0; returns int by promotion;
                 returns 0 if the indexed TMP cell pointer is null)
       ↓ assigned via `bVar6 = TMP_ReadSlopeType(...); this->SlopeIndex = bVar6;`
       ↓ in CellClass__RecalcAttributes @ 0x0047D2B0 (lower 8 bits stored)
cell+0x11C  (1 byte, "SlopeIndex")
       ↓ propagated through ILocomotion vtable slot at offset 0x6C:
       ↓ LocomotionClass__ForEach_SetSlopeIndex @ 0x004E1570 walks the
       ↓ piggyback chain calling `vtable[0x6C](slope_byte)` on each. The
       ↓ byte is widened to a 4-byte stack arg by the compiler before the
       ↓ vtable call.
DriveLocomotionClass__Force_New_Slope @ 0x004AFB40
       ↓ MOV ECX, [ESP+0x8]    ; reads 4-byte dword arg directly
       ↓ MOV [EAX+0x18], ECX   ; writes 4-byte dword to locomotor+0x18
locomotor+0x18  (4 bytes, current slope index)
```

**No transformation** between cell and locomotor — just a byte→dword copy
through a vtable boundary. The exact widening (MOVZX vs. MOVSX) happens at
the call site in the compiler-emitted prologue. For TMPs that hold the
documented TS++ enum values 0-20 the distinction is moot (high bit is never
set). For pathological TMP bytes ≥ 0x80 the locomotor would receive a
sign-extended negative int (since `*(char *)` in `TMP_ReadSlopeType` is
`signed char`), which would cause `slope_type * 0x30` in
`VXL_GetFacingMatrix` to compute a large negative offset — out-of-bounds
read. Standard YR TMPs do not exercise this; flagging as a tiny robustness
risk only.

`DriveLocomotionClass__Force_New_Slope` also writes:
- `+0x1C` ← param_2 (previous slope copy)
- `+0x20` ← `g_CurrentFrameCounter` (transition start frame)
- `+0x24` ← uninitialized stack slot `local_c` (Ghidra decompile shows this
  as a literal use of an uninitialized local — likely a Ghidra reconstruction
  artifact since the asm reads from `[ESP+8]` again at offset `0x004AFB61`,
  meaning the same param_2 dword is written here)
- `+0x28` ← 0 (transition duration)
- `+0x2C` ← 0 (transition timer)

After `Force_New_Slope`, the transition duration at `+0x2C` is 0, so the
`Draw_Matrix` interpolation branch (which fires when `+0x2C != 0`) does
NOT run — slope changes through this path are instantaneous. Slope
interpolation must be triggered by some other code path that writes
non-zero values to `+0x28`/`+0x2C`. The body Draw_Matrix interpolation gate
is `iVar4 = *(int *)(param_1 + 0x2c); if (iVar4 == 0) { non-interpolated path }`.

## Body slope-source read width

| Path | Source field read | Width | Conversion |
|---|---|---|---|
| Body (`DriveLocomotionClass__Draw_Matrix` @ `0x004AFF60`) | `*(int *)(param_1 + 0x18)` (locomotor+0x18) | 4 bytes (int) | passed directly as EDX to `VXL_GetFacingMatrix` |
| Body (`ShipLocomotionClass__Draw_Matrix` @ `0x0069F670`) | same field on ship locomotor | 4 bytes | same |
| TunnelLocomotion (`FUN_00729B40`, state 0) | `*(byte *)(iVar4 + 0x11c)` (cell+0x11C) | 1 byte (unsigned) | zero-extended to dword (TS-dormant in YR) |

**Note on the prior "body vs turret" framing:** the third row above was
previously labeled "Turret (`Turret_barrel_tilt`)" and used to argue that
body and turret paths could disagree by one tick on slope. Since `FUN_00729B40`
is actually TunnelLocomotion::Draw_Matrix (TS-dormant), no such disagreement
exists in YR — there is currently no documented turret-on-slope read path at
all. The follow-up investigation flagged at the top of this doc should
identify where turret slope (if any) is sampled.

The TunnelLocomotion path's pre-cache write
(`*param_3 = *param_3 * 0x40 + (uint)bVar1`) is byte-pattern-identical to
the body path's `direction*64 + slope_type` packing — interesting only as
evidence that this code was copy-pasted from the original body-tilt routine,
not a separate engineering effort.

## Open questions left after this pass

1. **Empirical TMP byte distribution (Q6 from plan).** Whether any standard
   YR map TMP files actually emit `slope_type ∈ [17, 20]` was deferred —
   the plan marked this as optional. If no shipping TMP uses 17+, the
   degenerate-matrix divergence is theoretical; if some do, it is
   high-frequency and worth a renderer fallback. Recommended next step:
   glob `<ra2-install>/*.mix`
   for embedded TMPs and tally the `+0x2A` byte distribution in a separate
   investigation pass.
2. ~~**Slope interpolation trigger.**~~ **RESOLVED 2026-05-19** — the
   trigger is the inline slope-change detector at the start of
   `DriveLocomotionClass::Process` @ `0x004B0500`. When the current-cell
   slope index (`CellClass+0x11C`) differs from the cached locomotor slope,
   `Process` writes a 3-tick smooth transition (`+0x2C ← 3`, plus snapshots
   of frame-counter / prev-slope fields) via a register-to-memory encoding
   (`MOV EAX, 3; MOV [EDX+0xC], EAX` at `~0x004B053E`). `Force_New_Slope`
   does NOT cancel this — Process does not dispatch slot `+0x50`
   (`Update_Facing_From_Type`) inline. `Force_New_Slope` is only invoked at
   move-start, via direct call from `TechnoClass::Set_Destination` @
   `0x00742BE6` (see [FORCE_NEW_SLOPE_CALLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FORCE_NEW_SLOPE_CALLERS_GHIDRA_REPORT.md)).
   Net behavior: every slope-cell-crossing during normal driving produces a
   3-tick slerp via `VXL_InterpolatedFacing` and the
   `DAT_00b43188` quaternion table — interpolation is YR-active, not dead
   code. See [VXL_INTERPOLATED_FACING_AND_SLOPE_TRANSITION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_INTERPOLATED_FACING_AND_SLOPE_TRANSITION_GHIDRA_REPORT.md)
   for the slerp / quaternion-table mechanism (note: that report's "no
   runtime writer" conclusion at the top is itself wrong — see swarm
   reconciliation notes for why).
3. **`Force_New_Slope` `+0x24` write.** Decompile shows
   `*(undefined4 *)(param_1 + 0x24) = local_c;` where `local_c` is
   uninitialized in the C view. The asm at `0x004AFB61` shows
   `MOV ECX, [ESP+0x8]` again, suggesting the value is just a re-read of
   `param_2`. Confirmed harmless for slope semantics — flagged only because
   the C decompile would mislead a reader.
4. **`local_2c = local_2c + -0x1e;` in `CellClass__RecalcAttributes`.**
   Subtracts 30 from a value related to height before integer-dividing by
   15 to produce `field_0x11d`. Unrelated to SlopeIndex; tangent.

## Summary for implementers

To bring the Rust renderer to parity for slope types 9-20 in
`compute_slope_rotation`:

| slope_type | Rust output should be |
|---|---|
| 9, 10, 11, 12 | Same matrix as 5, 6, 7, 8 respectively (CORNER tilt at NW/NE/SE/SW) |
| 13, 14, 15, 16 | EDGE tilt magnitude with NW/NE/SE/SW compass directions (a NEW combination not present in 1-8) |
| 17, 18, 19, 20 | Either (a) match gamemd's degenerate behavior (zero matrix → vertices collapse to origin), or (b) clamp to identity. The user may prefer (b) for visual robustness — gamemd's behavior here is "the unit becomes invisible," which is unlikely to be a desired parity target. Decide during `/brainstorm`. |
| ≥ 21 | Should NEVER occur from valid TMP data. Defensive clamp recommended. |

The constants are already in place
([src/render/vxl_raster.rs:46,52](src/render/vxl_raster.rs#L46-L52)):
- `EDGE_TILT_RAD   = 0.521_476_7` (atan(13√2/32))
- `CORNER_TILT_RAD = 0.385_882_7` (atan(13/32))

The only addition required is extending the match arm to cover 9-16 with
the correct (compass, tilt) pairs from the per-entry table above. The
`unit_atlas.rs` pre-render range and the `units.rs` consumer-side `≤ 8`
clamp must both be widened (to 16 if matching gamemd's populated set, or
to 20 if including a defensive clamp for the unpopulated entries).
