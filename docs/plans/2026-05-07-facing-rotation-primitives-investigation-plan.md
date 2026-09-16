# Facing & Rotation Primitives — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass focused on consolidating
> facing/rotation math across gamemd.exe — **verify existing high-confidence reports,
> reconcile them, and fill the named gaps**. Do NOT re-cover ground that
> [UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md)
> and [BULLETCLASS_TRAJECTORY_AND_HOMING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md)
> already cover at HIGH confidence. The deliverable is one unified primitives doc, not
> a re-derivation.

**Topic:** facing and rotation primitives — the math layer underneath every rotating
entity (unit body, turret, voxel barrel, aircraft heading, infantry walk facing, building
animations, projectile pitch/heading)
**Scope Size:** Medium — ~28 functions, 8 INI keys, 6 prior reports to reconcile
**Est. Effort:** ~6-8 hours of `/re-investigate` work
- Phase 1 (core, FULL depth): 7 functions × ~25min = ~3 hours
- Phase 2 (depth, MEDIUM): 10 functions × ~10min = ~1.5 hours
- Phase 3 (context, LIGHT): 11 functions × ~5min = ~1 hour
- Plus reconciliation + writing: ~1.5 hours
**Prior Research:** 6 reports touch the topic (see §2). Two are HIGH-confidence on
their slices but no doc unifies the primitives layer.
**Expected Output:** `docs/research/FACING_ROTATION_PRIMITIVES_GHIDRA_REPORT.md`
— a single authoritative reference covering FacingClass, ROT semantics, body vs turret
vs barrel separation, voxel matrix linkage, and the canonical facing-from-delta /
clamp-to-ROT primitives.
**Next Pipeline Step:** After report: `/disparity-scan facing` to compare gamemd findings
against current Rust implementation (which is already substantial — see §8).

---

## 1. Goal

When this investigation finishes, the report must answer:

1. **What is the canonical "step a rotating entity toward a desired facing" primitive in
   gamemd.exe?** Specifically: the formula, the clamp behavior at the wrap boundary, the
   ROT-units-to-step conversion, and which entities/locomotors consume it.
2. **How are body, turret, barrel, and voxel-render facings related?** Which fields are
   stored on the entity, which on the type, which are derived per-frame.
3. **Where do per-class facing variants diverge from the primitive?** Specifically:
   infantry (Walk), aircraft (Fly), buildings (animation facing), bullets (homing).
4. **What is the canonical 8-bit ↔ 16-bit conversion convention?** Document the round
   form used everywhere (`(short)(facing + (facing >> 0x1F & 0xFFU) >> 8)` per Agent D)
   and any exceptions.
5. **Where in the tick pipeline does rotation actually run?** Confirm
   "AI → TurretAI + Facing_Update" finding from Agent D and verify the equivalent for
   Walk/Fly/Hover/Jumpjet locomotors.

---

## 2. Prior Research Inventory

| Report | Scope Covered | Confidence | Known Gaps |
|--------|---------------|------------|------------|
| [UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md](../../../ra2-rust-game-docs/UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md) | **FacingClass 24-byte struct (current/desired/ROT/timer/in_motion), turret/barrel facing separation, ROT shifted by 8, body/turret discretization formulas, atan2(dy,-dx) convention, 8-cell TurretAI scan, IFV auto-deploy** | HIGH | Type+0x67C semantic, Type+0x6AF/+0x6AD instance flags, vtable+0x4E4/+0x2E4 semantic labels |
| [BULLETCLASS_TRAJECTORY_AND_HOMING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md) | Homing missile facing/pitch math; helpers `IsWithinROT/GetTurnDelta/ClampToROT` at 0x5B2990/0x5B2950/0x5B29C0; 16-bit pitch convention 0x3FFF=level | HIGH | None stated. Note: bullet uses **same** primitives as units (0x5B29C0 etc.) but does not share unit body code |
| [SPATIAL_PRIMITIVES_LAYER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPATIAL_PRIMITIVES_LAYER_GHIDRA_REPORT.md) | 8-dir tables 0x89F688 / 0x89F6D8; atan2 at 0x4CAE30; facing convention as a category | HIGH | Math primitives not deep-covered (this plan fills it) |
| [LOCOMOTION_MATH_AND_CONSTANTS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOCOMOTION_MATH_AND_CONSTANTS.md) | Drive track waypoints with embedded facing; 11 locomotor CLSIDs; ROT in track flags | HIGH | Per-locomotor rotation step (drive vs walk vs fly vs jumpjet) not normalized |
| [FIRE_AT_ANALYSIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_ANALYSIS.md) | Muzzle-flash anim picked by turret facing; ROF gate; facing in projectile launch velocity | HIGH | Doesn't touch the rotation step itself |
| [INFANTRYCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRYCLASS_GHIDRA_REPORT.md) | SequenceTypeClass+0x0C facing-direction value; sequence remap (Walk→Panic etc.) | MEDIUM | **No infantry rotation step decompiled** — major gap |
| [DRIVE_TRACK_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_TRACK_SYSTEM.md) | Drive track curves; facing transitions per step | HIGH | Doesn't decompose the FacingClass step itself |
| [VOXEL_SLOPE_TILT_SYSTEM.md](../../../ra2-rust-game-docs/VOXEL_SLOPE_TILT_SYSTEM.md), [VXL_DRAW_MATRIX_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_DRAW_MATRIX_GHIDRA_REPORT.md) | Voxel matrix from facing for render | HIGH | Linkage from FacingClass.current → matrix index not documented in primitives terms |

**Conflicts between reports:** None substantive. atan2 convention `atan2(dy, -dx)` is
consistent (UNITCLASS_TURRET_TRACKING + BULLETCLASS_TRAJECTORY). 16-bit shifted ROT
storage is consistent. FIRE_AT mentions a `>> 0xC + 1 >> 1 & 7` discretization for
harvester turning that differs from TURRET_TRACKING §5.2 — Agent A flagged as
non-contradictory (different use cases), but the report should reconcile.

**Implication for scope:** ~50% of the topic is already at HIGH confidence in
TURRET_TRACKING + BULLETCLASS_TRAJECTORY. The plan scopes:
1. **Verification** of those reports' key claims (since the executor hasn't read them
   end-to-end yet) — should take ~30min, not a re-derivation.
2. **Gap fill** for: pedestrian (Walk), aircraft (Fly), building animation, voxel
   matrix linkage as a primitive, the two unnamed helpers, and ROT field offset.
3. **Reconciliation** — produce one unified primitives doc that supersedes scattered
   coverage.

---

## 3. Function Inventory

| # | Phase | Address | Current Name | Scope Reason | Depth | TS Risk |
|---|-------|---------|--------------|--------------|-------|---------|
| 1 | 1 | `0x004C9300` | `FacingClass__UpdateFacing` | **Primary entry point** — per-tick step on the 22-byte FacingClass; called by drive/walk/hover/jumpjet/aircraft/Unlimbo (25+ callsites) | FULL | Low |
| 2 | 1 | `0x005B29C0` | `Facing__ClampToROT` | Clamp current toward target by ROT delta with signed wrap | FULL | Low |
| 3 | 1 | `0x005B2950` | `Facing__GetTurnDelta` | Signed `(cur − desired)` short, wrap-safe | FULL | Low |
| 4 | 1 | `0x005B2990` | `Facing__IsWithinROT` | `|cur − desired| ≤ ROT` test | FULL | Low |
| 5 | 1 | `0x005B2930` | `FUN_005B2930` (UNNAMED) | Sibling helper — likely "build target facing from delta" — needs decomp | FULL | Low |
| 6 | 1 | `0x005B2970` | `FUN_005B2970` (UNNAMED) | Sibling helper alongside #5 in HomingTrack | FULL | Low |
| 7 | 1 | `0x004265B0` | `AnimClass__CalcFacingDir` | **Canonical facing-from-delta** — atan2(dy,dx)→ftol→16-bit | FULL | Low |
| 8 | 1 | `0x00736990` | `UnitClass__Facing_Update` | Body-facing per-tick (incl. carrier/passenger logic) | FULL | Low — verify `+0xCA1` flag |
| 9 | 1 | `0x007468C0` | `UnitClass__TurretAI` | Turret tick — 8-cell scan, sets desired facing (rotation step is in #1) | MEDIUM | Low |
| 10 | 1 | `0x005B20F0` | `BulletClass__HomingTrack` | Reference rotation algorithm using #2-#6 + atan2 — confirms primitive consumers | MEDIUM | Low (already documented in BULLETCLASS_TRAJECTORY) |
| 11 | 2 | `0x00712170` | `TechnoTypeClass__ReadINI` | **Locate ROT field offset only** — find `"ROT"` xref / ReadInt site, extract `+0xXXX` | LIGHT | Low |
| 12 | 2 | `0x004B04D0` | `DriveLocomotionClass__Update_Facing_From_Type` | Type+0x11C byte → vtable+0x7C `Set_Facing`; confirms type→facing default path | MEDIUM | Low |
| 13 | 2 | `0x004B0F20` | `DriveLocomotionClass__Process_Drive_Track` | Track-step facing change; consumer of #1 + 8-dir tables | MEDIUM | Low |
| 14 | 2 | `0x0075AE00` | `WalkLocomotionClass__Set_Facing` | Walker wrapper — usually a thin call to #1, but verify | LIGHT | Low |
| 15 | 2 | `0x00729B40` | `Turret_barrel_tilt` | Voxel barrel pitch update — separate from yaw, uses 16-bit pitch | MEDIUM | Low |
| 16 | 2 | `0x00458810` | `BuildVXLTurretMatrix` | Builds final turret 3x4 matrix from yaw + pitch + body facing | MEDIUM | Low |
| 17 | 2 | `0x0055A730` | `BuildFacingRotationMatrix` | Generic facing → 3x4 matrix builder; render-side primitive | MEDIUM | Low |
| 18 | 2 | `0x007559B0` | `VXL_GetFacingMatrix` | Indexes `g_VXL_FacingMatrices` table by facing — defines matrix granularity (likely 32 slots) | MEDIUM | Low |
| 19 | 2 | `0x00755A40` | `VXL_InterpolatedFacing` | Quaternion Slerp between two precomputed facing matrices | MEDIUM | **Med — verify YR invokes slerp branch** (not just discrete) |
| 20 | 2 | `0x005AE6F0` | `Matrix3x4_BuildFromRotateXAndFacing` | Pitch×yaw matrix builder | LIGHT | Low |
| — | 2 | (TBD during exec) | `WalkLocomotionClass::Process` rotation step | **Gap fill** — locate Walk's per-tick facing update path. Phase 1 deferred candidate. | MEDIUM | Low |
| — | 2 | (TBD during exec) | `FlyLocomotionClass::Process` rotation step | **Gap fill** — aircraft heading update primitive | MEDIUM | Low |
| — | 2 | (TBD during exec) | `JumpjetLocomotionClass::Process` rotation step | **Gap fill** — jumpjet uses separate `JumpJetTurnRate=` INI key (Agent B); confirm the path differs from drive | MEDIUM | Low |
| 21 | 3 | `0x007360C0` | `UnitClass__AI` (caller of #8, #9) | Establishes tick order: AI → TurretAI + Facing_Update | LIGHT | Low |
| 22 | 3 | `0x00451F60` | `BuildingClass__UpdateAnimFacingAndDirection` | Building turret/anim facing distribution | LIGHT | Low |
| 23 | 3 | `0x00452000` | `BuildingClass__UpdateAllAnimFacings` | Sister of #22 | LIGHT | Low |
| 24 | 3 | `0x00465D70` | `Deploy_facing_calculator` | Trivial getter at TechnoClass+0xEDC — confirms cached "deploy-time facing" field | LIGHT | Low |
| 25 | 3 | `0x00706BD0` | `VXL_turret_draw` | Render consumer of facing → frame index | LIGHT | Low |
| 26 | 3 | `0x004B4C80` | `DriveLocomotionClass__Is_Surfacing` | Tunnel/surface facing transitions — confirm reachability in YR | LIGHT | **Med — tunnel-only** |
| 27 | 3 | `0x004D9E70` | `TechnoClass__Is_Surfacing` | Same as #26 | LIGHT | Med |
| 28 | 3 | `0x004CAE30` | `Math__atan2` | Generic atan2 — already known; confirm calling convention only | LIGHT | Low |

**Phase 1 checkpoint:** Pause after #1-#10 and verify TURRET_TRACKING + BULLETCLASS_TRAJECTORY
findings against this fresh decomp. If those reports are accurate (expected), Phase 1
output is mostly a confirmation + the two unnamed helpers (#5, #6) + the ROT-step formula
written as a single canonical pseudocode block. Then proceed to Phase 2 + the gap-fill
discovery for Walk / Fly / Jumpjet rotation paths.

**Total:** 28 named functions + 3 to-be-discovered locomotor rotation steps = **31 functions**.
Within plan size.

---

## 4. Detail Checklist

The executor must extract:

### Magic numbers
- ROT-to-facing-step conversion: how many facing units per ROT per tick? (TURRET_TRACKING
  says ROT is "shifted by 8" — verify the exact formula).
- 8-cell TurretAI scan radius (TURRET_TRACKING claim — verify).
- The 8↔16 bit round formula `(short)(facing + (facing >> 0x1F & 0xFFU) >> 8)` — confirm
  every site that uses it (saw it in #13 per Agent D).
- The 32-step or 64-step body-facing snap formula `((ushort)((*puVar7 >> 7) + 1 >> 1) & 0xFF) + 8) * 0x100`
  in #8 — extract its exact use (Agent D saw it on a "shaking carryall" path).
- Voxel matrix table size at `g_VXL_FacingMatrices` (referenced from #18) — count the
  entries (likely 32, 64, or 256).
- Pitch encoding: 0x3FFF = level per BULLETCLASS_TRAJECTORY — verify and extract
  pitch-up / pitch-down boundaries.

### Bit flags and masks
- `+0xCA1` flag on UnitClass — Agent D flagged for verification (in #8).
- `+0xEDC` cached deploy facing field on TechnoClass (#24).
- `+0x6AF`, `+0x6AD` instance flags on Techno mentioned in TURRET_TRACKING — verify (dirty
  / rotation-pending state).
- `+0x67C` on TechnoType — TURRET_TRACKING flagged it as "no-turret path selector" — confirm.
- `+0x11C` byte on TechnoType (#12) — default facing.

### State machine states
- FacingClass internal state: in_motion flag, timer, current, desired, rate-of-turn —
  document the 22-byte (or 24-byte per TURRET_TRACKING) layout authoritatively.
- TurretAI states: idle / scanning / tracking / harvest-spinning. TURRET_TRACKING
  mentions harvest-spinning (target = current+32768) — verify.

### INI keys to verify
- `ROT=` field offset on TechnoTypeClass — locate at #11. Default value.
- `JumpJetTurnRate=` per-type field offset.
- `DeployFacing=` field offset.
- `MissileROTVar=` global `Rules+0xXXX` offset.
- `WindDirection=` (FacingType-typed; informational only — not consumed by primitives).
- Default `TurnRate=4` in `[General]` — what does this gate? Likely per-frame default ROT
  for unspecified types — verify.

### Struct offsets to extract
- TechnoType `+0x11C` (default facing byte).
- TechnoType `+0x67C` (no-turret selector).
- TechnoType `+0x6AD`, `+0x6AF` (rotation-pending flags) — TURRET_TRACKING claims.
- Techno `+0xCA1` (carryall passenger flag in #8).
- Techno `+0xEDC` (cached deploy facing).
- FacingClass full 22/24-byte layout.
- BulletClass facing/pitch fields used by HomingTrack.

### Clamps, rounding, off-by-ones
- ROT clamp at the 16-bit wrap boundary: does `Facing__ClampToROT` use signed shorts
  throughout? Verify no overflow at `0x7FFF / 0x8000`.
- 8-bit↔16-bit round direction (toward zero per `(facing + (facing >> 0x1F & 0xFFU) >> 8)`).
- Whether ROT=0 is "infinite turn rate" (instant) or "zero turn rate" (locked). Rust
  treats it as instant for infantry; verify gamemd does the same.

### Edge cases to test
- Wrap at 0/65535 boundary in `Facing__GetTurnDelta`.
- Body facing while passenger inside (TURRET_TRACKING mentions vehicle-with-passenger
  rate logic in #8) — confirm.
- Building turret rotation when building destroyed mid-rotation.
- Voxel slerp branch: when does `param_2 != param_3` actually trigger in YR? (Agent D
  flagged TS-legacy risk.)

### Timing / ordering
- Confirm tick order: `UnitClass::AI` calls `TurretAI` then `Facing_Update`, OR the other
  way. Agent D found `UnitClass::AI @ 0x7360C0` calls both — verify order from disasm.
- Where does building animation facing fit? (#22-#23 — likely separate from unit tick.)
- How does projectile facing tick fit? `BulletClass::AI` per
  [BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md).

### TS-legacy flags
- `VXL_InterpolatedFacing` Quaternion slerp (#19) — verify YR actually invokes the slerp
  path (only when source matrix ≠ target matrix). May be TS-era smooth-rotation
  carryover.
- `Is_Surfacing` paths (#26, #27) — tunneling barely used in YR; confirm.
- `Building Anim Facing` (#22-#23) — TS deploy-anim system overlap risk.
- DirType 16-bit — TS heritage but confirmed live in YR (no concern, just historical
  note for the report).

### Vtable dispatches
- Locomotor vtable `+0x7C` = `Set_Facing` (Drive/Walk).
- TechnoClass vtable `+0x3F4`, `+0x49C`, `+0x470` — turret-AI dispatch sites; not yet
  labeled. Resolve at least one.
- TechnoClass vtable `+0x4E4`, `+0x2E4` — TURRET_TRACKING flagged for semantic labeling.

---

## 5. INI Keys in Scope

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|---------|-------------------|----------------------------|
| `ROT=` | per-TechnoType | 0 | Body / turret rotation rate; degrees-per-frame at 15fps base | **YES** — `[object_type.rs](../src/rules/object_type.rs):244` as `turret_rot: i32` |
| `JumpJetTurnRate=` | per-jumpjet-infantry | varies (2/6/10/12/100) | Airborne rotation rate; independent of ground ROT | Partial — `JumpjetParams` per [LOCOMOTION_MATH_AND_CONSTANTS.md §13.7](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOCOMOTION_MATH_AND_CONSTANTS.md), verify field |
| `DeployFacing=` | per-deployable-unit | 0 | Initial facing on deploy (0=N..7=NW) | TBD — check during exec |
| `MissileROTVar=` | `[General]` | 0.25 | Random fluctuation % for guided missile turn rate | NO — gap |
| `WindDirection=` | `[General]` | 1 | Map-level wind direction as FacingType — informational, not a primitive | NO — gap (low priority) |
| `TurnRate=` | `[General]` | 4 | Default unit turn rate fallback?? — unclear, verify | TBD |
| `RadarEventRotationSpeed=` | `[General]` | 0.05 | UI radar event spin — render-only, not a primitive | Out of scope |
| `Rotates=` | art.ini per-image | yes/no | Voxel rotation enable flag (render gate) | TBD |
| `TurretRotateSound=` | per-unit-with-turret | (sound name) | Audio cue during turret rotation — informational | Out of scope (audio doc) |
| `V3RocketTurnRate=`, `DMislTurnRate=`, `CMislTurnRate=` | `[General]` | 0.05 / 0.08 / 0.10 | Rocket-specific pitch maneuverability — bullet domain, partial in [LOCOMOTION_MATH_AND_CONSTANTS.md §11](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOCOMOTION_MATH_AND_CONSTANTS.md) | NO — 36 missile keys total per locomotion doc |

ROT distribution sample (Agent B): ROT=5 most common (61 units), ROT=1 (10 units),
ROT=40 (5 units), ROT=60 (3), ROT=100 (1). Useful for sanity-checking the rotation-step
formula at typical values.

---

## 6. Caller & Integration Map

### Binary callers worth following

| Caller Address | Calls Into | When Invoked | Decompile? |
|----------------|------------|--------------|------------|
| `UnitClass::AI @ 0x7360C0` | #8, #9, indirectly #1 | Per-tick on every active unit | YES — confirms tick order |
| `BulletClass::AI` (TBD addr) | #2-#7, #10 | Per-tick on every in-flight bullet | LIGHT — already covered in BULLETCLASS_TRAJECTORY |
| `BuildingClass::AI` (TBD) | #22, #23 | Per-tick on every building | LIGHT — confirms building turret tick scheduling |
| Locomotor `Process` virtual (Drive/Walk/Hover/Jumpjet/Fly/Ship) | #1 directly via vtable+0x7C | Per-tick during locomotor work | MEDIUM — at minimum confirm Walk + Fly + Jumpjet invoke #1 (if not, those have separate paths) |
| `TechnoClass::Unlimbo` | #1 | Once at object spawn / placement | LIGHT |
| `TechnoClass::Mission_Attack` | #1 (via Set_Facing) | Per-mission state | LIGHT |
| `TechnoClass::Begin_Takeoff` | #1 | Aircraft state transition | LIGHT |

### Rust integration

Current Rust facing/rotation surface (per Agent C):
- [util/facing_table.rs](../src/util/facing_table.rs) — sine/cosine LUT, facing_to_movement
- [util/fixed_math.rs:264-343](../src/util/fixed_math.rs#L264-L343) — `facing_from_delta_int`, `facing_from_delta_int_u16`, `dir_to_cell_delta`
- [sim/movement/turret.rs](../src/sim/movement/turret.rs) — full 16-bit turret system, `tick_turret_rotation`, ROT clamp helpers
- [sim/movement/movement_step.rs:69-137](../src/sim/movement/movement_step.rs#L69-L137) — body facing update on path step
- [sim/movement/movement_tick.rs](../src/sim/movement/movement_tick.rs) — ground movement tick orchestration
- [sim/movement/drive_track.rs](../src/sim/movement/drive_track.rs) — pre-computed track curves
- [sim/game_entity.rs:54-59](../src/sim/game_entity.rs#L54-L59) — `facing: u8`, `facing_target: Option<u8>`, `turret_facing: Option<u16>`
- [rules/object_type.rs:244](../src/rules/object_type.rs#L244) — `turret_rot: i32`

**Disparity-scan target after report**: compare gamemd's exact ROT-step formula and
clamp behavior against Rust's `rot_to_facing_delta_u16` and `shortest_rotation_u16` to
catch off-by-step / wrap-direction drift.

### Callers explicitly NOT in scope
- Pure render consumers (sprite/voxel frame-from-facing lookup) beyond the matrix
  builders #16-#20 — covered well in existing voxel docs.
- Audio cues (TurretRotateSound) — audio-system concern.
- UI/radar rotation (RadarEventRotationSpeed) — UI-only.

---

## 7. TS-Legacy Risk Register

| Item | Risk | Verification Approach |
|------|------|----------------------|
| **`VXL_InterpolatedFacing` Quaternion slerp (#19)** | Med — likely TS-era smooth-rotation carryover. May be dead in YR. | Trace callers of #19; check if param_2 ≠ param_3 ever holds in normal YR play (i.e., is the slerp branch hot or always discrete?). |
| **`Is_Surfacing` paths (#26, #27)** | Med — tunneling barely used in YR. | Confirm whether any standard YR unit triggers tunnel; if not, mark as TS-legacy and skip detailed decomp. |
| **Building animation facing (#22, #23)** | Low-Med — TS deploy-anim overlap. | Trace one stock YR building with rotating turret (Prism Tower, Tesla Coil) through the function; confirm it's the live path. |
| **`MissileROTVar=` randomization** | Low | Verify gamemd actually applies the variance per-shot in HomingTrack (not just at INI parse). |
| **DirType 16-bit storage** | None — confirmed live in YR | Already verified in TURRET_TRACKING. Note in report as "TS heritage but live primitive". |
| **`Floater=` projectile + `Rules.Gravity` interaction** | Low — already addressed in INRANGE doc | Cross-reference for completeness. |

---

## 8. Current Rust Implementation Surface

Already substantial — see §6 "Rust integration" for full file list. Key state on entities:

```rust
// On GameEntity:
facing: u8                          // body facing (always)
facing_target: Option<u8>           // desired body (vehicles only)
turret_facing: Option<u16>          // 16-bit turret (units with turrets)

// On ObjectType:
turret_rot: i32                     // ROT in deg/frame at 15fps
```

Known gaps in Rust per Agent C:
- Facing→animation-frame mapping (sequence selection) — render-side, separate from
  this investigation.
- Reverse 16→8 conversion explicit helper — formula exists implicitly.
- Per-tick rotation queue / batching — none today (per-entity).
- Body rotation step uses `shortest_rotation` + `rot_to_facing_delta_u16` — verify the
  combined formula matches gamemd's `Facing__ClampToROT`.

The investigation's report should call out **specific Rust functions to compare** in a
"Rust parity notes" section so a follow-up `/disparity-scan facing` lands quickly.

---

## 9. Deferred Open Questions

These are questions the scoping scan surfaced but couldn't answer — execute as part of
the investigation:

1. **What is the exact `ROT` field offset on TechnoTypeClass?** Agent D found no
   standalone `"ROT"` string in .data; locate via xref-walk inside `TechnoTypeClass::ReadINI`
   (#11). Likely 4 bytes, near other rotation/movement fields.
2. **What does `JumpJetTurnRate=` field offset look like, and is it consumed by a different
   primitive than #1?** (Jumpjet may use its own rotation step.)
3. **`TurnRate=4` default in `[General]` — what does it gate?** Possibly a fallback for
   types with no `ROT=`. Verify.
4. **Where does Walk locomotor's per-tick rotation step live?** Agent D got
   `WalkLocomotionClass__Set_Facing @ 0x75AE00` (a wrapper) but not the per-tick stepper
   — needed for infantry parity.
5. **Where does Fly locomotor's heading update live?** Aircraft heading needs its own
   primitive trace.
6. **What is in `g_VXL_FacingMatrices` (referenced from #18)?** Count of slots, layout,
   indexing function — defines voxel rotation granularity.
7. **Do the two unnamed helpers `FUN_005B2930` and `FUN_005B2970` (#5, #6) participate
   in unit body rotation, or only in bullet homing?** Cross-reference callers.
8. **Is the harvester-turn discretization in FIRE_AT (`>> 0xC + 1 >> 1 & 7`) the same
   primitive as TURRET_TRACKING §5.2's body-discretization, or genuinely different?**
9. **Voxel slerp branch reachability in YR** — does the slerp path actually fire in
   normal play, or is it dead? (TS-legacy concern.)
10. **TechnoClass+0xEDC** — confirm cached deploy facing semantic. Used where else
    besides #24?

---

## 10. Execution Strategy

**Recommended: Multi-phase single-session `/re-investigate`** with the Phase 1 checkpoint
gating Phase 2/3.

Steps:

1. **Pre-read** (~15min): Skim TURRET_TRACKING and BULLETCLASS_TRAJECTORY end-to-end so
   the executor doesn't re-derive what's already HIGH-confidence. Note exact claims to
   verify, not regenerate.
2. **Phase 1 — Core primitives** (~3 hours):
   - Decompile #1-#7 to FULL depth.
   - Verify TURRET_TRACKING's ROT-shift-by-8 claim, atan2(dy,-dx) convention, FacingClass
     22/24-byte layout.
   - Resolve the two unnamed helpers (#5, #6) and document them.
   - Write a single canonical pseudocode block for "step facing toward desired by ROT"
     that all consumers reduce to.
   - **Checkpoint**: summarize findings; if scope drifts, revise plan.
3. **Phase 2 — Type-side + locomotor + voxel** (~1.5 hours):
   - Decompile #11-#20 to MEDIUM depth.
   - Locate ROT field offset in TechnoTypeClass::ReadINI.
   - Discover Walk / Fly / Jumpjet rotation-step paths (gap fill).
   - Document voxel matrix table at #18.
4. **Phase 3 — Context and edges** (~1 hour):
   - Decompile #21-#28 to LIGHT depth.
   - Confirm tick order via UnitClass::AI.
   - Verify TS-legacy risks (slerp, tunnel paths, building anim facing live in YR).
5. **Synthesis & write** (~1.5 hours):
   - Reconcile findings across the 6 prior reports.
   - Produce `FACING_ROTATION_PRIMITIVES_GHIDRA_REPORT.md` with one canonical primitive
     section + per-class variants section + Rust parity notes.
   - Mark each prior report's relevant section as "superseded" (don't delete — leave
     the historical findings intact).

**Subagent batching is NOT recommended** for this investigation — the central goal is
*reconciliation* across primitives that all touch the same underlying state machine.
That synthesis works better in a single context than across parallel agents. Save
subagents for "extract 30 magic numbers from these 30 functions" style work, which
this isn't.

---

## 11. Success Criteria

The executed research document must:

- [ ] Answer all 5 questions in §1 with HIGH confidence.
- [ ] Include every Phase 1 function (#1-#10) decompiled FULL depth, every Phase 2
      MEDIUM, every Phase 3 LIGHT — or explicitly justify omission.
- [ ] Resolve the 10 deferred questions in §9 (or re-document any that remain
      unresolved with a clear "still open" tag).
- [ ] State **"Active in YR: Yes/No/Conditional"** for every finding, especially the
      TS-legacy risks in §7.
- [ ] Cite Ghidra addresses for every HIGH-confidence claim.
- [ ] Include a single canonical pseudocode block for the rotation step primitive
      that consumers (drive, walk, fly, jumpjet, bullet, building) all reduce to (or
      explicitly enumerate per-locomotor variants if they don't unify).
- [ ] Include a "Rust parity notes" section listing exact comparison targets:
      `Facing__ClampToROT` vs `rot_to_facing_delta_u16`, gamemd 16↔8 round formula vs
      Rust's `body_facing_to_turret`, etc.
- [ ] Mark all relevant sections in the 6 prior reports as "superseded by
      `FACING_ROTATION_PRIMITIVES_GHIDRA_REPORT.md`" or "retained as deep-dive on X" —
      avoid duplicate authority on the same claim.

---

## Sources

- **Ghidra addresses sampled** (Agent D scoping pass, light): `0x004C9300`, `0x005B29C0`,
  `0x005B2950`, `0x005B2990`, `0x005B2930`, `0x005B2970`, `0x004265B0`, `0x00736990`,
  `0x007468C0`, `0x005B20F0`, `0x00712170`, `0x004B04D0`, `0x004B0F20`, `0x0075AE00`,
  `0x00729B40`, `0x00458810`, `0x0055A730`, `0x007559B0`, `0x00755A40`, `0x005AE6F0`,
  `0x007360C0`, `0x00451F60`, `0x00452000`, `0x00465D70`, `0x00706BD0`, `0x004B4C80`,
  `0x004D9E70`, `0x004CAE30`. Plus the 8-dir tables `0x89F688` / `0x89F6D8`.
- **Docs scanned** (Agent A): UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING, BULLETCLASS_TRAJECTORY_AND_HOMING,
  SPATIAL_PRIMITIVES_LAYER, LOCOMOTION_MATH_AND_CONSTANTS, FIRE_AT_ANALYSIS,
  INFANTRYCLASS, DRIVE_TRACK_SYSTEM, DRIVE_SHARP_TURN_FALLBACK_RE, VOXEL_SLOPE_TILT_SYSTEM,
  VXL_DRAW_MATRIX, plus passing references in BUILDINGCLASS_*.
- **INI files checked** (Agent B): `ini/rulesmd.ini`, `ini/artmd.ini`, `ini/rules.ini`,
  `ini/art.ini`.
- **Rust files mapped** (Agent C): `src/util/facing_table.rs`, `src/util/fixed_math.rs`,
  `src/sim/movement/turret.rs`, `src/sim/movement/movement_step.rs`,
  `src/sim/movement/movement_tick.rs`, `src/sim/movement/drive_track.rs`,
  `src/sim/game_entity.rs`, `src/rules/object_type.rs`.
- **Related plans:** None — this is the first plan for facing/rotation primitives.
- **Related candidate doc**: [SPATIAL_RESEARCH_CANDIDATES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPATIAL_RESEARCH_CANDIDATES.md)
  §1 (this investigation closes that candidate).
