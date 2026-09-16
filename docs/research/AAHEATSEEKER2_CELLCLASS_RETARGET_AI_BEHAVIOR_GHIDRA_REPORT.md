# AAHeatSeeker2 CellClass Retarget AI Behavior - Ghidra Research Report

**Date:** 2026-05-20  
**Binary:** `gamemd.exe` (Yuri's Revenge)  
**Address(es):** `BulletClass::AI @ 0x004666E0`, `BulletClass` pointer-expired handler body `0x004684E0..0x004685C6`, `BulletClass::UpdateTarget @ 0x00468430`, `BulletClass::HomingTrack @ 0x005B20F0`, `MapClass::Get_CellClass @ 0x005657A0`, `CellClass` vtable `0x007E4EEC`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** ROT>0 AAHeatSeeker2/DRAGON bullet behavior after `BulletClass+0x10C` has been retargeted from a removed/destroyed non-high-flying ground object to a `CellClass*`.  
**Non-Scope:** exact HomingTrack turn math, full proximity detector internals, damage formulas, all non-destruction limbo transitions, and every projectile that happens to reuse `AAHeatSeeker2`.  
**Confidence:** High for the scoped vtable dispatches, coordinate reads, WhatAmI result, arming/homing flags, and safe active YR path. Medium for semantic names of some BulletType flags because this slice records their branch effect only.  
**Active in YR:** Yes. Stock YR `[GGI] Secondary=MissileLauncher`, `[MissileLauncher] Projectile=AAHeatSeeker2`, and `[AAHeatSeeker2] Image=DRAGON`, `ROT=60`, `AA=yes`, `AG=yes` put deployed Guardian GI missiles on this standard `BulletClass::AI` path.

## 1. Overview

When a ground target dies or is removed, the bullet invalidation handler can replace `BulletClass+0x10C` with `MapClass::Get_CellClass(last_target_cell)` instead of clearing it. The ROT>0 homing branch in `BulletClass::AI` safely accepts that `CellClass*`: it calls CellClass coordinate virtuals, observes `CellClass::WhatAmI() == 0x0B`, does not take the ObjectClass-only alternate-coordinate dispatch, and passes the non-aircraft flag into `HomingTrack`.

This means the missile continues to home toward the last known cell center/bridge-adjusted cell coordinate, not toward the destroyed object and not through the null-target sentinel branch. Active in YR: Yes; this is the normal stock YR bullet path after the already-verified retarget branch fires.

## 2. Key Offsets and Vtable Slots

| Field / slot | Verified value | Meaning in this slice | Evidence | Active in YR |
|---|---:|---|---|---|
| `BulletClass+0x10C` | target pointer | Holds object pointer before invalidation; may become `CellClass*` after ground-target removal | AI reads `[EBP+0x10C]`; handler writes `[ESI+0x10C]` at `0x00468594` | Yes |
| `BulletTypeClass+0x2DC` | `ROT` | `AAHeatSeeker2 ROT=60` selects ROT>0 homing branch | `BulletClass::AI @ 0x004666E0`; `rulesmd.ini:25687` | Yes |
| `CellClass` primary vtable | `0x007E4EEC` | Constructor-installed vtable used after retarget | `CellClass::Constructor @ 0x0047BBF0`, store at `0x0047BD5C` | Yes |
| `CellClass` vtable `+0x2C` | `0x00487E60` | `WhatAmI`, returns `0x0B` | PE/vtable byte read; assembly `MOV EAX,0xB; RET` | Yes |
| `CellClass` vtable `+0x48` | `0x00486840` | cell center coord: `(cell_x << 8)+0x80`, `(cell_y << 8)+0x80`, ground Z from `0x0047B3A0` | assembly `0x00486840..0x00486886` | Yes |
| `CellClass` vtable `+0x54` | `0x00410530` | false stub, so CellClass is not "high-flying"/late-falling | vtable byte read; `FUN_00410530` returns `0` | Yes |
| `CellClass` vtable `+0x58` | `0x00486890` | target coordinate used by AI; delegates to `+0x48`, adding `DAT_0089E7B4` to Z when `cell+0x140 & 0x100` | assembly `0x00486890..0x004868F8` | Yes |
| `CellClass` `AbstractFlags+0x14` | low bits cleared | `CellClass` does not set ObjectClass bit `0x02`; AI therefore skips vtable `+0xA4` alternate-coordinate call | `AbstractClass::Constructor_Full @ 0x00410170`; `CellClass::Constructor @ 0x0047BBF0`; AI `0x00466B83..0x00466BAF` | Yes |

## 3. Retarget Writer Recap

The standard removed-target writer is the `BulletClass` pointer-expired handler body at `0x004684E0`. If the expired pointer equals `BulletClass+0x10C`, map editor mode is off, target vtable `+0x54` is false, and the target cell is not the off-map sentinel, the handler calls `MapClass::Get_CellClass @ 0x005657A0` and writes the returned pointer back to `+0x10C` at `0x00468594`. Active in YR: Yes; this is reached from `ObjectClass::UnInit -> Detach_From_All_Lists -> BulletClass` vtable slot `+0x28`, per the prior report.

`MapClass::Get_CellClass` indexes `Y * 0x200 + X`; if the index is outside `0..0x3FFFF` or the cell pointer is null, it stores the requested cell coordinate in `DAT_00ABDC74` and returns the dummy cell at `DAT_00ABDC50`. Active in YR: Yes. For normal destroyed ground targets in playable map cells, the return is the real map cell; the fallback still returns a `CellClass`-compatible object rather than a dangling pointer.

`BulletClass::UpdateTarget @ 0x00468430` mirrors the same cell-retarget/clear logic but remains chrono/teleport-specific from the prior report. Active in YR: Conditional; not the normal target-death path.

## 4. ROT>0 AI After `+0x10C = CellClass*`

### 4.1 Coordinate read ordering

In the ROT>0 branch, `BulletClass::AI` first checks whether `+0x10C` is null. For a retargeted cell it is non-null, so AI calls target vtable `+0x58` at `0x00466B53..0x00466B5D` and copies the returned coordinate into its homing target local. Active in YR: Yes.

For `CellClass`, vtable `+0x58` resolves to `0x00486890`. That method checks `cell+0x140 & 0x100`: if set, it calls `+0x48` and adds `DAT_0089E7B4` to returned Z; otherwise it returns the `+0x48` coordinate unchanged. Active in YR: Yes. The bridge-structural bit is standard `CellClass` map data; this report does not rederive the runtime value of `DAT_0089E7B4`.

Because `CellClass.AbstractFlags` bit `0x02` is not set, the AI branch at `0x00466B83..0x00466BAF` does not call target vtable `+0xA4`. Active in YR: Yes. This is the critical safety distinction: `+0xA4` is ObjectClass-path behavior, and the retargeted CellClass does not satisfy the ObjectClass flag gate.

### 4.2 WhatAmI and aircraft flags

After the coordinate read, `BulletClass::AI` computes the aircraft-special homing flag by calling target vtable `+0x2C` only when `+0x10C` is non-null. For `CellClass`, slot `+0x2C` returns `0x0B`, so the compare against `2` fails and AI passes `0` as the aircraft flag to `BulletClass::HomingTrack @ 0x005B20F0`. Evidence: `0x00466CD4..0x00466CE7`, CellClass vtable slot `0x007E4F18 -> 0x00487E60`. Active in YR: Yes.

The same strict `WhatAmI()==2` predicate is used at fire time for the arming override. The retarget to `CellClass*` happens after launch, so it cannot retroactively change the detector's launch-time `Arm=2` for a ground target. Active in YR: Yes. Evidence: `BulletClass::Fire @ 0x00468A3F..0x00468A63`; `[AAHeatSeeker2] Arm=2` at `rulesmd.ini:25679`.

### 4.3 Homing and lost-target behavior

A retargeted `CellClass*` is not the null-target sentinel path. The sentinel branch at `0x00466B51..0x00466B67` is used only if `+0x10C == 0`, assigning `DAT_0089DE30/34/38` as the target coordinate. Active in YR: Yes. Since the CellClass pointer is non-null and returns an ordinary coordinate, the later "sentinel target plus height >= Rules.FlightLevel" detonation branch does not fire for the retargeted-cell case unless the CellClass method itself returned the sentinel, which this slice did not observe.

`HomingTrack` therefore receives a normal coordinate and the non-aircraft flag. Its ground-target branch remains active, including terrain/height sampling and pitch correction. Active in YR: Yes. Evidence: `BulletClass::AI` call at `0x00466D31`; `HomingTrack @ 0x005B20F0`.

## 5. Detonation Coordinate Snapping

Two snapping opportunities matter after the target is a `CellClass*`.

First, the ROT>0 close-target branch can set the pending detonation coordinate to the target coordinate returned by vtable `+0x58`, provided the target coordinate is not the null sentinel and the projectile type flag at `+0x294` is false. For `AAHeatSeeker2`, this uses the CellClass `+0x58` coordinate, not the destroyed object's last object center. Evidence: `BulletClass::AI @ 0x004666E0`, decompiled branch after the `HomingTrack` distance check; sentinel comparison against `DAT_0089DE30/34/38`. Active in YR: Yes.

Second, near the final detonation path, if the bullet still has a target pointer and the proximity/near-object condition is active, AI calls target vtable `+0x58` for a distance test and may then call target vtable `+0x48` before writing the bullet coordinate through its own vtable `+0x1B4`. Evidence: assembly `0x00467CA9..0x00467E4D`: target `+0x58` at `0x00467D02`, target `+0x48` at `0x00467E47`, bullet coordinate setter at `0x00467E4D`. Active in YR: Yes. For `CellClass`, this means final snap can use the cell center/ground coord (`+0x48`) after a `+0x58` bridge-aware distance test.

`BulletClass::BulletDetonation @ 0x00468D80` also treats a non-null target generically. For a `CellClass*`, its target `+0x54` false stub means the ObjectClass-specific `piVar7` path is not taken, while the generic distance-to-target `+0x48` checks remain safe. Active in YR: Yes.

## 6. INI Keys

| File | Section | Key | Value | Effect in this slice | Active in YR |
|---|---|---:|---|---|---|
| `rulesmd.ini:3868` | `[GGI]` | `Secondary` | `MissileLauncher` | deployed Guardian GI weapon source | Yes |
| `rulesmd.ini:22574` | `[MissileLauncher]` | `Projectile` | `AAHeatSeeker2` | creates this bullet type | Yes |
| `rulesmd.ini:22575` | `[MissileLauncher]` | `Speed` | `30` | target speed for homing bullet, not retarget-specific | Yes |
| `rulesmd.ini:25679` | `[AAHeatSeeker2]` | `Arm` | `2` | launch-time detector delay for non-`WhatAmI()==2` ground target | Yes |
| `rulesmd.ini:25682` | `[AAHeatSeeker2]` | `Proximity` | `no` | parsed but does not disable this ROT>0 detector/AI path | Parsed yes; not a disable here |
| `rulesmd.ini:25684..25685` | `[AAHeatSeeker2]` | `AA` / `AG` | `yes` / `yes` | fire legality for air and ground targets | Yes |
| `rulesmd.ini:25686..25687` | `[AAHeatSeeker2]` | `Image` / `ROT` | `DRAGON` / `60` | DRAGON homing missile; ROT>0 branch | Yes |
| `artmd.ini:14755..14760` | `[DRAGON]` | `UseLineTrail`, `Rotates` | `yes`, `yes` | presentation only; confirms stock DRAGON projectile identity | Yes |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Pointer-expired retarget to `CellClass*` | verified | `0x004684E0..0x00468594`; prior target invalidation report | none for this slice |
| `MapClass::Get_CellClass` return safety | verified | `0x005657A0` | none |
| `CellClass` vtable base and slots | verified | constructor stores `0x007E4EEC`; PE/vtable slots `+0x2C/+0x48/+0x54/+0x58/+0xA4` read | none |
| AI first coordinate read from retargeted cell | verified | `0x00466B53..0x00466BAF`; `CellClass +0x58 @ 0x00486890` | none |
| AI `WhatAmI()==2` aircraft flag after retarget | verified | `0x00466CD4..0x00466CE7`; `CellClass::WhatAmI @ 0x00487E60` returns `0x0B` | none |
| Fire-time arming override unaffected by later retarget | verified | `BulletClass::Fire @ 0x00468A3F..0x00468A63`; `rulesmd.ini:25679` | none |
| Null/sentinel lost-target branch distinction | verified | `0x00466B51..0x00466B67`; sentinel checks in AI | none |
| Detonation coordinate snapping to CellClass coords | verified | `0x00467CA9..0x00467E4D`; `BulletDetonation @ 0x00468D80` | exact semantic names for type flags `+0x294/+0x2A2` are not rederived |
| Non-destruction limbo retarget behavior | deferred | out-of-scope from parent slot | separate limbo/transport/garrison census |

## 8. Open Questions - Final State

[RESOLVED] OQ-AAH-CELL-001 - Does AI dereference the retargeted `CellClass*` as if it were an ObjectClass and crash/read object fields? Answer: no in the scoped ROT>0 branch; it uses virtual calls and skips ObjectClass-only `+0xA4` because CellClass lacks AbstractFlags bit `0x02`. Evidence: `0x00466B53..0x00466BAF`; `AbstractClass::Constructor_Full @ 0x00410170`; `CellClass::Constructor @ 0x0047BBF0`.

[RESOLVED] OQ-AAH-CELL-002 - Which coordinate does homing read from a retargeted cell? Answer: vtable `+0x58`; for CellClass this delegates to cell center `+0x48` and may add `DAT_0089E7B4` to Z on `cell+0x140 & 0x100`. Evidence: `0x00466B53..0x00466B5D`; CellClass vtable `0x007E4F44 -> 0x00486890`.

[RESOLVED] OQ-AAH-CELL-003 - What does the aircraft-special flag become after retarget? Answer: false, because `CellClass::WhatAmI()` returns `0x0B`, not `2`. Evidence: `0x00466CD4..0x00466CE7`; CellClass slot `+0x2C`.

[RESOLVED] OQ-AAH-CELL-004 - Can the later CellClass target change the launch-time arm delay? Answer: no; the `WhatAmI()==2` arming override is evaluated in `BulletClass::Fire` before target removal/retarget. Evidence: `0x00468A3F..0x00468A63`; `rulesmd.ini:25679`.

[RESOLVED] OQ-AAH-CELL-005 - Is the retargeted-cell case the same as lost/null target? Answer: no; null target alone selects `DAT_0089DE30/34/38`; a non-null CellClass returns ordinary cell coordinates. Evidence: `0x00466B51..0x00466B67`; `0x00486840`; `0x00486890`.

[DEFERRED] OQ-AAH-CELL-006 - What exact gameplay label belongs to BulletType flags `+0x294` and `+0x2A2` used around detonation snapping? Category: out-of-scope. This report only records their branch effect for AAHeatSeeker2; a BulletType flag inventory should name them.

[DEFERRED] OQ-AAH-CELL-007 - Do every transport/garrison/non-destruction limbo transition retarget or preserve in-flight pointers? Category: out-of-scope. This slot covers the destroyed/removed ground-target-to-CellClass path already established by the prior report.

## Sources

- Ghidra decompile/read-only:
  - `BulletClass::AI @ 0x004666E0`
  - `BulletClass::Fire @ 0x00468670`
  - `BulletClass::BulletDetonation @ 0x00468D80`
  - `BulletClass::UpdateTarget @ 0x00468430`
  - `BulletClass` pointer-expired handler body `0x004684E0..0x004685C6`
  - `BulletClass::HomingTrack @ 0x005B20F0`
  - `MapClass::Get_CellClass @ 0x005657A0`
  - `CellClass::Constructor @ 0x0047BBF0`
  - `AbstractClass::Constructor_Full @ 0x00410170`
  - `CellClass` slot methods at `0x00487E60`, `0x00486840`, `0x00410530`, `0x00486890`, `0x00557E10`
- Read-only binary vtable byte inspection of `gamemd.exe`:
  - `CellClass` vtable base `0x007E4EEC`
  - `+0x2C -> 0x00487E60`
  - `+0x48 -> 0x00486840`
  - `+0x54 -> 0x00410530`
  - `+0x58 -> 0x00486890`
  - `+0xA4 -> 0x00557E10`
- Prior reports:
  - [docs/research/GGI_MISSILELAUNCHER_AAHEATSEEKER2_PROJECTILE_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GGI_MISSILELAUNCHER_AAHEATSEEKER2_PROJECTILE_LIFECYCLE_GHIDRA_REPORT.md)
  - [docs/research/BULLETCLASS_TARGET_INVALIDATION_AAHEATSEEKER2_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TARGET_INVALIDATION_AAHEATSEEKER2_GHIDRA_REPORT.md)
  - [docs/research/AAHEATSEEKER2_TARGET_TYPE_HOMING_GROUND_ROCKETEER_AIRCRAFT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_TARGET_TYPE_HOMING_GROUND_ROCKETEER_AIRCRAFT_GHIDRA_REPORT.md)
  - [docs/research/AAHEATSEEKER2_ARMING_PROXIMITY_DETECTOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AAHEATSEEKER2_ARMING_PROXIMITY_DETECTOR_GHIDRA_REPORT.md)
  - [docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md)
  - `docs/research/CELLCLASS_STRUCT_GHIDRA_REPORT.md`
- INI:
  - `ini/rulesmd.ini:3868`
  - `ini/rulesmd.ini:22574..22575`
  - `ini/rulesmd.ini:25679..25690`
  - `ini/artmd.ini:14755..14760`
