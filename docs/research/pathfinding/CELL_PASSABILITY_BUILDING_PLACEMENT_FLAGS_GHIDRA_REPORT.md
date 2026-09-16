# Cell Passability Building Placement Flags - Ghidra Research Report

**Address(es):** `0x0047C620` (`Cell_passability_building_placement`)
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** ordinary per-cell building-placement gates and Rust-facing taxonomy for the fields read by `0x0047C620`: `CellClass+0x124`, `+0x140`, `+0x11C`, object/overlay exceptions, land/speed checks, and special building exceptions.
**Non-Scope:** MCV deploy origin, foundation origin math, AddOccupy/RemoveOccupy hidden occupancy, wall extension fill, building unlimbo side effects.
**Confidence:** High for the primary function and checked fields; Medium for human-readable field names imported from prior verified reports.
**Active in YR:** Yes. `BuildingPlacement_per_cell_draw @ 0x0047EC90`, `BuildingClass` vtable `+0x1AC` wrapper `0x00449440`, wall shadow callers `0x006D5C50/0x006D59D0`, and placement execution path `0x0043F180` all call this function in live placement contexts.

## 1. Overview

`Cell_passability_building_placement` is the live per-cell validator used by ready-building previews, building placement execution, wall placement shadows, and the `BuildingClass` A* predicate wrapper. It first applies building-type/object exceptions, then overlay/wall/gate special cases, then falls back to buildable-land/speed checks.

The function is not a generic unit pathing check. It is a building-placement predicate whose output is boolean-like: accepted cells return `1`, rejected cells return `0`.

## 2. Key Offsets and Fields

| Field | Meaning for placement | Active in YR | Evidence |
|---|---|---|---|
| `CellClass+0x124` | Ground occupation bitmask. Ordinary buildings require low 6 bits clear unless the building is a laser-fence segment path. | Yes | `0x0047C620` reads `cell+0x124 & 0x3F`; prior cell layout audit confirms ground occupation flags. |
| `CellClass+0x140 bit 0x100` | Bridge structural flag; blocks tiberium/laser-fence-style overlay placement and normal terrain placement fallback. | Yes | `0x0047C620` rejects when `(flags >> 8) & 1`; bridge layout reports identify `0x100` as bridge structural. |
| `CellClass+0x140 bit 0x400` | Additional cell flag that blocks the same overlay/normal placement fallback. Exact semantic name not re-derived here. | Yes | `0x0047C620` rejects when `cell+0x140 & 0x400`; prior bridge pathfinding audit says `0x400` was not pathfinding-relevant and remained unresolved. |
| `CellClass+0x11C` | Slope/ramp index byte. Ordinary placement accepts only `0`; nonzero rejects. | Yes | `0x0047C620` checks `cell+0x11C == 0`; prior audit verifies `+0x11C` is SlopeIndex written by `RecalcAttributes`. |
| `CellClass+0x11E` | Overlay/wall frame or state byte; values `> 0x0F` enable wall/gate replacement exceptions for matching owner. | Yes | `0x0047C620` uses `0x0F < *(byte *)(cell+0x11E)` in wall-overlay exception branches. |
| `CellClass+0x44` | Overlay type index, `-1` if none. Wall overlay ids `2`, `0`, and `0x1A` receive special handling. | Yes | `0x0047C620` reads `cell+0x44`; `0x0043F180` and overlay docs corroborate overlay removal/placement path. |
| `CellClass+0xEC` | LandType row index into the speed/buildable table. | Yes | `0x0047C620` indexes `g_SpeedType_LandType_Table` by `speedType + landType*9`; speed-table report verifies layout. |
| `CellClass+0x38` | IsoTileType index; used only by `WaterBound`/naval-style special checks. | Yes | `0x0047C620` validates index against `g_IsometricTileTypeClass_Array_Count` and reads IsoTileType `+0x2E0`. |
| `CellClass+0x50` | House/owner index of existing overlay/cell ownership. Used for wall/gate owner match. | Yes | `0x0047C620` maps `cell+0x50` through `g_HouseClass_Array` and compares to placement owner. |
| `CellClass+0xE4` | Ground object list. Scanned for object blockers and special exceptions. | Yes | `0x0047C620`, `FindOccupierByRTTI @ 0x0047C4D0`, `Find_Nearest_Object @ 0x0047C3D0`. |

## 3. Building Type Flags Consumed

| Field | Placement meaning | Active in YR | Evidence |
|---|---|---|---|
| `BuildingType+0x16B7` | Upgrade/garrison-related placement exception: if set, the validator tolerates only a matching existing building owned by the same owner in the cell. | Conditional | `0x0047C620` branches on `+0x16B7`; `BuildingPlacement_per_cell_draw @ 0x0047EC90` separately uses `CanAcceptUpgrade` in the same preview mode. Stock activation depends on building data. |
| `BuildingType+0x16BE` | `LaserFencePost=`. Treated with `+0x16B7` in the object-exception branch. | Conditional | [FIRESTORM_LASER_FENCE_POST_INTERACTIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRESTORM_LASER_FENCE_POST_INTERACTIONS_GHIDRA_REPORT.md) verifies parse key; `0x0047C620` reads `+0x16BE`. No stock `rulesmd.ini` building sets it. |
| `BuildingType+0x16BF` | `LaserFence=`. Uses a separate branch: rejects existing buildings and existing TerrainClass objects, but permits same-owner laser-fence building replacement. | Conditional | `FIRESTORM...` verifies parse key; `0x0047C620` reads `+0x16BF`. No stock `rulesmd.ini` building sets it. |
| `BuildingType+0xE58` | Building spawns/replaces a terrain tile via `ToTile=`-style type pointer. Placement rejects IsoTileTypes whose `+0x2E0` flag is false and rejects existing building occupants. | Conditional | `0x0047C620` checks `param_3+0xE58`; `rulesmd.ini` has `ToTile=Green01` entries. |
| `BuildingType+0xE54` | Overlay type pointer for wall/gate replacement matching; compares overlay type index at `OverlayType+0x294`. | Conditional | `0x0047C620` reads `param_3+0xE54` and then `+0x294`. |
| `TechnoType+0xCCE` | `Naval=`/water placement branch modifier: normal fallback uses buildable table when false; naval/water-bound branch requires IsoTileType index in `[DAT_00AA0738, +0xE)`. | Conditional | `0x0047C620`; unit-build-time docs verify `+0xCCE` as Naval; stock naval yards set `Naval=yes`/`WaterBound=yes`. |

## 4. Core Taxonomy for Ordinary Building Placement

### A. Global/editor gates

If `g_MapEditorMode != 0`, the function immediately accepts. Otherwise, the cell must be on screen in game mode (`TechnoClass__IsOnScreen @ 0x00578540`) or inside map bounds in map-editor mode (`Cell_in_bounds_check @ 0x00568300`).

Active in YR: Yes. Evidence: top of `0x0047C620`; preview caller `0x0047EC90`.

### B. Object occupancy gates

For normal non-laser-fence, non-upgrade buildings, any nearest visible object returned by `CellClass__Find_Nearest_Object @ 0x0047C3D0` rejects placement, and any occupant with RTTI `0x24` also rejects. Buildings (`WhatAmI()==6`) are rejected directly from the ground object list.

For `LaserFence=` (`BuildingType+0x16BF`), an existing building is allowed only when the existing occupant is also a laser-fence building and its owner equals the requested owner; otherwise it rejects. For `LaserFencePost=` or `+0x16B7`, an existing object is tolerated only if it resolves to a building that can accept the special placement path in the caller.

Active in YR: Yes for object scans; Conditional for laser-fence flags because stock YR has no `LaserFence=`/`LaserFencePost=` building. Evidence: `0x0047C620`, `0x0047C3D0`, `0x0047C4D0`, `FIRESTORM...` report.

### C. Ground occupancy bit gate

After object exceptions, ordinary placement rejects if `cell+0x124 & 0x3F` is nonzero. This is stricter than infantry subcell checks that often use `0x1F`; building placement treats any of the low six ground occupation bits as blocking.

Active in YR: Yes. Evidence: `0x0047C620`; [RALLY_POINTS_AND_UNIT_SPAWNING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RALLY_POINTS_AND_UNIT_SPAWNING.md) and bridge cell-offset audits identify `+0x124` as ground occupation flags.

### D. Wall/overlay replacement gates

When the cell overlay type is `2` or `0`, placement can accept if the building's overlay pointer (`BuildingType+0xE54`) matches the existing overlay type and `cell+0x11E > 0x0F`, or if the requested building type pointer is one of `Rules+0x87C`, `Rules+0x86C`, or `Rules+0x870`, and the existing cell owner matches the placement owner.

When the cell overlay type is `0x1A`, the same shape applies but the special building pointers are `Rules+0x874` or `Rules+0x878`.

Active in YR: Conditional. The logic is live, but `rulesmd.ini` sets all five `[General]` gate/tower pointers (`GDIGateOne`, `GDIGateTwo`, `NodGateOne`, `NodGateTwo`, `WallTower`) to `GADUMY`, so stock real gates do not receive this bypass. Evidence: `0x0047C620`; `FIRESTORM...` lines for `RulesClass+0x86C..+0x87C`.

### E. Laser-fence / tiberium-overlay placement gate

For `BuildingType+0x16BF != 0`, if the cell overlay is `0x7E` or `OverlayToTiberiumIndex @ 0x005FDD20` returns not `-1`, placement accepts only when all three are true: `!(cell+0x140 & 0x100)`, `!(cell+0x140 & 0x400)`, and `cell+0x11C == 0`.

Active in YR: Conditional. The code is live, but stock YR does not set `LaserFence=yes`; tiberium overlays and bridge/slope cells are normal stock data. Evidence: `0x0047C620`, `0x005FDD20`, `FIRESTORM...` report.

### F. Map-editor overlay fallback

If not already accepted and the game is not in map editor, nonempty overlay cells reject. In map editor, overlays whose `OverlayType+0x2A8` byte is set reject; otherwise the function continues to terrain fallback.

Active in YR: Yes for map-editor mode only; ordinary gameplay takes the non-editor rejection. Evidence: `0x0047C620`.

### G. Terrain fallback for ordinary buildings

If the candidate has no blocking overlay and `speedType == -1`, the ordinary building fallback requires: no bridge structural bit `0x100`, no `0x400` flag, and `SlopeIndex == 0`. If `TechnoType+0xCCE` is false, it returns the per-LandType `Buildable=` byte from `g_SpeedType_LandType_Table` column 8. If `TechnoType+0xCCE` is true, it instead accepts only IsoTileType indices in a 14-entry range starting at `DAT_00AA0738`.

If `speedType != -1`, the function ignores the `Buildable=` byte and accepts when `g_SpeedType_LandType_Table[speedType + landType*9] != 0.0`.

Active in YR: Yes. Evidence: `0x0047C620` read at `0x0047CA58`; [SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md) verifies base `0x0089EA40`, 9-slot row stride, and col 8 `Buildable=`.

## 5. Rust-Facing Rules Summary

For ordinary ready-building placement, Rust should model these as ordered gates:

1. Resolve cell visibility/bounds per gameplay/editor context.
2. Apply building-type object exceptions first (`LaserFence`, `LaserFencePost`/upgrade, `ToTile`/overlay-building type).
3. Reject low-six ground occupancy bits (`CellClass+0x124 & 0x3F`) unless an earlier exception allowed replacement.
4. If overlay id is a wall/special overlay, apply wall/gate replacement rules and owner match; otherwise ordinary gameplay rejects nonempty overlay.
5. For terrain fallback, require no bridge structural bit `0x100`, no `0x400`, and `SlopeIndex == 0`.
6. Use `Buildable=` when `speedType == -1`; use the SpeedType-vs-LandType nonzero speed matrix when `speedType != -1`.
7. For naval/water-bound building types, use the IsoTileType range check rather than ordinary `Buildable=`.

## 6. Current Rust Implementation Status

Rust has a placement pipeline in `src/sim/production/production_placement.rs`. It already validates foundation footprint cells, overlap, build area, `WaterBound`/`Naval` terrain, overlay/terrain-object blocking, bridge deck rejection, and slope rejection.

Rust does not currently mirror the full `0x0047C620` taxonomy as an ordered binary-compatible per-cell predicate. Notable gaps relative to this slice are: low-six `+0x124` bit semantics vs entity/pathgrid abstractions; wall/gate `Rules+0x86C..+0x87C` bypasses; laser-fence object replacement; `ToTile=` terrain replacement; and exact `Buildable=` vs SpeedType fallback split.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `Cell_passability_building_placement @ 0x0047C620` | verified | Ghidra decompile | none for stated field taxonomy |
| `CellClass__Find_Nearest_Object @ 0x0047C3D0` | verified | Ghidra decompile | none for blocker classification |
| `CellClass__FindOccupierByRTTI @ 0x0047C4D0` | verified | Ghidra decompile | none for RTTI-list search |
| `CellClass__OverlayToTiberiumIndex @ 0x005FDD20` | verified | Ghidra decompile | none for return `-1` vs valid branch |
| Preview caller `0x0047EC90` | verified | Ghidra decompile | exact drawing frames out-of-scope |
| Placement execution caller `0x0043F180` | touched-not-exhausted | Ghidra decompile | placement side effects out-of-scope |
| Building vtable wrapper `0x00449440` | verified | Ghidra decompile | none for active-call proof |
| Wall shadow callers `0x006D5C50/0x006D59D0` | touched-not-exhausted | Ghidra decompile | wall shadow drawing out-of-scope |
| `CellClass+0x140 bit 0x400` semantic name | deferred | `0x0047C620` read | Needs separate cell-flag audit; not needed to classify placement effect |

## 8. Open Questions - Final State

[RESOLVED] OQ-1 - Is `0x0047C620` active in YR placement? Yes. Called by ready-building preview, placement execution, wall shadows, and BuildingClass wrapper. Evidence: callers `0x0047EC90`, `0x0043F180`, `0x00449440`, `0x006D5C50`, `0x006D59D0`.

[RESOLVED] OQ-2 - Does ordinary placement consult `CellClass+0x124`? Yes, it rejects `cell+0x124 & 0x3F != 0`. Evidence: `0x0047C620`.

[RESOLVED] OQ-3 - What do `+0x140` and `+0x11C` do here? They are terrain fallback blockers: bit `0x100`, bit `0x400`, or nonzero `SlopeIndex` reject. Evidence: `0x0047C620`; `+0x100/+0x11C` names from prior bridge cell-offset audits.

[RESOLVED] OQ-4 - Does ordinary placement use `Buildable=` or SpeedType? Both, depending on the passed speed type: `-1` uses `Buildable=` column; any other speed type uses nonzero speed matrix. Evidence: `0x0047C620`; speed table report.

[RESOLVED] OQ-5 - Are gate/wall special pointers active in stock YR? The code is active but effectively neutralized for real gates because YR sets the five `[General]` gate/tower pointers to `GADUMY`. Evidence: `FIRESTORM...` report; `0x0047C620` pointer comparisons.

[DEFERRED] OQ-6 - Exact semantic name of `CellClass+0x140 bit 0x400`. Category: out-of-scope. The placement effect is verified as blocking; naming requires a broader cell flag audit.

## Sources

- Ghidra decompile: `Cell_passability_building_placement @ 0x0047C620`
- Ghidra decompile: `CellClass__Find_Nearest_Object @ 0x0047C3D0`
- Ghidra decompile: `CellClass__FindOccupierByRTTI @ 0x0047C4D0`
- Ghidra decompile: `CellClass__OverlayToTiberiumIndex @ 0x005FDD20`
- Ghidra decompile: `BuildingPlacement_per_cell_draw @ 0x0047EC90`
- Ghidra decompile: `FUN_0043F180`, `FUN_00449440`, `OverlayWall_PlacementShadow @ 0x006D5C50`, `FirestormWall_PlacementShadow @ 0x006D59D0`
- Prior verified docs: [FIRESTORM_LASER_FENCE_POST_INTERACTIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRESTORM_LASER_FENCE_POST_INTERACTIONS_GHIDRA_REPORT.md), [SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPEEDTYPE_LANDTYPE_TABLE_GHIDRA_REPORT.md), [RALLY_POINTS_AND_UNIT_SPAWNING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RALLY_POINTS_AND_UNIT_SPAWNING.md), bridge cell-offset audit entries in [AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md)
- INI cross-checks: `ini/rulesmd.ini` `[General]` gate/tower values, `WaterBound=`, `Naval=`, `ToTile=`, and stock absence of `LaserFence=` / `LaserFencePost=`
