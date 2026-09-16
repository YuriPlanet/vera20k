# Low Bridge TubeClass Behavior in Yuri's Revenge -- Ghidra Research Report

**Address(es):** `0x00484AB0`, `0x00484F20`, `0x0047D2B0`, `0x00727FD0`, `0x0056D6E0`, `0x0056C510`, `0x00582D70`, `0x007359F0`
**Confidence:** High for the live predicate, construction path, key fields, zone-record use, and unit/infantry tube movement; Medium for several low-bridge repair/damage side effects because the full walker family was not re-expanded line by line here.
**Active in YR:** Yes. The checked paths are reached from normal map attribute recalculation, bridge-zone building, infantry/unit AI, cursor/action handling, and bridge damage/repair code in `gamemd.exe`. No TS-only scenario flag gate was found on the low-bridge TubeClass paths covered below.

## 1. Overview

Low/wood bridge traversal in YR is not just "low bridge overlay means passable road." A cell is treated as a low bridge pathing cell only when it has a valid `TubeClass` index in `CellClass+0x116` and its final `LandType` is `10` (`Tunnel`). The retail low-bridge overlay INI sections say `Land=Road`, but the live binary low-bridge movement and zone logic keys on the tube-backed tunnel cell state, not on the overlay land alone.

The auto-created low-bridge tubes are created during `CellClass::RecalcAttributes`. The constructor creates a same-cell tube shell: entry coord and exit coord are initially identical, the direction comes from the four-entry table at `0x0081CC20`, the path-step array is filled with `-1`, and the path length remains `0`. This means the automatic low-bridge tube records are per qualifying cell, not one shared object for the whole bridge span.

## 2. Verified Binary Facts

| What | Evidence | Confidence | Active in YR? |
|---|---|---:|---|
| `CellClass::IsLowBridgeCell` requires a valid tube index and `LandType == 10`. | `0x00484AB0`: checks `0 <= *(i16*)(cell+0x116) < DAT_008b4148` and `*(i32*)(cell+0xEC) == 10`. | High | Yes |
| `CellClass::GetTubeAtCell` is only bounds-checked on `+0x116`; it does not re-check land type. | `0x00484F20`: returns `g_TubeArray[index]` when `0 <= index < DAT_008b4148`, else `0`. | High | Yes |
| Low bridge tube construction is inside `CellClass::RecalcAttributes`, after final land type is computed. | `0x0047D2B0`, branch near `0x0047D8EC-0x0047D940`: `LandType == 10`, invalid current tube index, tile range in one of four low/tunnel ranges, then `operator_new(0x1C4)` and `TubeClass::Constructor`. | High | Yes |
| The construction tile ranges are four consecutive-tile bands. | `0x0047D2B0`: accepts `IsoTileTypeIndex` in `[DAT_00AA1054, +3]`, `[DAT_00ABB108, +3]`, `[DAT_00AA10B4, +3]`, or `[DAT_00ABAD2C, +3]`. | High | Yes |
| The auto tube direction table is `[2, 4, 6, 0]`. | `0x0081CC20` memory bytes decode as dwords `2, 4, 6, 0`; callsite `0x0047D935` loads `DAT_0081CC20[(tile - range_base)]`. | High | Yes |
| `TubeClass::Constructor` stores entry and exit coord to the same input coord for auto-created tubes. | `0x00727FD0`: writes `param_1[9] = *coord` (`+0x24`) and `param_1[10] = *coord` (`+0x28`). | High | Yes |
| Auto-created low-bridge tubes have no path steps. | `0x00727FD0`: writes `Tube+0x1C0 = 0`, then fills 100 dwords at `Tube+0x30..+0x1BC` with `-1`. | High | Yes |
| The constructor appends itself to `g_TubeArray` and writes the resulting index to the entry cell. | `0x00727FD0`: appends into `g_TubeArray + DAT_008b4148*4`, increments `DAT_008b4148`, finds its own index, writes `*(i16*)(entry_cell+0x116) = index`. | High | Yes |
| The constructor skips the cell-index write only for coord `(0,0)`. | `0x00727FD0`: guarded by `if ((*coord != 0) || (coord[1] != 0))`. | High | Yes |
| `[Tubes]` INI/map section tubes are a separate explicit-tube path. | `0x007283C0`: reads `[Tubes]`, constructs with coord `(0,0)`, then overwrites entry `+0x24`, direction `+0x2C`, exit `+0x28`, direction steps `+0x30`, path length `+0x1C0`, and writes the tube index to the entry cell. | High | Yes, if map contains `[Tubes]` |
| `MapCoord_Step_By_Direction` treats direction `8` as a tube jump. | `0x0042D490`: direction `8` reads current cell `+0x116`; if not `-1`, output becomes `g_TubeArray[index]->+0x28`, else output coord becomes `0`. | High | Yes |
| Path walking treats direction `8` the same way. | `Path_walk_directions_to_cell @ 0x00429780`: if a path step is `8`, it reads the current cell tube index and jumps to `Tube+0x28`; missing tube yields coord `0`. | High | Yes |
| `UnitClass::AI` runs tube movement while unit field `+0x684` is a non-negative signed byte. | `0x007363B0`: `if (-1 < (char)param_1[0x1A1]) UnitClass::TubeMovement(); vtable+0x4A0(0); return;`. | High | Yes |
| `UnitClass::TubeMovement` reads `g_TubeArray[(char)unit+0x684]`, path-step byte `unit+0x685`, tube entry `+0x24`, exit `+0x28`, direction `+0x2C`, steps `+0x30`, and length `+0x1C0`. | `0x007359F0` decompilation. | High | Yes |
| Infantry has its own tube movement routine, structurally parallel to unit tube movement. | `InfantryClass::AI @ 0x0051BF00` calls `FUN_0051B350` when `+0x684` is non-negative; `FUN_0051B350` reads the same tube fields. | High | Yes |
| Low bridge cursor/action handling is live. | `InfantryClass::What_Action_OnCell @ 0x0051F900` checks `IsLowBridgeCell`; `FootClass::ClickedAction_Cell @ 0x004D8100` case `0x23` uses `GetTubeAtCell`, `Tube+0x28`, and `Tube+0x2C` to route the order. | High | Yes |
| `ComputeBridgeZones` builds low bridge records from low bridge tube cells. | `0x0056D6E0`: non-high/wood branch calls `IsLowBridgeCell` several times, uses `GetTubeAtCell`, writes a 16-byte record with `bridge_kind = 1`. | High | Yes |
| `FindBridgeRecord` skips low bridge records. | `0x0056DA10`: `if (record+0x0C == 0)` before considering the record; low records from `ComputeBridgeZones` have `+0x0C == 1`. | High | Yes |
| `UpdateBridgeZonesHelper` does not skip low bridge records while adding active bridge/tube zone edges. | `0x0056C510`: iterates records whose intact byte `+0x08` is nonzero; no `+0x0C` bridge-kind test in that loop. | High | Yes |
| The lower-level temporary zone graph injector has a non-high-bridge tube path. | `FUN_00582D70`: if the starting cell is not high/wood bridge, it calls `GetTubeAtCell`, uses `Tube+0x2C`, adjacent low/tube cells, and `Path_walk_directions_to_cell(Tube+0x1C0, Tube+0x30)` to create three temp graph connection pairs. | High | Yes |

## 3. Class Layout / Key Offsets

### `CellClass` fields used by low bridge tubes

| Offset | Type | Meaning | Evidence |
|---:|---|---|---|
| `+0x24` | packed `CellStruct` / two `i16` | Cell map coordinate. Used as tube constructor input and bridge record endpoint source. | `0x0047D2B0`, `0x00727FD0`, `0x0056D6E0` |
| `+0x38` / typed decompiler `IsoTileTypeIndex` | int/short tile index context | Tile identity used for low/tunnel construction range checks. | `0x0047D2B0` |
| `+0x44` | overlay/tile state in bridge damage helpers | Low bridge overlay index/state used by low destroy/repair walkers. | `0x0057BAA0`, `0x0057F200` |
| `+0x4C` | byte zone type | Written into zone-map cell data after `RecalcZoneType`; not part of `IsLowBridgeCell`. | `0x0047D2B0`, `0x0056C510` |
| `+0xEC` | `i32 LandType` | Must be `10` for `IsLowBridgeCell`. | `0x00484AB0` |
| `+0x116` | `i16 tube_index` | `-1`/invalid means no tube; valid index points into `g_TubeArray`. | `0x00484AB0`, `0x00484F20`, `0x00727FD0` |
| `+0x11A` | byte tile sub-state / low bridge direction-ish state | Read by low bridge damage state machine and surface walkers; also decompiler names this as `Height` in terrain paths, so treat contextually. | `0x00571490`, `0x0047D2B0` |
| `+0x11B` | signed byte level | Used by bridge collapse/repair height adjustments and zone data. | `0x00571490`, `0x0047D2B0` |
| `+0x11C` | byte slope/ramp index | Used by bridge traversal and low bridge repair marking. | `0x004D9C60`, `0x00578E60` |
| `+0x11E` | byte bridge damage state | Low state machine changes this for damage/collapse. | `0x00571490` |
| `+0x140` | flags | Bridge flags such as `0x80`, `0x100`, `0x200`, `0x400`, `0x800` drive damage/destruction decisions. | `0x00571490`, `0x00570050` |

### `TubeClass` fields used here

`TubeClass` allocation size is `0x1C4` in both the auto constructor path and `[Tubes]` parser path.

| Offset | Type | Meaning | Evidence |
|---:|---|---|---|
| `+0x24` | packed `CellStruct` | Entry coord. Auto low-bridge constructor sets this to the source cell. | `0x00727FD0` |
| `+0x28` | packed `CellStruct` | Exit coord. Auto low-bridge constructor initially sets it equal to entry. Explicit `[Tubes]` parser overwrites it. | `0x00727FD0`, `0x007283C0` |
| `+0x2C` | `i32` | Direction. Auto low-bridge constructor uses `[2,4,6,0]`; movement exit-facing and zone graph use it. | `0x00727FD0`, `0x0081CC20`, `0x007359F0`, `0x00582D70` |
| `+0x30..+0x1BC` | `i32[100]` | Direction/path step buffer. Auto constructor fills every slot with `-1`. Explicit `[Tubes]` parser fills until sentinel `-1` or 100 entries. | `0x00727FD0`, `0x007283C0` |
| `+0x1C0` | `i32` | Path-step count. Auto low-bridge constructor leaves this as `0`; explicit parser starts at `-1`, increments per parsed step, and stops on `-1`. | `0x00727FD0`, `0x007283C0` |

### Globals

| Address | Meaning | Evidence |
|---:|---|---|
| `0x008B413C` / `g_TubeArray` | Dynamic array of `TubeClass*`. | `0x00484F20`, `0x00727FD0`, `0x007359F0` |
| `0x008B4148` | Current tube count. | `0x00484AB0`, `0x00484F20`, `0x00727FD0` |
| `0x0081CC20` | Auto low/tunnel tube direction table, dwords `[2,4,6,0]`. | `read_memory(0x0081CC20)`, callsite `0x0047D935` |

## 4. Core Logic

### 4.1 Low bridge cell predicate

Verified pseudocode from `CellClass::IsLowBridgeCell @ 0x00484AB0`:

```text
tube_index = *(i16 *)(cell + 0x116)
if tube_index >= 0
   and tube_index < tube_count
   and *(i32 *)(cell + 0xEC) == 10:
    return true
return false
```

Tiny details:

- `LandType == 10` is mandatory. A low bridge overlay with `Land=Road` is not enough.
- The tube index must be below the current `g_TubeArray` count, not merely non-negative.
- `GetTubeAtCell` only checks the tube index bounds; code that needs "low bridge cell" semantics must use `IsLowBridgeCell`, not only `GetTubeAtCell`.

### 4.2 Auto low bridge tube construction during `RecalcAttributes`

Verified pseudocode from `CellClass::RecalcAttributes @ 0x0047D2B0`:

```text
compute final cell LandType from overlay/tile/LAT/slope rules

if LandType == 10
   and (cell.tube_index < 0 or cell.tube_index >= tube_count)
   and IsoTileTypeIndex is in one of four low/tunnel 4-tile ranges:
       range_base = the matching range base
       sub = IsoTileTypeIndex - range_base
       if sub != -1:
           tube = operator_new(0x1C4)
           if tube != null:
               TubeClass::Constructor(tube, cell.coord, DirectionTable[sub])
```

Constructor effects from `0x00727FD0`:

```text
tube.entry = coord
tube.exit = coord
tube.direction = dir
tube.path_len = 0
for i in 0..100:
    tube.path[i] = -1
append tube to g_TubeArray
if coord != (0,0):
    cell(coord).tube_index = index_of_this_tube
```

Tiny details:

- The auto construction path runs only if the existing `cell+0x116` is invalid. A valid existing tube index prevents constructing another tube for that cell.
- The four accepted tile ranges are inclusive `base..=base+3`.
- The `iVar9 - iVar13 != -1` check is redundant after the inclusive range tests for normal input, but it is present before allocation.
- Allocation failure simply skips tube construction; no fallback tube index is written.
- The constructor's `(0,0)` guard matters for explicit `[Tubes]` loading, where the constructor is called with `(0,0)` and the parser writes fields afterward.

### 4.3 One tube per bridge, segment, or cell?

For the automatic low-bridge path in `CellClass::RecalcAttributes`, the verified answer is: **one `TubeClass` per qualifying low/tunnel cell whose `cell+0x116` is currently invalid**.

Evidence:

- `RecalcAttributes` is a per-cell method.
- It passes the current cell coordinate to the constructor.
- The constructor writes the new tube index only to that same coordinate's `CellClass+0x116`.
- The auto tube's entry and exit coords are equal to that same coordinate.
- No span-wide scan or shared bridge object is created in this constructor branch.

This corrects the stale claim in older bridge notes that low bridge tubes are "per bridge." The binary evidence for the auto low-bridge construction branch is per-cell. Whole-bridge connectivity is synthesized later by zone and movement/path logic using adjacent low/tube cells and the tube direction/path data.

### 4.4 Direction `8` sentinel / tube jump behavior

`MapCoord_Step_By_Direction @ 0x0042D490` and `Path_walk_directions_to_cell @ 0x00429780` both reserve direction `8` for a tube jump:

```text
if direction != 8:
    coord += normal direction delta
else:
    cell = Map.GetCell(current_coord)
    if cell.tube_index == -1:
        coord = 0
    else:
        coord = g_TubeArray[cell.tube_index].exit
```

For auto-created low-bridge tubes, `exit == entry`, so a direction-8 jump from that cell is same-cell unless a later path has an explicit tube exit. Explicit map `[Tubes]` can have a real remote exit and path steps.

## 5. Integration Points

### 5.1 Map load / attribute build order

The important verified ordering is:

1. Map cells go through `CellClass::RecalcAttributes`.
2. `RecalcAttributes` computes final land/zone state and creates low/tunnel tube shells for eligible `LandType == 10` cells.
3. Later bridge zone construction can call `IsLowBridgeCell`, which depends on the `cell+0x116` tube index already existing.
4. `MapClass::ComputeBridgeZones` builds both high and low bridge records from the recomputed map state.
5. `MapClass::UpdateBridgeZonesHelper` rebuilds movement-zone ID arrays and adds active bridge/tube edges.

Evidence:

- `ComputeBridgeZones @ 0x0056D6E0` xrefs include map-load/init callers.
- Existing [BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md) also records the same high-level order and was spot-checked here against the live decompilation.

### 5.2 `ComputeBridgeZones` low-bridge path

Low branch from `0x0056D6E0`:

```text
if !IsBridge(cell) and !IsWoodBridge(cell):
    if IsLowBridgeCell(cell):
        check neighboring low bridge cells in two opposite-axis patterns:
            pattern A: step dir 2 and dir 6 are low
            pattern B: step dir 4 and dir 0 are low
        if accepted:
            tube = GetTubeAtCell(cell)
            if linear_index(tube.exit) < linear_index(cell.coord):
                push BridgeRecord {
                    endpoint_a = cell.coord
                    endpoint_b = tube.exit
                    intact = 1
                    bridge_kind = 1
                }
```

Tiny details:

- The record duplicate/order filter uses `FUN_0042B1C0 @ 0x0042B1C0`, a map-cell linearization:

```text
((x - map_min + -1 + y) * map_min)
  + (((x - y) + -1 + map_min) >> 1)
```

- The decompiler elides some implicit parameter setup around the two `FUN_0042B1C0` calls, but the branch is clearly a less-than filter before record insertion.
- Low records write `bridge_kind = 1`; high/wood records write `bridge_kind = 0`.

### 5.3 `UpdateBridgeZonesHelper` and the temp graph injector

`UpdateBridgeZonesHelper @ 0x0056C510`:

- Clears prior zone arrays.
- Flood-fills base zones from `MapClass+0x68` 4-byte-per-cell zone data.
- Iterates bridge records backwards by 16-byte record size.
- For every active record (`record+0x08 != 0`), connects endpoint zones.
- Does not test `record+0x0C`, so low records participate in this pass.

`FUN_00582D70`:

- Handles high/wood bridges in one branch using bridge tile orientation tables.
- Handles non-high/wood bridge/tube cells in the other branch:
  - `GetTubeAtCell(start_cell)`;
  - read `Tube+0x2C` direction;
  - compute side cells at `direction + 2` and `direction - 2`;
  - call `GetTubeAtCell` on adjacent tube cells;
  - reject if either adjacent tube is missing;
  - call `Path_walk_directions_to_cell(Tube+0x1C0, Tube+0x30)` for each;
  - insert three temp graph pairs with flag low byte `0`.

Implementation boundary: bridge/tube records are not only for high bridges. High-only searches use `FindBridgeRecord`; zone graph construction uses all active records.

### 5.4 `UnitClass::TubeMovement`

`UnitClass::AI @ 0x007363B0` enters `UnitClass::TubeMovement @ 0x007359F0` whenever the signed byte at unit offset `+0x684` is non-negative.

`TubeMovement` behavior visible to players:

- Moves the unit through tube/tunnel/low-bridge route state instead of normal ground locomotion.
- Interpolates world position between tube entry and exit centers.
- Uses tube path step `unit+0x685` and `Tube+0x30 + step*4`.
- If the current step is not `-1`, it advances by direction deltas and adjusts Z by `(exit_ground_height - entry_ground_height) / tube.path_len`.
- At tube end, it moves the unit to `Tube+0x28`, clears tube state by writing `0xFF` to the signed byte at `+0x684`, sets movement/state flags, updates facing from `Tube+0x2C` when a tube remains at the exit cell, and handles occupant blocking at the exit cell.

Tiny details:

- The code reads tube path entries through `(char)unit+0x684`, so the active tube index is signed-byte sized in this movement state even though `CellClass+0x116` is an `i16`.
- Unit path step is the byte at `+0x685`.
- Auto low-bridge tubes have `path_len == 0` and path[0] `-1`; that takes the end/exit branch rather than the interpolated multi-step branch.

### 5.5 Infantry tube movement and low bridge UI actions

Infantry:

- `InfantryClass::AI @ 0x0051BF00` checks the same signed-byte tube-active field and calls `FUN_0051B350`.
- `FUN_0051B350` mirrors the unit tube logic but places infantry into cells via infantry-specific placement logic at tube exit.

UI/action:

- `InfantryClass::What_Action_OnCell @ 0x0051F900` checks low bridge cells while evaluating click actions.
- When `iVar2 == 1` and the target cell is low bridge, it calls `FUN_00484F10`, which returns `1`, so the returned action is `0x23`.
- `FootClass::ClickedAction_Cell @ 0x004D8100`, case `0x23`, reads the clicked cell's tube, reads `Tube+0x28`, then reads the exit cell tube and computes a command target adjacent to the exit using `(Tube+0x2C - 4) & 7`.

Player-visible implication: clicking/moving infantry over low bridge tube cells has special low-bridge action routing, not just ordinary move-to-road behavior.

## 6. Damage, Destruction, and Repair

### Verified damage/destruction facts

Low bridge damage is handled by live low bridge functions, not by the tube constructor itself:

- `ProcessBridgeDamageStateMachine_Low @ 0x00571490`
- `ProcessBridgeDestruction_Low @ 0x00570050`
- `DestroyBridge_Low @ 0x0057BAA0`
- `MapClass::RepairBridge_Low @ 0x0057F200`
- `MapClass::MarkBridgesForRepair_Low @ 0x00578E60`

Verified effects:

- Low bridge damage/collapse changes overlay/tile state via `MapClass::SetOverlayAndPropagate`.
- It calls low-specific ramp/surface update helpers such as `UpdateRamp_NS_DamageA_Low`, `UpdateRamp_NS_CollapseA_Low`, `UpdateRamp_EW_DamageA_Low`, and `UpdateRamp_EW_CollapseB_Low`.
- Collapse calls `CellClass::BlowUpBridge` on a 3-cell strip, with different strip orientation for NS/EW paths.
- Collapse calls `MapClass::UpdateAdjacentBridges`.
- Zone state is updated through `InvalidateBridgeZones` or `ValidateBridgeZones`, followed by `UpdateBridgeZonesHelper` only if the validate/invalidate call reports a change.
- Repair code walks low bridge overlay ranges (`0x4A..0x65` in the decompiled checks) and calls low-specific repair walkers.

### Tube field mutation during damage/repair

In the low damage/destruction/repair functions decompiled for this report, I did not find a direct write clearing `CellClass+0x116` or removing a `TubeClass` from `g_TubeArray`. The visible invalidation path is through bridge overlay/state changes, zone validate/invalidate, and a full `UpdateBridgeZonesHelper` recompute.

Confidence: Medium. The statement is based on the decompiled primary low damage/destruction/repair entry points above, but not on an exhaustive binary-wide write search for every possible `+0x116` write.

Implementation implication: do not assume low bridge destruction necessarily deletes tube records. Model the separation explicitly: tube identity may remain attached to a cell while active bridge/zone/overlay state decides whether it contributes connectivity.

## 7. INI Keys and Data

| Source | Key/section | Verified value | Relevance |
|---|---|---|---|
| `ini/rulesmd.ini` and `ini/rules.ini` | `[OverlayTypes]` entries `77..104 = LOBRDG01..LOBRDG28`, `125..128 = LOBRDGE1..4`, `209..236 = LOBRDB01..28`, `237..240 = LOBRDGB1..4` | Low bridge overlay families exist in YR data. | Overlay identity and visible art/damage states. |
| `ini/rulesmd.ini` and `ini/rules.ini` | `[LOBRDGxx]`, `[LOBRDGEx]`, `[LOBRDBxx]`, `[LOBRDGBx]` | `Land=Road` | Important mismatch with binary movement predicate: overlay INI land is not sufficient for low-bridge tube pathing. |
| `ini/rulesmd.ini` / `rules.ini` | `TunnelSpeed=1` | Present in general rules. | Relevant to tunnel movement broadly, but not directly read in the decompiled low-bridge predicate here. |
| `ini/rulesmd.ini` / `rules.ini` | `BridgeStrength`, `BridgeDestruction`, `DestroyableBridges` | Present. | Relevant to bridge damage/repair, not to low-bridge tube construction. |
| Map INI | `[Tubes]` | Parsed by `0x007283C0` when present. | Explicit map-authored tubes use same `TubeClass` structure but are distinct from auto low-bridge per-cell tube shells. |

## 8. Repo / Docs Mismatches

### Current Rust status

Verified source scan:

- `src/map/resolved_terrain.rs:302-313` currently forces low bridge overlays to `LandType::Road`, clears water, and makes water cells under low bridges passable for ground units.
- `src/map/resolved_terrain.rs:1080-1096` treats non-high bridge overlays as `BridgeDirection::Low` with deck level equal to ground.
- `src/sim/bridge_state/mod.rs:465-503` now has `BridgeRecordKind::{High, Low}` and records `bridge_kind`.
- `src/sim/bridge_state/mod.rs:1551-1557` records `bridge_kind` from terrain group data.
- `src/sim/pathfinding/zone_build.rs:54-68` distinguishes all-active bridge records from high-active-only searches.
- `rg` found no `TubeClass`, tube index, `GetTubeAtCell`, or `TubeMovement` equivalent in `src/`.

### Mismatches against binary

| Area | Rust/current docs state | Binary finding | Impact |
|---|---|---|---|
| Low bridge passability | Low bridge overlays are made road/passable ground. | Low bridge cell predicate requires valid tube index and `LandType == 10`. | Ground routing, click actions, and zone connectivity can differ on low bridge maps. |
| Tube model | No runtime tube index or TubeClass equivalent. | `CellClass+0x116`, `g_TubeArray`, and `TubeClass` are live and read by map zones and unit/infantry movement. | Missing state needed for parity. |
| Tube granularity | Some old docs imply per-bridge tubes. | Auto low-bridge construction is per qualifying cell. | Implementation should not start with one shared object per whole low bridge unless a later design deliberately derives an equivalent model. |
| Bridge record kind | Recently fixed in Rust. | Binary writes low records as `bridge_kind = 1`; `FindBridgeRecord` skips them, zone helper uses active records. | Rust now has the right field, but low records are still built from overlay groups rather than tube-backed low cells. |
| `Land=Road` interpretation | Current Rust treats low bridge INI land as movement truth. | Binary low-bridge movement truth is final `LandType == 10` plus tube. | INI overlay land must not erase the tube/tunnel behavior. |

### Existing docs verified or corrected

- [BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md) is broadly correct that low bridge overlays are not enough and that `IsLowBridgeCell` requires `cell+0x116` plus `LandType == 10`.
- `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` sections around low bridge tubes contain useful field facts, but the line saying "TubeClass is per-bridge" is stale for the automatic low-bridge construction path verified here. The live `RecalcAttributes` branch is per qualifying cell.
- `docs/gap-scans/2026-05-15-disparity-scan-bridges-end-to-end.md` and `docs/gap-scans/2026-05-15-disparity-scan-bridge-business-logic.md` correctly identify the missing Rust TubeClass/tube-index model as a current low-bridge parity gap.

## 9. Inferred Behavior

These are implementation-facing interpretations derived from verified facts, not direct one-line binary statements:

- Low bridge cells are best understood as tunnel-backed cells whose visible overlay family is low bridge. The overlay provides art/damage state; the tube/tunnel cell state provides movement and zone behavior.
- Automatic low-bridge tube shells are same-cell records, but they still matter because other systems use tube direction, neighboring tube cells, and direction-8 sentinel behavior to synthesize connectivity and movement transitions.
- Damage and repair appear to toggle low bridge traversability through overlay/state/zone changes rather than by destroying the tube objects themselves.
- For Rust parity, "make water under low bridge into road" can reproduce some simple pathing outcomes but misses cursor/action routing, direction-8 path semantics, per-cell tube identity, and the exact bridge/tube zone graph.

## 10. Open Questions

1. The exact parameter setup around the two `FUN_0042B1C0` calls in `ComputeBridgeZones` should be assembly-verified if implementing the duplicate/order filter exactly. The decompiler confirms a less-than filter before low-record insertion, but the implicit inputs deserve a final asm pass.
2. A binary-wide write audit of `CellClass+0x116` would confirm whether any damage/repair helper ever clears or rewrites low bridge tube indices outside the primary functions checked here.
3. The exact movement/path planner transition that writes the active tube index into unit/infantry `+0x684` for low bridges should be traced before implementation. This report verified `TubeMovement` consumes the state, not every producer.
4. Zone type mapping for `LandType == 10` should be tied to the repo's movement-class/passability tables during design. `IsLowBridgeCell` itself does not check `ZoneType`, but zone building consumes the recomputed zone byte.
5. The low bridge surface/ramp helper family is large. This report only records its interaction boundary with tubes/zones; exact visual tile mutation should continue to rely on the existing low bridge surface reports unless a new rendering/damage investigation is requested.

## 11. Implementation Implications

Do not implement low bridges as ordinary road terrain. The binary-visible model needs these separable pieces:

1. Per-cell tube identity for qualifying low/tunnel bridge cells:
   - `tube_index` equivalent;
   - tube entry coord;
   - tube exit coord;
   - tube direction;
   - path-step list and path length, even if auto low bridges start as zero-length.
2. A low-bridge predicate equivalent to:

```text
valid_tube_index(cell) && land_type(cell) == Tunnel(10)
```

3. Direction-8 tube jump handling in path coordinate stepping and any path smoothing/path-walking equivalent.
4. Movement/click-action code that can enter a tube state and consume it over ticks for units and infantry.
5. Bridge-zone construction that can build low bridge records from tube-backed cells, while keeping `FindBridgeRecord` high-only behavior.
6. Damage/repair code that updates overlay/state/zones without assuming tube records are deleted.

Current Rust already has `BridgeEndpointRecord.bridge_kind`, which removes one recent blocker. The remaining foundational gap is that the low bridge records and pathing are not sourced from live tube-backed low bridge cells.

## Sources

### Ghidra functions decompiled / checked

- `CellClass::IsLowBridgeCell @ 0x00484AB0`
- `CellClass::GetTubeAtCell @ 0x00484F20`
- `CellClass::RecalcAttributes @ 0x0047D2B0` / low tube branch around `0x0047D8EC-0x0047D940`
- `TubeClass::Constructor @ 0x00727FD0`
- `[Tubes]` parser / loader `FUN_007283C0`
- `MapCoord_Step_By_Direction @ 0x0042D490`
- `Path_walk_directions_to_cell @ 0x00429780`
- `FUN_0042B1C0` map-cell linearizer
- `MapClass::ComputeBridgeZones @ 0x0056D6E0`
- `MapClass::FindBridgeRecord @ 0x0056DA10`
- `MapClass::UpdateBridgeZonesHelper @ 0x0056C510`
- `FUN_00582D70` bridge/tube temp-edge injector
- `UnitClass::AI @ 0x007363B0`
- `UnitClass::TubeMovement @ 0x007359F0`
- `InfantryClass::AI @ 0x0051BF00`
- `FUN_0051B350` infantry tube movement
- `InfantryClass::What_Action_OnCell @ 0x0051F900`
- `FootClass::ClickedAction_Cell @ 0x004D8100`
- `CheckBridgeTraversal @ 0x004D9C60`
- `ProcessBridgeDamageStateMachine_Low @ 0x00571490`
- `ProcessBridgeDestruction_Low @ 0x00570050`
- `DestroyBridge_Low @ 0x0057BAA0`
- `MapClass::RepairBridge_Low @ 0x0057F200`
- `MapClass::MarkBridgesForRepair_Low @ 0x00578E60`

### Static data checked

- `0x0081CC20`: dword direction table `[2, 4, 6, 0]`
- `0x0082A734`, `0x0082A774`, `0x0082A7B4`, `0x0082A944`: bridge orientation/height tables touched by bridge zone logic
- `g_TubeArray @ 0x008B413C`, tube count `DAT_008B4148`

### Docs and repo files checked

- `docs/research/BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md`
- `docs/research/BRIDGE_SYSTEM.md`
- `docs/research/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`
- `docs/research/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md`
- [docs/research/ZONE_MAP_BUILD_LEVEL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ZONE_MAP_BUILD_LEVEL_GHIDRA_REPORT.md)
- `docs/research/PATH_SMOOTHING_AND_SPEED_RAMPING_GHIDRA_REPORT.md`
- `docs/gap-scans/2026-05-15-disparity-scan-bridges-end-to-end.md`
- `docs/gap-scans/2026-05-15-disparity-scan-bridge-business-logic.md`
- `src/map/resolved_terrain.rs`
- `src/sim/bridge_state/mod.rs`
- `src/sim/pathfinding/zone_build.rs`
- `ini/rules.ini`, `ini/rulesmd.ini`, `ini/art.ini`, `ini/artmd.ini`
