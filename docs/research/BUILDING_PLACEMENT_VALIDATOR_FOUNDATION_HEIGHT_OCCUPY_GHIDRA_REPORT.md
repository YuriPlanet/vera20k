# Building Placement Validator Foundation / Height / Occupy - Ghidra Research Report

**Address(es):** `0x00716150`, `0x0047C620`, `0x0045EE70`, `0x004FB0E0`, `0x007393C0`, `0x00441F60`, `0x005683C0`, `0x005687F0`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** final building placement validation for normal ready placement and unit/MCV deploy as it relates to base foundation cells, AddOccupy/RemoveOccupy, build-blocked terrain, and mixed-height cells.  
**Non-Scope:** placement preview rendering, wall/overlay placement visuals, full A* movement after placement, exact naming of every cell flag bit, and unrelated deploy lifecycle transfer details.  
**Confidence:** High for base foundation vs Add/Remove separation, height absence, and MCV/ready placement terrain gate equivalence; Medium for exact semantic names of some cell flag bits in `Cell_passability_building_placement`.  
**Active in YR:** Yes. These are normal YR building placement and `DeploysInto=` paths; no TS-only FogOfWar gate controls the scoped checks.

## 1. Overview

The active placement validators walk the building type's base foundation cell list. They do not merge `AddOccupy%d` or `RemoveOccupy%d` into the placement footprint, and they do not require all foundation cells to share the same terrain height. Terrain/build-block legality is handled per foundation cell by `Cell_passability_building_placement @ 0x0047C620`, not by a separate same-height rectangle test.

MCV/unit deploy does not use a stricter height/build-block validator than ordinary ready placement. `UnitClass::Deploy @ 0x007393C0` calls the building type placement virtual before creating/unlimboing the building; the virtual path is the same base-foundation/per-cell passability class used by normal building placement validation. The later `BuildingTypeClass::CanBePlacedAt @ 0x0045EE70` path is an object/overlay/scatter validator and likewise walks the base foundation list only.

## 2. Class Layout / Key Offsets

| Offset / item | Meaning | Evidence | Active in YR |
|---|---|---|---|
| `BuildingTypeClass+0xDFC` | base foundation cell-list pointer | prior report: assigned from foundation table at `0x0046152C..0x00461541`; returned by `0x0045EC20` | Yes |
| vtable `+0x90` | returns foundation cell offsets, sentinel `(0x7FFF,0x7FFF)` | `0x00716150`, `0x0045EE70` call slot `+0x90` and walk sentinel list | Yes |
| vtable `+0xA8` | placement virtual for type cell validation | `UnitClass::Deploy @ 0x007394D8`; vtable data xrefs to `FUN_00716150` | Yes |
| `BuildingTypeClass+0x1624..0x1660` | eight `AddOccupy%d` pairs | parser loop `0x00461425..0x00461486`; string `0x0081A634` | Conditional: parsed always, effect gated |
| `BuildingTypeClass+0x1664..0x16A0` | eight `RemoveOccupy%d` pairs | parser loop `0x0046148A..0x004614D5`; string `0x0081A624` | Conditional: parsed always, effect gated |
| `BuildingTypeClass+0x1766` | `CanHideThings` gate, default true | prior report: constructor default and read at `0x0046140F` | Conditional |
| `CellClass+0x100` | hidden occupancy counter adjusted by Add/Remove path | `TechnoClass` enter/exit writers `0x005683C0`, `0x005687F0` | Conditional |
| `CellClass+0x11B` / `+0x11C` | level / slope byte family | placement draw reads for sprite Y, passability reads `+0x11C`; no validator compares all `+0x11B` values | Yes |
| `CellClass+0x140` | terrain/blocking flags used by per-cell predicate | `Cell_passability_building_placement @ 0x0047C620` tests `0x100`-derived bit and `0x400` | Yes |

## 3. Core Logic

### Ready placement validation

The building type placement virtual `FUN_00716150 @ 0x00716150` takes a type and target cell, gets the type's foundation offsets through vtable `+0x90`, adds each offset to the target cell, and validates each resulting cell. For building types (`WhatAmI()==7` in this type-class context), each base foundation cell is passed to `Cell_passability_building_placement @ 0x0047C620`.

Active in YR: Yes. Evidence: `0x00716150` has vtable data xrefs and is the building-type placement virtual reached by active placement/deploy validation paths. It has no TS-only guard and no FogOfWar gate.

### Per-cell terrain/build-block predicate

`Cell_passability_building_placement @ 0x0047C620` is the build-blocked terrain/object predicate. It rejects ordinary building placement on hard cell occupants and terrain/overlay states, with special exceptions for walls/laser fence/bridge repair hut style buildings. For ordinary no-overlay terrain it checks in-bounds/on-screen status, occupation bits at `CellClass+0x124`, blocking flags at `CellClass+0x140`, slope byte `+0x11C`, and land/speed type tables.

Active in YR: Yes. Evidence: direct calls from `BuildingPlacement_per_cell_draw @ 0x0047EC90`, wall placement helpers, `FUN_00716150 @ 0x00716209`, and building/overlay placement wrappers. No TS-only feature gate was found for these checks.

### Commit-time ready placement

`HouseClass::Place_Production @ 0x004FB0E0` creates/gets the ready object and calls the building object's unlimbo/place virtual (`vtable+0xD8`) with the clicked cell coordinate. It does not add a same-height check after the ready-placement validator. On failure it plays `EVA_CannotDeployHere` for the player and clears UI placement state.

Active in YR: Yes. Evidence: normal factory-complete ready building path at `0x004FB0E0`; called for RTTI 6/7 building production.

### MCV/unit deploy validation

`UnitClass::Deploy @ 0x007393C0` gets the `DeploysInto` building type, computes the target origin, then calls the target building type's vtable `+0xA8` before facing correction and before allocating the new `BuildingClass`. If that virtual returns false, the human-player path plays cannot-deploy EVA and exits; the non-player path may additionally call `BuildingTypeClass::CanBePlacedAt @ 0x0045EE70` for object/scatter handling.

Active in YR: Yes. Stock `AMCV`, `SMCV`, `PCV`, and `SMIN` use `DeploysInto=` in `rulesmd.ini`, and this is the live conversion path. No TS-only gate was found.

### CanBePlacedAt object/scatter validator

`BuildingTypeClass::CanBePlacedAt @ 0x0045EE70` also walks the base foundation list from vtable `+0x90`. It rejects out-of-bounds cells, disallowed overlays/buildings, terrain objects, enemy/non-scatterable occupants, and performs allied scatter side effects when appropriate. It does not call the Add/Remove hidden occupancy tables and does not compare terrain heights across the foundation.

Active in YR: Yes, but not in the normal human sidebar-ready placement commit.
Its complete direct-caller census is `UnitClass::Deploy @ 0x00739536`,
`FUN_006ED4D0 @ 0x006ED6E0`, and
`BuildingClass__ExitObject_Main @ 0x00445210`. Normal ready placement instead
uses `HouseClass::Place_Production -> BuildingClass::Unlimbo ->
BuildingClass::Can_Enter_Cell -> BuildingType +0xA8 -> 0x0047C620`, so its
ground occupants reject placement rather than taking this helper's allied
scatter side effect.

## 4. INI Keys

| Key | Binary behavior in this slice | Active in YR |
|---|---|---|
| `Foundation=` | selects the base foundation table/list walked by `FUN_00716150` and `CanBePlacedAt`; modifiers are not folded into it | Yes |
| `AddOccupy1..8=` | parsed into eight pairs; not read by placement validators; affects hidden occupancy writer when `CanHideThings` permits it | Conditional |
| `RemoveOccupy1..8=` | parsed into eight pairs; not read by placement validators; decrements/cancels hidden occupancy on enter | Conditional |
| `CanHideThings=` | gates hidden occupancy height/add/remove effects, not base placement validation | Conditional, default true |
| `OccupyHeight=` | hidden occupancy depth input; not a foundation same-height requirement | Conditional |
| `WaterBound=` / land-speed data | participates indirectly through per-cell passability/land-type checks, not through Add/Remove | Yes |

Retail examples: `[GAREFN]` has `Foundation=4x3`, `CanHideThings=True`, `AddOccupy1=-1,0`, `AddOccupy2=-1,-1`, `RemoveOccupy1=3,1`; `[NACNST]` has `Foundation=4x4` plus eight `RemoveOccupy` cells. These modifiers do not change placement validation cells.

## 5. Integration Points

| Path | Validation used | Height behavior | Add/Remove behavior | Active in YR |
|---|---|---|---|---|
| Ready sidebar placement validation | `FUN_00716150` -> `Cell_passability_building_placement` over vtable `+0x90` base cells | no all-cells-same-height gate | not read | Yes |
| Ready sidebar commit | `HouseClass::Place_Production` -> object unlimbo | no added same-height gate | not read by commit validator | Yes |
| MCV/unit `DeploysInto` | `UnitClass::Deploy` -> target type vtable `+0xA8`; failure path may call `CanBePlacedAt` | no stricter deploy-only same-height gate | not read by validators | Yes |
| Building occupancy after placement | `BuildingClass::Place_OccupyMap` and `TechnoClass` enter/exit | base cells plus hidden occupancy writer; not a placement accept gate | `Cell+0x100` hidden occupancy only | Yes / Conditional |

## 6. Current Rust Implementation Status

Current Rust comparison is informational only:

| Rust area | Status vs scoped binary finding | Evidence |
|---|---|---|
| Ready placement rectangle | correct class for Add/Remove: base dimensions | `src/sim/production/production_placement.rs` |
| Ready placement path-grid dependency | can be indirectly polluted if path grid uses adjusted footprint blockers | prior footprint audit |
| MCV deploy mixed-height check | mismatch if it rejects mixed-height clear foundations | [traces/IMPLEMENTATION_MCV_DEPLOY_MIXED_HEIGHT_TRACE_RERUN_2026-05-21.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/IMPLEMENTATION_MCV_DEPLOY_MIXED_HEIGHT_TRACE_RERUN_2026-05-21.md) |
| Add/Remove footprint helper consumers | mismatch where one adjusted footprint is used as ordinary structure occupancy | prior foundation/footprint reports |

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Type placement virtual `FUN_00716150` | verified | decompile `0x00716150`; xrefs at vtables and `0x00464AD9` | exact class names for all vtable owners not needed for this slice |
| Per-cell build-block predicate | verified | decompile `0x0047C620`; xrefs from preview, wall, type validator | exact semantic names for every cell flag bit deferred |
| Ready commit path | verified | decompile `HouseClass::Place_Production @ 0x004FB0E0` | no UI preview visuals audited |
| MCV/unit deploy validation | verified | decompile `UnitClass::Deploy @ 0x007393C0`; xref to `CanBePlacedAt @ 0x00739536` | full state-transfer lifecycle out of scope |
| `CanBePlacedAt` object/scatter validator | verified | decompile `0x0045EE70` | exact scatter ordering beyond scoped outcome not expanded |
| Add/Remove parser and hidden occupancy writers | verified from prior + spot check | strings `0x0081A634/0x0081A624`, parse `0x00461425..0x004614E8`, writers `0x005683C0/0x005687F0` | exact downstream `Cell+0x100` readers are sibling slot 1 |
| Mixed-height rejection absence | verified | no terrain-height equality in `0x00716150`, `0x0047C620`, `0x0045EE70`, `0x00440580` | runtime fixture testing could still be useful, but binary path has no such gate |

## 8. Open Questions - Final State

- `[RESOLVED] OQ-1 - Does final placement validation use base foundation only? -> Yes. Both `FUN_00716150` and `CanBePlacedAt` walk vtable `+0x90` base offsets ending at `(0x7FFF,0x7FFF)`.` Evidence: `0x00716150`, `0x0045EE70`. Active in YR: Yes.
- `[RESOLVED] OQ-2 - Do AddOccupy/RemoveOccupy participate in the placement validator? -> No. They are parsed/stored separately and consumed by `TechnoClass` hidden occupancy writers, not by `FUN_00716150`, `Cell_passability_building_placement`, or `CanBePlacedAt`.` Evidence: `0x00461425..0x004614E8`, `0x005683C0`, `0x005687F0`, `0x00716150`, `0x0045EE70`. Active in YR: Conditional for hidden occupancy; No for placement validation.
- `[RESOLVED] OQ-3 - Is there a mixed-height foundation rejection? -> No in the inspected active validation and unlimbo paths. No code compares all foundation cell levels to a reference level.` Evidence: `0x00716150`, `0x0047C620`, `0x0045EE70`, `0x00440580`. Active in YR: Yes.
- `[RESOLVED] OQ-4 - Does MCV/unit deploy differ from sidebar ready placement on height/build-block checks? -> No material difference for scoped checks: deploy calls the building type placement virtual before allocation, and ready placement uses the same base-foundation/per-cell predicate family; neither adds same-height rejection.` Evidence: `0x007393C0`, `0x00716150`, `0x0047C620`, `0x004FB0E0`. Active in YR: Yes.
- `[DEFERRED] OQ-5 - Which exact gameplay readers consume `CellClass+0x100` hidden occupancy after placement?` Category: out-of-scope; reason: assigned to sibling `CELLCLASS_0X100_HIDDEN_OCCUPANCY_READERS` slot.
- `[DEFERRED] OQ-6 - Exact names for every `CellClass+0x140` placement-blocking bit.` Category: requires-different-system-context; reason: this slot only needed participation in the validator, not global flag taxonomy.

## Sources

- Ghidra read-only decompiled: `FUN_00716150 @ 0x00716150`, `Cell_passability_building_placement @ 0x0047C620`, `BuildingTypeClass::CanBePlacedAt @ 0x0045EE70`, `HouseClass::Place_Production @ 0x004FB0E0`, `UnitClass::Deploy @ 0x007393C0`, `BuildingTypeClass_ReadINI_Water @ 0x0045FE50/0x00461425..0x004614E8`.
- Ghidra xrefs: `CanBePlacedAt` direct xrefs from `0x00739536`, `0x006ED6E0`, `0x00445210`; `Cell_passability_building_placement` xrefs from `0x0047EC90`, `0x00716209`, wall/overlay helpers.
- Prior reports referenced: [BUILDING_FOUNDATION_OCCUPY_MODIFIERS_PARITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_FOUNDATION_OCCUPY_MODIFIERS_PARITY_GHIDRA_REPORT.md), [BUILDING_FOOTPRINT_CONSUMER_DISCREPANCY_AUDIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_FOOTPRINT_CONSUMER_DISCREPANCY_AUDIT_GHIDRA_REPORT.md), [BUILDINGCLASS_UNLIMBO_AND_PLACEMENT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_UNLIMBO_AND_PLACEMENT.md), [BUILDING_MCV_DEPLOY_ORIGIN_BLAST_RADIUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_MCV_DEPLOY_ORIGIN_BLAST_RADIUS_GHIDRA_REPORT.md).
- INI checked: `ini/artmd.ini`, `ini/rulesmd.ini` for `Foundation=`, `AddOccupy=`, `RemoveOccupy=`, `CanHideThings=`, stock `DeploysInto=`.
